//! GitCraft's settings, stored by craft-settings in `config.toml`
//! (`%APPDATA%\gitcraft\` on Windows, `~/.config/gitcraft/` on Linux).

use craft_settings::{DocumentMut, LoadReport, MigrateError, Settings};
use serde::{Deserialize, Serialize};

/// Smallest window we allow (also the window's minimum size).
pub const MIN_WINDOW_SIZE: [f32; 2] = [480.0, 320.0];
/// Largest width or height accepted from the file; anything bigger is a damaged value.
const MAX_WINDOW_SIDE: f32 = 16_384.0;
const DEFAULT_WINDOW_SIZE: [f32; 2] = [1100.0, 700.0];

/// Everything GitCraft saves. Each field becomes a `[section]` in `config.toml`.
///
/// `#[serde(default)]` makes a missing key fall back to its default instead of failing the load
/// (craft-settings' rule for settings types).
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Prefs {
    pub window: WindowPrefs,
}

/// The main window's size, in egui points (logical pixels, independent of display scaling).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct WindowPrefs {
    /// Size when not maximized, so un-maximizing after a restart returns to it.
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self { width: DEFAULT_WINDOW_SIZE[0], height: DEFAULT_WINDOW_SIZE[1], maximized: false }
    }
}

impl WindowPrefs {
    /// The saved size made safe to use: the file can be hand-edited or damaged, so a value that
    /// isn't a finite number falls back to the default, and the rest is clamped to a sane range.
    pub fn sanitized_size(&self) -> [f32; 2] {
        let side = |value: f32, default: f32, min: f32| if value.is_finite() { value.clamp(min, MAX_WINDOW_SIDE) } else { default };
        [side(self.width, DEFAULT_WINDOW_SIZE[0], MIN_WINDOW_SIZE[0]), side(self.height, DEFAULT_WINDOW_SIZE[1], MIN_WINDOW_SIZE[1])]
    }
}

// A trait implementation: craft-settings is generic over any type that implements `Settings`,
// like a C++ template constrained by a concept. The constants pick the folder and file version.
impl Settings for Prefs {
    const APP_ID: &'static str = "gitcraft";
    const SCHEMA_VERSION: u32 = 1;

    fn migrate(_from: u32, _doc: &mut DocumentMut) -> Result<(), MigrateError> {
        // Version 1 is the first; there is nothing older to upgrade.
        Ok(())
    }
}

/// A one-line message for the status bar if loading the settings hit a problem, else `None`.
pub fn load_problem_message(report: &LoadReport) -> Option<String> {
    if let Some(reason) = &report.read_only {
        return Some(format!("Settings won't be saved: {reason}"));
    }
    if let Some(path) = &report.broken_moved_to {
        return Some(format!("Settings file was damaged and has been reset; the old file is at {}", path.display()));
    }
    if let Some(first) = report.section_errors.first() {
        return Some(format!("Settings section [{}] was invalid and has been reset: {}", first.section, first.message));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_settings::SettingsStore;

    #[test]
    fn sanitized_size_keeps_normal_values() {
        let prefs = WindowPrefs { width: 1280.0, height: 800.0, maximized: false };
        assert_eq!(prefs.sanitized_size(), [1280.0, 800.0]);
    }

    #[test]
    fn sanitized_size_clamps_and_rejects_damaged_values() {
        let tiny = WindowPrefs { width: 10.0, height: -5.0, maximized: false };
        assert_eq!(tiny.sanitized_size(), MIN_WINDOW_SIZE);

        let huge = WindowPrefs { width: 1e9, height: f32::INFINITY, maximized: false };
        assert_eq!(huge.sanitized_size(), [MAX_WINDOW_SIDE, DEFAULT_WINDOW_SIZE[1]]);

        let nan = WindowPrefs { width: f32::NAN, height: f32::NAN, maximized: false };
        assert_eq!(nan.sanitized_size(), DEFAULT_WINDOW_SIZE);
    }

    #[test]
    fn window_size_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (mut store, report) = SettingsStore::<Prefs>::open_at(dir.path());
            assert_eq!(load_problem_message(&report), None);
            store.update(|p| p.window = WindowPrefs { width: 1234.0, height: 567.0, maximized: true });
            store.flush().unwrap();
        }
        let (store, _) = SettingsStore::<Prefs>::open_at(dir.path());
        assert_eq!(store.get().window, WindowPrefs { width: 1234.0, height: 567.0, maximized: true });
    }

    #[test]
    fn damaged_file_falls_back_to_defaults_with_a_message() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "this is [not valid toml").unwrap();

        let (store, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert_eq!(store.get(), &Prefs::default());
        let message = load_problem_message(&report).unwrap();
        assert!(message.contains("damaged"), "{message}");
    }

    #[test]
    fn wrong_type_resets_only_that_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "schema_version = 1\n[window]\nwidth = \"wide\"\n").unwrap();

        let (store, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert_eq!(store.get().window, WindowPrefs::default());
        assert!(load_problem_message(&report).unwrap().contains("[window]"));
    }
}
