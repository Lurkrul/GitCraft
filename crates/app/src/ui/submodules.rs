//! Submodules: the Repository → Submodules… window (each submodule with its state and Open,
//! Initialise and Update), the "parent / submodule" trail shown while one is open, and the summary
//! the Changes view shows for a changed submodule.

use std::path::PathBuf;

use gitcraft_git::{FileDiff, LineKind, Submodule};

use crate::ui::repository::repository_name;

/// The window's contents: `None` while the list is being read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubmodulesWindow {
    pub list: Option<Result<Vec<Submodule>, String>>,
}

/// What was picked in the window (submodule paths are relative to the open repository).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmodulePick {
    Open(String),
    Init(String),
    Update(String),
    Close,
}

/// A submodule's state in words.
pub fn state_text(sub: &Submodule) -> String {
    let short = |commit: &str| commit.get(..7).unwrap_or(commit).to_string();
    match &sub.checked_out {
        None => "Not initialised (not cloned yet)".to_string(),
        Some(commit) if *commit != sub.recorded => {
            let changes = if sub.dirty { ", with uncommitted changes" } else { "" };
            format!("Out of date: {} checked out, {} recorded{changes}", short(commit), short(&sub.recorded))
        }
        Some(_) if sub.dirty => "Up to date, with uncommitted changes".to_string(),
        Some(_) => "Up to date".to_string(),
    }
}

/// Draws the window; returns what was picked this frame.
pub fn window(ctx: &egui::Context, state: &SubmodulesWindow) -> Option<SubmodulePick> {
    let mut picked = None;
    let mut open = true;
    egui::Window::new("Submodules").id(egui::Id::unique("gitcraft-submodules")).open(&mut open).collapsible(false).default_width(520.0).show(
        ctx,
        |ui| match &state.list {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading the submodules…");
                });
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            Some(Ok(list)) if list.is_empty() => {
                ui.weak("This repository has no submodules.");
            }
            Some(Ok(list)) => {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    for sub in list {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.strong(&sub.path);
                                ui.weak(state_text(sub));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let update = ui
                                    .add_enabled(sub.out_of_date(), egui::Button::new("Update…"))
                                    .on_hover_text("Check out the commit this repository records for it")
                                    .on_disabled_hover_text("Already at the recorded commit (or not initialised).");
                                if update.clicked() {
                                    picked = Some(SubmodulePick::Update(sub.path.clone()));
                                }
                                if !sub.initialized() && ui.button("Initialise").on_hover_text("Clone it and check out the recorded commit").clicked()
                                {
                                    picked = Some(SubmodulePick::Init(sub.path.clone()));
                                }
                                let open =
                                    ui.add_enabled(sub.initialized(), egui::Button::new("Open")).on_disabled_hover_text("Initialise it first.");
                                if open.clicked() {
                                    picked = Some(SubmodulePick::Open(sub.path.clone()));
                                }
                            });
                        });
                        ui.separator();
                    }
                });
            }
        },
    );
    if !open {
        picked = Some(SubmodulePick::Close);
    }
    picked
}

/// The "parent / submodule" trail: each parent is a link back to it. Returns the index of the
/// parent clicked.
pub fn breadcrumb(ui: &mut egui::Ui, parents: &[PathBuf], current: &std::path::Path) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal(|ui| {
        for (index, parent) in parents.iter().enumerate() {
            if ui.link(repository_name(parent)).on_hover_text(format!("Back to {}", parent.display())).clicked() {
                clicked = Some(index);
            }
            ui.weak("/");
        }
        ui.strong(repository_name(current)).on_hover_text("A submodule of the repository before it");
    });
    clicked
}

/// What a changed submodule's diff says: the commit recorded in HEAD, the one checked out now,
/// and whether it has uncommitted changes (`Subproject commit <id>[-dirty]` lines).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubmoduleChange {
    pub recorded: Option<String>,
    pub checked_out: Option<String>,
    pub dirty: bool,
}

pub fn submodule_change(diff: &FileDiff) -> SubmoduleChange {
    let mut change = SubmoduleChange::default();
    for line in diff.hunks.iter().flat_map(|hunk| &hunk.lines) {
        let Some(commit) = line.text.strip_prefix("Subproject commit ") else {
            continue;
        };
        let (commit, dirty) = match commit.strip_suffix("-dirty") {
            Some(commit) => (commit, true),
            None => (commit, false),
        };
        match line.kind {
            LineKind::Removed => change.recorded = Some(commit.to_string()),
            LineKind::Added => {
                change.checked_out = Some(commit.to_string());
                change.dirty |= dirty;
            }
            LineKind::Context | LineKind::NoNewline => {}
        }
    }
    change
}

/// The Changes view's panel for a changed submodule (instead of a diff). Returns true if "Open
/// submodule" was clicked.
pub fn change_panel(ui: &mut egui::Ui, path: &str, change: &SubmoduleChange) -> bool {
    let short = |commit: &Option<String>| commit.as_deref().map(|c| c.get(..7).unwrap_or(c).to_string()).unwrap_or_else(|| "none".into());
    let mut open = false;
    ui.vertical(|ui| {
        ui.add_space(12.0);
        ui.heading(format!("Submodule {path}"));
        ui.add_space(6.0);
        ui.label(format!("Recorded in the last commit: {}", short(&change.recorded)));
        let dirty = if change.dirty { " (with uncommitted changes inside it)" } else { "" };
        ui.label(format!("Checked out now: {}{dirty}", short(&change.checked_out)));
        ui.add_space(6.0);
        ui.weak("Ticking it commits the checked-out commit as this repository's new recorded commit. Changes inside the submodule are committed in the submodule itself: open it to see them.");
        ui.add_space(8.0);
        open = ui.button("Open submodule").clicked();
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::{DiffLine, Hunk};

    fn sub(checked_out: Option<char>, dirty: bool) -> Submodule {
        Submodule { path: "lib".into(), recorded: "a".repeat(40), checked_out: checked_out.map(|c| c.to_string().repeat(40)), dirty }
    }

    #[test]
    fn states_in_words() {
        assert!(state_text(&sub(None, false)).starts_with("Not initialised"));
        assert_eq!(state_text(&sub(Some('a'), false)), "Up to date");
        assert_eq!(state_text(&sub(Some('a'), true)), "Up to date, with uncommitted changes");
        assert_eq!(state_text(&sub(Some('b'), true)), "Out of date: bbbbbbb checked out, aaaaaaa recorded, with uncommitted changes");
    }

    #[test]
    fn reads_a_submodule_diff() {
        let line = |kind, text: &str| DiffLine { kind, old_line: None, new_line: None, text: text.into() };
        let diff = FileDiff {
            hunks: vec![Hunk {
                header: "@@ -1 +1 @@".into(),
                lines: vec![
                    line(LineKind::Removed, &format!("Subproject commit {}", "a".repeat(40))),
                    line(LineKind::Added, &format!("Subproject commit {}-dirty", "b".repeat(40))),
                ],
            }],
            ..FileDiff::default()
        };
        assert_eq!(submodule_change(&diff), SubmoduleChange { recorded: Some("a".repeat(40)), checked_out: Some("b".repeat(40)), dirty: true });
        assert_eq!(submodule_change(&FileDiff::default()), SubmoduleChange::default());
    }

    #[test]
    fn draws_every_state() {
        let ctx = egui::Context::default();
        let states = [
            SubmodulesWindow { list: None },
            SubmodulesWindow { list: Some(Err("nope".into())) },
            SubmodulesWindow { list: Some(Ok(vec![])) },
            SubmodulesWindow { list: Some(Ok(vec![sub(None, false), sub(Some('b'), true)])) },
        ];
        for state in &states {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    assert_eq!(window(ui.ctx(), state), None);
                    assert_eq!(breadcrumb(ui, &[PathBuf::from("parent")], std::path::Path::new("parent/lib")), None);
                    assert!(!change_panel(ui, "lib", &SubmoduleChange::default()));
                });
                output.textures_delta.clear();
            }
        }
    }
}
