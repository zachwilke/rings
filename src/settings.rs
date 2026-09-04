//! Persistent interactive preferences, kept deliberately dependency-free.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: String,
    pub export_dir: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "tokyo-night".into(),
            export_dir: PathBuf::from("."),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut value = Self::default();
        for line in text.lines() {
            if let Some(theme) = line.strip_prefix("theme=") {
                if crate::tui::theme::names().contains(&theme) {
                    value.theme = theme.to_string();
                }
            } else if let Some(dir) = line.strip_prefix("export_dir=") {
                if !dir.is_empty() {
                    value.export_dir = expand_home(dir);
                }
            }
        }
        value
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path =
            config_path().ok_or_else(|| "no user config directory is available".to_string())?;
        let parent = path
            .parent()
            .ok_or_else(|| "invalid config path".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        let body = format!(
            "theme={}\nexport_dir={}\n",
            self.theme,
            self.export_dir.display()
        );
        std::fs::write(&path, body).map_err(|e| format!("cannot save {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn cycle_theme(&mut self, delta: isize) {
        let names = crate::tui::theme::names();
        let current = names
            .iter()
            .position(|name| *name == self.theme)
            .unwrap_or(0) as isize;
        self.theme = names[(current + delta).rem_euclid(names.len() as isize) as usize].to_string();
    }

    pub fn set_export_dir(&mut self, raw: &str) -> Result<(), String> {
        let path = expand_home(raw.trim());
        if path.as_os_str().is_empty() {
            return Err("export folder cannot be empty".into());
        }
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("cannot use {}: {e}", path.display()))?;
        if !path.is_dir() {
            return Err(format!("{} is not a directory", path.display()));
        }
        self.export_dir = path;
        Ok(())
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("rings").join("config"));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("rings")
                .join("config")
        })
}

fn expand_home(raw: &str) -> PathBuf {
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(raw.trim_start_matches(['~', '/', '\\']));
        }
    }
    Path::new(raw).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_cycle_wraps() {
        let mut value = Settings::default();
        value.theme = crate::tui::theme::names()[0].to_string();
        value.cycle_theme(-1);
        assert_eq!(value.theme, *crate::tui::theme::names().last().unwrap());
    }

    #[test]
    fn export_folder_is_created_and_checked() {
        let tmp = tempfile::TempDir::new().unwrap();
        let folder = tmp.path().join("exports");
        let mut value = Settings::default();
        value.set_export_dir(folder.to_str().unwrap()).unwrap();
        assert_eq!(value.export_dir, folder);
        assert!(folder.is_dir());
    }
}
