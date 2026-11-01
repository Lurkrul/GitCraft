//! The Repository tab: the branch toolbar and the history.

use std::path::Path;

use gitcraft_git::{LogScope, Ref, Refs, SyncState};

use crate::commands::Command;
use crate::graph::History;
use craft_ui::layout::SplitAxis;

use crate::ui::branches::{self, BranchPick};
use crate::ui::details::{self, DetailsAction, DetailsState};
use crate::ui::diff_view::{self, DiffState};
use crate::ui::recent::{self, RecentItem, RepositoryPick};
use crate::ui::ref_labels::LabelMap;
use crate::ui::split::Split;
use crate::ui::sync::{self, NetOp, SyncPick};
use crate::ui::tabs::{self, RepoView};
use crate::ui::toolbar_box;
use crate::ui::{commit_list, split};

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

/// Something picked in the toolbar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarPick {
    /// Another branch (or all branches) in the branch dropdown.
    Scope(LogScope),
    /// A repository in the repository dropdown.
    Repository(RepositoryPick),
    /// Something in the Current branch dropdown.
    Branch(BranchPick),
    /// The sync button (fetch, pull, push, publish, or Cancel).
    Sync(SyncPick),
}

/// The toolbar under the tabs: the Current branch dropdown (check out and manage branches), the
/// history filter ("Show:"), then the repository dropdown (its name, full path on hover; opened,
/// the recent repositories). Returns what was picked this frame. `show_filter` is false where the
/// history isn't shown (the Commit tab).
/// What the toolbar needs to draw.
pub struct ToolbarView<'a> {
    pub root: &'a Path,
    pub refs: &'a Refs,
    /// The history filter's choice.
    pub selected: &'a LogScope,
    pub recent: &'a [RecentItem],
    /// False where the history isn't shown (the Commit tab): no filter then.
    pub show_filter: bool,
    pub sync: Option<&'a SyncState>,
    /// The network command running, if any.
    pub running: Option<&'a NetOp>,
}

pub fn toolbar(ui: &mut egui::Ui, view: &ToolbarView<'_>) -> Option<ToolbarPick> {
    let mut picked = None;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Like GitHub Desktop: repository, branch, then the Push/Pull box, side by side.
        let repository_hover = format!("{}\nClick to switch to another repository.", view.root.display());
        if let Some(pick) =
            toolbar_box::dropdown(ui, "Current repository", &repository_name(view.root), &repository_hover, |ui| recent::list(ui, view.recent))
        {
            picked = Some(ToolbarPick::Repository(pick));
        }
        let branch_hover = "Click to check out another branch, or create one. Right-click a branch in the list for more.";
        if let Some(pick) =
            toolbar_box::dropdown(ui, "Current branch", &branches::current_label(view.refs), branch_hover, |ui| branches::list(ui, view.refs))
        {
            picked = Some(ToolbarPick::Branch(pick));
        }
        if let Some(pick) = sync::sync_box(ui, view.sync, view.running) {
            picked = Some(ToolbarPick::Sync(pick));
        }
        if view.show_filter {
            ui.add_space(12.0);
            ui.label("Show:").on_hover_text("Which branch's history to show (this doesn't check anything out)");
            // craft-ui's combo box, not egui's (clippy.toml forbids that one; see craft-libs).
            craft_ui::ComboBox::from_id_salt("branch").selected_text(scope_label(view.selected)).width(200.0).height(DROPDOWN_HEIGHT).show_ui(
                ui,
                |ui| {
                    for row in branch_rows(view.refs) {
                        match row {
                            BranchRow::Heading(text) => {
                                ui.add_space(4.0);
                                ui.weak(text);
                            }
                            BranchRow::Choice { scope, label } => {
                                let is_selected = &scope == view.selected;
                                if ui.selectable_label(is_selected, label).clicked() && !is_selected {
                                    picked = Some(ToolbarPick::Scope(scope));
                                }
                            }
                        }
                    }
                },
            );
        }
    });
    ui.add_space(4.0);
    picked
}
/// What the history area needs to draw.
pub struct BodyView<'a> {
    pub history: &'a History,
    pub labels: &'a LabelMap,
    /// The selected commit's id.
    pub selected: Option<&'a str>,
    pub details: &'a DetailsState,
    /// The selected file's path, in the details' file list.
    pub selected_file: Option<&'a str>,
    pub diff: &'a DiffState,
    /// A row to scroll into view this frame (after keyboard navigation).
    pub scroll_to: Option<usize>,
}

/// The user's layout choices for the Repository tab.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoLayout {
    /// The left pane's share of the width, the same in the Changes and History views.
    pub side_ratio: f32,
    /// The details' share of the right pane's height (the diff gets the rest).
    pub details_ratio: f32,
    /// Whether the details panel's "More info" is open.
    pub details_expanded: bool,
    /// Changes or History.
    pub view: RepoView,
}

impl Default for RepoLayout {
    fn default() -> Self {
        RepoLayout {
            side_ratio: split::DEFAULT_SIDE_RATIO,
            details_ratio: split::DEFAULT_DETAILS_RATIO,
            details_expanded: false,
            view: RepoView::default(),
        }
    }
}

/// Something the user picked in the history area this frame.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    /// A row of the history list.
    Row(usize),
    /// A commit by id (a parent link in the details panel).
    Commit(String),
    /// A changed file of the selected commit, by path.
    File(String),
}

/// The history area, below the toolbar: the history on the left; on the right the selected
/// commit's details above the selected file's diff. Both dividers can be dragged.
pub fn body(ui: &mut egui::Ui, view: &BodyView<'_>, layout: &mut RepoLayout) -> Option<Pick> {
    if view.history.log.commits.is_empty() {
        tabs::view_tabs(ui, &mut layout.view);
        ui.centered_and_justified(|ui| {
            ui.label("This repository has no commits yet.");
        });
        return None;
    }
    let history_split = Split {
        id: egui::Id::unique("history-details-split"),
        axis: SplitAxis::Horizontal,
        min_pane: 220.0,
        label: "Resize the history and details",
    };
    let details_split =
        Split { id: egui::Id::unique("details-diff-split"), axis: SplitAxis::Vertical, min_pane: 80.0, label: "Resize the details and the diff" };

    // One variable per pane: the closures are alive at the same time, and Rust allows only one
    // of them to borrow a given variable mutably.
    let (mut row, mut action) = (None, None);
    // `RepoLayout`'s fields are borrowed separately, so each closure can change its own.
    let RepoLayout { side_ratio, details_ratio, details_expanded, view: repo_view } = layout;
    split::two_panes(
        ui,
        &history_split,
        side_ratio,
        |ui| {
            tabs::view_tabs(ui, repo_view);
            row = commit_list::show(ui, view.history, view.labels, view.selected, view.scroll_to);
        },
        |ui| {
            split::two_panes(
                ui,
                &details_split,
                details_ratio,
                |ui| action = details::show(ui, view.details, details_expanded, view.selected_file),
                |ui| {
                    // No checkboxes here: committed history is only looked at.
                    diff_view::show(ui, view.diff, (view.selected, view.selected_file), None);
                },
            );
        },
    );
    row.map(Pick::Row).or(action.map(|action| match action {
        DetailsAction::SelectCommit(id) => Pick::Commit(id),
        DetailsAction::SelectFile(path) => Pick::File(path),
    }))
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
