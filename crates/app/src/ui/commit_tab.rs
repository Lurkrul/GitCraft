//! The Commit tab: the working tree's changed files on the left, the selected file's diff on the
//! right.

use craft_ui::layout::SplitAxis;
use gitcraft_git::{MAX_STATUS_ENTRIES, Status, StatusEntry, WorkingChange};

use crate::ui::diff_view::{self, DiffState};
use crate::ui::file_row;
use crate::ui::split::{self, Split};

/// The Commit tab's state for the open repository.
#[derive(Debug, Default)]
pub struct Changes {
    /// `None` until the first `git status` has been read.
    pub status: Option<Status>,
    /// The selected file's path.
    pub selected: Option<String>,
    pub diff: DiffState,
    /// Why the last read failed, shown while there is no list yet.
    pub error: Option<String>,
}

impl Changes {
    pub fn entry(&self, path: &str) -> Option<&StatusEntry> {
        self.status.as_ref()?.entries.iter().find(|entry| entry.path == path)
    }
}

/// What a row shows for a change: its letter, colour and description.
fn letter(ui: &egui::Ui, change: WorkingChange) -> file_row::Letter<'static> {
    let (letter, color, tooltip) = match change {
        WorkingChange::Added => ('A', file_row::added_color(ui), "Added"),
        WorkingChange::Modified => ('M', file_row::MODIFIED_COLOR, "Modified"),
        WorkingChange::Deleted => ('D', file_row::removed_color(ui), "Deleted"),
        WorkingChange::Renamed => ('R', file_row::RENAMED_COLOR, "Renamed"),
        WorkingChange::TypeChanged => ('T', ui.visuals().text_color(), "Type changed"),
        WorkingChange::Conflicted => ('!', ui.visuals().error_fg_color, "Conflicted: resolve the merge conflict first"),
    };
    file_row::Letter { letter, color, tooltip }
}

/// "3 changed files"
pub fn count_label(count: usize) -> String {
    format!("{count} changed file{}", if count == 1 { "" } else { "s" })
}

/// Draws the tab. `ratio` is the file list's share of the width. Returns the file the user
/// clicked, if any.
pub fn show(ui: &mut egui::Ui, changes: &Changes, ratio: &mut f32) -> Option<String> {
    let Some(status) = &changes.status else {
        ui.centered_and_justified(|ui| match &changes.error {
            Some(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            None => {
                ui.spinner();
            }
        });
        return None;
    };
    if status.entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading("No local changes");
            ui.weak("Changes you make to files in this repository show up here.");
        });
        return None;
    }

    let split =
        Split { id: egui::Id::unique("changes-diff-split"), axis: SplitAxis::Horizontal, min_pane: 200.0, label: "Resize the changes and the diff" };
    let mut clicked = None;
    split::two_panes(
        ui,
        &split,
        ratio,
        |ui| clicked = file_list(ui, status, changes.selected.as_deref()),
        |ui| diff_view::show(ui, &changes.diff, ("working", changes.selected.as_deref())),
    );
    clicked
}

fn file_list(ui: &mut egui::Ui, status: &Status, selected: Option<&str>) -> Option<String> {
    ui.add_space(4.0);
    ui.label(count_label(status.entries.len()));
    if status.truncated {
        ui.weak(format!("Only the first {MAX_STATUS_ENTRIES} are listed."));
    }
    ui.separator();
    let mut clicked = None;
    egui::ScrollArea::vertical().id_salt("changed-files").auto_shrink(false).show(ui, |ui| {
        for (index, entry) in status.entries.iter().enumerate() {
            let is_selected = selected == Some(entry.path.as_str());
            let tooltip = entry.submodule.then(|| "A submodule".to_string());
            let response = file_row::show(ui, ("working-file", index), &letter(ui, entry.change()), &entry.path, tooltip, is_selected, |_| {});
            if response.clicked() && !is_selected {
                clicked = Some(entry.path.clone());
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::{Change, EntryKind};

    fn entry(path: &str, kind: EntryKind, staged: Change, unstaged: Change) -> StatusEntry {
        StatusEntry { path: path.into(), old_path: None, kind, staged, unstaged, submodule: false }
    }

    fn sample() -> Changes {
        let status = Status {
            entries: vec![
                entry("a.txt", EntryKind::Tracked, Change::Unmodified, Change::Modified),
                entry("b.txt", EntryKind::Untracked, Change::Unmodified, Change::Added),
                entry("c.txt", EntryKind::Unmerged, Change::Unmerged, Change::Unmerged),
            ],
            truncated: true,
        };
        Changes { status: Some(status), selected: Some("a.txt".into()), diff: DiffState::Loading, error: None }
    }

    #[test]
    fn counts_files() {
        assert_eq!(count_label(1), "1 changed file");
        assert_eq!(count_label(3), "3 changed files");
    }

    #[test]
    fn finds_entries_by_path() {
        let changes = sample();
        assert_eq!(changes.entry("b.txt").map(StatusEntry::change), Some(WorkingChange::Added));
        assert!(changes.entry("zzz").is_none());
    }

    #[test]
    fn draws_every_state() {
        let failed = Changes { error: Some("nope".into()), ..Changes::default() };
        let clean = Changes { status: Some(Status::default()), ..Changes::default() };
        let states = [Changes::default(), failed, clean, sample()];
        let ctx = egui::Context::default();
        let mut ratio = split::DEFAULT_CHANGES_RATIO;
        for changes in &states {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    show(ui, changes, &mut ratio);
                });
                output.textures_delta.clear();
            }
        }
    }
}
