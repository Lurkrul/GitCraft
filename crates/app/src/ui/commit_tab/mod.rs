//! The Commit tab: the working tree's changed files, each with a checkbox, and the commit
//! message box on the left; the selected file's diff on the right, where single lines and hunks
//! can be ticked too.
//!
//! Like GitHub Desktop, every change starts ticked and Commit takes exactly what is ticked. The
//! ticks are GitCraft's own (its "pre-staging"): what is staged in git's index doesn't affect them,
//! and the index is only changed when Commit is clicked.

mod menu;
mod ticks;

pub use menu::discard_question;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use craft_ui::layout::SplitAxis;
use gitcraft_git::{AmendInfo, CommitRequest, FileDiff, LineRef, MAX_STATUS_ENTRIES, Status, StatusEntry, WorkingChange};

use crate::ui::diff_view::{self, DiffState, DiffView, TickClick, TickTarget, Ticks};
use crate::ui::file_row;
use crate::ui::split::{self, Split};
use ticks::Partial;

/// Whether a file goes into the next commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// All of its changes.
    All,
    /// None: it stays as it is on disk, unstaged.
    Nothing,
    /// Some of its lines, as picked in the diff (see [`Changes::partial`]). Shown as a
    /// part-filled box.
    Lines,
}

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
    /// Each changed file's tick, by path. Kept across refreshes; files no longer changed drop out.
    pub ticks: BTreeMap<String, Tick>,
    /// The picked lines of every file whose tick is [`Tick::Lines`] (and only those).
    pub partial: BTreeMap<String, Partial>,
    /// The line last clicked in the diff (file and line), where a Shift+click range starts.
    pub tick_anchor: Option<(String, LineRef)>,
    pub summary: String,
    pub description: String,
    pub amend: bool,
    /// The last commit's message and whether it was pushed, once read (for Amend).
    pub amend_info: Option<AmendInfo>,
    /// A commit is running.
    pub committing: bool,
    /// Why the last commit failed (a hook's output, say).
    pub commit_error: Option<String>,
}

/// Something the user did in the tab that the app has to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Show this file's diff.
    Select(String),
    Commit,
    /// The Amend box was ticked: the last commit's message is needed.
    AmendTurnedOn,
    /// A choice from a file's right-click menu.
    File {
        path: String,
        command: FileCommand,
    },
}

/// The right-click menu of a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCommand {
    /// Throw the file's changes away (after a confirmation).
    Discard,
    /// Add the file to `.gitignore`.
    IgnoreFile,
    /// Add `*.<extension>` to `.gitignore`.
    IgnoreExtension(String),
    CopyPath,
    CopyRelativePath,
    /// Show the file in the file manager.
    Reveal,
    /// Open with the default program.
    Open,
    /// Open with a program picked before.
    OpenWith(PathBuf),
    /// Pick a program with a file picker.
    ChooseProgram,
    /// The system's own "Open with" dialog (Windows).
    SystemOpenWith,
}

impl Changes {
    pub fn entry(&self, path: &str) -> Option<&StatusEntry> {
        self.status.as_ref()?.entries.iter().find(|entry| entry.path == path)
    }

    /// Takes in a freshly read status: files seen for the first time start ticked (whatever git's
    /// index holds), files no longer changed are forgotten, and the rest keep the user's choice.
    pub fn set_status(&mut self, status: Status) {
        let still_changed = |path: &String| status.entries.iter().any(|entry| &entry.path == path);
        self.ticks.retain(|path, _| still_changed(path));
        self.partial.retain(|path, _| still_changed(path));
        for entry in &status.entries {
            self.ticks.entry(entry.path.clone()).or_insert(Tick::All);
        }
        self.status = Some(status);
    }

    pub fn tick(&self, path: &str) -> Tick {
        self.ticks.get(path).copied().unwrap_or(Tick::All)
    }

    /// Sets a whole file's tick (dropping any picked lines).
    pub fn set_tick(&mut self, path: &str, tick: Tick) {
        self.partial.remove(path);
        self.ticks.insert(path.to_string(), tick);
    }

    fn entries(&self) -> &[StatusEntry] {
        self.status.as_ref().map(|status| status.entries.as_slice()).unwrap_or_default()
    }

    /// Ticks or unticks every file.
    pub fn set_all(&mut self, tick: Tick) {
        self.partial.clear();
        for value in self.ticks.values_mut() {
            *value = tick;
        }
    }

    /// A freshly read diff for `path` arrived: show it, and move that file's picked lines onto it.
    pub fn diff_loaded(&mut self, path: &str, diff: FileDiff) {
        if let Some(partial) = self.partial.get_mut(path) {
            if diff.can_pick_lines() {
                partial.remap(diff.clone());
                self.normalize(path);
            } else {
                // The file can no longer be split (it became binary or too large): leave it out
                // rather than guess, so the user sees the unticked box and decides.
                self.set_tick(path, Tick::Nothing);
            }
        }
        self.diff = DiffState::Loaded(Box::new(DiffView::new(diff)));
    }

    /// Whether a line of the selected file's diff is ticked.
    fn line_ticked(&self, path: &str, at: LineRef) -> bool {
        match self.tick(path) {
            Tick::All => true,
            Tick::Nothing => false,
            Tick::Lines => self.partial.get(path).is_none_or(|partial| partial.is_ticked(at)),
        }
    }

    /// Why the selected file's line boxes can't be clicked, or `None` if they can.
    fn line_ticks_blocker(&self, diff: &FileDiff) -> Option<&'static str> {
        if self.committing {
            Some("Committing…")
        } else if !diff.can_pick_lines() {
            Some("Single lines can't be picked in this file (too many changes to show). Tick or untick the whole file in the list.")
        } else {
            None
        }
    }

    /// A checkbox in the selected file's diff was clicked.
    pub fn click_tick(&mut self, click: TickClick) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let DiffState::Loaded(view) = &self.diff else {
            return;
        };
        if self.line_ticks_blocker(&view.diff).is_some() {
            return;
        }
        // Which lines the click covers, and whether they become ticked.
        let (lines, ticked) = match click.target {
            TickTarget::Line(at) => {
                let ticked = !self.line_ticked(&path, at);
                let lines = match &self.tick_anchor {
                    Some((anchor_path, anchor)) if click.shift && *anchor_path == path => ticks::range(&view.diff, *anchor, at),
                    _ => vec![at],
                };
                self.tick_anchor = Some((path.clone(), at));
                (lines, ticked)
            }
            TickTarget::Hunk(_) | TickTarget::Block(_) => {
                let lines = match click.target {
                    TickTarget::Hunk(hunk) => ticks::hunk_lines(&view.diff, hunk),
                    TickTarget::Block(block) => view.blocks.get(block).map(|block| block.lines.clone()).unwrap_or_default(),
                    TickTarget::Line(_) => Vec::new(),
                };
                // A fully ticked hunk or block is unticked; anything else becomes fully ticked.
                let ticked = !lines.iter().all(|at| self.line_ticked(&path, *at));
                (lines, ticked)
            }
        };
        if lines.is_empty() {
            return;
        }

        // Start from the file's current state, as picked lines.
        let mut partial = match (self.tick(&path), self.partial.remove(&path)) {
            (Tick::Lines, Some(partial)) => partial,
            (Tick::Nothing, _) => {
                let all = ticks::line_keys(&view.diff).into_iter().map(|(_, key)| key).collect();
                Partial::new(view.diff.clone(), all)
            }
            _ => Partial::new(view.diff.clone(), BTreeSet::new()),
        };
        partial.set(&lines, ticked);
        self.partial.insert(path.clone(), partial);
        self.ticks.insert(path.clone(), Tick::Lines);
        self.normalize(&path);
    }

    /// A file with every line ticked (or none) is simply ticked (or not).
    fn normalize(&mut self, path: &str) {
        let Some(partial) = self.partial.get(path) else {
            return;
        };
        if partial.all_ticked() {
            self.set_tick(path, Tick::All);
        } else if partial.none_ticked() {
            self.set_tick(path, Tick::Nothing);
        }
    }

    /// The commit message: the summary, then a blank line and the description if there is one.
    pub fn message(&self) -> String {
        let summary = self.summary.trim();
        let description = self.description.trim_end();
        if description.trim().is_empty() { summary.to_string() } else { format!("{summary}\n\n{description}") }
    }

    /// Why Commit can't be clicked right now, or `None` if it can.
    pub fn blocker(&self) -> Option<&'static str> {
        if self.committing {
            Some("Committing…")
        } else if self.amend && self.amend_info.is_none() {
            // Needed first: it says whether the commit was pushed (see the app's warning).
            Some("Reading the last commit…")
        } else if self.summary.trim().is_empty() {
            Some("Write a summary first.")
        } else if !self.amend && self.entries().iter().all(|entry| self.tick(&entry.path) == Tick::Nothing) {
            Some("Tick at least one file to commit.")
        } else {
            None
        }
    }

    /// What to commit: `None` if Commit can't be clicked, or an error if a file's picked lines
    /// can't be turned into a patch.
    pub fn commit_request(&self) -> Option<Result<CommitRequest, String>> {
        if self.blocker().is_some() {
            return None;
        }
        let mut request = CommitRequest { include: Vec::new(), exclude: Vec::new(), partial: Vec::new(), message: self.message(), amend: self.amend };
        for entry in self.entries() {
            let path = entry.path.clone();
            match self.tick(&path) {
                Tick::All => request.include.push(path),
                Tick::Nothing => request.exclude.push(path),
                Tick::Lines => match self.partial.get(&path).map(|partial| partial.patch(&path)) {
                    Some(Ok(Some(file))) => request.partial.push(file),
                    // Nothing picked after all: leave the file out.
                    Some(Ok(None)) | None => request.exclude.push(path),
                    Some(Err(message)) => return Some(Err(message)),
                },
            }
        }
        Some(Ok(request))
    }

    /// Starts an amend from the last commit's message, unless the user has already written one.
    pub fn take_amend_info(&mut self, info: AmendInfo) {
        if self.amend && self.summary.trim().is_empty() && self.description.trim().is_empty() {
            let (summary, description) = split_message(&info.message);
            self.summary = summary;
            self.description = description;
        }
        self.amend_info = Some(info);
    }

    /// After a successful commit: an empty message box, ready for the next one.
    pub fn committed(&mut self) {
        self.summary.clear();
        self.description.clear();
        self.amend = false;
        self.amend_info = None;
        self.committing = false;
        self.commit_error = None;
    }
}

/// Splits a commit message into its summary (first line) and description (the rest, without the
/// blank line between them).
pub fn split_message(message: &str) -> (String, String) {
    let (summary, rest) = message.split_once('\n').unwrap_or((message, ""));
    (summary.trim().to_string(), rest.trim_start_matches(['\r', '\n']).trim_end().to_string())
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

/// The Commit button's label.
pub fn commit_label(status: &Status, amend: bool) -> String {
    match (&status.branch, amend) {
        (_, true) => "Amend last commit".to_string(),
        (Some(branch), false) => format!("Commit to {branch}"),
        (None, false) => "Commit (detached HEAD)".to_string(),
    }
}

/// Draws the tab. `ratio` is the left pane's share of the width; `programs` are the ones remembered
/// for "Open with".
pub fn show(ui: &mut egui::Ui, changes: &mut Changes, ratio: &mut f32, programs: &[PathBuf]) -> Option<Action> {
    if changes.status.is_none() {
        ui.centered_and_justified(|ui| match &changes.error {
            Some(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            None => {
                ui.spinner();
            }
        });
        return None;
    }

    let split =
        Split { id: egui::Id::unique("changes-diff-split"), axis: SplitAxis::Horizontal, min_pane: 240.0, label: "Resize the changes and the diff" };
    // The two panes are drawn by two closures alive at the same time. The left one changes
    // `changes` (ticks, message), so the right one can't also borrow it: the diff is moved out
    // for the duration (`mem::take` leaves the default, `Empty`, behind) and put back after, and
    // the line ticks are copied out (just the selected file's tick and its unticked lines).
    let diff = std::mem::take(&mut changes.diff);
    let shown = changes.selected.clone();
    let is_clean = changes.entries().is_empty();
    let path = shown.clone().unwrap_or_default();
    let tick = changes.tick(&path);
    let unticked = changes.partial.get(&path).map(Partial::unticked_lines).unwrap_or_default();
    let blocker = match &diff {
        DiffState::Loaded(view) => changes.line_ticks_blocker(&view.diff),
        _ => None,
    };
    let mut action = None;
    let mut clicked = None;
    split::two_panes(
        ui,
        &split,
        ratio,
        |ui| action = left_pane(ui, changes, programs),
        |ui| {
            if is_clean {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.heading("No local changes");
                    ui.weak("Changes you make to files in this repository show up here.");
                });
            } else {
                let is_ticked = |at: LineRef| match tick {
                    Tick::All => true,
                    Tick::Nothing => false,
                    Tick::Lines => !unticked.contains(&at),
                };
                let ticks = Ticks { enabled: blocker.is_none(), disabled_reason: blocker.unwrap_or_default(), is_ticked: &is_ticked };
                clicked = diff_view::show(ui, &diff, ("working", shown.as_deref()), Some(&ticks));
            }
        },
    );
    changes.diff = diff;
    if let Some(click) = clicked {
        changes.click_tick(click);
    }
    action
}

fn left_pane(ui: &mut egui::Ui, changes: &mut Changes, programs: &[PathBuf]) -> Option<Action> {
    let mut action = None;
    ui.add_space(4.0);
    header(ui, changes);
    ui.separator();
    // The message box is anchored to the bottom of the pane (added first, so it gets its space);
    // the list fills what is left above it.
    let commit = egui::Panel::bottom("commit-box")
        .resizable(false)
        .show(ui, |ui| {
            ui.add_space(6.0);
            let action = commit_box(ui, changes);
            ui.add_space(4.0);
            action
        })
        .inner;
    egui::ScrollArea::vertical().id_salt("changed-files").auto_shrink(false).show(ui, |ui| {
        if changes.entries().is_empty() {
            ui.weak("No local changes.");
        }
        if let Some(picked) = file_list(ui, changes, programs) {
            action = Some(picked);
        }
    });
    commit.or(action)
}

/// The "all files" checkbox and the file count.
fn header(ui: &mut egui::Ui, changes: &mut Changes) {
    let entries = changes.entries();
    let all = entries.iter().all(|entry| changes.tick(&entry.path) == Tick::All);
    let none = entries.iter().all(|entry| changes.tick(&entry.path) == Tick::Nothing);
    let count = entries.len();
    let truncated = changes.status.as_ref().is_some_and(|status| status.truncated);
    ui.horizontal(|ui| {
        let mut checked = all && count > 0;
        let checkbox = egui::Checkbox::without_text(&mut checked).indeterminate(!all && !none);
        let response = ui.add_enabled(count > 0 && !changes.committing, checkbox).on_hover_text("Include all files, or none");
        if response.changed() {
            changes.set_all(if all { Tick::Nothing } else { Tick::All });
        }
        ui.label(count_label(count));
    });
    if truncated {
        ui.weak(format!("Only the first {MAX_STATUS_ENTRIES} are listed."));
    }
}

/// The files, each with its checkbox and right-click menu. Returns what was clicked, if anything.
fn file_list(ui: &mut egui::Ui, changes: &mut Changes, programs: &[PathBuf]) -> Option<Action> {
    let mut clicked = None;
    let mut new_ticks = Vec::new();
    for (index, entry) in changes.entries().iter().enumerate() {
        let tick = changes.tick(&entry.path);
        ui.horizontal(|ui| {
            // The checkbox sits outside the clickable row, so ticking doesn't also select.
            let mut checked = tick != Tick::Nothing;
            let checkbox = egui::Checkbox::without_text(&mut checked).indeterminate(tick == Tick::Lines);
            let mut response = ui.add_enabled(!changes.committing, checkbox);
            response = match tick {
                Tick::Lines => response.on_hover_text("Only the ticked lines will be committed. Click to include the whole file."),
                Tick::All | Tick::Nothing => response,
            };
            if response.changed() {
                // A part-filled box becomes fully ticked, as in GitHub Desktop.
                new_ticks.push((entry.path.clone(), if tick == Tick::All { Tick::Nothing } else { Tick::All }));
            }
            let is_selected = changes.selected.as_deref() == Some(entry.path.as_str());
            let tooltip = entry.submodule.then(|| "A submodule".to_string());
            let row = file_row::show(
                ui,
                ("working-file", index),
                &letter(ui, entry.change()),
                file_row::LetterSide::Right,
                &entry.path,
                tooltip,
                is_selected,
                |_| {},
            );
            if row.clicked() && !is_selected {
                clicked = Some(Action::Select(entry.path.clone()));
            }
            let mut command = None;
            row.context_menu(|ui| command = menu::file_menu(ui, entry, programs));
            if let Some(command) = command {
                clicked = Some(Action::File { path: entry.path.clone(), command });
            }
        });
    }
    // Applied after the loop, which borrows the entries.
    for (path, tick) in new_ticks {
        changes.set_tick(&path, tick);
    }
    clicked
}

/// Summary, description, Amend and the Commit button.
fn commit_box(ui: &mut egui::Ui, changes: &mut Changes) -> Option<Action> {
    let summary_id = ui.scope_id().with("commit-summary");
    let description_id = ui.scope_id().with("commit-description");
    // Ctrl+Enter (⌘Enter on macOS) commits from either text box. Taken before the boxes are
    // drawn, so the description doesn't also get a new line.
    let typing = ui.memory(|memory| memory.focused().is_some_and(|id| id == summary_id || id == description_id));
    let shortcut = typing && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));

    let mut action = None;
    let editable = !changes.committing;
    ui.add_enabled(
        editable,
        egui::TextEdit::singleline(&mut changes.summary).id(summary_id).hint_text("Summary (required)").desired_width(f32::INFINITY),
    );
    ui.add_enabled(
        editable,
        egui::TextEdit::multiline(&mut changes.description).id(description_id).hint_text("Description").desired_rows(4).desired_width(f32::INFINITY),
    );

    let unborn = changes.status.as_ref().is_some_and(|status| status.unborn);
    let amend = ui
        .add_enabled(editable && !unborn, egui::Checkbox::new(&mut changes.amend, "Amend last commit"))
        .on_disabled_hover_text("There is no commit to amend yet.");
    if amend.changed() && changes.amend {
        action = Some(Action::AmendTurnedOn);
    }

    let label = changes.status.as_ref().map(|status| commit_label(status, changes.amend)).unwrap_or_default();
    let blocker = changes.blocker();
    ui.horizontal(|ui| {
        let button = egui::Button::new(label).min_size(egui::vec2(ui.available_width(), 0.0));
        let response = ui.add_enabled(blocker.is_none(), button).on_disabled_hover_text(blocker.unwrap_or_default());
        if response.clicked() || (shortcut && blocker.is_none()) {
            action = Some(Action::Commit);
        }
    });
    if changes.committing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Committing…");
        });
    }
    if let Some(error) = &changes.commit_error {
        egui::ScrollArea::vertical().id_salt("commit-error").max_height(80.0).show(ui, |ui| {
            ui.colored_label(ui.visuals().error_fg_color, error);
        });
    }
    action
}

#[cfg(test)]
mod tests;
