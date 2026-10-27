//! Custom presets (SPEC P9.4): TOML files in `<config>/presets/`, beside the built-in ones.
//! Names are unique across both sets; files are named after a slug of the preset name.

use clippro_score::{builtin, Preset};
use std::path::{Path, PathBuf};

pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "preset".into() } else { s }
}

pub struct UserPresets {
    pub dir: PathBuf,
}

impl UserPresets {
    pub fn list(&self) -> Vec<Preset> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<Preset> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| Preset::parse(&t).ok())
            .collect();
        out.sort_by_key(|p| p.preset.name.to_lowercase());
        out
    }

    pub fn get(&self, name: &str) -> Option<Preset> {
        self.list().into_iter().find(|p| p.preset.name == name)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.toml", slug(name)))
    }

    fn check_name(&self, name: &str, replacing: Option<&str>) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the preset a name.".into());
        }
        if builtin(name).is_some() {
            return Err(format!("“{name}” is a built-in preset; choose another name."));
        }
        if replacing != Some(name) && (self.get(name).is_some() || self.path(name).exists()) {
            return Err(format!("A preset called “{name}” already exists."));
        }
        Ok(())
    }

    /// Saves `p`; `overwrite` allows replacing an existing custom preset of the same name.
    pub fn save(&self, p: &Preset, overwrite: bool) -> Result<(), String> {
        let name = p.preset.name.trim().to_string();
        self.check_name(&name, overwrite.then_some(name.as_str()))?;
        p.validate().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let text = p.to_toml().map_err(|e| e.to_string())?;
        let path = self.path(&name);
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        if builtin(name).is_some() {
            return Err("Built-in presets can't be deleted.".into());
        }
        let p = self.path(name);
        if self.get(name).is_none() {
            return Err(format!("No custom preset called “{name}”."));
        }
        std::fs::remove_file(p).map_err(|e| e.to_string())
    }

    pub fn rename(&self, old: &str, new: &str) -> Result<(), String> {
        let mut p = self.get(old).ok_or_else(|| format!("No custom preset called “{old}”."))?;
        self.check_name(new, None)?;
        p.preset.name = new.trim().to_string();
        self.save(&p, false)?;
        std::fs::remove_file(self.path(old)).map_err(|e| e.to_string())
    }

    /// Imports a preset file. A name already in use gets " 2", " 3", … appended.
    pub fn import(&self, file: &Path) -> Result<Preset, String> {
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        let mut p = Preset::parse(&text).map_err(|e| format!("Not a valid preset: {e}"))?;
        let base = p.preset.name.trim().to_string();
        let mut k = 1;
        while self.check_name(&p.preset.name, None).is_err() {
            k += 1;
            p.preset.name = format!("{base} {k}");
            if k > 999 {
                return Err("Couldn't find a free name for the preset.".into());
            }
        }
        self.save(&p, false)?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clippro_score::Settings;

    #[test]
    fn save_list_rename_import_delete() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-presets-test");
        let _ = std::fs::remove_dir_all(&dir);
        let up = UserPresets { dir: dir.clone() };
        let mut s = Settings::from_preset(&builtin("Cleanup").unwrap()).unwrap();
        s.score_lo = 0.42;
        let p = Preset::from_settings(&s, "My Cleanup", "Looser");
        up.save(&p, false).unwrap();
        assert!(up.save(&p, false).is_err(), "duplicate name refused");
        up.save(&p, true).unwrap();
        assert!(up.save(&Preset::from_settings(&s, "Cleanup", ""), false).is_err(), "built-in name refused");
        assert_eq!(up.list().len(), 1);
        let back = Settings::from_preset(&up.get("My Cleanup").unwrap()).unwrap();
        assert_eq!(back.score_lo, 0.42);

        up.rename("My Cleanup", "Loose cleanup").unwrap();
        assert!(up.get("My Cleanup").is_none() && up.get("Loose cleanup").is_some());

        let file = dir.join("exported.toml.in");
        std::fs::write(&file, up.get("Loose cleanup").unwrap().to_toml().unwrap()).unwrap();
        let imported = up.import(&file).unwrap();
        assert_eq!(imported.preset.name, "Loose cleanup 2");
        std::fs::write(&file, "not toml [").unwrap();
        assert!(up.import(&file).is_err());

        up.delete("Loose cleanup").unwrap();
        up.delete("Loose cleanup 2").unwrap();
        assert!(up.delete("Cleanup").is_err());
        assert!(up.list().is_empty());
        assert_eq!(slug("Wedding / Event (v2)"), "wedding-event-v2");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
