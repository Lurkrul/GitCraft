//! A changed file's right-click menu, and the question asked before discarding.

use std::path::PathBuf;

use gitcraft_git::{EntryKind, StatusEntry, WorkingChange};

use super::FileCommand;
use crate::files;
use crate::ui::confirm::Confirm;

/// A menu item that closes the menu when clicked.
fn menu_item(ui: &mut egui::Ui, enabled: bool, label: &str, disabled_reason: &str) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).on_disabled_hover_text(disabled_reason).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A changed file's right-click menu. Returns the choice, on the frame it is made.
pub fn file_menu(ui: &mut egui::Ui, entry: &StatusEntry, programs: &[PathBuf]) -> Option<FileCommand> {
    let mut chosen = None;
    let untracked = entry.kind == EntryKind::Untracked;
    // A deleted file isn't on disk, so there is nothing to show or open.
    let on_disk = entry.change() != WorkingChange::Deleted;

    if menu_item(ui, !entry.submodule, "Discard changes…", "To put a submodule back to its recorded commit, use Update in Repository → Submodules….")
    {
        chosen = Some(FileCommand::Discard);
    }
    ui.separator();
    let not_untracked = "Only new (untracked) files can be ignored: git keeps tracking files it already has.";
    if menu_item(ui, untracked, "Ignore file", not_untracked) {
        chosen = Some(FileCommand::IgnoreFile);
    }
    if let Some(extension) = files::extension_of(&entry.path)
        && menu_item(ui, untracked, &format!("Ignore all .{extension} files"), not_untracked)
    {
        chosen = Some(FileCommand::IgnoreExtension(extension));
    }
    ui.separator();
    if menu_item(ui, true, "Copy path", "") {
        chosen = Some(FileCommand::CopyPath);
    }
    if menu_item(ui, true, "Copy relative path", "") {
        chosen = Some(FileCommand::CopyRelativePath);
    }
    ui.separator();
    let gone = "The file has been deleted.";
    if menu_item(ui, on_disk, &format!("Show in {}", files::FILE_MANAGER_NAME), gone) {
        chosen = Some(FileCommand::Reveal);
    }
    if menu_item(ui, on_disk, "Open", gone) {
        chosen = Some(FileCommand::Open);
    }
    ui.add_enabled_ui(on_disk, |ui| {
        ui.menu_button("Open with", |ui| {
            for program in programs {
                let name = program.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_else(|| program.display().to_string());
                let clicked = ui.button(name).on_hover_text(program.display().to_string()).clicked();
                if clicked {
                    ui.close();
                    chosen = Some(FileCommand::OpenWith(program.clone()));
                }
            }
            if !programs.is_empty() {
                ui.separator();
            }
            if menu_item(ui, true, "Choose a program…", "") {
                chosen = Some(FileCommand::ChooseProgram);
            }
            if files::HAS_SYSTEM_OPEN_WITH && menu_item(ui, true, "Choose another app (Windows)…", "") {
                chosen = Some(FileCommand::SystemOpenWith);
            }
        });
    });
    chosen
}

/// The confirmation shown before discarding a file's changes: it says exactly what will be lost
/// and where the current file goes.
pub fn discard_question(entry: &StatusEntry) -> Confirm {
    let path = &entry.path;
    let trash = files::TRASH_NAME;
    let body = match (entry.kind, entry.change()) {
        (EntryKind::Untracked, _) => format!("\"{path}\" is a new file that git doesn't track. It will be moved to the {trash}."),
        (_, WorkingChange::Deleted) => format!("\"{path}\" was deleted. Discarding brings back the version from the last commit."),
        (_, WorkingChange::Added) => {
            format!("\"{path}\" is new since the last commit. It will be removed from the repository and moved to the {trash}.")
        }
        _ => format!(
            "All changes to \"{path}\" since the last commit will be lost, staged ones included: the file goes back to how it was \
             in the last commit. The current file is moved to the {trash} first, so you can get it back from there."
        ),
    };
    Confirm { title: "Discard changes?".into(), body, action: "Discard changes".into() }
}

/// The confirmation shown before discarding some lines of a file: how many added and removed lines,
/// and what happens to them.
pub fn discard_lines_question(path: &str, added: usize, removed: usize) -> Confirm {
    let count = |n: usize, what: &str| format!("{n} {what} line{}", if n == 1 { "" } else { "s" });
    let what = match (added, removed) {
        (0, removed) => count(removed, "removed"),
        (added, 0) => count(added, "added"),
        (added, removed) => format!("{} and {}", count(added, "added"), count(removed, "removed")),
    };
    let body = format!(
        "The changes of {what} in \"{path}\" will be lost: added lines are taken out and removed lines put back, as in the last \
         commit. The file's other changes stay. A copy of the current file goes to the {} first, so you can get it back from there.",
        files::TRASH_NAME
    );
    Confirm { title: "Discard lines?".into(), body, action: "Discard lines".into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::Change;

    fn entry(path: &str, kind: EntryKind, staged: Change, unstaged: Change) -> StatusEntry {
        StatusEntry { path: path.into(), old_path: None, kind, staged, unstaged, submodule: false }
    }

    fn unstaged(path: &str) -> StatusEntry {
        entry(path, EntryKind::Tracked, Change::Unmodified, Change::Modified)
    }

    #[test]
    fn discard_questions_say_what_happens() {
        let edited = discard_question(&unstaged("a.txt"));
        assert!(edited.body.contains("will be lost") && edited.body.contains(files::TRASH_NAME), "{}", edited.body);
        let new = discard_question(&entry("n.txt", EntryKind::Untracked, Change::Unmodified, Change::Added));
        assert!(new.body.contains("new file"), "{}", new.body);
        let deleted = discard_question(&entry("d.txt", EntryKind::Tracked, Change::Unmodified, Change::Deleted));
        assert!(deleted.body.contains("brings back"), "{}", deleted.body);
        assert_eq!(edited.action, "Discard changes");
    }

    #[test]
    fn discard_lines_questions_count_what_goes() {
        assert!(discard_lines_question("a.txt", 1, 0).body.contains("1 added line in \"a.txt\""));
        assert!(discard_lines_question("a.txt", 0, 3).body.contains("3 removed lines"));
        let both = discard_lines_question("a.txt", 2, 1);
        assert!(both.body.contains("2 added lines and 1 removed line") && both.body.contains(files::TRASH_NAME), "{}", both.body);
        assert_eq!(both.action, "Discard lines");
    }

    #[test]
    fn the_menu_draws_for_every_kind_of_file() {
        let entries = [
            unstaged("src/a.rs"),
            entry("new.log", EntryKind::Untracked, Change::Unmodified, Change::Added),
            entry("gone.txt", EntryKind::Tracked, Change::Unmodified, Change::Deleted),
            StatusEntry { submodule: true, ..unstaged("vendor/lib") },
        ];
        let ctx = egui::Context::default();
        for entry in &entries {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert_eq!(file_menu(ui, entry, &[PathBuf::from("code")]), None, "nothing is chosen without a click");
            });
            output.textures_delta.clear();
        }
    }
}
