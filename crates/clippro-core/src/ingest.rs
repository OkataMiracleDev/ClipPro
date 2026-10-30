//! Project-level proxies and rendered selects (SPEC §19, PLAN P13.2/P13.3).

use crate::session::{media_ref, Result, Session, SessionError};
use crate::store::lock;
use clippro_ingest::proxy::{make_proxy, ProxyOptions, ProxySource};
use clippro_ingest::render::{file_name, render, tc_at, RenderItem, RenderOptions, RenderOutcome};
use clippro_media::MediaInfo;
use clippro_score::{Settings, SortOrder};
use clippro_types::ClipId;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

impl Session {
    /// Makes a proxy for a clip and links it (the clip is then analysed from the proxy).
    pub fn make_proxy(&self, clip: ClipId, opts: &ProxyOptions, cancel: &AtomicBool, progress: impl FnMut(f32)) -> Result<PathBuf> {
        let (path, info) = {
            let db = lock(&self.db);
            let m = self.clip_media(&db, clip)?;
            let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
            (PathBuf::from(&m.path), info)
        };
        let src = ProxySource { path, duration_us: info.duration_us, start_tc: info.start_timecode.clone(), reel: info.reel.clone() };
        let out = make_proxy(&src, opts, cancel, progress).map_err(SessionError::Other)?;
        self.link_proxy(clip, &out)?;
        Ok(out)
    }

    /// Renders every current select (in `sort` order) into `opts.out_dir`. `handles` renders the
    /// span with handles. `progress(index, count, fraction)`.
    pub fn render_selects(&self, settings: &Settings, sort: SortOrder, opts: &RenderOptions, handles: bool, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize, f32)) -> Result<Vec<RenderOutcome>> {
        let (media, selects, meta) = self.export_inputs(settings, sort)?;
        let names = self.clip_names()?;
        let mut keyframes: HashMap<usize, Vec<i64>> = HashMap::new();
        let mut used: Vec<String> = Vec::new();
        let mut out = Vec::new();
        let n = selects.len();
        for (i, (s, (clip, info))) in selects.iter().zip(&meta).enumerate() {
            let m = &media[s.media];
            let mr = media_ref(info, m.id, &m.name, m.path.clone());
            let (a, b) = if handles { (s.in_us, s.out_us) } else { (s.start_us, s.end_us) };
            let item = RenderItem {
                src: m.path.clone(),
                start_us: a,
                end_us: b,
                rate: mr.rate,
                start_tc_frames: info.start_timecode.as_ref().map(|_| mr.start_tc_frames),
                drop_frame: mr.drop_frame,
                reel: info.reel.clone(),
            };
            let keys = match keyframes.get(&s.media) {
                Some(k) => k.clone(),
                None => {
                    let k = if opts.stream_copy { clippro_media::keyframes(&m.path).unwrap_or_default() } else { Vec::new() };
                    keyframes.insert(s.media, k.clone());
                    k
                }
            };
            let clip_name = names.get(clip).map(|x| x.0.clone()).unwrap_or_else(|| m.name.clone());
            let tc = |us: i64| tc_at(&item, us).unwrap_or_else(|| format!("{:.2}s", us as f64 / 1e6));
            let tokens = [
                ("clip", clip_name),
                ("index", format!("{:03}", i + 1)),
                ("tc_in", tc(a)),
                ("tc_out", tc(b)),
                ("score", format!("{}", (s.score * 100.0).round() as i64)),
                ("rating", if s.rating > 0 { format!("{}star", s.rating) } else { String::new() }),
                ("label", s.label.map(|l| l.name().to_string()).unwrap_or_default()),
            ];
            let mut name = file_name(&opts.template, &tokens);
            // Never overwrite another select rendered in this run.
            let base = name.clone();
            let mut k = 2;
            while used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
                name = format!("{base}_{k}");
                k += 1;
            }
            used.push(name.clone());
            let path = opts.out_dir.join(format!("{name}.mov"));
            let r = render(&item, &keys, opts, &path, cancel, |f| progress(i, n, f)).map_err(SessionError::Other)?;
            out.push(r);
        }
        Ok(out)
    }
}
