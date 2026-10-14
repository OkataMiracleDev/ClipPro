//! Times analysis of each given clip (dev diagnostic): `cargo run --release --example timeclip -- FILES…`
use clippro_analysis::clip::{analyze_clip, AnalyzeOptions};
use clippro_measure::calibration::Calibration;
use std::sync::atomic::AtomicBool;

fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        let info = match clippro_media::probe(path) {
            Ok(i) => i,
            Err(e) => {
                println!("{p}: {e}");
                continue;
            }
        };
        let t = std::time::Instant::now();
        let r = analyze_clip(path, &info, &AnalyzeOptions::default(), 7, Calibration::embedded(), &AtomicBool::new(false), |_, _| {});
        let secs = t.elapsed().as_secs_f32();
        println!(
            "{:<22} {:>7.2}s {:>6.2}x realtime  {}",
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            secs,
            info.duration_us as f32 / 1e6 / secs,
            r.as_ref().map(|m| format!("{} samples", m.len())).unwrap_or_else(|e| e.to_string())
        );
        if let Ok(m) = &r {
            use clippro_measure::columns::Col;
            let med = |c: Col| clippro_measure::util::median(m.col(c));
            println!(
                "    row_band {:.3} flicker {:.3} stab {:.3} stab_conf {:.2} zr {:.3} wr {:.3} subj {:.4} cam {:.4} mad_pair {:.5} noise {:.4}",
                med(Col::RowBand), med(Col::FlickerRaw), med(Col::StabRaw), med(Col::StabConf), med(Col::CamZr),
                med(Col::CamWr), med(Col::SubjRaw), med(Col::CamMag), med(Col::MadPair), med(Col::NoiseSigma)
            );
            if let Ok(key) = std::env::var("TIMECLIP_COL") {
                if let Some(c) = Col::from_key(&key) {
                    let v: Vec<String> = (0..m.len()).map(|k| format!("{:.2}:{:.5}", m.time_us[k] as f64 / 1e6, m.get(c, k))).collect();
                    println!("    {key}: {}", v.join(" "));
                }
            }
        }
    }
}
