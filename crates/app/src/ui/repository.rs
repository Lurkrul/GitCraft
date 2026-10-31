//! The Repository tab: the branch toolbar and the history.

use std::path::Path;

use gitcraft_git::Commit;

use crate::commands::Command;
use crate::ui::commit_list;

/// The branch dropdown's choice. Only "All branches" for now; real branches arrive with refs.
const ALL_BRANCHES: &str = "All branches";

/// The repository's display name: its folder name, or the whole path if it has none (a drive
/// root such as `E:\`).
pub fn repository_name(root: &Path) -> String {
    match root.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => root.display().to_string(),
    }
}

/// The toolbar under the tabs: branch dropdown on the left, then the repository name (full path
/// on hover).
pub fn toolbar(ui: &mut egui::Ui, root: &Path) {
    ui.horizontal(|ui| {
        ui.label("Branch:");
        // craft-ui's combo box, not egui's (clippy.toml forbids that one; see craft-libs).
        craft_ui::ComboBox::from_id_salt("branch").selected_text(ALL_BRANCHES).width(200.0).show_ui(ui, |ui| {
            // `selectable_label(true, ..)` shows the one entry as chosen; nothing else to pick yet.
            let _ = ui.selectable_label(true, ALL_BRANCHES);
        });
        ui.separator();
        ui.strong(repository_name(root)).on_hover_text(root.display().to_string());
    });
}

/// The history area, below the toolbar.
pub fn body(ui: &mut egui::Ui, commits: &[Commit]) {
    if commits.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("This repository has no commits yet.");
        });
    } else {
        commit_list::show(ui, commits);
    }
}

/// Shown in the Repository tab while no repository is open.
pub fn welcome(ui: &mut egui::Ui) {
    let shortcut = Command::OpenRepository.shortcut().map(|chord| ui.ctx().format_shortcut(&chord.to_egui_shortcut())).unwrap_or_default();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading(crate::app::APP_NAME);
        ui.label("No repository open.");
        // Plain words, no arrow: egui's built-in font has no "→" glyph.
        ui.label(format!("Choose {} from the File menu ({shortcut}) to open one.", Command::OpenRepository.label()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_the_folder_name() {
        assert_eq!(repository_name(Path::new("/home/me/src/GitCraft")), "GitCraft");
        assert_eq!(repository_name(Path::new("/")), "/");
    }
}
