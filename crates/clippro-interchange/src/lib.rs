//! Exporters and importers for NLE interchange formats (SPEC §16, P11.1).

pub mod edl;
pub mod fcp7;
pub mod fcpxml;
pub mod import;
pub mod model;
pub mod otio;
pub mod report;
pub mod validate;
pub mod xml;

pub use model::{build, build_stringout, ExportOptions, Layout, MarkerColor, MediaRef, SelectInput, Separator, Timeline};

/// Export formats (SPEC EXP-1..8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Xmeml,
    Fcpxml,
    Otio,
    Edl,
    Ale,
    AvidMarkers,
    Csv,
    Json,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Xmeml => "xml",
            Format::Fcpxml => "fcpxml",
            Format::Otio => "otio",
            Format::Edl => "edl",
            Format::Ale => "ale",
            Format::AvidMarkers => "txt",
            Format::Csv => "csv",
            Format::Json => "json",
        }
    }

    pub fn parse(s: &str) -> Option<Format> {
        Some(match s.trim().to_lowercase().as_str() {
            "xmeml" | "xml" | "fcp7" | "premiere" => Format::Xmeml,
            "fcpxml" | "fcp" | "finalcut" => Format::Fcpxml,
            "otio" => Format::Otio,
            "edl" | "cmx3600" => Format::Edl,
            "ale" | "avid" => Format::Ale,
            "avid-markers" | "markers" => Format::AvidMarkers,
            "csv" => Format::Csv,
            "json" => Format::Json,
            _ => return None,
        })
    }

    pub fn render(self, t: &Timeline) -> String {
        match self {
            Format::Xmeml => fcp7::export(t),
            Format::Fcpxml => fcpxml::export(t),
            Format::Otio => otio::export(t),
            Format::Edl => edl::export_edl(t, false),
            Format::Ale => edl::export_ale(t),
            Format::AvidMarkers => edl::export_avid_markers(t),
            Format::Csv => report::export_csv(t),
            Format::Json => report::export_json(t),
        }
    }
}
