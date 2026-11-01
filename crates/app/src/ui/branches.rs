//! Local branches: the toolbar's "Current branch" dropdown (check out, new, rename, delete) and the
//! dialog that asks for a branch name.

use gitcraft_git::Refs;

/// Most rows the open dropdown shows before it scrolls.
const DROPDOWN_HEIGHT: f32 = 400.0;

/// A change to the local branches, carried out by the background worker. Names are short
/// (`feature/login`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOp {
    /// Create `name` at `start` (a full ref name or commit hash; `None` for HEAD), and check it out
    /// if `checkout`.
    Create {
        name: String,
        start: Option<String>,
        checkout: bool,
    },
    Switch {
        name: String,
    },
    Rename {
        old: String,
        new: String,
    },
    Delete {
        name: String,
    },
}

/// What was picked in the "Current branch" dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchPick {
    Checkout(String),
    /// "New branch…", from HEAD (`None`) or from a branch (its full ref name).
    New {
        from: Option<String>,
    },
    Rename(String),
    Delete(String),
}

/// The checked-out branch's short name for the button, or what HEAD is instead.
pub fn current_label(refs: &Refs) -> String {
    match refs.head_branch() {
        Some(name) => name.strip_prefix("refs/heads/").unwrap_or(name).to_string(),
        None => "(detached HEAD)".to_string(),
    }
}

/// The "Current branch" dropdown: "New branch…", then the local branches (the current one
/// checked). Clicking a branch checks it out; right-clicking one offers more.
pub fn current_branch(ui: &mut egui::Ui, refs: &Refs) -> Option<BranchPick> {
    let mut picked = None;
    let current = refs.head_branch().map(str::to_string);
    craft_ui::ComboBox::from_id_salt("current-branch")
        .selected_text(egui::RichText::new(current_label(refs)).strong())
        .width(200.0)
        .height(DROPDOWN_HEIGHT)
        .show_ui(ui, |ui| {
            if ui.button("New branch…").clicked() {
                picked = Some(BranchPick::New { from: None });
            }
            ui.separator();
            for branch in refs.branches() {
                let is_current = current.as_deref() == Some(branch.name.as_str());
                let name = branch.short_name().to_string();
                let row = ui.add(egui::Button::selectable(is_current, &name)).on_hover_text(if is_current {
                    "The checked-out branch. Right-click for more."
                } else {
                    "Click to check out. Right-click for more."
                });
                if row.clicked() && !is_current {
                    picked = Some(BranchPick::Checkout(name.clone()));
                }
                row.context_menu(|ui| {
                    if let Some(pick) = branch_menu(ui, &name, &branch.name, is_current) {
                        picked = Some(pick);
                    }
                });
            }
        });
    picked
}

/// A branch's right-click menu.
fn branch_menu(ui: &mut egui::Ui, name: &str, full_name: &str, is_current: bool) -> Option<BranchPick> {
    let mut picked = None;
    let mut item = |ui: &mut egui::Ui, enabled: bool, label: &str, why_not: &str, pick: BranchPick| {
        if ui.add_enabled(enabled, egui::Button::new(label)).on_disabled_hover_text(why_not).clicked() {
            ui.close();
            picked = Some(pick);
        }
    };
    item(ui, !is_current, "Check out", "Already checked out.", BranchPick::Checkout(name.to_string()));
    item(ui, true, "New branch from here…", "", BranchPick::New { from: Some(full_name.to_string()) });
    ui.separator();
    item(ui, true, "Rename…", "", BranchPick::Rename(name.to_string()));
    item(ui, !is_current, "Delete…", "Check out another branch first.", BranchPick::Delete(name.to_string()));
    picked
}

/// The name dialog's purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKind {
    /// A new branch, starting at `start` (a full ref name; `None` for HEAD), shown as `from`.
    New {
        start: Option<String>,
        from: String,
    },
    Rename {
        old: String,
    },
}

/// The dialog asking for a branch name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchDialog {
    pub kind: DialogKind,
    pub name: String,
    /// New branch: check it out once created.
    pub checkout: bool,
    /// Why git refused the last try (shown in the dialog, which stays open).
    pub error: Option<String>,
    /// Waiting for git: the buttons are disabled.
    pub busy: bool,
}

impl BranchDialog {
    pub fn new_branch(start: Option<String>, from: String) -> BranchDialog {
        BranchDialog { kind: DialogKind::New { start, from }, name: String::new(), checkout: true, error: None, busy: false }
    }

    pub fn rename(old: String) -> BranchDialog {
        BranchDialog { name: old.clone(), kind: DialogKind::Rename { old }, checkout: false, error: None, busy: false }
    }

    /// Why the name can't be used yet (checks that need no git), or `None`. Git checks the rest.
    pub fn blocker(&self) -> Option<&'static str> {
        let name = self.name.trim();
        if name.is_empty() {
            Some("Type a name.")
        } else if name.contains(char::is_whitespace) {
            Some("Branch names can't contain spaces (use - or / instead).")
        } else if matches!(&self.kind, DialogKind::Rename { old } if old == name) {
            Some("Type a different name.")
        } else {
            None
        }
    }

    /// The change to make, if the name can be tried.
    pub fn op(&self) -> Option<BranchOp> {
        if self.blocker().is_some() || self.busy {
            return None;
        }
        let name = self.name.trim().to_string();
        Some(match &self.kind {
            DialogKind::New { start, .. } => BranchOp::Create { name, start: start.clone(), checkout: self.checkout },
            DialogKind::Rename { old } => BranchOp::Rename { old: old.clone(), new: name },
        })
    }
}

/// The dialog's answer this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAnswer {
    Submit(BranchOp),
    Cancel,
}

/// Draws the dialog (Enter submits, Esc or a click outside cancels).
pub fn dialog(ctx: &egui::Context, dialog: &mut BranchDialog) -> Option<DialogAnswer> {
    let mut answer = None;
    let (title, button) = match &dialog.kind {
        DialogKind::New { .. } => ("New branch", "Create branch"),
        DialogKind::Rename { .. } => ("Rename branch", "Rename"),
    };
    let modal = egui::Modal::new(egui::Id::unique("gitcraft-branch-dialog")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading(title);
        ui.add_space(6.0);
        match &dialog.kind {
            DialogKind::New { from, .. } => ui.label(format!("Starts from {from}.")),
            DialogKind::Rename { old } => ui.label(format!("New name for {old}:")),
        };
        let edit =
            ui.add_enabled(!dialog.busy, egui::TextEdit::singleline(&mut dialog.name).hint_text("feature/my-change").desired_width(f32::INFINITY));
        // The name box has the keyboard when the dialog opens.
        if !edit.has_focus() && !dialog.busy && ui.memory(|memory| memory.focused().is_none()) {
            edit.request_focus();
        }
        if edit.changed() {
            dialog.error = None;
        }
        if matches!(dialog.kind, DialogKind::New { .. }) {
            ui.add_enabled(!dialog.busy, egui::Checkbox::new(&mut dialog.checkout, "Check out the new branch"));
        }
        if let Some(why) = dialog.blocker().filter(|_| !dialog.name.is_empty()) {
            ui.weak(why);
        }
        if let Some(error) = &dialog.error {
            ui.add(egui::Label::new(egui::RichText::new(error).color(ui.visuals().error_fg_color)).wrap());
        }
        ui.add_space(8.0);
        let enter = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let op = dialog.op();
            let submit = ui.add_enabled(op.is_some(), egui::Button::new(button)).on_disabled_hover_text(dialog.blocker().unwrap_or("Working…"));
            if ui.button("Cancel").clicked() {
                answer = Some(DialogAnswer::Cancel);
            }
            if let Some(op) = op.filter(|_| submit.clicked() || enter) {
                answer = Some(DialogAnswer::Submit(op));
            }
            if dialog.busy {
                ui.spinner();
            }
        });
    });
    if answer.is_none() && modal.should_close() && !dialog.busy {
        answer = Some(DialogAnswer::Cancel);
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::{Head, Ref, RefKind};

    fn refs(head: Head) -> Refs {
        let branch = |name: &str| Ref { name: format!("refs/heads/{name}"), kind: RefKind::Branch, target: "a".repeat(40) };
        Refs { refs: vec![branch("feature/x"), branch("main")], head }
    }

    #[test]
    fn the_button_names_the_current_branch() {
        assert_eq!(current_label(&refs(Head::Branch { name: "refs/heads/main".into(), commit: "a".repeat(40) })), "main");
        assert_eq!(current_label(&refs(Head::Unborn { name: "refs/heads/main".into() })), "main");
        assert_eq!(current_label(&refs(Head::Detached { commit: "a".repeat(40) })), "(detached HEAD)");
    }

    #[test]
    fn new_branch_dialog_checks_names_and_builds_the_op() {
        let mut dialog = BranchDialog::new_branch(Some("refs/heads/main".into()), "main".into());
        assert_eq!(dialog.op(), None, "no name yet");
        dialog.name = "my change".into();
        assert!(dialog.blocker().unwrap().contains("spaces"));
        dialog.name = "  feature/y ".into();
        assert_eq!(dialog.op(), Some(BranchOp::Create { name: "feature/y".into(), start: Some("refs/heads/main".into()), checkout: true }));
        dialog.busy = true;
        assert_eq!(dialog.op(), None, "not twice while git works");
    }

    #[test]
    fn rename_dialog_starts_with_the_old_name() {
        let mut dialog = BranchDialog::rename("main".into());
        assert_eq!(dialog.name, "main");
        assert_eq!(dialog.blocker(), Some("Type a different name."));
        dialog.name = "trunk".into();
        assert_eq!(dialog.op(), Some(BranchOp::Rename { old: "main".into(), new: "trunk".into() }));
    }

    #[test]
    fn draws_the_dropdown_and_dialogs() {
        let ctx = egui::Context::default();
        let refs = refs(Head::Branch { name: "refs/heads/main".into(), commit: "a".repeat(40) });
        let mut dialogs = [BranchDialog::new_branch(None, "main".into()), BranchDialog::rename("feature/x".into())];
        dialogs[1].error = Some("git said no".into());
        for dialog_state in &mut dialogs {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    assert_eq!(current_branch(ui, &refs), None);
                    assert_eq!(dialog(ui.ctx(), dialog_state), None, "no answer without input");
                });
                output.textures_delta.clear();
            }
        }
    }
}
