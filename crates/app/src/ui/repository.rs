//! The Repository tab: the branch toolbar and the history.

use std::path::Path;

use gitcraft_git::{Commit, LogScope, Ref, Refs};

use crate::commands::Command;
use crate::ui::commit_list;

const ALL_BRANCHES: &str = "All branches";
/// Most rows the open dropdown shows before it scrolls.
const DROPDOWN_HEIGHT: f32 = 400.0;

/// What the dropdown shows for a scope: "All branches", or the branch's short name.
pub fn scope_label(scope: &LogScope) -> String {
    match scope {
        LogScope::AllBranches => ALL_BRANCHES.to_string(),
        LogScope::Ref(name) => {
            let short = ["refs/heads/", "refs/remotes/", "refs/tags/"].iter().find_map(|prefix| name.strip_prefix(prefix));
            short.unwrap_or(name).to_string()
        }
    }
}

/// The dropdown's rows, in order: all branches, then local branches (the current one marked),
/// then remote branches under a heading. Pure, so the order and labels can be tested.
#[derive(Debug, PartialEq, Eq)]
pub enum BranchRow {
    Choice { scope: LogScope, label: String },
    Heading(&'static str),
}

pub fn branch_rows(refs: &Refs) -> Vec<BranchRow> {
    let choice = |r: &Ref, current: bool| BranchRow::Choice {
        scope: LogScope::Ref(r.name.clone()),
        label: if current { format!("{} (current)", r.short_name()) } else { r.short_name().to_string() },
    };
    let mut rows = vec![BranchRow::Choice { scope: LogScope::AllBranches, label: ALL_BRANCHES.to_string() }];
    let head = refs.head_branch();
    let locals: Vec<_> = refs.branches().collect();
    if !locals.is_empty() {
        rows.push(BranchRow::Heading("Local"));
        rows.extend(locals.into_iter().map(|r| choice(r, Some(r.name.as_str()) == head)));
    }
    let remotes: Vec<_> = refs.remote_branches().collect();
    if !remotes.is_empty() {
        rows.push(BranchRow::Heading("Remote"));
        rows.extend(remotes.into_iter().map(|r| choice(r, false)));
    }
    rows
}

/// The repository's display name: its folder name, or the whole path if it has none (a drive
/// root such as `E:\`).
pub fn repository_name(root: &Path) -> String {
    match root.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => root.display().to_string(),
    }
}

/// The toolbar under the tabs: branch dropdown on the left, then the repository name (full path
/// on hover). Returns the scope the user picked this frame, if they picked a different one.
pub fn toolbar(ui: &mut egui::Ui, root: &Path, refs: &Refs, selected: &LogScope) -> Option<LogScope> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label("Branch:");
        // craft-ui's combo box, not egui's (clippy.toml forbids that one; see craft-libs).
        craft_ui::ComboBox::from_id_salt("branch").selected_text(scope_label(selected)).width(220.0).height(DROPDOWN_HEIGHT).show_ui(ui, |ui| {
            for row in branch_rows(refs) {
                match row {
                    BranchRow::Heading(text) => {
                        ui.add_space(4.0);
                        ui.weak(text);
                    }
                    BranchRow::Choice { scope, label } => {
                        let is_selected = &scope == selected;
                        if ui.selectable_label(is_selected, label).clicked() && !is_selected {
                            picked = Some(scope);
                        }
                    }
                }
            }
        });
        ui.separator();
        ui.strong(repository_name(root)).on_hover_text(root.display().to_string());
    });
    picked
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

    use gitcraft_git::{Head, RefKind};

    fn r(name: &str, kind: RefKind) -> Ref {
        Ref { name: name.into(), kind, target: "a".repeat(40) }
    }

    #[test]
    fn rows_list_local_then_remote_branches_and_mark_the_current_one() {
        let refs = Refs {
            refs: vec![
                r("refs/heads/feature", RefKind::Branch),
                r("refs/heads/main", RefKind::Branch),
                r("refs/remotes/origin/main", RefKind::RemoteBranch),
                r("refs/tags/v1", RefKind::Tag),
            ],
            head: Head::Branch { name: "refs/heads/main".into(), commit: "a".repeat(40) },
        };
        let choice = |scope: LogScope, label: &str| BranchRow::Choice { scope, label: label.into() };
        assert_eq!(
            branch_rows(&refs),
            [
                choice(LogScope::AllBranches, "All branches"),
                BranchRow::Heading("Local"),
                choice(LogScope::Ref("refs/heads/feature".into()), "feature"),
                choice(LogScope::Ref("refs/heads/main".into()), "main (current)"),
                BranchRow::Heading("Remote"),
                choice(LogScope::Ref("refs/remotes/origin/main".into()), "origin/main"),
            ]
        );
    }

    #[test]
    fn no_branches_leaves_only_all_branches() {
        let refs = Refs { refs: vec![], head: Head::Unborn { name: "refs/heads/main".into() } };
        assert_eq!(branch_rows(&refs).len(), 1);
    }

    #[test]
    fn scope_labels_are_short() {
        assert_eq!(scope_label(&LogScope::AllBranches), "All branches");
        assert_eq!(scope_label(&LogScope::Ref("refs/heads/feature/x".into())), "feature/x");
        assert_eq!(scope_label(&LogScope::Ref("refs/remotes/origin/main".into())), "origin/main");
    }

    #[test]
    fn name_is_the_folder_name() {
        assert_eq!(repository_name(Path::new("/home/me/src/GitCraft")), "GitCraft");
        assert_eq!(repository_name(Path::new("/")), "/");
    }
}
