//! Log-profile → Rec.709 display transforms (P6.5, SPEC VIE-6, ALGORITHMS §3).
//!
//! Each profile is decoded with the vendor-published curve (normalised code value CV/1023 →
//! scene-linear reflection, 18 % grey = 0.18), converted from the camera gamut to BT.709
//! primaries, then shown through one shared display rendering (`display`). Constants and
//! reference values were checked against colour-science 0.4.7, which implements the vendor
//! documents; `testdata/colour/log_refs.json` holds the references the tests compare to.
//!
//! The same curves feed the viewer (as LUTs, `preview_lut`) and score-time exposure measures
//! (`to_display`), so what the user sees and what the Luma sub-metric measures agree.

use clippro_types::ColourProfile;

/// Normalised code value (CV/1023) → scene-linear reflection.
pub fn decode(profile: ColourProfile, cv: f32) -> f32 {
    let y = cv as f64;
    let x = match profile {
        ColourProfile::Rec709 => bt1886_eotf(y),
        // Sony S-Log3 (S-Gamut3/S-Log3 technical summary).
        ColourProfile::SLog3 => {
            if y >= 171.2102946929 / 1023.0 {
                10f64.powf((y * 1023.0 - 420.0) / 261.5) * (0.18 + 0.01) - 0.01
            } else {
                (y * 1023.0 - 95.0) * 0.01125 / (171.2102946929 - 95.0)
            }
        }
        // ARRI LogC3, EI 800, SUP 3.x, linear scene exposure factor.
        ColourProfile::LogC3 => {
            let (cut, a, b, c, d, e, f) = (0.010591, 5.555556, 0.052272, 0.24719, 0.385537, 5.367655, 0.092809);
            if y > e * cut + f {
                (10f64.powf((y - d) / c) - b) / a
            } else {
                (y - f) / e
            }
        }
        // ARRI LogC4 specification.
        ColourProfile::LogC4 => {
            let a = (2f64.powi(18) - 16.0) / 117.45;
            let b = (1023.0 - 95.0) / 1023.0;
            let c = 95.0 / 1023.0;
            let s = (7.0 * 2f64.ln() * 2f64.powf(7.0 - 14.0 * c / b)) / (a * b);
            let t = (2f64.powf(14.0 * (-c / b) + 6.0) - 64.0) / a;
            if y >= 0.0 {
                (2f64.powf(14.0 * ((y - c) / b) + 6.0) - 64.0) / a
            } else {
                y * s + t
            }
        }
        // Panasonic V-Log/V-Gamut reference manual.
        ColourProfile::VLog => {
            let (cut2, b, c, d) = (0.181, 0.00873, 0.241514, 0.598206);
            if y < cut2 {
                (y - 0.125) / 5.6
            } else {
                10f64.powf((y - d) / c) - b
            }
        }
        // Canon Log 2 v1.2 (reflection = linear × 0.9).
        ColourProfile::CLog2 => {
            let v = if y < 0.092864125 {
                -(10f64.powf((0.092864125 - y) / 0.24136077) - 1.0) / 87.09937546
            } else {
                (10f64.powf((y - 0.092864125) / 0.24136077) - 1.0) / 87.09937546
            };
            v * 0.9
        }
        // Canon Log 3 v1.2.
        ColourProfile::CLog3 => {
            let v = if y < 0.097465473 {
                -(10f64.powf((0.12783901 - y) / 0.36726845) - 1.0) / 14.98325
            } else if y <= 0.15277891 {
                (y - 0.12512219) / 1.9754798
            } else {
                (10f64.powf((y - 0.12240537) / 0.36726845) - 1.0) / 14.98325
            };
            v * 0.9
        }
        // Nikon N-Log specification.
        ColourProfile::NLog => {
            let (cut2, a, b, c, d) = (452.0 / 1023.0, 650.0 / 1023.0, 0.0075, 150.0 / 1023.0, 619.0 / 1023.0);
            if y < cut2 {
                (y / a).powi(3) * (y / a).signum().abs() - b
            } else {
                ((y - d) / c).exp()
            }
        }
        // Fujifilm F-Log2 data sheet.
        ColourProfile::FLog2 => {
            let (cut2, a, b, c, d, e, f) = (0.100686685370811, 5.555556, 0.064829, 0.245281, 0.384316, 8.799461, 0.092864);
            if y < cut2 {
                (y - f) / e
            } else {
                10f64.powf((y - d) / c) / a - b / a
            }
        }
        // DJI D-Log (white paper).
        ColourProfile::DLog => {
            if y <= 0.14 {
                (y - 0.0929) / 6.025
            } else {
                (10f64.powf(3.89616 * y - 2.27752) - 0.0108) / 0.9892
            }
        }
        // Apple Log profile white paper.
        ColourProfile::AppleLog => {
            let (r0, rt, sigma, beta, gamma, delta) = (-0.05641088, 0.01, 47.28711236, 0.00964052, 0.08550479, 0.69336945);
            let pt = sigma * (rt - r0) * (rt - r0);
            if y >= pt {
                2f64.powf((y - delta) / gamma) - beta
            } else if y >= 0.0 {
                (y / sigma).sqrt() + r0
            } else {
                r0
            }
        }
        // RED Log3G10 (IPP2 white paper, v3).
        ColourProfile::Log3G10 => {
            let (a, b, c, g) = (0.224282, 155.975327, 0.01, 15.1927);
            if y < 0.0 {
                y / g - c
            } else {
                (10f64.powf(y / a) - 1.0) / b - c
            }
        }
        // Blackmagic Film Generation 5 (Blackmagic Generation 5 Color Science white paper).
        ColourProfile::BmdFilmGen5 => {
            let (a, b, c, d, e, lin_cut) = (0.08692876065491224, 0.005494072432257808, 0.5300133392291939, 8.283605932402494, 0.09246575342465753, 0.005);
            if y < d * lin_cut + e {
                (y - e) / d
            } else {
                ((y - c) / a).exp() - b
            }
        }
    };
    x as f32
}

/// Camera gamut → BT.709 primaries (row-major, applied to linear RGB).
pub fn gamut_to_709(profile: ColourProfile) -> [[f32; 3]; 3] {
    const I: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    // Rec.2020 primaries (N-Gamut, F-Gamut and Apple Log are BT.2020).
    const BT2020: [[f32; 3]; 3] = [[1.660491, -0.587641139, -0.072849863], [-0.124550475, 1.1328999, -0.008349423], [-0.018150763, -0.100578898, 1.11872966]];
    const CINEMA_GAMUT: [[f32; 3]; 3] = [[1.9238613, -0.798760663, -0.125100633], [-0.204310848, 1.49589851, -0.291587662], [-0.023685021, -0.420127011, 1.44381203]];
    match profile {
        ColourProfile::Rec709 => I,
        ColourProfile::SLog3 => [[1.62694741, -0.540138539, -0.086808871], [-0.178515527, 1.41794093, -0.2394254], [-0.044436115, -0.195919966, 1.24035608]],
        ColourProfile::LogC3 => [[1.61752447, -0.53728621, -0.080238147], [-0.070572707, 1.33461286, -0.264040208], [-0.021102047, -0.22695361, 1.24805592]],
        ColourProfile::LogC4 => [[1.89312344, -0.780881504, -0.112241939], [-0.205700358, 1.34025749, -0.134557131], [-0.012705743, -0.152184876, 1.16489062]],
        ColourProfile::VLog => [[1.80657357, -0.695698131, -0.110878568], [-0.170089035, 1.30595477, -0.135864822], [-0.025205692, -0.154467764, 1.17967367]],
        ColourProfile::CLog2 | ColourProfile::CLog3 => CINEMA_GAMUT,
        ColourProfile::NLog | ColourProfile::FLog2 | ColourProfile::AppleLog => BT2020,
        ColourProfile::DLog => [[1.67484185, -0.579967416, -0.094927179], [-0.09812538, 1.33404613, -0.23587725], [-0.04100864, -0.243021857, 1.28375495]],
        ColourProfile::Log3G10 => [[1.98197545, -0.90043307, -0.081545505], [-0.178143136, 1.50046851, -0.322324468], [-0.101796111, -0.535263225, 1.63705955]],
        ColourProfile::BmdFilmGen5 => [[1.5684047, -0.522674385, -0.045730312], [-0.086359008, 1.34491333, -0.258554325], [-0.052047146, -0.249155258, 1.3012024]],
    }
}

/// BT.1886 display EOTF (γ 2.4, zero black) for Rec.709-coded video.
fn bt1886_eotf(v: f64) -> f64 {
    v.max(0.0).powf(2.4)
}

/// ClipPro's preview display rendering: scene-linear → display code value (0–1).
///
/// A filmic shoulder (Narkowicz's ACES fit at 0.6 exposure) followed by the BT.1886 inverse.
/// 18 % grey lands at about 0.44 and 90 % white at about 0.83, close to the vendors' own
/// Rec.709 LUTs, while highlights roll off instead of clipping at 1.0. It's a viewing
/// transform, not a grade.
pub fn display(lin: f32) -> f32 {
    let x = (lin.max(0.0) as f64) * 0.6;
    let t = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
    t.clamp(0.0, 1.0).powf(1.0 / 2.4) as f32
}

/// Limited-range luma fraction ((Y−16)/219, as measured) → normalised code value CV/1023.
#[inline]
pub fn luma_to_code(v: f32) -> f32 {
    (v * 876.0 + 64.0) / 1023.0
}

/// Maps a stored (encoded) luma value to its display-referred Rec.709 value. Rec.709 footage
/// passes through unchanged.
pub fn to_display(profile: ColourProfile, v: f32) -> f32 {
    if !profile.is_log() || v.is_nan() {
        return v;
    }
    display(decode(profile, luma_to_code(v)))
}

/// Per-channel preview LUT for the viewer: `n` entries over normalised code value 0–1, each the
/// scene-linear value (the shader applies `gamut_to_709` and `display` after it).
pub fn preview_lut(profile: ColourProfile, n: usize) -> Vec<f32> {
    let n = n.max(2);
    (0..n).map(|i| decode(profile, i as f32 / (n - 1) as f32)).collect()
}

/// ClipPro's default guess at a clip's profile from camera metadata (tags lower-cased).
pub fn guess_profile(tags: &[(String, String)]) -> Option<ColourProfile> {
    let has = |needle: &str| tags.iter().any(|(k, v)| k.to_ascii_lowercase().contains("gamma") && v.to_ascii_lowercase().replace(['-', ' ', '_'], "").contains(needle) || v.to_ascii_lowercase().replace(['-', ' ', '_'], "") == needle);
    if has("slog3") {
        Some(ColourProfile::SLog3)
    } else if has("vlog") {
        Some(ColourProfile::VLog)
    } else if has("clog3") || has("canonlog3") {
        Some(ColourProfile::CLog3)
    } else if has("clog2") || has("canonlog2") {
        Some(ColourProfile::CLog2)
    } else if has("nlog") {
        Some(ColourProfile::NLog)
    } else if has("flog2") {
        Some(ColourProfile::FLog2)
    } else if has("dlog") {
        Some(ColourProfile::DLog)
    } else if has("applelog") {
        Some(ColourProfile::AppleLog)
    } else if has("logc4") {
        Some(ColourProfile::LogC4)
    } else if has("logc3") || has("logc") {
        Some(ColourProfile::LogC3)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn refs() -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/colour/log_refs.json");
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    const ALL: [ColourProfile; 12] = [
        ColourProfile::SLog3,
        ColourProfile::LogC3,
        ColourProfile::LogC4,
        ColourProfile::VLog,
        ColourProfile::CLog2,
        ColourProfile::CLog3,
        ColourProfile::NLog,
        ColourProfile::FLog2,
        ColourProfile::DLog,
        ColourProfile::AppleLog,
        ColourProfile::Log3G10,
        ColourProfile::BmdFilmGen5,
    ];

    fn key(p: ColourProfile) -> String {
        serde_json::to_value(p).unwrap().as_str().unwrap().to_string()
    }

    /// PLAN P6.5: outputs match the references at 10 points each (±0.002; we hold 1e-4
    /// relative), 18 % grey decodes to 0.18, and matrices match.
    #[test]
    fn curves_match_vendor_references() {
        let r = refs();
        for p in ALL {
            let e = &r["profiles"][key(p)];
            assert!(e.is_object(), "no reference for {}", key(p));
            for pt in e["points"].as_array().unwrap() {
                let (y, want) = (pt[0].as_f64().unwrap() as f32, pt[1].as_f64().unwrap() as f32);
                let got = decode(p, y);
                assert!((got - want).abs() <= 1e-4 * want.abs().max(1.0), "{}: decode({y}) = {got}, want {want}", key(p));
            }
            let grey = decode(p, e["grey_code"].as_f64().unwrap() as f32);
            assert!((grey - 0.18).abs() < 1e-4, "{}: grey decodes to {grey}", key(p));
            let black = decode(p, e["black_code"].as_f64().unwrap() as f32);
            assert!(black.abs() < 1e-4, "{}: black decodes to {black}", key(p));
            let m = gamut_to_709(p);
            for (i, row) in e["to_709"].as_array().unwrap().iter().enumerate() {
                for (j, v) in row.as_array().unwrap().iter().enumerate() {
                    assert!((m[i][j] - v.as_f64().unwrap() as f32).abs() < 1e-6, "{}: matrix[{i}][{j}]", key(p));
                }
            }
            // Rows sum to 1: neutral stays neutral.
            for row in m {
                assert!((row.iter().sum::<f32>() - 1.0).abs() < 2e-3, "{}: row sum {}", key(p), row.iter().sum::<f32>());
            }
        }
    }

    #[test]
    fn display_rendering_anchors() {
        assert!((display(0.18) - 0.44).abs() < 0.01, "grey at {}", display(0.18));
        assert!((display(0.9) - 0.83).abs() < 0.02, "white at {}", display(0.9));
        assert_eq!(display(0.0), 0.0);
        assert!(display(40.0) <= 1.0 && display(40.0) > 0.99);
        // Monotonic.
        let mut last = -1.0;
        for i in 0..400 {
            let d = display(i as f32 * 0.05);
            assert!(d >= last);
            last = d;
        }
        // Log grey through the whole chain lands mid-screen for every profile.
        let r = refs();
        for p in ALL {
            let code = r["profiles"][key(p)]["grey_code"].as_f64().unwrap() as f32;
            let luma = (code * 1023.0 - 64.0) / 876.0;
            let d = to_display(p, luma);
            assert!((d - 0.44).abs() < 0.01, "{}: grey shows at {d}", key(p));
        }
        assert_eq!(to_display(ColourProfile::Rec709, 0.37), 0.37);
    }

    #[test]
    fn profile_guess_from_tags() {
        let t = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
        assert_eq!(guess_profile(&t("com.sony.CaptureGammaEquation", "s-log3-cine")), Some(ColourProfile::SLog3));
        assert_eq!(guess_profile(&t("CaptureGammaEquation", "S-Log3")), Some(ColourProfile::SLog3));
        assert_eq!(guess_profile(&t("gamma", "V-Log")), Some(ColourProfile::VLog));
        assert_eq!(guess_profile(&t("com.apple.proapps.gamma", "Apple Log")), Some(ColourProfile::AppleLog));
        assert_eq!(guess_profile(&t("encoder", "Lavf")), None);
    }
}
