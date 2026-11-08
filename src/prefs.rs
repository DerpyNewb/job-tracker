//! Settings and the recent-files list. Saved by eframe between runs (in its own storage file under
//! %APPDATA%, not in the synced folder).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::DEFAULT_STALE_DAYS;
use crate::store::KEEP_BACKUPS;
pub use crate::theme::Accent;

/// eframe's app id: names the settings folder, `%APPDATA%\Job tracker\data`.
pub const APP_ID: &str = "Job tracker";
/// The key the settings are stored under in eframe's `app.ron`.
pub const PREFS_KEY: &str = "prefs";

/// How many workbooks the Recent list remembers.
pub const MAX_RECENT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Match Windows",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Most recent first.
    pub recent: Vec<PathBuf>,
    /// Open the most recent workbook when the app starts.
    pub reopen_last: bool,
    pub theme: ThemeChoice,
    pub accent: Accent,
    /// 1.0 is normal size.
    pub text_scale: f32,
    /// Days at Applied or Screening, with no follow-up date, before a row needs attention.
    pub stale_days: i64,
    /// When adding an application, set its follow-up date this many days ahead. 0 leaves it empty.
    pub follow_up_after_days: i64,
    pub keep_backups: usize,
    /// Where the Resume sent list comes from. None looks for `resumes/` beside `job-search/`.
    pub resumes_dir: Option<PathBuf>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            recent: Vec::new(),
            reopen_last: true,
            theme: ThemeChoice::System,
            accent: Accent::Pine,
            text_scale: 1.0,
            stale_days: DEFAULT_STALE_DAYS,
            follow_up_after_days: 0,
            keep_backups: KEEP_BACKUPS,
            resumes_dir: None,
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    // Windows paths are case-insensitive.
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

impl Prefs {
    /// Puts `path` at the top of the Recent list.
    pub fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| !same_file(p, path));
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn forget(&mut self, path: &Path) {
        self.recent.retain(|p| !same_file(p, path));
    }

    /// The workbook to open at start, if the setting is on and the file is still there.
    pub fn startup_file(&self) -> Option<&Path> {
        if !self.reopen_last {
            return None;
        }
        self.recent
            .first()
            .map(PathBuf::as_path)
            .filter(|p| p.exists())
    }

    /// The settings the window saved last, for `jt`. Defaults when the window never ran.
    pub fn load_saved() -> Prefs {
        let kv: std::collections::HashMap<String, String> = eframe::storage_dir(APP_ID)
            .and_then(|d| std::fs::read_to_string(d.join("app.ron")).ok())
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default();
        let mut p: Prefs = kv
            .get(PREFS_KEY)
            .and_then(|s| ron::from_str(s).ok())
            .unwrap_or_default();
        p.clamp();
        p
    }

    /// Keeps values that came from an older or hand-edited settings file in range.
    pub fn clamp(&mut self) {
        self.text_scale = self.text_scale.clamp(0.8, 1.6);
        self.stale_days = self.stale_days.clamp(1, 120);
        self.follow_up_after_days = self.follow_up_after_days.clamp(0, 90);
        self.keep_backups = self.keep_backups.clamp(1, 200);
        self.recent.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_moves_to_top_without_duplicates() {
        let mut p = Prefs::default();
        p.remember(Path::new(r"C:\a\Jobs.xlsx"));
        p.remember(Path::new(r"C:\b\Other.xlsx"));
        p.remember(Path::new(r"c:\A\jobs.XLSX"));
        assert_eq!(p.recent.len(), 2);
        assert_eq!(p.recent[0], PathBuf::from(r"c:\A\jobs.XLSX"));
    }

    #[test]
    fn recent_list_is_capped() {
        let mut p = Prefs::default();
        for i in 0..12 {
            p.remember(Path::new(&format!(r"C:\w{i}.xlsx")));
        }
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert_eq!(p.recent[0], PathBuf::from(r"C:\w11.xlsx"));
    }

    #[test]
    fn startup_file_respects_the_setting_and_skips_missing_files() {
        let real = std::env::temp_dir().join(format!("jt-prefs-{}.xlsx", std::process::id()));
        std::fs::write(&real, b"x").unwrap();
        let mut p = Prefs::default();
        p.remember(&real);
        assert_eq!(p.startup_file(), Some(real.as_path()));
        p.reopen_last = false;
        assert_eq!(p.startup_file(), None);
        p.reopen_last = true;
        p.remember(Path::new(r"C:\does-not-exist\gone.xlsx"));
        assert_eq!(
            p.startup_file(),
            None,
            "the most recent one is gone, so nothing opens"
        );
        std::fs::remove_file(real).unwrap();
    }

    #[test]
    fn old_settings_files_fill_in_defaults() {
        // eframe stores settings as RON. Fields missing from an older file take their defaults.
        let p: Prefs = ron::from_str("(stale_days: 10)").unwrap();
        assert_eq!(p.stale_days, 10);
        assert!(p.reopen_last);
        assert_eq!(p.keep_backups, KEEP_BACKUPS);
    }
}
