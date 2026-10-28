//! Manual overrides and annotations (SPEC TUN-11, P9.7).
//!
//! Overrides (force in/out, trim, manual segment) are time ranges the engine applies after
//! segmentation, so they survive re-scoring. Annotations (stars, a colour label, a note) are time
//! ranges too: they attach to whichever select overlaps them most, so a select that shifts a
//! little after a tuning change keeps its rating.

use crate::session::{Result, Session, SessionError};
use crate::store::lock;
use clippro_types::ClipId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(skip)]
    pub id: i64,
    #[serde(skip)]
    pub clip: ClipId,
    #[serde(skip)]
    pub start_us: i64,
    #[serde(skip)]
    pub end_us: i64,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub note: String,
}

impl Annotation {
    pub fn is_empty(&self) -> bool {
        self.rating == 0 && self.label.is_none() && self.note.trim().is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverrideInfo {
    pub id: i64,
    pub clip: ClipId,
    pub kind: String,
    pub start_us: i64,
    pub end_us: i64,
}

pub const OVERRIDE_KINDS: [&str; 4] = ["force_in", "force_out", "trim", "manual"];

fn overlap(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

/// The annotation of `clip` that overlaps [start, end) the most (at least half of the shorter).
pub fn best_match(anns: &[Annotation], clip: ClipId, start: i64, end: i64) -> Option<&Annotation> {
    anns.iter()
        .filter(|a| a.clip == clip)
        .map(|a| (overlap(a.start_us, a.end_us, start, end), a))
        .filter(|(o, a)| *o * 2 >= (a.end_us - a.start_us).min(end - start).max(1))
        .max_by_key(|(o, _)| *o)
        .map(|(_, a)| a)
}

impl Session {
    pub fn annotations(&self) -> Result<Vec<Annotation>> {
        Ok(lock(&self.db)
            .annotations()?
            .into_iter()
            .map(|(id, clip, start_us, end_us, json)| Annotation { id, clip, start_us, end_us, ..serde_json::from_str(&json).unwrap_or(Annotation { id: 0, clip, start_us: 0, end_us: 0, rating: 0, label: None, note: String::new() }) })
            .collect())
    }

    /// Sets the annotation of the select spanning [start, end) of `clip`; an empty annotation
    /// removes it. Returns the annotation's id (0 when removed).
    pub fn set_annotation(&self, clip: ClipId, start_us: i64, end_us: i64, rating: u8, label: Option<String>, note: &str) -> Result<i64> {
        if end_us <= start_us {
            return Err(SessionError::Other("The range is empty.".into()));
        }
        let all = self.annotations()?;
        let existing = best_match(&all, clip, start_us, end_us).map(|a| a.id);
        let a = Annotation { id: 0, clip, start_us, end_us, rating: rating.min(5), label: label.filter(|l| !l.trim().is_empty()), note: note.trim().to_string() };
        let db = lock(&self.db);
        if a.is_empty() {
            if let Some(id) = existing {
                db.remove_override(id)?;
            }
            return Ok(0);
        }
        let json = serde_json::to_string(&a).map_err(|e| SessionError::Other(e.to_string()))?;
        Ok(db.put_annotation(existing, clip, start_us, end_us, &json)?)
    }

    /// Scoring overrides (not annotations).
    pub fn list_overrides(&self) -> Result<Vec<OverrideInfo>> {
        Ok(lock(&self.db)
            .overrides()?
            .into_iter()
            .filter(|o| OVERRIDE_KINDS.contains(&o.kind.as_str()))
            .map(|o| OverrideInfo { id: o.id, clip: o.clip_id, kind: o.kind, start_us: o.start_us, end_us: o.end_us })
            .collect())
    }

    pub fn add_override(&self, clip: ClipId, kind: &str, start_us: i64, end_us: i64) -> Result<i64> {
        if !OVERRIDE_KINDS.contains(&kind) {
            return Err(SessionError::Other(format!("Unknown override “{kind}”.")));
        }
        if end_us <= start_us {
            return Err(SessionError::Other("The range is empty.".into()));
        }
        Ok(lock(&self.db).add_override(clip, kind, start_us, end_us)?)
    }

    pub fn remove_override(&self, id: i64) -> Result<()> {
        Ok(lock(&self.db).remove_override(id)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: i64, s: i64, e: i64) -> Annotation {
        Annotation { id, clip: ClipId(1), start_us: s, end_us: e, rating: 3, label: None, note: String::new() }
    }

    #[test]
    fn annotations_follow_shifted_selects() {
        let anns = [a(1, 1_000_000, 4_000_000), a(2, 10_000_000, 12_000_000)];
        assert_eq!(best_match(&anns, ClipId(1), 1_200_000, 4_400_000).map(|x| x.id), Some(1), "small shift keeps the rating");
        assert_eq!(best_match(&anns, ClipId(1), 3_500_000, 9_000_000).map(|x| x.id), None, "little overlap: not the same select");
        assert_eq!(best_match(&anns, ClipId(2), 1_000_000, 4_000_000).map(|x| x.id), None, "other clip");
        assert_eq!(best_match(&anns, ClipId(1), 9_000_000, 13_000_000).map(|x| x.id), Some(2));
    }
}
