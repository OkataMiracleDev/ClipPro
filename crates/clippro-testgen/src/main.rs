//! Synthetic test-footage generator with ground truth (PLAN P0.4).
//!
//! `clippro-testgen --out testdata/gen [--mini] [--only a,b,c]`
//! Writes media into `--out` and truth JSON into `testdata/truth` (or `--truth DIR`).

mod audio;
mod clips;
mod encode;
use clippro_testgen::scene;

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut out = PathBuf::from("testdata/gen");
    let mut truth = None;
    let mut mini = false;
    let mut only = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("--out needs a value"))?),
            "--truth" => truth = Some(PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("--truth needs a value"))?)),
            "--mini" => mini = true,
            "--only" => only = Some(args.next().unwrap_or_default().split(',').map(String::from).collect()),
            "-h" | "--help" => {
                println!("clippro-testgen --out DIR [--truth DIR] [--mini] [--only name,name]");
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let truth_dir = truth.unwrap_or_else(|| {
        let mut t = out.clone();
        t.pop();
        t.join("truth")
    });
    clips::ensure_dir(&out)?;
    clips::ensure_dir(&truth_dir)?;
    let ff = encode::Ffmpeg::find()?;
    eprintln!("ffmpeg: {}", ff.exe.display());
    let (w, h) = if mini { (854, 480) } else { (1920, 1080) };
    let started = std::time::Instant::now();
    eprintln!("textures…");
    let tex = scene::Texture::new(2048, 11);
    let tex2 = scene::Texture::new(2048, 77);
    let g = clips::Gen { ff, out, truth_dir, w, h, mini, tex, tex2, only, written: Default::default(), skipped: Default::default() };
    g.all()?;
    eprintln!("{} files in {:.1}s", g.written.borrow().len(), started.elapsed().as_secs_f32());
    for (n, why) in g.skipped.borrow().iter() {
        eprintln!("skipped {n}: {why}");
    }
    Ok(())
}
