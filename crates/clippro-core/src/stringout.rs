//! Stringout builder (SPEC EXP-12, PLAN P11.6): the selects grouped by clip, camera, date,
//! reel, rating or colour label, groups in order, separated by gaps or headed by slugs.

use crate::session::{Result, Session};
use clippro_interchange::{build_stringout, ExportOptions, MarkerColor, SelectInput, Separator, Timeline};
use clippro_media::MediaInfo;
use clippro_score::{Settings, SortOrder};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Clip,
    Camera,
    Date,
    Reel,
    Rating,
    Label,
    /// AI: the people seen in the select.
    Person,
    /// AI: the select's shot type.
    Shot,
    /// AI: take groups (several takes of the same shot or line), best take first inside each.
    Take,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupOrder {
    /// By the group key: names A–Z, dates oldest first, ratings and labels best first.
    #[default]
    Key,
    /// The group holding the best select first.
    BestFirst,
}

/// (title, sort key) of a select's group. Groups without a value sort last.
pub fn group_key(by: GroupBy, s: &SelectInput, clip_name: &str, info: &MediaInfo) -> (String, String) {
    let missing = |t: &str| (t.to_string(), "\u{10FFFF}".to_string());
    match by {
        GroupBy::Clip => (clip_name.to_string(), clip_name.to_lowercase()),
        GroupBy::Camera => match info.camera_model.as_deref().filter(|c| !c.trim().is_empty()) {
            Some(c) => (c.trim().to_string(), c.trim().to_lowercase()),
            None => missing("Unknown camera"),
        },
        GroupBy::Date => match info.creation_time.as_deref().map(|d| d.chars().take(10).collect::<String>()).filter(|d| d.len() == 10) {
            Some(d) => (d.clone(), d),
            None => missing("No date"),
        },
        GroupBy::Reel => match info.reel.as_deref().filter(|r| !r.trim().is_empty()) {
            Some(r) => (r.trim().to_string(), r.trim().to_lowercase()),
            None => missing("No reel"),
        },
        GroupBy::Rating => match s.rating.min(5) {
            0 => missing("Unrated"),
            // Plain text: slugs go into EDLs, which are ASCII.
            r => (format!("{r} star{}", if r == 1 { "" } else { "s" }), format!("{}", 5 - r)),
        },
        GroupBy::Label => match s.label {
            Some(l) => {
                let name = l.name();
                let mut t = name.to_string();
                t[..1].make_ascii_uppercase();
                (t, format!("{:02}", MarkerColor::ALL.iter().position(|c| *c == l).unwrap_or(99)))
            }
            None => missing("No label"),
        },
        GroupBy::Person => match s.people.is_empty() {
            true => missing("No one"),
            false => (s.people.join(", "), s.people.join(", ").to_lowercase()),
        },
        GroupBy::Shot => match s.shot.is_empty() {
            true => missing("Unknown shot"),
            false => (s.shot.clone(), s.shot.to_lowercase()),
        },
        GroupBy::Take => match s.take {
            Some((g, _)) => (format!("Take group {}", g + 1), format!("{g:06}")),
            None => missing("Single takes"),
        },
    }
}

/// Groups selects (already in play order) and orders the groups.
pub fn group_selects(selects: Vec<(SelectInput, String, MediaInfo)>, by: GroupBy, order: GroupOrder) -> Vec<(String, Vec<SelectInput>)> {
    let mut groups: Vec<(String, String, f32, Vec<SelectInput>)> = Vec::new();
    for (s, name, info) in selects {
        let (title, key) = group_key(by, &s, &name, &info);
        match groups.iter_mut().find(|g| g.0 == title) {
            Some(g) => {
                g.2 = g.2.max(s.score);
                g.3.push(s);
            }
            None => groups.push((title, key, s.score, vec![s])),
        }
    }
    match order {
        GroupOrder::Key => groups.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))),
        GroupOrder::BestFirst => groups.sort_by(|a, b| b.2.total_cmp(&a.2)),
    }
    // Takes play best first inside their group.
    if by == GroupBy::Take {
        for g in &mut groups {
            g.3.sort_by_key(|s| s.take.map_or(u32::MAX, |t| t.1));
        }
    }
    groups.into_iter().map(|g| (g.0, g.3)).collect()
}

impl Session {
    /// The current selects as a stringout timeline.
    pub fn stringout_timeline(&self, settings: &Settings, sort: SortOrder, opts: &ExportOptions, by: GroupBy, order: GroupOrder, sep: Separator) -> Result<Timeline> {
        let (media, selects, meta) = self.export_inputs(settings, sort)?;
        let names = self.clip_names()?;
        let rows: Vec<(SelectInput, String, MediaInfo)> = selects
            .into_iter()
            .zip(meta)
            .map(|(s, (clip, info))| {
                let name = names.get(&clip).map(|n| n.0.clone()).unwrap_or_default();
                (s, name, info)
            })
            .collect();
        let groups = group_selects(rows, by, order);
        Ok(build_stringout(media, &groups, sep, opts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(score: f32, rating: u8, label: Option<MarkerColor>) -> SelectInput {
        SelectInput { media: 0, start_us: 0, end_us: 1_000_000, in_us: 0, out_us: 1_000_000, score, flags: vec![], rating, label, note: String::new(), transcript: String::new(), people: vec![], shot: String::new(), take: None }
    }

    fn info(camera: Option<&str>, date: Option<&str>) -> MediaInfo {
        MediaInfo { camera_model: camera.map(str::to_string), creation_time: date.map(str::to_string), ..MediaInfo::default() }
    }

    fn titles(g: &[(String, Vec<SelectInput>)]) -> Vec<(&str, usize)> {
        g.iter().map(|(t, s)| (t.as_str(), s.len())).collect()
    }

    #[test]
    fn groups_by_ai_fields() {
        let ai = |people: &[&str], shot: &str, take: Option<(u32, u32)>, score: f32| {
            let s = SelectInput { people: people.iter().map(|p| p.to_string()).collect(), shot: shot.into(), take, ..sel(score, 0, None) };
            (s, "C".to_string(), info(None, None))
        };
        let rows = || vec![ai(&["Sarah"], "Close-up", Some((1, 2)), 0.6), ai(&[], "Wide", None, 0.7), ai(&["Sarah"], "Close-up", Some((1, 1)), 0.9), ai(&["Sarah", "Tom"], "", Some((0, 1)), 0.8)];
        assert_eq!(titles(&group_selects(rows(), GroupBy::Person, GroupOrder::Key)), [("Sarah", 2), ("Sarah, Tom", 1), ("No one", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Shot, GroupOrder::Key)), [("Close-up", 2), ("Wide", 1), ("Unknown shot", 1)]);
        let takes = group_selects(rows(), GroupBy::Take, GroupOrder::Key);
        assert_eq!(titles(&takes), [("Take group 1", 1), ("Take group 2", 2), ("Single takes", 1)]);
        assert_eq!(takes[1].1.iter().map(|s| s.take.unwrap().1).collect::<Vec<_>>(), [1, 2], "best take first");
    }

    #[test]
    fn groups_by_key_and_by_best() {
        let rows = || {
            vec![
                (sel(0.6, 3, Some(MarkerColor::Blue)), "B".to_string(), info(Some("FX6"), Some("2026-10-12T09:00:00Z"))),
                (sel(0.9, 0, None), "A".to_string(), info(None, Some("2026-10-11T18:00:00Z"))),
                (sel(0.7, 5, Some(MarkerColor::Red)), "B".to_string(), info(Some("FX6"), None)),
                (sel(0.5, 3, None), "C".to_string(), info(Some("C70"), Some("2026-10-12T10:00:00Z"))),
            ]
        };
        assert_eq!(titles(&group_selects(rows(), GroupBy::Clip, GroupOrder::Key)), [("A", 1), ("B", 2), ("C", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Camera, GroupOrder::Key)), [("C70", 1), ("FX6", 2), ("Unknown camera", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Date, GroupOrder::Key)), [("2026-10-11", 1), ("2026-10-12", 2), ("No date", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Rating, GroupOrder::Key)), [("5 stars", 1), ("3 stars", 2), ("Unrated", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Label, GroupOrder::Key)), [("Red", 1), ("Blue", 1), ("No label", 2)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Reel, GroupOrder::Key)), [("No reel", 4)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Clip, GroupOrder::BestFirst)), [("A", 1), ("B", 2), ("C", 1)]);
        assert_eq!(titles(&group_selects(rows(), GroupBy::Camera, GroupOrder::BestFirst)), [("Unknown camera", 1), ("FX6", 2), ("C70", 1)]);
        // Inside a group, the selects keep their play order.
        let g = group_selects(rows(), GroupBy::Clip, GroupOrder::Key);
        assert_eq!(g[1].1.iter().map(|s| s.score).collect::<Vec<_>>(), [0.6, 0.7]);
    }
}
