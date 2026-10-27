//! HTML report and contact sheet (SPEC EXP-8, P7.6). Both are single self-contained files:
//! styles and script inline, thumbnails embedded as base64 WebP, no external requests. The
//! contact sheet is laid out for printing (A4 landscape); the app turns it into a PDF through
//! WebView2.

use crate::session::{media_ref, Result, Session, SessionError};
use crate::store::lock;
use base64::Engine as _;
use clippro_media::video::{DecodeOptions, HwMode, VideoDecoder};
use clippro_media::MediaInfo;
use clippro_score::{Settings, SortOrder};
use clippro_types::{ClipId, FlagKind, Timecode};
use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Clone, Debug)]
pub struct ReportOptions {
    /// Shown in the header, e.g. the project name.
    pub title: String,
    /// Generation time as the caller wants it shown (the report doesn't read the clock).
    pub generated: String,
    pub thumb_width: u32,
}

impl Default for ReportOptions {
    fn default() -> Self {
        ReportOptions { title: "ClipPro selects".into(), generated: String::new(), thumb_width: 320 }
    }
}

/// One select, ready to render.
#[derive(Clone, Debug)]
pub struct ReportRow {
    pub index: usize,
    pub clip: String,
    pub path: String,
    pub tc_in: String,
    pub tc_out: String,
    pub duration_s: f64,
    pub mean: f32,
    pub best: f32,
    pub flags: Vec<String>,
    /// `data:image/webp;base64,…`, empty when the frame couldn't be decoded.
    pub thumb: String,
}

#[derive(Clone, Debug)]
pub struct ReportClip {
    pub name: String,
    pub duration_s: f64,
    pub selects: usize,
    pub selected_s: f64,
    pub best: f32,
    pub flags: Vec<(String, usize)>,
}

#[derive(Clone, Debug)]
pub struct ReportData {
    pub title: String,
    pub generated: String,
    pub preset: String,
    pub rows: Vec<ReportRow>,
    pub clips: Vec<ReportClip>,
    pub footage_s: f64,
    pub selected_s: f64,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn dur(s: f64) -> String {
    let s = s.max(0.0);
    if s >= 3600.0 {
        format!("{}:{:02}:{:02}", (s / 3600.0) as u64, ((s % 3600.0) / 60.0) as u64, (s % 60.0) as u64)
    } else {
        format!("{}:{:05.2}", (s / 60.0) as u64, s % 60.0)
    }
}

fn webp_thumb(rgb: &[u8], w: u32, h: u32) -> Option<String> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out).encode(rgb, w, h, image_webp::ColorType::Rgb8).ok()?;
    Some(format!("data:image/webp;base64,{}", base64::engine::general_purpose::STANDARD.encode(out)))
}

impl Session {
    /// Everything the report and the contact sheet show, with a thumbnail of each select's
    /// best-scoring sample.
    pub fn report_data(&self, settings: &Settings, sort: SortOrder, opts: &ReportOptions) -> Result<ReportData> {
        let sel = self.score(settings)?;
        let refs = sel.sorted(sort);
        let inputs = self.clip_inputs()?;
        // Clip id → (name, original path, probe, media id).
        let mut meta: HashMap<ClipId, (String, String, MediaInfo, i64)> = HashMap::new();
        {
            let db = lock(&self.db);
            for c in db.clips()? {
                let Some(m) = db.media(c.media_id)? else { continue };
                let info: MediaInfo = serde_json::from_str(&m.probe_json).map_err(|e| SessionError::Other(e.to_string()))?;
                meta.insert(c.id, (c.name, m.path, info, m.id.0));
            }
        }

        // Best sample per select, then thumbnails clip by clip (one decoder each, in time order).
        let mut rows: Vec<ReportRow> = Vec::with_capacity(refs.len());
        let mut thumbs_at: HashMap<ClipId, Vec<(usize, i64)>> = HashMap::new();
        for (i, r) in refs.iter().enumerate() {
            let Some((name, path, info, _)) = meta.get(&r.clip) else { continue };
            let cs = sel.clips.iter().find(|c| c.clip == r.clip);
            let scores = cs.map(|c| &c.scores.score[..]).unwrap_or(&[]);
            let (a, b) = (r.segment.first_sample.min(scores.len()), (r.segment.last_sample + 1).min(scores.len()));
            let (best_k, best) = (a..b).map(|k| (k, scores[k])).filter(|(_, v)| v.is_finite()).fold((a, f32::NAN), |acc, x| if acc.1.is_nan() || x.1 > acc.1 { x } else { acc });
            let at_us = inputs.iter().find(|c| c.id == r.clip).and_then(|c| c.ms.time_us.get(best_k).copied()).unwrap_or(r.segment.start_us);
            thumbs_at.entry(r.clip).or_default().push((rows.len(), at_us));
            let mr = media_ref(info, 0, name, path.into());
            let tc = |us: i64| Timecode::from_frames(mr.start_tc_frames + mr.rate.us_to_frames(us), mr.rate, mr.drop_frame).to_string();
            let flags = cs
                .map(|c| {
                    let fl = &c.scores.flags;
                    let bits = fl[a.min(fl.len())..b.min(fl.len())].iter().fold(0u32, |acc, f| acc | f);
                    FlagKind::ALL.iter().filter(|f| bits & f.bit() != 0).map(|f| f.label().to_string()).collect()
                })
                .unwrap_or_default();
            rows.push(ReportRow {
                index: i + 1,
                clip: name.clone(),
                path: path.clone(),
                tc_in: tc(r.segment.start_us),
                tc_out: tc(r.segment.end_us),
                duration_s: r.segment.len_us() as f64 / 1e6,
                mean: r.segment.mean,
                best,
                flags,
                thumb: String::new(),
            });
        }
        for (clip, mut at) in thumbs_at {
            let Some((_, _, _, media)) = meta.get(&clip) else { continue };
            let Some((path, info)) = self.preview_source(clippro_types::MediaId(*media))? else { continue };
            let Ok(mut dec) = VideoDecoder::open(&path, &info, &DecodeOptions { threads: 0, hw: HwMode::Auto }) else { continue };
            at.sort_by_key(|x| x.1);
            for (row, us) in at {
                let Ok(Some(f)) = dec.frame_near(us) else { continue };
                if let Ok((w, h, rgb)) = dec.to_rgb(&f, opts.thumb_width) {
                    rows[row].thumb = webp_thumb(&rgb, w, h).unwrap_or_default();
                }
            }
        }

        let mut clips = Vec::new();
        let mut footage_s = 0.0;
        for input in &inputs {
            let Some((name, _, _, _)) = meta.get(&input.id) else { continue };
            let cs = sel.clips.iter().find(|c| c.clip == input.id);
            let segs = cs.map(|c| &c.segments[..]).unwrap_or(&[]);
            let mut runs: HashMap<FlagKind, usize> = HashMap::new();
            if let Some(c) = cs {
                for f in FlagKind::ALL {
                    let mut on = false;
                    for bits in c.scores.flags.iter() {
                        let now = bits & f.bit() != 0;
                        if now && !on {
                            *runs.entry(f).or_default() += 1;
                        }
                        on = now;
                    }
                }
            }
            let mut flags: Vec<(String, usize)> = FlagKind::ALL.iter().filter_map(|f| runs.get(f).map(|n| (f.label().to_string(), *n))).collect();
            flags.sort_by(|a, b| b.1.cmp(&a.1));
            let duration_s = input.duration_us as f64 / 1e6;
            footage_s += duration_s;
            clips.push(ReportClip {
                name: name.clone(),
                duration_s,
                selects: segs.len(),
                selected_s: segs.iter().map(|s| s.len_us()).sum::<i64>() as f64 / 1e6,
                best: cs.map(|c| c.scores.score.iter().copied().filter(|v| v.is_finite()).fold(f32::NAN, f32::max)).unwrap_or(f32::NAN),
                flags,
            });
        }
        let selected_s = rows.iter().map(|r| r.duration_s).sum();
        Ok(ReportData { title: opts.title.clone(), generated: opts.generated.clone(), preset: settings.preset_name.clone(), rows, clips, footage_s, selected_s })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    Html,
    ContactSheet,
}

impl ReportKind {
    pub fn parse(s: &str) -> Option<ReportKind> {
        match s {
            "html" => Some(ReportKind::Html),
            "contact-sheet" | "contact_sheet" => Some(ReportKind::ContactSheet),
            _ => None,
        }
    }
}

impl Session {
    /// Renders a report to `path`; returns the number of selects.
    pub fn write_report(&self, settings: &Settings, sort: SortOrder, kind: ReportKind, opts: &ReportOptions, path: &std::path::Path) -> Result<usize> {
        let d = self.report_data(settings, sort, opts)?;
        let html = match kind {
            ReportKind::Html => render_html(&d),
            ReportKind::ContactSheet => render_contact_sheet(&d),
        };
        std::fs::write(path, html).map_err(|e| SessionError::Other(format!("{}: {e}", path.display())))?;
        Ok(d.rows.len())
    }
}

/// Current UTC time as "YYYY-MM-DD HH:MM UTC" (for callers without a local clock string).
pub fn utc_now_string() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

const STYLE: &str = r#"
:root{--bg:#fbfbfd;--fg:#1d1d1f;--fg2:#6e6e73;--line:#e3e3e8;--card:#fff;--accent:#0a66d8;--lo:#d9534f;--mid:#d4a017;--hi:#3a9a4a;color-scheme:light}
@media (prefers-color-scheme:dark){:root{--bg:#1c1c1e;--fg:#f5f5f7;--fg2:#a1a1a6;--line:#3a3a3c;--card:#2c2c2e;--accent:#4a9eff;color-scheme:dark}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:13px/1.45 -apple-system,"Segoe UI Variable","Segoe UI",system-ui,sans-serif;padding:28px 24px}
h1{font-size:22px;margin:0 0 2px;text-wrap:balance}h2{font-size:12px;text-transform:uppercase;letter-spacing:.06em;color:var(--fg2);margin:28px 0 8px}
.meta{color:var(--fg2)}.stats{display:flex;flex-wrap:wrap;gap:24px;margin:18px 0 4px}.stats b{display:block;font-size:20px;font-variant-numeric:tabular-nums}
.wrap{overflow-x:auto;border:1px solid var(--line);border-radius:10px;background:var(--card)}
table{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}th,td{padding:7px 10px;text-align:left;border-bottom:1px solid var(--line);vertical-align:middle;white-space:nowrap}
th{font-size:11px;font-weight:600;color:var(--fg2);cursor:pointer;user-select:none;position:sticky;top:0;background:var(--card)}th[aria-sort=ascending]::after{content:" \25B4"}th[aria-sort=descending]::after{content:" \25BE"}
tr:last-child td{border-bottom:0}td.num{text-align:right;font-family:ui-monospace,"Cascadia Mono",Consolas,monospace}td.mono{font-family:ui-monospace,"Cascadia Mono",Consolas,monospace}
img.th{width:128px;height:72px;object-fit:cover;border-radius:4px;display:block;background:#000}
.score{display:inline-block;min-width:34px;padding:1px 6px;border-radius:6px;color:#fff;text-align:center;font-weight:600}
.flags{white-space:normal;color:var(--fg2);font-size:12px}.path{color:var(--fg2);font-size:11px;max-width:360px;overflow:hidden;text-overflow:ellipsis}
footer{margin-top:28px;color:var(--fg2);font-size:11px}
"#;

const SORT_JS: &str = r#"
document.querySelectorAll("table[data-sortable]").forEach(function(t){
  t.querySelectorAll("th").forEach(function(th,i){
    th.addEventListener("click",function(){
      var asc=th.getAttribute("aria-sort")!=="ascending";
      t.querySelectorAll("th").forEach(function(o){o.removeAttribute("aria-sort")});
      th.setAttribute("aria-sort",asc?"ascending":"descending");
      var body=t.tBodies[0],rows=Array.prototype.slice.call(body.rows);
      rows.sort(function(a,b){
        var x=a.cells[i].getAttribute("data-v")||a.cells[i].textContent,y=b.cells[i].getAttribute("data-v")||b.cells[i].textContent;
        var nx=parseFloat(x),ny=parseFloat(y),c=(!isNaN(nx)&&!isNaN(ny))?nx-ny:x.localeCompare(y);
        return asc?c:-c;
      });
      rows.forEach(function(r){body.appendChild(r)});
    });
  });
});
"#;

fn score_chip(v: f32) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let colour = if v < 0.4 { "var(--lo)" } else if v < 0.65 { "var(--mid)" } else { "var(--hi)" };
    format!(r#"<span class="score" style="background:{colour}">{}</span>"#, (v * 100.0).round())
}

fn head(title: &str, extra_style: &str) -> String {
    format!("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{STYLE}{extra_style}</style></head><body>", esc(title))
}

fn summary(d: &ReportData) -> String {
    let pct = if d.footage_s > 0.0 { 100.0 * d.selected_s / d.footage_s } else { 0.0 };
    format!(
        r#"<h1>{}</h1><div class="meta">Preset {} · {}</div><div class="stats"><div><b>{}</b>selects</div><div><b>{}</b>selected</div><div><b>{:.1}%</b>of footage</div><div><b>{}</b>clips, {} of footage</div></div>"#,
        esc(&d.title),
        esc(&d.preset),
        esc(&d.generated),
        d.rows.len(),
        dur(d.selected_s),
        pct,
        d.clips.len(),
        dur(d.footage_s),
    )
}

/// The full report: summary, a sortable selects table with thumbnails, and a per-clip table.
pub fn render_html(d: &ReportData) -> String {
    let mut h = head(&d.title, "");
    h.push_str(&summary(d));
    h.push_str(r#"<h2>Selects</h2><div class="wrap"><table data-sortable><thead><tr><th>#</th><th>Frame</th><th>Clip</th><th>In</th><th>Out</th><th>Length</th><th>Score</th><th>Best</th><th>Flags</th></tr></thead><tbody>"#);
    for r in &d.rows {
        let img = if r.thumb.is_empty() { String::new() } else { format!(r#"<img class="th" alt="" src="{}">"#, r.thumb) };
        let _ = write!(
            h,
            r#"<tr><td class="num">{}</td><td>{img}</td><td>{}<div class="path" title="{p}">{p}</div></td><td class="mono">{}</td><td class="mono">{}</td><td class="num" data-v="{:.3}">{}</td><td data-v="{:.4}">{}</td><td data-v="{:.4}">{}</td><td class="flags">{}</td></tr>"#,
            r.index,
            esc(&r.clip),
            r.tc_in,
            r.tc_out,
            r.duration_s,
            dur(r.duration_s),
            r.mean,
            score_chip(r.mean),
            if r.best.is_finite() { r.best } else { -1.0 },
            score_chip(r.best),
            esc(&r.flags.join(", ")),
            p = esc(&r.path),
        );
    }
    h.push_str("</tbody></table></div>");
    h.push_str(r#"<h2>Clips</h2><div class="wrap"><table data-sortable><thead><tr><th>Clip</th><th>Length</th><th>Selects</th><th>Selected</th><th>Best</th><th>Flag runs</th></tr></thead><tbody>"#);
    for c in &d.clips {
        let flags = c.flags.iter().map(|(f, n)| format!("{f} {n}")).collect::<Vec<_>>().join(", ");
        let _ = write!(
            h,
            r#"<tr><td>{}</td><td class="num" data-v="{:.3}">{}</td><td class="num">{}</td><td class="num" data-v="{:.3}">{}</td><td data-v="{:.4}">{}</td><td class="flags">{}</td></tr>"#,
            esc(&c.name),
            c.duration_s,
            dur(c.duration_s),
            c.selects,
            c.selected_s,
            dur(c.selected_s),
            if c.best.is_finite() { c.best } else { -1.0 },
            score_chip(c.best),
            esc(&flags),
        );
    }
    let _ = write!(h, "</tbody></table></div><footer>Generated by ClipPro {}. Scores are 0–100; Best is the highest-scoring moment in the select.</footer><script>{SORT_JS}</script></body></html>", env!("CARGO_PKG_VERSION"));
    h
}

const SHEET_STYLE: &str = r#"
@page{size:A4 landscape;margin:10mm}
body{padding:20px}@media print{body{padding:0}}.sheet{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-top:14px}
.card{break-inside:avoid;border:1px solid var(--line);border-radius:8px;overflow:hidden;background:var(--card)}
.card img,.card .ph{width:100%;aspect-ratio:16/9;object-fit:cover;display:block;background:#000}
.card .t{padding:6px 8px;font-size:11px}.card .n{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.card .r{display:flex;justify-content:space-between;align-items:center;color:var(--fg2);font-family:ui-monospace,"Cascadia Mono",Consolas,monospace;font-size:10px;margin-top:2px}
.stats{margin-top:10px}h1{font-size:18px}
@media print{:root{--bg:#fff;--card:#fff;--fg:#000;--fg2:#555;--line:#ccc}body{-webkit-print-color-adjust:exact;print-color-adjust:exact}}
"#;

/// The contact sheet: a print-ready grid of each select's best frame with timecode and score.
pub fn render_contact_sheet(d: &ReportData) -> String {
    let mut h = head(&format!("{} — contact sheet", d.title), SHEET_STYLE);
    h.push_str(&summary(d));
    h.push_str(r#"<div class="sheet">"#);
    for r in &d.rows {
        let img = if r.thumb.is_empty() { r#"<div class="ph"></div>"#.to_string() } else { format!(r#"<img alt="" src="{}">"#, r.thumb) };
        let _ = write!(
            h,
            r#"<div class="card">{img}<div class="t"><div class="n">{}. {}</div><div class="r"><span>{} – {}</span>{}</div></div></div>"#,
            r.index,
            esc(&r.clip),
            r.tc_in,
            r.tc_out,
            score_chip(r.mean),
        );
    }
    let _ = write!(h, "</div><footer>Generated by ClipPro {}.</footer></body></html>", env!("CARGO_PKG_VERSION"));
    h
}
