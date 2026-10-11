//! Parser for values typed into sliders (SPEC §14.2, PLAN P0.5).
//!
//! Durations accept: `4`, `4.5`, `1:30`, `01:02:03`, `90s`, `1.5m`, `1h`, `250ms`, `1h30m`,
//! `2m 10s`, `none`, `inf`, `∞`, `unlimited`. A bare number means seconds.
//! Counts accept integers or `none`. Scores accept 0–100 with an optional `%`.
//! The same vectors are checked by the TypeScript parser (`testdata/typed_values.json`).

use std::fmt::Write;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TypedError {
    #[error("empty value")]
    Empty,
    #[error("couldn't read '{0}'")]
    Invalid(String),
    #[error("'{0}' is out of range")]
    OutOfRange(String),
}

fn is_unlimited(s: &str) -> bool {
    matches!(s, "none" | "inf" | "infinity" | "∞" | "unlimited" | "no limit" | "max")
}

fn parse_num(s: &str) -> Option<f64> {
    if s.is_empty() || s.starts_with('+') {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

/// Parses a duration in seconds. `Ok(None)` means unlimited.
pub fn parse_duration(input: &str) -> Result<Option<f64>, TypedError> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        return Err(TypedError::Empty);
    }
    if is_unlimited(&s) {
        return Ok(None);
    }
    let bad = || TypedError::Invalid(input.trim().to_string());
    // Clock form: m:s or h:m:s (seconds may be fractional).
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(bad());
        }
        let mut total = 0.0;
        for (i, p) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            let v = if last {
                parse_num(p).ok_or_else(bad)?
            } else {
                if !p.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad());
                }
                p.parse::<f64>().map_err(|_| bad())?
            };
            if v < 0.0 || (i > 0 && v >= 60.0) {
                return Err(TypedError::OutOfRange(input.trim().to_string()));
            }
            total = total * 60.0 + v;
        }
        return Ok(Some(total));
    }
    // Bare number → seconds.
    if let Some(v) = parse_num(&s) {
        if v < 0.0 {
            return Err(TypedError::OutOfRange(input.trim().to_string()));
        }
        return Ok(Some(v));
    }
    // Unit form, possibly compound: "1h30m", "2m 10s", "250ms".
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = compact.as_bytes();
    let mut i = 0;
    let mut total = 0.0;
    let mut any = false;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        if start == i {
            return Err(bad());
        }
        let v: f64 = compact[start..i].parse().map_err(|_| bad())?;
        let ustart = i;
        while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
            i += 1;
        }
        let mult = match &compact[ustart..i] {
            "ms" => 0.001,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
            "f" | "fr" => return Err(bad()), // frames need a rate; not accepted here
            _ => return Err(bad()),
        };
        total += v * mult;
        any = true;
    }
    if !any {
        return Err(bad());
    }
    Ok(Some(total))
}

/// Parses a count. `Ok(None)` means unlimited.
pub fn parse_count(input: &str) -> Result<Option<u32>, TypedError> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        return Err(TypedError::Empty);
    }
    if is_unlimited(&s) {
        return Ok(None);
    }
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(TypedError::Invalid(input.trim().to_string()));
    }
    s.parse::<u32>().map(Some).map_err(|_| TypedError::OutOfRange(input.trim().to_string()))
}

/// Parses a score typed as 0–100 (optional `%`). Returns a fraction in [0, 1].
pub fn parse_score(input: &str) -> Result<f64, TypedError> {
    let s = input.trim().trim_end_matches('%').trim();
    if s.is_empty() {
        return Err(TypedError::Empty);
    }
    let v = parse_num(s).ok_or_else(|| TypedError::Invalid(input.trim().to_string()))?;
    if !(0.0..=100.0).contains(&v) {
        return Err(TypedError::OutOfRange(input.trim().to_string()));
    }
    Ok(v / 100.0)
}

/// Formats seconds for display in sliders: `0.25s`, `4s`, `1:30`, `1:02:03`, `none`.
pub fn format_duration(secs: Option<f64>) -> String {
    let Some(s) = secs else { return "none".into() };
    if s < 60.0 {
        let mut out = String::new();
        let r = (s * 100.0).round() / 100.0;
        if (r - r.round()).abs() < 1e-9 {
            let _ = write!(out, "{}s", r.round() as i64);
        } else {
            let _ = write!(out, "{}s", r);
        }
        return out;
    }
    let total = s.round() as i64;
    let (h, m, sec) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        duration: Vec<(String, Option<f64>)>,
        duration_invalid: Vec<String>,
        count: Vec<(String, Option<u32>)>,
        count_invalid: Vec<String>,
        score: Vec<(String, f64)>,
        score_invalid: Vec<String>,
    }

    fn vectors() -> Vectors {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/typed_values.json");
        let text = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn shared_vectors() {
        let v = vectors();
        for (input, want) in v.duration {
            let got = parse_duration(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
            match (got, want) {
                (None, None) => {}
                (Some(g), Some(w)) => assert!((g - w).abs() < 1e-9, "{input}: {g} != {w}"),
                _ => panic!("{input}: {got:?} != {want:?}"),
            }
        }
        for input in v.duration_invalid {
            assert!(parse_duration(&input).is_err(), "{input} should be invalid");
        }
        for (input, want) in v.count {
            assert_eq!(parse_count(&input).unwrap(), want, "{input}");
        }
        for input in v.count_invalid {
            assert!(parse_count(&input).is_err(), "{input} should be invalid");
        }
        for (input, want) in v.score {
            assert!((parse_score(&input).unwrap() - want).abs() < 1e-9, "{input}");
        }
        for input in v.score_invalid {
            assert!(parse_score(&input).is_err(), "{input} should be invalid");
        }
    }

    #[test]
    fn format() {
        assert_eq!(format_duration(None), "none");
        assert_eq!(format_duration(Some(4.0)), "4s");
        assert_eq!(format_duration(Some(0.25)), "0.25s");
        assert_eq!(format_duration(Some(90.0)), "1:30");
        assert_eq!(format_duration(Some(3723.0)), "1:02:03");
    }
}
