//! The Commit tab: the working tree's changed files, each with a checkbox, and the commit
//! message box on the left; the selected file's diff on the right.
//!
//! Like GitHub Desktop, every change starts ticked and Commit takes exactly what is ticked. If
//! something was already staged (with `git add` in a terminal) when a file first appears, the
//! ticks start out matching what is staged instead, so GitCraft commits what the command line
//! would.

use std::collections::BTreeMap;
use std::path::PathBuf;

use craft_ui::layout::SplitAxis;
use gitcraft_git::{AmendInfo, Change, CommitRequest, EntryKind, MAX_STATUS_ENTRIES, Status, StatusEntry, WorkingChange};

use crate::files;
use crate::ui::confirm::Confirm;
use crate::ui::diff_view::{self, DiffState};
use crate::ui::file_row;
use crate::ui::split::{self, Split};

/// Whether a file goes into the next commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// All of its changes.
    All,
    /// None: it stays as it is on disk, unstaged.
    Nothing,
    /// Exactly what is already staged (it was partly staged in another tool). Shown as a
    /// part-filled box. Picking single lines comes in a later step.
    AsStaged,
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

    /// Takes in a freshly read status: files seen for the first time get their starting tick,
    /// files no longer changed are forgotten, and the rest keep the user's choice.
    pub fn set_status(&mut self, status: Status) {
        let any_staged = status.entries.iter().any(StatusEntry::has_staged);
        self.ticks.retain(|path, _| status.entries.iter().any(|entry| &entry.path == path));
        for entry in &status.entries {
            self.ticks.entry(entry.path.clone()).or_insert_with(|| first_tick(entry, any_staged));
        }
        self.status = Some(status);
    }

    pub fn tick(&self, path: &str) -> Tick {
        self.ticks.get(path).copied().unwrap_or(Tick::All)
    }

    fn entries(&self) -> &[StatusEntry] {
        self.status.as_ref().map(|status| status.entries.as_slice()).unwrap_or_default()
    }

    /// Ticks or unticks every file.
    pub fn set_all(&mut self, tick: Tick) {
        for value in self.ticks.values_mut() {
            *value = tick;
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

    /// What to commit, if Commit can be clicked.
    pub fn commit_request(&self) -> Option<CommitRequest> {
        if self.blocker().is_some() {
            return None;
        }
        let paths = |wanted: Tick| self.entries().iter().filter(|entry| self.tick(&entry.path) == wanted).map(|entry| entry.path.clone()).collect();
        Some(CommitRequest {
            include: paths(Tick::All),
            exclude: paths(Tick::Nothing),
            partial: Vec::new(),
            message: self.message(),
            amend: self.amend,
        })
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

/// A file's tick when it first appears. Normally everything is ticked; but if anything at all is
/// staged, the ticks follow the index: fully staged files ticked, unstaged ones not, and files
/// staged only in part keep exactly their staged part.
fn first_tick(entry: &StatusEntry, any_staged: bool) -> Tick {
    if !any_staged {
        Tick::All
    } else if !entry.has_staged() {
        Tick::Nothing
    } else if entry.unstaged == Change::Unmodified {
        Tick::All
    } else {
        Tick::AsStaged
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
    // for the duration (`mem::take` leaves the default, `Empty`, behind) and put back after.
    let diff = std::mem::take(&mut changes.diff);
    let shown = changes.selected.clone();
    let is_clean = changes.entries().is_empty();
    let mut action = None;
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
                diff_view::show(ui, &diff, ("working", shown.as_deref()));
            }
        },
    );
    changes.diff = diff;
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
            let checkbox = egui::Checkbox::without_text(&mut checked).indeterminate(tick == Tick::AsStaged);
            let mut response = ui.add_enabled(!changes.committing, checkbox);
            if tick == Tick::AsStaged {
                response =
                    response.on_hover_text("Partly staged in another tool: only the staged part will be committed. Click to include the whole file.");
            }
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
            row.context_menu(|ui| command = file_menu(ui, entry, programs));
            if let Some(command) = command {
                clicked = Some(Action::File { path: entry.path.clone(), command });
            }
        });
    }
    // Applied after the loop, which borrows the entries.
    changes.ticks.extend(new_ticks);
    clicked
}

/// A menu item that closes the menu when clicked.
fn menu_item(ui: &mut egui::Ui, enabled: bool, label: &str, disabled_reason: &str) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).on_disabled_hover_text(disabled_reason).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A changed file's right-click menu. Returns the choice, on the frame it is made.
fn file_menu(ui: &mut egui::Ui, entry: &StatusEntry, programs: &[PathBuf]) -> Option<FileCommand> {
    let mut chosen = None;
    let untracked = entry.kind == EntryKind::Untracked;
    // A deleted file isn't on disk, so there is nothing to show or open.
    let on_disk = entry.change() != WorkingChange::Deleted;

    if menu_item(ui, !entry.submodule, "Discard changes…", "Submodules come in a later version.") {
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
mod tests {
    use super::*;

    fn entry(path: &str, kind: EntryKind, staged: Change, unstaged: Change) -> StatusEntry {
        StatusEntry { path: path.into(), old_path: None, kind, staged, unstaged, submodule: false }
    }

    fn status(entries: Vec<StatusEntry>) -> Status {
        Status { entries, truncated: false, branch: Some("main".into()), unborn: false }
    }

    fn unstaged(path: &str) -> StatusEntry {
        entry(path, EntryKind::Tracked, Change::Unmodified, Change::Modified)
    }

    fn sample() -> Changes {
        let mut changes = Changes::default();
        changes.set_status(status(vec![
            unstaged("a.txt"),
            entry("b.txt", EntryKind::Untracked, Change::Unmodified, Change::Added),
            entry("c.txt", EntryKind::Unmerged, Change::Unmerged, Change::Unmerged),
        ]));
        changes.selected = Some("a.txt".into());
        changes
    }

    #[test]
    fn counts_files() {
        assert_eq!(count_label(1), "1 changed file");
        assert_eq!(count_label(3), "3 changed files");
    }

    #[test]
    fn everything_starts_ticked_when_nothing_is_staged() {
        let changes = sample();
        assert!(["a.txt", "b.txt", "c.txt"].iter().all(|path| changes.tick(path) == Tick::All));
    }

    #[test]
    fn ticks_follow_the_index_when_something_is_staged() {
        let mut changes = Changes::default();
        changes.set_status(status(vec![
            entry("fully.txt", EntryKind::Tracked, Change::Modified, Change::Unmodified),
            entry("partly.txt", EntryKind::Tracked, Change::Modified, Change::Modified),
            unstaged("unstaged.txt"),
            entry("new.txt", EntryKind::Untracked, Change::Unmodified, Change::Added),
        ]));
        assert_eq!(changes.tick("fully.txt"), Tick::All);
        assert_eq!(changes.tick("partly.txt"), Tick::AsStaged);
        assert_eq!(changes.tick("unstaged.txt"), Tick::Nothing);
        assert_eq!(changes.tick("new.txt"), Tick::Nothing);
    }

    #[test]
    fn refreshes_keep_choices_and_forget_files_that_are_gone() {
        let mut changes = sample();
        changes.ticks.insert("a.txt".into(), Tick::Nothing);
        changes.set_status(status(vec![unstaged("a.txt"), unstaged("d.txt")]));
        assert_eq!(changes.tick("a.txt"), Tick::Nothing, "kept");
        assert_eq!(changes.tick("d.txt"), Tick::All, "a new file starts ticked");
        assert_eq!(changes.ticks.len(), 2, "b and c are gone");
    }

    #[test]
    fn commit_requests_follow_the_ticks() {
        let mut changes = sample();
        assert_eq!(changes.blocker(), Some("Write a summary first."));
        assert_eq!(changes.commit_request(), None);

        changes.summary = "  Fix login  ".into();
        changes.description = "Long story.\n\n".into();
        changes.ticks.insert("b.txt".into(), Tick::Nothing);
        let request = changes.commit_request().unwrap();
        assert_eq!(request.include, ["a.txt", "c.txt"]);
        assert_eq!(request.exclude, ["b.txt"]);
        assert_eq!(request.message, "Fix login\n\nLong story.");
        assert!(!request.amend);

        changes.set_all(Tick::Nothing);
        assert_eq!(changes.blocker(), Some("Tick at least one file to commit."));
        changes.amend = true;
        assert_eq!(changes.blocker(), Some("Reading the last commit…"));
        changes.amend_info = Some(AmendInfo { message: "Old".into(), pushed_to: None });
        assert_eq!(changes.blocker(), None, "an amend can change only the message");
        changes.committing = true;
        assert_eq!(changes.blocker(), Some("Committing…"));
    }

    #[test]
    fn partly_staged_files_are_neither_added_nor_unstaged() {
        let mut changes = Changes::default();
        changes.set_status(status(vec![entry("partly.txt", EntryKind::Tracked, Change::Modified, Change::Modified)]));
        changes.summary = "Staged part".into();
        let request = changes.commit_request().unwrap();
        assert!(request.include.is_empty() && request.exclude.is_empty());
    }

    #[test]
    fn amend_starts_from_the_last_message_unless_one_is_written() {
        let info = AmendInfo { message: "Old summary\n\nOld body\nline 2".into(), pushed_to: None };
        let mut changes = sample();
        changes.amend = true;
        changes.take_amend_info(info.clone());
        assert_eq!((changes.summary.as_str(), changes.description.as_str()), ("Old summary", "Old body\nline 2"));

        let mut written = sample();
        written.amend = true;
        written.summary = "Mine".into();
        written.take_amend_info(info);
        assert_eq!(written.summary, "Mine");
    }

    #[test]
    fn a_finished_commit_clears_the_box() {
        let mut changes = sample();
        changes.summary = "x".into();
        changes.description = "y".into();
        changes.amend = true;
        changes.committing = true;
        changes.commit_error = Some("old".into());
        changes.committed();
        assert!(changes.summary.is_empty() && changes.description.is_empty() && !changes.amend && !changes.committing);
        assert_eq!(changes.commit_error, None);
    }

    #[test]
    fn splits_messages() {
        assert_eq!(split_message("One line"), ("One line".into(), String::new()));
        assert_eq!(split_message("Sum\n\nBody\n"), ("Sum".into(), "Body".into()));
        assert_eq!(split_message("Sum\r\n\r\nBody"), ("Sum".into(), "Body".into()));
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

    #[test]
    fn button_names_the_branch() {
        let mut s = status(vec![]);
        assert_eq!(commit_label(&s, false), "Commit to main");
        assert_eq!(commit_label(&s, true), "Amend last commit");
        s.branch = None;
        assert_eq!(commit_label(&s, false), "Commit (detached HEAD)");
    }

    #[test]
    fn draws_every_state() {
        let failed = Changes { error: Some("nope".into()), ..Changes::default() };
        let mut clean = Changes::default();
        clean.set_status(status(vec![]));
        let mut busy = sample();
        busy.committing = true;
        busy.commit_error = Some("hook failed".into());
        let mut states = [Changes::default(), failed, clean, sample(), busy];
        let ctx = egui::Context::default();
        let mut ratio = split::DEFAULT_CHANGES_RATIO;
        for changes in &mut states {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    show(ui, changes, &mut ratio, &[PathBuf::from("editor")]);
                });
                output.textures_delta.clear();
            }
        }
    }
}
