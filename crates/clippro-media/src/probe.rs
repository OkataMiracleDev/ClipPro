//! `probe(path)`: container, streams and metadata (SPEC §9.1, P1.1, P1.6).

use crate::ffi::{codec_name, cstr, dict_entries, q2d, ts_to_us, Input, MediaError, Result};
use clippro_types::{FrameRate, Rational};
use ffmpeg_sys_next as ff;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub index: usize,
    pub codec: String,
    pub profile: String,
    pub pix_fmt: String,
    pub bit_depth: u8,
    /// Chroma subsampling as "4:2:0" / "4:2:2" / "4:4:4" / "4:0:0".
    pub chroma: String,
    pub width: u32,
    pub height: u32,
    pub sar: (i32, i32),
    pub fps: FrameRate,
    pub vfr: bool,
    pub color_range: String,
    pub color_primaries: String,
    pub color_transfer: String,
    pub color_matrix: String,
    /// Display rotation in degrees (counter-clockwise), from the display matrix.
    pub rotation: i32,
    pub intra_only: bool,
    pub frames: Option<i64>,
    pub bit_rate: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub index: usize,
    pub codec: String,
    pub channels: u32,
    pub layout: String,
    pub sample_rate: u32,
    pub language: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub path: String,
    pub container: String,
    pub size: u64,
    pub duration_us: i64,
    /// First video pts (µs); sample times are relative to this.
    pub start_us: i64,
    pub video: Option<VideoInfo>,
    pub audio: Vec<AudioInfo>,
    pub start_timecode: Option<String>,
    pub reel: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub creation_time: Option<String>,
    pub gps: Option<String>,
    /// Camera-RAW or other formats that need a linked proxy (SPEC §9.2).
    pub proxy_required: bool,
    /// All container and stream tags (for the inspector and later parsing).
    pub tags: Vec<(String, String)>,
}

const RAW_EXTENSIONS: &[&str] = &["braw", "r3d", "ari", "arx", "crm", "nev", "dng"];

fn known(p: *const libc::c_char) -> String {
    let s = cstr(p);
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

fn first_tag<'a>(tags: &'a [(String, String)], keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some((_, v)) = tags.iter().find(|(tk, _)| tk.eq_ignore_ascii_case(k)) {
            if !v.trim().is_empty() {
                return Some(v.trim());
            }
        }
    }
    None
}

pub fn probe(path: &Path) -> Result<MediaInfo> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let input = match Input::open(path) {
        Ok(i) => i,
        Err(e) if RAW_EXTENSIONS.contains(&ext.as_str()) => {
            return Ok(MediaInfo {
                path: path.to_string_lossy().into_owned(),
                container: ext,
                size,
                proxy_required: true,
                tags: vec![("clippro.open_error".into(), e.to_string())],
                ..Default::default()
            })
        }
        Err(e) => return Err(e),
    };
    let mut info = MediaInfo {
        path: path.to_string_lossy().into_owned(),
        container: input.format_name(),
        size,
        duration_us: input.duration_us().max(0),
        ..Default::default()
    };
    let mut tags = dict_entries(unsafe { (*input.ctx).metadata });
    let vidx = input.best_stream(ff::AVMediaType::AVMEDIA_TYPE_VIDEO);
    for (i, &sp) in input.streams().iter().enumerate() {
        let st = unsafe { &*sp };
        let par = unsafe { &*st.codecpar };
        let stags = dict_entries(st.metadata);
        for (k, v) in &stags {
            tags.push((format!("stream{i}.{k}"), v.clone()));
        }
        match par.codec_type {
            ff::AVMediaType::AVMEDIA_TYPE_VIDEO if Some(i) == vidx => {
                info.video = Some(video_info(i, st, par));
                if st.start_time != ff::AV_NOPTS_VALUE {
                    info.start_us = ts_to_us(st.start_time, st.time_base);
                }
                if st.duration > 0 && st.duration != ff::AV_NOPTS_VALUE {
                    info.duration_us = ts_to_us(st.duration, st.time_base);
                }
            }
            ff::AVMediaType::AVMEDIA_TYPE_AUDIO => {
                let mut layout = [0 as libc::c_char; 128];
                unsafe { ff::av_channel_layout_describe(&par.ch_layout, layout.as_mut_ptr(), layout.len()) };
                info.audio.push(AudioInfo {
                    index: i,
                    codec: codec_name(par.codec_id),
                    channels: par.ch_layout.nb_channels.max(0) as u32,
                    layout: cstr(layout.as_ptr()),
                    sample_rate: par.sample_rate.max(0) as u32,
                    language: first_tag(&stags, &["language"]).unwrap_or("").to_string(),
                });
            }
            _ => {}
        }
    }
    // Timecode: container tag, video stream tag, or a tmcd/data stream tag.
    info.start_timecode = first_tag(&tags, &["timecode"])
        .map(String::from)
        .or_else(|| tags.iter().find(|(k, _)| k.ends_with(".timecode")).map(|(_, v)| v.clone()));
    info.reel = first_tag(&tags, &["reel_name", "com.apple.proapps.reel", "material_package_name", "tape_name"])
        .map(String::from)
        .or_else(|| tags.iter().find(|(k, _)| k.ends_with(".reel_name")).map(|(_, v)| v.clone()));
    info.camera_make = first_tag(&tags, &["make", "com.apple.quicktime.make", "com.android.manufacturer", "manufacturer"]).map(String::from);
    info.camera_model = first_tag(&tags, &["model", "com.apple.quicktime.model", "com.android.model", "product_name"]).map(String::from);
    info.lens = first_tag(&tags, &["lens", "com.apple.quicktime.camera.lens_model", "lens_model"]).map(String::from);
    info.creation_time = first_tag(&tags, &["creation_time", "com.apple.quicktime.creationdate", "date"]).map(String::from)
        .or_else(|| tags.iter().find(|(k, _)| k.ends_with(".creation_time")).map(|(_, v)| v.clone()));
    info.gps = first_tag(&tags, &["location", "com.apple.quicktime.location.iso6709", "location-eng"]).map(String::from);
    info.proxy_required = RAW_EXTENSIONS.contains(&ext.as_str()) && info.video.is_none();
    info.tags = tags;
    if info.video.is_none() && info.audio.is_empty() && !info.proxy_required {
        return Err(MediaError::NoStream("video or audio"));
    }
    Ok(info)
}

fn video_info(i: usize, st: &ff::AVStream, par: &ff::AVCodecParameters) -> VideoInfo {
    let fmt: ff::AVPixelFormat = unsafe { std::mem::transmute(par.format) };
    let desc = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    let (depth, chroma) = if desc.is_null() {
        (8, "unknown".to_string())
    } else {
        let d = unsafe { &*desc };
        let depth = d.comp[0].depth as u8;
        let chroma = if d.nb_components < 3 {
            "4:0:0".to_string()
        } else {
            match (d.log2_chroma_w, d.log2_chroma_h) {
                (1, 1) => "4:2:0",
                (1, 0) => "4:2:2",
                (0, 0) => "4:4:4",
                (2, 0) => "4:1:1",
                _ => "other",
            }
            .to_string()
        };
        (depth, chroma)
    };
    let avg = q2d(st.avg_frame_rate);
    let rfr = q2d(st.r_frame_rate);
    let fps_f = if avg > 0.0 { avg } else if rfr > 0.0 { rfr } else { 25.0 };
    let fps = FrameRate::from_f64(fps_f);
    let vfr = avg > 0.0 && rfr > 0.0 && (avg - rfr).abs() / rfr > 0.01;
    let profile = cstr(unsafe { ff::avcodec_profile_name(par.codec_id, par.profile) });
    let cdesc = unsafe { ff::avcodec_descriptor_get(par.codec_id) };
    let intra_only = !cdesc.is_null() && unsafe { (*cdesc).props } & ff::AV_CODEC_PROP_INTRA_ONLY as i32 != 0;
    let rotation = display_rotation(par);
    VideoInfo {
        index: i,
        codec: codec_name(par.codec_id),
        profile,
        pix_fmt: cstr(unsafe { ff::av_get_pix_fmt_name(fmt) }),
        bit_depth: depth,
        chroma,
        width: par.width.max(0) as u32,
        height: par.height.max(0) as u32,
        sar: (par.sample_aspect_ratio.num, par.sample_aspect_ratio.den),
        fps,
        vfr,
        color_range: known(unsafe { ff::av_color_range_name(par.color_range) }),
        color_primaries: known(unsafe { ff::av_color_primaries_name(par.color_primaries) }),
        color_transfer: known(unsafe { ff::av_color_transfer_name(par.color_trc) }),
        color_matrix: known(unsafe { ff::av_color_space_name(par.color_space) }),
        rotation,
        intra_only,
        frames: (st.nb_frames > 0).then_some(st.nb_frames),
        bit_rate: par.bit_rate,
    }
}

fn display_rotation(par: &ff::AVCodecParameters) -> i32 {
    unsafe {
        let sd = ff::av_packet_side_data_get(
            par.coded_side_data,
            par.nb_coded_side_data,
            ff::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
        );
        if sd.is_null() || (*sd).size < 36 {
            return 0;
        }
        let r = ff::av_display_rotation_get((*sd).data as *const i32);
        if r.is_nan() {
            0
        } else {
            (r.round() as i32).rem_euclid(360)
        }
    }
}

impl MediaInfo {
    pub fn fps(&self) -> FrameRate {
        self.video.as_ref().map(|v| v.fps).unwrap_or(FrameRate(Rational::from_int(25)))
    }
}
