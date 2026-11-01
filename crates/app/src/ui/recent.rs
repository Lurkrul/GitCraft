//! Recent repositories: the list shown in File → Open Recent and in the toolbar's repository
//! dropdown.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ui::repository::repository_name;

/// One remembered repository, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentItem {
    pub path: PathBuf,
    /// The folder wasn't there the last time it was checked (deleted, or on a drive that isn't
    /// connected). Shown greyed out.
    pub missing: bool,
    /// The repository open now.
    pub current: bool,
}

impl RecentItem {
    /// The repository's folder name.
    pub fn name(&self) -> String {
        repository_name(&self.path)
    }

    /// The menu's label: the name, then the full path (menu items have no hover text).
    pub fn menu_label(&self) -> String {
        format!("{} — {}", self.name(), self.path.display())
    }
}

/// The list to show: the remembered paths, newest first, each marked if it is open now or
/// missing.
pub fn items(paths: &[PathBuf], current: Option<&Path>, missing: &BTreeSet<PathBuf>) -> Vec<RecentItem> {
    paths.iter().map(|path| RecentItem { path: path.clone(), missing: missing.contains(path), current: current == Some(path.as_path()) }).collect()
}

/// What was picked in the toolbar's repository dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryPick {
    Open(PathBuf),
    /// "Open other repository…": show the folder picker.
    Other,
}

/// The repository box's list: the recent repositories (the open one marked, missing ones greyed
/// out; full path on hover), then "Open other repository…".
pub fn list(ui: &mut egui::Ui, recent: &[RecentItem]) -> Option<RepositoryPick> {
    let mut picked = None;
    for item in recent {
        let row = ui.add_enabled(!item.missing, egui::Button::selectable(item.current, item.name()));
        let row = if item.missing {
            row.on_disabled_hover_text(format!("{}\nThis folder can't be found.", item.path.display()))
        } else {
            row.on_hover_text(item.path.display().to_string())
        };
        if row.clicked() && !item.current {
            picked = Some(RepositoryPick::Open(item.path.clone()));
        }
    }
    if !recent.is_empty() {
        ui.separator();
    }
    if ui.button("Open other repository…").clicked() {
        picked = Some(RepositoryPick::Other);
    }
    picked
}
#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn items_mark_the_open_and_missing_ones() {
        let missing = BTreeSet::from([path("gone")]);
        let list = items(&[path("open"), path("gone"), path("other")], Some(&path("open")), &missing);
        let flags: Vec<_> = list.iter().map(|item| (item.name(), item.current, item.missing)).collect();
        assert_eq!(flags, [("open".to_string(), true, false), ("gone".to_string(), false, true), ("other".to_string(), false, false)]);
        assert!(list[0].menu_label().starts_with("open — "), "{}", list[0].menu_label());
    }

    #[test]
    fn the_list_draws_with_and_without_entries() {
        let ctx = egui::Context::default();
        let entries = items(&[path("a"), path("b")], Some(&path("a")), &BTreeSet::from([path("b")]));
        for recent in [entries.as_slice(), &[]] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert_eq!(list(ui, recent), None, "nothing picked without a click");
            });
            output.textures_delta.clear();
        }
    }
}
