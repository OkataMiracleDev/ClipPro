//! FFmpeg discovery and raw-frame piping.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub struct Ffmpeg {
    pub exe: PathBuf,
    pub encoders: String,
}

impl Ffmpeg {
    /// Prefers `CLIPPRO_FFMPEG_DEV`, then any ffmpeg on PATH that has libx264, then the first found.
    pub fn find() -> anyhow::Result<Ffmpeg> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(p) = std::env::var("CLIPPRO_FFMPEG_DEV") {
            candidates.push(PathBuf::from(p));
        }
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                let exe = dir.join("ffmpeg.exe");
                if exe.exists() {
                    candidates.push(exe);
                }
            }
        }
        if let Some(home) = std::env::var_os("LOCALAPPDATA") {
            let p = PathBuf::from(home).join("Programs").join("ffmpeg").join("bin").join("ffmpeg.exe");
            if p.exists() {
                candidates.push(p);
            }
        }
        let mut fallback = None;
        for exe in candidates {
            let Ok(out) = Command::new(&exe).args(["-hide_banner", "-encoders"]).output() else { continue };
            let encoders = String::from_utf8_lossy(&out.stdout).into_owned();
            if encoders.contains("libx264") {
                return Ok(Ffmpeg { exe, encoders });
            }
            if fallback.is_none() {
                fallback = Some(Ffmpeg { exe, encoders });
            }
        }
        fallback.ok_or_else(|| anyhow::anyhow!("ffmpeg not found (set CLIPPRO_FFMPEG_DEV)"))
    }

    pub fn has(&self, enc: &str) -> bool {
        self.encoders.split_whitespace().any(|w| w == enc)
    }

    /// Default H.264 encoder arguments.
    pub fn h264(&self) -> Vec<String> {
        if self.has("libx264") {
            s(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-g", "50", "-bf", "2", "-pix_fmt", "yuv420p"])
        } else if self.has("libopenh264") {
            s(&["-c:v", "libopenh264", "-b:v", "20M", "-g", "50", "-pix_fmt", "yuv420p"])
        } else {
            s(&["-c:v", "h264_mf", "-b:v", "20M", "-pix_fmt", "yuv420p"])
        }
    }
}

pub fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// A running encoder reading raw frames from stdin.
pub struct Pipe {
    child: Child,
}

impl Pipe {
    /// `pix_in` is `yuv444p` or `yuv444p16le`. `extra_inputs` are added before the output args.
    pub fn start(
        ff: &Ffmpeg,
        w: usize,
        h: usize,
        fps: &str,
        pix_in: &str,
        extra_inputs: &[String],
        out_args: &[String],
        out: &Path,
    ) -> anyhow::Result<Pipe> {
        let mut args = s(&["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", pix_in]);
        args.extend(s(&["-s", &format!("{w}x{h}"), "-r", fps, "-i", "-"]));
        args.extend(extra_inputs.iter().cloned());
        args.extend(out_args.iter().cloned());
        args.push(out.to_string_lossy().into_owned());
        let child = Command::new(&ff.exe).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).spawn()?;
        Ok(Pipe { child })
    }

    pub fn write(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.child.stdin.as_mut().ok_or_else(|| anyhow::anyhow!("encoder stdin closed"))?.write_all(bytes)?;
        Ok(())
    }

    pub fn finish(mut self) -> anyhow::Result<()> {
        drop(self.child.stdin.take());
        let st = self.child.wait()?;
        anyhow::ensure!(st.success(), "ffmpeg exited with {st}");
        Ok(())
    }
}

/// Writes 16-bit PCM WAV.
pub fn write_wav(path: &Path, rate: u32, channels: &[Vec<f32>]) -> anyhow::Result<()> {
    let n = channels.first().map(|c| c.len()).unwrap_or(0);
    let ch = channels.len() as u16;
    let data_len = n as u32 * ch as u32 * 2;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&ch.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(ch * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        for c in channels {
            let v = (c[i].clamp(-1.0, 1.0) * 32767.0).round() as i16;
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, b)?;
    Ok(())
}
