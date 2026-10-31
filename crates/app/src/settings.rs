//! GitCraft's settings, stored by craft-settings in `config.toml`
//! (`%APPDATA%\gitcraft\` on Windows, `~/.config/gitcraft/` on Linux).

use std::path::{Path, PathBuf};

use craft_settings::{DocumentMut, LoadReport, MigrateError, Settings};
use serde::{Deserialize, Serialize};

use crate::ui::repository::RepoLayout;
use crate::ui::split;

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
    pub layout: LayoutPrefs,
    pub repositories: RepositoryPrefs,
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

/// The dividers of the Repository and Commit tabs, and the details panel's "More info" choice.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct LayoutPrefs {
    /// The history's share of the width.
    pub history_ratio: f64,
    /// The details' share of the right pane's height.
    pub details_ratio: f64,
    pub details_expanded: bool,
    /// The Commit tab's changed-files list's share of the width.
    pub changes_ratio: f64,
}

impl Default for LayoutPrefs {
    fn default() -> Self {
        LayoutPrefs::from(&RepoLayout::default())
    }
}

// `From` is Rust's conversion trait, like a converting constructor in C++.
impl From<&RepoLayout> for LayoutPrefs {
    fn from(layout: &RepoLayout) -> Self {
        // Rounded to 3 places in an f64 so the file reads `0.4`, not the f32's `0.4000000059604645`.
        let round = |ratio: f32| (f64::from(ratio) * 1000.0).round() / 1000.0;
        LayoutPrefs {
            history_ratio: round(layout.history_ratio),
            details_ratio: round(layout.details_ratio),
            details_expanded: layout.details_expanded,
            changes_ratio: round(layout.changes_ratio),
        }
    }
}

impl LayoutPrefs {
    /// The saved layout made safe to use: ratios from the file can be anything.
    pub fn to_layout(&self) -> RepoLayout {
        // `as f32` turns huge values into infinity and keeps NaN; `sane_ratio` handles both.
        RepoLayout {
            history_ratio: split::sane_ratio(self.history_ratio as f32),
            details_ratio: split::sane_ratio(self.details_ratio as f32),
            details_expanded: self.details_expanded,
            changes_ratio: split::sane_ratio(self.changes_ratio as f32),
        }
    }
}

/// Most repositories remembered; the oldest is forgotten first.
pub const MAX_RECENT_REPOSITORIES: usize = 20;

/// Repositories opened before, most recent first. The first is reopened at startup.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct RepositoryPrefs {
    pub recent: Vec<RecentRepository>,
}

/// One remembered repository.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct RecentRepository {
    /// The repository's top folder. A `String`, not a `PathBuf`: TOML can only hold text, so a
    /// path that isn't valid Unicode is simply not remembered.
    pub path: String,
    /// The branch shown in the dropdown (a full ref name such as `refs/heads/main`), or `None`
    /// for "All branches".
    pub branch: Option<String>,
}

impl RecentRepository {
    /// Whether this entry is the folder `root`. Compared as paths, not text, so `E:/src/x` and
    /// `E:\src\x` (a hand-edited file, say) are the same repository.
    fn is(&self, root: &Path) -> bool {
        Path::new(&self.path) == root
    }
}

impl RepositoryPrefs {
    /// The repository to reopen at startup.
    pub fn last(&self) -> Option<PathBuf> {
        self.recent.first().filter(|recent| !recent.path.is_empty()).map(|recent| PathBuf::from(&recent.path))
    }

    /// The branch remembered for `root`, if any.
    pub fn branch_for(&self, root: &Path) -> Option<&str> {
        self.recent.iter().find(|recent| recent.is(root))?.branch.as_deref()
    }

    /// Records that `root` was opened: it moves to the front (keeping its branch) and the list is
    /// trimmed to its maximum. A non-Unicode path is skipped.
    pub fn opened(&mut self, root: &Path) {
        let Some(path) = root.to_str() else {
            return;
        };
        let branch = self.recent.iter().position(|recent| recent.is(root)).and_then(|index| self.recent.remove(index).branch);
        self.recent.insert(0, RecentRepository { path: path.to_string(), branch });
        self.recent.truncate(MAX_RECENT_REPOSITORIES);
    }

    /// Records the branch shown for `root` (`None` for all branches), if `root` is remembered.
    pub fn set_branch(&mut self, root: &Path, branch: Option<String>) {
        if let Some(recent) = self.recent.iter_mut().find(|recent| recent.is(root)) {
            recent.branch = branch;
        }
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

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn opened_repositories_move_to_the_front_and_keep_their_branch() {
        let mut repos = RepositoryPrefs::default();
        repos.opened(&root("a"));
        repos.set_branch(&root("a"), Some("refs/heads/feature".into()));
        repos.opened(&root("b"));
        assert_eq!(repos.last(), Some(root("b")));
        repos.opened(&root("a"));
        assert_eq!(repos.last(), Some(root("a")));
        assert_eq!(repos.recent.len(), 2, "no duplicates");
        assert_eq!(repos.branch_for(&root("a")), Some("refs/heads/feature"));
        assert_eq!(repos.branch_for(&root("b")), None);
    }

    #[test]
    fn the_recent_list_is_capped() {
        let mut repos = RepositoryPrefs::default();
        for n in 0..MAX_RECENT_REPOSITORIES + 5 {
            repos.opened(&root(&format!("repo{n}")));
        }
        assert_eq!(repos.recent.len(), MAX_RECENT_REPOSITORIES);
        assert_eq!(repos.last(), Some(root(&format!("repo{}", MAX_RECENT_REPOSITORIES + 4))));
    }

    #[test]
    fn a_branch_for_an_unknown_repository_is_ignored() {
        let mut repos = RepositoryPrefs::default();
        repos.set_branch(&root("never-opened"), Some("refs/heads/main".into()));
        assert!(repos.recent.is_empty());
        assert_eq!(repos.last(), None);
    }

    #[test]
    fn an_empty_saved_path_is_not_reopened() {
        let repos = RepositoryPrefs { recent: vec![RecentRepository::default()] };
        assert_eq!(repos.last(), None);
    }

    #[cfg(windows)]
    #[test]
    fn either_slash_finds_the_same_repository() {
        let mut repos = RepositoryPrefs { recent: vec![RecentRepository { path: "E:/src/demo".into(), branch: Some("refs/heads/x".into()) }] };
        assert_eq!(repos.branch_for(Path::new(r"E:\src\demo")), Some("refs/heads/x"));
        repos.opened(Path::new(r"E:\src\demo"));
        assert_eq!(repos.recent.len(), 1, "no duplicate entry");
        assert_eq!(repos.recent[0].branch.as_deref(), Some("refs/heads/x"));
    }

    #[test]
    fn saved_ratios_are_rounded() {
        let layout = RepoLayout { history_ratio: 0.4, details_ratio: 0.123_456, details_expanded: false, changes_ratio: 0.3 };
        let prefs = LayoutPrefs::from(&layout);
        assert_eq!((prefs.history_ratio, prefs.details_ratio), (0.4, 0.123));
    }

    #[test]
    fn damaged_ratios_are_made_sane() {
        let prefs = LayoutPrefs { history_ratio: f64::NAN, details_ratio: 40.0, details_expanded: true, changes_ratio: -1.0 };
        let layout = prefs.to_layout();
        assert_eq!(layout.history_ratio, 0.5);
        assert_eq!(layout.details_ratio, 0.9);
        assert!(layout.details_expanded);
        assert_eq!(layout.changes_ratio, 0.1);
    }

    #[test]
    fn layout_and_repositories_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let layout = LayoutPrefs { history_ratio: 0.3, details_ratio: 0.6, details_expanded: true, changes_ratio: 0.2 };
        {
            let (mut store, _) = SettingsStore::<Prefs>::open_at(dir.path());
            store.update(|p| {
                p.layout = layout.clone();
                p.repositories.opened(&root("a"));
                p.repositories.set_branch(&root("a"), Some("refs/heads/main".into()));
            });
            store.flush().unwrap();
        }
        let (store, report) = SettingsStore::<Prefs>::open_at(dir.path());
        assert_eq!(load_problem_message(&report), None);
        assert_eq!(store.get().layout, layout);
        assert_eq!(store.get().repositories.last(), Some(root("a")));
        assert_eq!(store.get().repositories.branch_for(&root("a")), Some("refs/heads/main"));
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
