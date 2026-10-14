//! Corpus clip definitions with ground truth (PLAN P0.4).

use crate::audio;
use crate::encode::{s, write_wav, Ffmpeg, Pipe};
use crate::scene::{rgb_to_ycbcr, Frame, Pose, Texture};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FPS: f32 = 25.0;
const TEX_CENTRE: f32 = 1024.0;

pub struct Gen {
    pub ff: Ffmpeg,
    pub out: PathBuf,
    pub truth_dir: PathBuf,
    pub w: usize,
    pub h: usize,
    pub mini: bool,
    pub tex: Texture,
    pub tex2: Texture,
    pub only: Option<Vec<String>>,
    pub written: RefCell<Vec<String>>,
    pub skipped: RefCell<Vec<(String, String)>>,
}

/// Encoder families used by the corpus.
#[derive(Clone, Copy, PartialEq)]
pub enum Enc {
    H264,
    ProResHq,
    DnxHr,
    Hevc10,
    H264_422_10,
    H264Vfr,
}

type Render<'a> = Box<dyn Fn(usize) -> Frame + 'a>;

impl Gen {
    fn scale(&self) -> f32 {
        1920.0 / self.w as f32
    }

    /// Texture px per frame width at zoom 1.
    fn fw(&self) -> f32 {
        1920.0
    }

    fn static_pose(&self) -> Pose {
        Pose { cx: TEX_CENTRE, cy: TEX_CENTRE, zoom: 1.0, rot: 0.0 }
    }

    fn render(&self, pose: Pose) -> Frame {
        Frame::render(&self.tex, self.w, self.h, pose, self.scale())
    }

    fn wants(&self, name: &str) -> bool {
        self.only.as_ref().map(|o| o.iter().any(|x| x == name)).unwrap_or(true)
    }

    fn write_truth(&self, name: &str, file: &str, dur: f32, mut truth: Value) -> anyhow::Result<()> {
        truth["name"] = json!(name);
        truth["file"] = json!(file);
        truth["fps"] = json!(FPS);
        truth["duration_s"] = json!(dur);
        truth["width"] = json!(self.w);
        truth["height"] = json!(self.h);
        if truth.get("intervals").is_none() {
            truth["intervals"] = json!([]);
        }
        let path = self.truth_dir.join(format!("{name}.json"));
        std::fs::write(path, serde_json::to_string_pretty(&truth)? + "\n")?;
        Ok(())
    }

    /// Renders `dur` seconds through `render` and encodes with `enc` (+ optional WAV audio).
    fn clip(&self, name: &str, dur: f32, enc: Enc, audio: Option<Vec<Vec<f32>>>, render: Render, truth: Value) -> anyhow::Result<()> {
        if !self.wants(name) {
            return Ok(());
        }
        let (ext, mut args, pix_in, size) = match enc {
            Enc::H264 => ("mp4", self.ff.h264(), "yuv444p", (self.w, self.h)),
            Enc::H264Vfr => {
                let mut a = s(&["-vf", "setpts='(N+0.35*sin(N*1.7))/25/TB'", "-fps_mode", "vfr"]);
                a.extend(self.ff.h264());
                ("mkv", a, "yuv444p", (self.w, self.h))
            }
            Enc::ProResHq => (
                "mov",
                s(&["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le", "-timecode", "01:00:00:00", "-metadata:s:v:0", "reel_name=A001C003"]),
                "yuv444p16le",
                (self.w, self.h),
            ),
            Enc::DnxHr => (
                "mxf",
                s(&["-c:v", "dnxhd", "-profile:v", "dnxhr_lb", "-pix_fmt", "yuv422p"]),
                "yuv444p",
                (1920, 1080),
            ),
            Enc::Hevc10 => {
                if !self.ff.has("libx265") {
                    self.skipped.borrow_mut().push((name.into(), "libx265 not available".into()));
                    return Ok(());
                }
                ("mp4", s(&["-c:v", "libx265", "-preset", "fast", "-crf", "18", "-pix_fmt", "yuv420p10le", "-x265-params", "log-level=error"]), "yuv444p16le", (self.w, self.h))
            }
            Enc::H264_422_10 => {
                if !self.ff.has("libx264") {
                    self.skipped.borrow_mut().push((name.into(), "libx264 not available".into()));
                    return Ok(());
                }
                ("mov", s(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-profile:v", "high422", "-pix_fmt", "yuv422p10le"]), "yuv444p16le", (self.w, self.h))
            }
        };
        let file = format!("{name}.{ext}");
        let out = self.out.join(&file);
        let mut inputs = Vec::new();
        let wav = self.out.join(format!("{name}.tmp.wav"));
        if let Some(a) = &audio {
            write_wav(&wav, 48_000, a)?;
            inputs = vec!["-i".into(), wav.to_string_lossy().into_owned()];
            let acodec = if ext == "mp4" { s(&["-c:a", "aac", "-b:a", "256k"]) } else { s(&["-c:a", "pcm_s16le"]) };
            args.extend(acodec);
            args.extend(s(&["-map", "0:v", "-map", "1:a", "-shortest"]));
        }
        if ext == "mp4" {
            args.extend(s(&["-movflags", "+faststart"]));
        }
        let nframes = (dur * FPS).round() as usize;
        let mut pipe = Pipe::start(&self.ff, size.0, size.1, "25", pix_in, &inputs, &args, &out)?;
        for n in 0..nframes {
            let mut f = render(n);
            if (f.w, f.h) != size {
                f = resize_nearest(&f, size.0, size.1);
            }
            let bytes = if pix_in == "yuv444p16le" { f.to_yuv444p16() } else { f.to_yuv444p() };
            pipe.write(&bytes)?;
        }
        pipe.finish()?;
        if audio.is_some() {
            let _ = std::fs::remove_file(&wav);
        }
        self.write_truth(name, &file, dur, truth)?;
        self.written.borrow_mut().push(file);
        eprintln!("  {name}");
        Ok(())
    }

    pub fn all(&self) -> anyhow::Result<()> {
        let (w, h) = (self.w, self.h);
        let ksig = w as f32 / 1920.0; // blur sigma scale so mini and full look alike
        let noise = |f: &mut Frame, n: usize, sigma: f32| f.add_noise(sigma, 0xC11F + n as u64 * 7919);

        // --- Focus ---
        let base = self.render(self.static_pose());
        let steps = [0.0f32, 0.7, 1.2, 1.8, 2.6, 3.6, 5.0];
        let blurred: Vec<Frame> = steps.iter().map(|s| base.blur(s * ksig)).collect();
        let intervals: Vec<Value> =
            (0..7).map(|i| json!({"t": [i as f32 + 0.2, i as f32 + 0.8], "expect": {"sharp_rank": i}})).collect();
        self.clip("blur_ramp", 7.0, Enc::H264, None, Box::new(|n| {
            let mut f = blurred[(n / 25).min(6)].clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"intervals": intervals, "metric": "sharp_full", "monotonic": "decreasing"}))?;

        let cache: Mutex<HashMap<u32, Frame>> = Mutex::new(HashMap::new());
        let base_ref = &base;
        self.clip("focus_hunt", 8.0, Enc::H264, None, Box::new(|n| {
            let t = n as f32 / FPS;
            let sigma = if (2.0..6.0).contains(&t) { 1.5 - 1.5 * (std::f32::consts::TAU * 1.5 * (t - 2.0)).cos() } else { 0.0 };
            let q = (sigma * 4.0).round() as u32;
            let mut f = {
                #[allow(clippy::unwrap_used)]
                let mut c = cache.lock().unwrap();
                c.entry(q).or_insert_with(|| base_ref.blur(q as f32 / 4.0 * ksig)).clone()
            };
            noise(&mut f, n, 0.004);
            f
        }), json!({"intervals": [
            {"t": [2.6, 5.4], "expect": {"flags": ["focus_hunting"]}},
            {"t": [0.2, 1.4], "expect": {"no_flags": ["focus_hunting"]}},
            {"t": [6.6, 7.8], "expect": {"no_flags": ["focus_hunting"]}}
        ]}))?;

        let soft = base.blur(6.0 * ksig);
        let bokeh = {
            let mut f = soft.clone();
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = (x as f32 / w as f32 - 0.5, y as f32 / h as f32 - 0.5);
                    if dx.abs() < 0.15 && dy.abs() < 0.15 {
                        let i = y * w + x;
                        f.y[i] = base.y[i];
                    }
                }
            }
            f
        };
        self.clip("bokeh_subject", 4.0, Enc::H264, None, Box::new(|n| {
            let mut f = bokeh.clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"intervals": [{"t": [0.2, 3.8], "expect": {"centre_ge_full": true}}]}))?;

        // --- Exposure ---
        self.clip("expo_ramp", 7.0, Enc::H264, None, Box::new(|n| {
            let stops = -4.0 + 7.0 * n as f32 / (7.0 * FPS - 1.0);
            let g = 2f32.powf(stops);
            let mut f = base_ref.clone();
            f.map_luma(|_, _, v| (v * g).min(1.0));
            noise(&mut f, n, 0.003);
            f
        }), json!({"metric": "p50", "monotonic": "increasing", "intervals": [
            {"t": [6.2, 6.9], "expect": {"flags": ["clipping"]}},
            {"t": [2.5, 4.0], "expect": {"no_flags": ["clipping"]}}
        ]}))?;

        for (name, val) in [("clip_white", 1.0f32), ("clip_black", 0.0f32)] {
            self.clip(name, 6.0, Enc::H264, None, Box::new(move |n| {
                let t = n as f32 / FPS;
                let frac = if t < 2.0 { 0.03 } else if t < 4.0 { 0.01 } else { 0.005 };
                // A centred square block covering `frac` of the frame.
                let side = ((frac * (w * h) as f32).sqrt()) as usize;
                let (x0, y0) = (w / 2 - side / 2, h / 2 - side / 2);
                let mut f = base_ref.clone();
                // Keep the rest of the frame away from the clip points.
                f.map_luma(|x, y, v| {
                    if x >= x0 && x < x0 + side && y >= y0 && y < y0 + side {
                        val
                    } else {
                        0.08 + 0.84 * v
                    }
                });
                noise(&mut f, n, 0.002);
                if val > 0.5 {
                    for y in y0..y0 + side {
                        for x in x0..x0 + side {
                            f.y[y * w + x] = 1.0;
                        }
                    }
                } else {
                    for y in y0..y0 + side {
                        for x in x0..x0 + side {
                            f.y[y * w + x] = 0.0;
                        }
                    }
                }
                f
            }), json!({"intervals": [
                {"t": [0.2, 1.8], "expect": {"flags": ["clipping"]}},
                {"t": [2.2, 5.8], "expect": {"no_flags": ["clipping"]}}
            ]}))?;
        }

        self.clip("low_light", 4.0, Enc::H264, None, Box::new(|n| {
            let mut f = base_ref.clone();
            f.map_luma(|_, _, v| v / 32.0);
            noise(&mut f, n, 0.02);
            f
        }), json!({"intervals": [{"t": [0.2, 3.8], "expect": {"low_light": true, "noise_min": 0.01}}]}))?;

        // --- Leaders ---
        self.clip("solid_black", 3.0, Enc::H264, None, Box::new(move |_| Frame::solid(w, h, 0.0, 0.5, 0.5)),
            json!({"intervals": [{"t": [0.2, 2.8], "expect": {"flags": ["solid_colour"], "no_select": true}}]}))?;
        self.clip("solid_grey", 3.0, Enc::H264, None, Box::new(move |_| Frame::solid(w, h, 0.5, 0.5, 0.5)),
            json!({"intervals": [{"t": [0.2, 2.8], "expect": {"flags": ["solid_colour"], "no_select": true}}]}))?;
        let bars = {
            let cols: [(f32, f32, f32); 7] = [(0.75, 0.75, 0.75), (0.75, 0.75, 0.0), (0.0, 0.75, 0.75), (0.0, 0.75, 0.0), (0.75, 0.0, 0.75), (0.75, 0.0, 0.0), (0.0, 0.0, 0.75)];
            let mut f = Frame::solid(w, h, 0.0, 0.5, 0.5);
            for y in 0..h {
                for x in 0..w {
                    let (r, g, b) = if y < h * 2 / 3 { cols[(x * 7 / w).min(6)] } else { (0.1, 0.1, 0.1) };
                    let (yy, cb, cr) = rgb_to_ycbcr(r, g, b);
                    let i = y * w + x;
                    f.y[i] = yy;
                    f.cb[i] = cb;
                    f.cr[i] = cr;
                }
            }
            f
        };
        self.clip("bars_smpte", 3.0, Enc::H264, None, Box::new(|_| bars.clone()),
            json!({"intervals": [{"t": [0.2, 2.8], "expect": {"flags": ["colour_bars"]}}]}))?;

        // --- Cuts ---
        let scenes: Vec<Frame> = (0..4)
            .map(|i| {
                let tex = if i % 2 == 0 { &self.tex } else { &self.tex2 };
                let mut f = Frame::render(tex, w, h, Pose { cx: 300.0 + 700.0 * i as f32, cy: 1700.0 - 500.0 * i as f32, zoom: 1.0 + 0.3 * i as f32, rot: 0.0 }, self.scale());
                let g = [1.0, 0.6, 1.3, 0.8][i];
                f.map_luma(|_, _, v| (v * g).min(0.98));
                f
            })
            .collect();
        self.clip("hard_cuts", 6.0, Enc::H264, None, Box::new(|n| {
            let t = n as f32 / FPS;
            let i = if t < 1.5 { 0 } else if t < 3.0 { 1 } else if t < 4.5 { 2 } else { 3 };
            let mut f = scenes[i].clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"cut_times": [1.5, 3.0, 4.5]}))?;

        // --- Camera moves ---
        let fw = self.fw();
        let speeds = [0.1f32, 0.3, 1.0];
        let pos_at = move |t: f32| -> f32 {
            // integral of piecewise speed (fw/s) over 2 s segments
            let mut d = 0.0;
            for (i, sp) in speeds.iter().enumerate() {
                let a = 2.0 * i as f32;
                if t > a {
                    d += sp * (t - a).min(2.0);
                }
            }
            d * fw
        };
        for (name, dx, dy, dir) in [("pan_right", 1.0f32, 0.0f32, 0.0f32), ("pan_left", -1.0, 0.0, 180.0), ("tilt_up", 0.0, -1.0, 90.0), ("tilt_down", 0.0, 1.0, 270.0)] {
            let ints: Vec<Value> = speeds
                .iter()
                .enumerate()
                .map(|(i, sp)| json!({"t": [2.0 * i as f32 + 0.25, 2.0 * i as f32 + 1.75], "expect": {"cam_dir_deg": dir, "cam_speed_fw_s": sp}}))
                .collect();
            let sp = self.static_pose();
            self.clip(name, 6.0, Enc::H264, None, Box::new(move |n| {
                let d = pos_at(n as f32 / FPS);
                let mut f = Frame::render(&self.tex, w, h, Pose { cx: sp.cx + dx * d, cy: sp.cy + dy * d, ..sp }, self.scale());
                noise(&mut f, n, 0.003);
                f
            }), json!({"intervals": ints}))?;
        }

        let sp = self.static_pose();
        self.clip("whip_pan", 4.0, Enc::H264, None, Box::new(move |n| {
            let t = n as f32 / FPS;
            let x_at = |t: f32| if t < 1.0 { 0.0 } else if t < 2.0 { 3.0 * (t - 1.0) * fw } else { 3.0 * fw };
            let sub: Vec<Frame> = (0..8)
                .map(|k| {
                    let tt = t + k as f32 / 8.0 * 0.5 / FPS;
                    Frame::render(&self.tex, w, h, Pose { cx: sp.cx + x_at(tt), ..sp }, self.scale())
                })
                .collect();
            let mut f = Frame::average(&sub);
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [
            {"t": [1.2, 1.8], "expect": {"flags": ["motion_blur"], "cam_speed_min": 2.0}},
            {"t": [2.5, 3.8], "expect": {"no_flags": ["motion_blur"]}}
        ]}))?;

        for (name, zr) in [("zoom_in", 0.3f32), ("zoom_out", -0.3f32)] {
            self.clip(name, 4.0, Enc::H264, None, Box::new(move |n| {
                let t = n as f32 / FPS;
                let mut f = Frame::render(&self.tex, w, h, Pose { zoom: (zr * t).exp() * if zr < 0.0 { 1.4 } else { 1.0 }, ..sp }, self.scale());
                noise(&mut f, n, 0.003);
                f
            }), json!({"intervals": [{"t": [0.3, 3.7], "expect": {"zr": zr}}]}))?;
        }
        for (name, wr) in [("rotate_ccw", 10f32.to_radians()), ("rotate_cw", -(10f32.to_radians()))] {
            self.clip(name, 4.0, Enc::H264, None, Box::new(move |n| {
                let t = n as f32 / FPS;
                // Content rotation in y-down coordinates is the negative of the CCW screen rate.
                let mut f = Frame::render(&self.tex, w, h, Pose { rot: -wr * t, zoom: 1.2, ..sp }, self.scale());
                noise(&mut f, n, 0.003);
                f
            }), json!({"intervals": [{"t": [0.3, 3.7], "expect": {"wr": wr}}]}))?;
        }

        // --- Stability ---
        let static_frame = self.render(sp);
        let sf = &static_frame;
        self.clip("tripod_static", 6.0, Enc::H264, None, Box::new(|n| {
            let mut f = sf.clone();
            noise(&mut f, n, 0.006);
            f
        }), json!({"intervals": [{"t": [0.5, 5.5], "expect": {"stab_min": 0.9, "no_flags": ["duplicate", "shaky"]}}]}))?;

        let jitter = shake_offsets(6.0, 0.10 * fw, 7);
        self.clip("handheld_shake", 6.0, Enc::H264, None, Box::new(|n| {
            let (ox, oy, r) = jitter[n];
            let mut f = Frame::render(&self.tex, w, h, Pose { cx: sp.cx + ox, cy: sp.cy + oy, rot: r, zoom: 1.1 }, self.scale());
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [{"t": [0.6, 5.4], "expect": {"flags": ["shaky"], "stab_max": 0.35}}]}))?;

        self.clip("gimbal_smooth", 6.0, Enc::H264, None, Box::new(move |n| {
            let t = n as f32 / FPS;
            let a = 0.1 * fw / (std::f32::consts::TAU * 0.2);
            let mut f = Frame::render(&self.tex, w, h, Pose { cx: sp.cx + a * (std::f32::consts::TAU * 0.2 * t).sin(), ..sp }, self.scale());
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [{"t": [0.6, 5.4], "expect": {"stab_min": 0.8, "no_flags": ["shaky"]}}]}))?;

        let sprite_r = 0.15 * h as f32;
        self.clip("subject_motion", 6.0, Enc::H264, None, Box::new(move |n| {
            let t = n as f32 / FPS;
            let mut f = sf.clone();
            let cx = 0.2 * w as f32 + 0.3 * w as f32 * t / 6.0 * 2.0;
            let cy = 0.5 * h as f32;
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                    if dx * dx + dy * dy < sprite_r * sprite_r {
                        let u = ((x as f32 - cx + 2000.0) * 0.21).sin() * ((y as f32) * 0.17).cos();
                        f.y[y * w + x] = 0.5 + 0.35 * u;
                    }
                }
            }
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [{"t": [0.5, 5.5], "expect": {"subj_gt_cam": 3.0}}]}))?;

        self.clip("freeze_frames", 6.0, Enc::H264, None, Box::new(|n| {
            let t = n as f32 / FPS;
            let held = (2.0..4.0).contains(&t);
            let mut f = sf.clone();
            noise(&mut f, if held { 50 } else { n }, 0.006);
            f
        }), json!({"intervals": [
            {"t": [2.3, 3.7], "expect": {"flags": ["duplicate"]}},
            {"t": [0.3, 1.7], "expect": {"no_flags": ["duplicate"]}},
            {"t": [4.3, 5.7], "expect": {"no_flags": ["duplicate"]}}
        ]}))?;

        self.clip("flicker_pulse", 4.0, Enc::H264, None, Box::new(|n| {
            let g = if n % 2 == 0 { 1.10 } else { 0.90 };
            let mut f = sf.clone();
            f.map_luma(|_, _, v| (v * g).min(1.0));
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [{"t": [0.5, 3.5], "expect": {"flicker_min": 0.5}}]}))?;
        self.clip("flicker_band", 4.0, Enc::H264, None, Box::new(move |n| {
            let t = n as f32 / FPS;
            let mut f = sf.clone();
            f.map_luma(|_, y, v| (v * (1.0 + 0.08 * (std::f32::consts::TAU * (6.0 * y as f32 / h as f32 + 2.0 * t)).sin())).min(1.0));
            noise(&mut f, n, 0.003);
            f
        }), json!({"intervals": [{"t": [0.5, 3.5], "expect": {"flicker_min": 0.5, "row_band_min": 0.15}}]}))?;

        let bar = ((h as f32 - w as f32 / 2.39) / 2.0).round() as usize;
        self.clip("letterbox", 4.0, Enc::H264, None, Box::new(move |n| {
            let mut f = sf.clone();
            noise(&mut f, n, 0.003);
            for y in 0..h {
                if y < bar || y >= h - bar {
                    for x in 0..w {
                        f.y[y * w + x] = 0.0;
                        f.cb[y * w + x] = 0.5;
                        f.cr[y * w + x] = 0.5;
                    }
                }
            }
            f
        }), json!({"active_rel": [0.0, bar as f32 / h as f32, 1.0, (h - 2 * bar) as f32 / h as f32]}))?;

        self.clip("vfr", 5.0, Enc::H264Vfr, None, Box::new(move |n| {
            let mut f = Frame::render(&self.tex, w, h, Pose { cx: sp.cx + 0.2 * fw * n as f32 / FPS, ..sp }, self.scale());
            noise(&mut f, n, 0.003);
            f
        }), json!({"vfr": true}))?;

        // --- Codec smoke clips ---
        let slow = move |n: usize| Frame::render(&self.tex, w, h, Pose { cx: sp.cx + 0.1 * fw * n as f32 / FPS, ..sp }, self.scale());
        self.clip("prores_hq", 2.0, Enc::ProResHq, None, Box::new(slow), json!({"codec": "prores", "timecode": "01:00:00:00", "reel": "A001C003"}))?;
        let slow_hd = move |n: usize| Frame::render(&self.tex, 1920, 1080, Pose { cx: sp.cx + 0.1 * fw * n as f32 / FPS, ..sp }, 1.0);
        self.clip("dnxhr", 1.0, Enc::DnxHr, None, Box::new(slow_hd), json!({"codec": "dnxhd"}))?;
        self.clip("hevc10", 2.0, Enc::Hevc10, None, Box::new(slow), json!({"codec": "hevc", "bit_depth": 10}))?;
        self.clip("h264_422_10", 2.0, Enc::H264_422_10, None, Box::new(slow), json!({"codec": "h264", "bit_depth": 10, "chroma": "4:2:2"}))?;

        // --- Audio ---
        let (amix, atruth) = audio::mix(10.0);
        self.clip("audio_mix", 10.0, Enc::H264, Some(amix), Box::new(|n| {
            let mut f = sf.clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"audio": atruth}))?;
        let offset = 3.48f32;
        let master = audio::sync_signal(8.0 + offset + 1.0);
        let cut = |start: f32| -> Vec<Vec<f32>> {
            let a = (start * 48_000.0) as usize;
            let b = a + 8 * 48_000;
            vec![master[a..b].to_vec(), master[a..b].to_vec()]
        };
        let s1 = &scenes[1];
        let s2 = &scenes[2];
        self.clip("multicam_a", 8.0, Enc::H264, Some(cut(0.0)), Box::new(|n| {
            let mut f = s1.clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"sync_group": "multicam", "audio_start_in_master_s": 0.0}))?;
        self.clip("multicam_b", 8.0, Enc::H264, Some(cut(offset)), Box::new(|n| {
            let mut f = s2.clone();
            noise(&mut f, n, 0.004);
            f
        }), json!({"sync_group": "multicam", "audio_start_in_master_s": offset, "sync_offset_s": offset}))?;

        // --- Takes ---
        for (i, amp) in [(1usize, 0.06f32), (2, 0.0), (3, 0.12)] {
            let jit = shake_offsets(5.0, amp * fw, 100 + i as u64);
            let name = format!("takes_{i}");
            self.clip(&name, 5.0, Enc::H264, None, Box::new(move |n| {
                let (ox, oy, r) = jit[n];
                let mut f = Frame::render(&self.tex, w, h, Pose { cx: sp.cx + ox, cy: sp.cy + oy, rot: r, zoom: 1.1 }, self.scale());
                noise(&mut f, n, 0.004);
                f
            }), json!({"take_group": "takes", "best_take": "takes_2"}))?;
        }

        // --- Long clip (cheap lavfi content) ---
        if self.wants("long_clip") {
            let secs = if self.mini { 120 } else { 720 };
            let file = "long_clip.mp4";
            let mut args = s(&["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"]);
            args.push(format!("testsrc2=s=1280x720:r=25:d={secs}"));
            if self.ff.has("libx264") {
                args.extend(s(&["-c:v", "libx264", "-preset", "ultrafast", "-crf", "28", "-g", "50", "-pix_fmt", "yuv420p"]));
            } else {
                args.extend(self.ff.h264());
            }
            args.push(self.out.join(file).to_string_lossy().into_owned());
            let st = std::process::Command::new(&self.ff.exe).args(&args).status()?;
            anyhow::ensure!(st.success(), "long_clip encode failed");
            self.write_truth("long_clip", file, secs as f32, json!({"long": true}))?;
            self.written.borrow_mut().push(file.into());
            eprintln!("  long_clip");
        }
        Ok(())
    }
}

fn resize_nearest(f: &Frame, w: usize, h: usize) -> Frame {
    let mut o = Frame::solid(w, h, 0.0, 0.5, 0.5);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * f.w / w, y * f.h / h);
            let (i, j) = (y * w + x, sy * f.w + sx);
            o.y[i] = f.y[j];
            o.cb[i] = f.cb[j];
            o.cr[i] = f.cr[j];
        }
    }
    o
}

/// Handheld jitter: per-frame (dx, dy, rot) whose velocity RMS is `vel_rms` texture px/s.
fn shake_offsets(dur: f32, vel_rms: f32, seed: u64) -> Vec<(f32, f32, f32)> {
    let n = (dur * FPS).round() as usize;
    if vel_rms <= 0.0 {
        return vec![(0.0, 0.0, 0.0); n];
    }
    let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut rnd = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng >> 40) as f32 / (1u64 << 24) as f32
    };
    let comps: Vec<(f32, f32, f32, f32)> = (0..6).map(|_| (2.0 + 6.0 * rnd(), rnd() * 6.283, rnd() * 6.283, rnd() * 6.283)).collect();
    let raw: Vec<(f32, f32, f32)> = (0..n)
        .map(|k| {
            let t = k as f32 / FPS;
            let (mut x, mut y, mut r) = (0.0, 0.0, 0.0);
            for (f, p1, p2, p3) in &comps {
                let a = 1.0 / f;
                x += a * (std::f32::consts::TAU * f * t + p1).sin();
                y += a * (std::f32::consts::TAU * f * t + p2).sin();
                r += a * (std::f32::consts::TAU * f * t + p3).sin();
            }
            (x, y, r)
        })
        .collect();
    // Scale so the per-frame velocity RMS (both axes combined) matches the target.
    let mut sq = 0.0;
    for k in 1..n {
        let (dx, dy) = (raw[k].0 - raw[k - 1].0, raw[k].1 - raw[k - 1].1);
        sq += (dx * dx + dy * dy) * FPS * FPS;
    }
    let rms = (sq / (n - 1).max(1) as f32).sqrt().max(1e-6);
    let g = vel_rms / rms;
    raw.into_iter().map(|(x, y, r)| (x * g, y * g, r * g * 0.0004)).collect()
}

pub fn ensure_dir(p: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(p)?;
    Ok(())
}
