//! The GitCraft window: state, per-frame logic and drawing.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use craft_dialogs::{Answer, Dialogs, FileDialog};
use craft_menu::MenuBar;
use craft_settings::{SaveStatus, SettingsStore};
use gitcraft_git::{DEFAULT_LOG_LIMIT, DiffRequest, LogScope, Refs, Repo};

use crate::commands::Command;
use crate::graph::History;
use crate::settings::Prefs;
use crate::ui::details::DetailsState;
use crate::ui::diff_view::{self, DiffState, DiffView};
use crate::ui::ref_labels::{self, LabelMap};
use crate::ui::repository::RepoLayout;
use crate::ui::status_bar::StatusLine;
use crate::ui::tabs::MainTab;
use crate::ui::{about, menu, placeholder, repository, status_bar, tabs};
use crate::worker::{Event, Job, Worker};

/// The product name, used for the window title and messages.
pub const APP_NAME: &str = "GitCraft";

const WORKER_STOPPED: &str = "GitCraft's background worker stopped unexpectedly. Please restart GitCraft.";

/// What a file dialog's answer is for. Only one purpose so far; more will follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogPurpose {
    OpenRepository,
}

/// The repository on screen and its history.
struct OpenRepo {
    repo: Repo,
    refs: Refs,
    /// Branch and tag labels by commit, rebuilt whenever `refs` changes.
    labels: LabelMap,
    history: History,
    /// The branch chosen in the dropdown. Can be ahead of `log_scope` while its history loads.
    selected_scope: LogScope,
    /// The scope `log` was read with.
    log_scope: LogScope,
    /// The selected commit's id.
    selected: Option<String>,
    /// The selected commit's details, for the right pane.
    details: DetailsState,
    /// The selected file of that commit, and its diff.
    selected_file: Option<String>,
    diff: DiffState,
}

impl OpenRepo {
    fn new(repo: Repo, refs: Refs, history: History) -> OpenRepo {
        let labels = ref_labels::label_map(&refs);
        let scope = LogScope::AllBranches;
        OpenRepo {
            repo,
            refs,
            labels,
            history,
            selected_scope: scope.clone(),
            log_scope: scope,
            selected: None,
            details: DetailsState::Empty,
            selected_file: None,
            diff: DiffState::Empty,
        }
    }

    /// The row of the selected commit in the current history, if it is in it.
    fn selected_row(&self) -> Option<usize> {
        let selected = self.selected.as_deref()?;
        self.history.log.commits.iter().position(|commit| commit.id == selected)
    }
}

/// All of the app's state. eframe owns it and calls `logic` and `ui` on it every frame.
pub struct GitCraftApp {
    settings: SettingsStore<Prefs>,
    /// `None` if the background thread could not be started or has stopped; `error` says why.
    worker: Option<Worker>,
    dialogs: Dialogs<DialogPurpose>,
    open: Option<OpenRepo>,
    /// The latest problem to show in the status bar. Cleared when a repository opens.
    error: Option<String>,
    /// The menus, laid out for this platform once at startup.
    menu_bar: MenuBar,
    /// The window title last sent to the OS, so it is only sent again when it changes.
    window_title: String,
    tab: MainTab,
    about_open: bool,
    /// The Repository tab's divider positions and "More info" choice.
    layout: RepoLayout,
    /// A history row to bring into view on the next frame.
    scroll_to: Option<usize>,
}

impl GitCraftApp {
    /// `settings` is moved in: the app owns the store from now on. `startup_problem` is anything
    /// found before the window existed (such as a damaged settings file) to show the user.
    pub fn new(ctx: &egui::Context, settings: SettingsStore<Prefs>, startup_problem: Option<String>) -> Self {
        // Each background helper gets its own handle to the egui context (a cheap, thread-safe
        // clone) so it can ask for a repaint when it has something new.
        let worker_ctx = ctx.clone();
        let dialog_ctx = ctx.clone();
        let (worker, worker_error) = match Worker::spawn(Arc::new(move || worker_ctx.request_repaint())) {
            Ok(worker) => (Some(worker), None),
            Err(err) => (None, Some(format!("Could not start GitCraft's background worker: {err}"))),
        };
        Self {
            settings,
            worker,
            dialogs: Dialogs::new(Arc::new(move || dialog_ctx.request_repaint())),
            open: None,
            error: worker_error.or(startup_problem),
            menu_bar: menu::laid_out(),
            window_title: APP_NAME.to_string(),
            // The Commit tab comes first but has nothing to show until Milestone 3.
            tab: MainTab::Repository,
            about_open: false,
            layout: RepoLayout::default(),
            scroll_to: None,
        }
    }

    /// Runs a command chosen from the menu or by its shortcut.
    fn run(&mut self, command: Command) {
        match command {
            Command::OpenRepository => {
                // Opening a repository is about the Repository tab, so show it.
                self.tab = MainTab::Repository;
                self.ask_for_repository();
            }
            Command::About => self.about_open = true,
        }
    }

    /// Runs commands whose keyboard shortcut was pressed this frame. Skipped while a menu or a
    /// dialog has the keyboard, so the keys go to them instead.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if craft_menu::bar::is_open(ctx) || self.dialogs.is_open() || self.about_open {
            return;
        }
        // If the keyboard is on the menu bar (after a tap on Alt), a shortcut leaves it and runs.
        craft_menu::bar::leave_for_shortcut(ctx);
        for command in Command::ALL {
            let Some(chord) = command.shortcut() else {
                continue;
            };
            // `consume_shortcut` returns true (once) if the keys were pressed, and removes the
            // key press so no widget also reacts to it.
            if ctx.input_mut(|input| input.consume_shortcut(&chord.to_egui_shortcut())) {
                self.run(command);
            }
        }
    }

    /// Asks the user for a folder to open (answered on a later frame).
    fn ask_for_repository(&mut self) {
        if self.dialogs.is_open() {
            self.dialogs.raise();
            return;
        }
        let mut dialog = FileDialog::folder().title("Open repository");
        // Start next to the repository that is open now, if any.
        if let Some(parent) = self.open.as_ref().and_then(|open| open.repo.root().parent()) {
            dialog = dialog.directory(parent);
        }
        self.dialogs.ask(dialog, DialogPurpose::OpenRepository);
    }

    /// Starts opening `path` in the background.
    fn open_folder(&mut self, path: PathBuf) {
        self.send(Job::Open(path));
    }

    /// Shows the open repository's history for `scope` (loaded in the background; the current
    /// history stays on screen until it arrives).
    fn select_scope(&mut self, scope: LogScope) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.selected_scope = scope.clone();
        let job = Job::LoadLog { repo: open.repo.clone(), scope };
        self.send(job);
    }

    /// Hands a job to the background worker, reporting it if the worker has stopped.
    fn send(&mut self, job: Job) {
        let sent = self.worker.as_mut().is_some_and(|worker| worker.send(job));
        if !sent {
            self.worker = None;
            self.error = Some(WORKER_STOPPED.to_string());
        }
    }

    /// Acts on file dialogs the user has answered.
    fn handle_dialog_answers(&mut self) {
        for (purpose, answer) in self.dialogs.take_answers() {
            match (purpose, answer) {
                (DialogPurpose::OpenRepository, Answer::Paths(paths)) => {
                    if let Some(path) = paths.into_iter().next() {
                        self.open_folder(path);
                    }
                }
                (_, Answer::Unavailable(reason)) => self.error = Some(format!("Could not show the folder picker: {reason}")),
                (_, Answer::Cancelled | Answer::Message(_)) => {}
            }
        }
    }

    /// Takes in whatever the background worker has finished.
    fn handle_worker_events(&mut self) {
        // Take the events out first: `worker` borrows `self.worker`, and that borrow has to end
        // before `handle_event` can borrow all of `self`.
        let (events, alive) = match self.worker.as_mut() {
            Some(worker) => (worker.poll(), worker.is_alive()),
            None => return,
        };
        for event in events {
            self.handle_event(event);
        }
        if !alive {
            self.worker = None;
            self.error = Some(WORKER_STOPPED.to_string());
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Opened { repo, refs, history } => {
                self.open = Some(OpenRepo::new(repo, refs, history));
                self.error = None;
            }
            // The repository already on screen (if any) stays; the error explains what happened.
            Event::OpenFailed { path, error } => self.error = Some(open_failure_message(&path, &error)),
            Event::LogLoaded { repo, scope, history } => {
                // Only if it is still what the user wants to see: they may have opened another
                // repository or picked another branch while this one loaded.
                if let Some(open) = self.open.as_mut().filter(|open| open.repo == repo && open.selected_scope == scope) {
                    open.history = history;
                    open.log_scope = scope;
                    // The selection stays if the commit is in this history too; otherwise it goes.
                    if open.selected_row().is_none() {
                        open.selected = None;
                        open.details = DetailsState::Empty;
                        open.selected_file = None;
                        open.diff = DiffState::Empty;
                    }
                    self.error = None;
                }
            }
            Event::DetailsLoaded { repo, id, result } => {
                // Only for the commit still selected: the user may have clicked on since.
                let mut first_file = None;
                if let Some(open) = self.open.as_mut().filter(|open| open.repo == repo && open.selected.as_deref() == Some(id.as_str())) {
                    open.details = match result {
                        Ok(details) => {
                            first_file = details.files.first().map(|file| file.path.clone());
                            DetailsState::Loaded(Box::new(details))
                        }
                        Err(error) => DetailsState::Failed(format!("Could not read this commit: {error}")),
                    };
                }
                // Show the first file's changes straight away, so a click on a commit shows a diff.
                if let Some(path) = first_file {
                    self.select_file(path);
                }
            }
            Event::DiffLoaded { repo, request, result } => {
                // Only for the file still selected, in the commit still selected.
                let current = |open: &&mut OpenRepo| {
                    open.repo == repo
                        && open.selected.as_deref() == Some(request.commit.as_str())
                        && open.selected_file.as_deref() == Some(request.path.as_str())
                };
                if let Some(open) = self.open.as_mut().filter(current) {
                    open.diff = match result {
                        Ok(diff) => DiffState::Loaded(Box::new(DiffView::new(diff))),
                        Err(error) => DiffState::Failed(diff_view::failure_message(&error)),
                    };
                }
            }
            Event::LogFailed { repo, scope, error } => {
                if let Some(open) = self.open.as_mut().filter(|open| open.repo == repo && open.selected_scope == scope) {
                    // Put the dropdown back to what the list still shows.
                    open.selected_scope = open.log_scope.clone();
                    self.error = Some(format!("Could not read the history of {}: {error}", repository::scope_label(&scope)));
                }
            }
        }
    }

    /// Records the window's current size and maximized state in the settings.
    ///
    /// The size is only taken while the window is in its normal state: while maximized we keep
    /// the previous size, so un-maximizing (even after a restart) returns to it. A minimized
    /// window reports a meaningless size and is ignored entirely.
    fn remember_window(&mut self, ctx: &egui::Context) {
        // `ctx.input` lends us the input state only inside the closure (egui holds a lock on it),
        // so we copy out the plain values we need and work with them after it returns.
        let (minimized, maximized, size) = ctx.input(|i| {
            let viewport = i.viewport();
            (viewport.minimized, viewport.maximized, viewport.inner_rect.map(|rect| rect.size()))
        });
        if minimized == Some(true) {
            return;
        }
        // `update` only marks the store dirty (and schedules a save) if a value really changed.
        self.settings.update(|prefs| {
            if let Some(maximized) = maximized {
                prefs.window.maximized = maximized;
            }
            if maximized != Some(true)
                && let Some(size) = size
            {
                prefs.window.width = size.x;
                prefs.window.height = size.y;
            }
        });
    }

    /// Lets the settings store save pending changes (it waits 500 ms after the last change).
    fn save_settings_when_due(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let tick = self.settings.tick(now);
        if let SaveStatus::Failed(err) = &tick.status {
            // The store keeps retrying; the user just needs to know.
            self.error = Some(format!("Could not save settings: {err}"));
        }
        // egui only repaints on input, so without this an idle window would never save.
        if let Some(deadline) = tick.next_deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
    }

    /// Everything that happens each frame apart from drawing. Separate from `eframe::App::logic`
    /// (which needs an `eframe::Frame`) so tests can run it without a window.
    fn update(&mut self, ctx: &egui::Context) {
        self.remember_window(ctx);
        self.handle_shortcuts(ctx);
        self.handle_arrow_keys(ctx);
        self.handle_dialog_answers();
        self.handle_worker_events();
        self.sync_window_title(ctx);
        self.save_settings_when_due(ctx);
    }

    /// The title the window should have: "GitCraft — <full path>" while a repository is open.
    fn wanted_window_title(&self) -> String {
        match &self.open {
            Some(open) => format!("{APP_NAME} — {}", open.repo.root().display()),
            None => APP_NAME.to_string(),
        }
    }

    /// Tells the OS the new title when it changes (not every frame).
    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let wanted = self.wanted_window_title();
        if wanted != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(wanted.clone()));
            self.window_title = wanted;
        }
    }

    fn is_busy(&self) -> bool {
        self.worker.as_ref().is_some_and(Worker::is_busy)
    }

    /// What the status bar should say right now.
    fn status_line(&self) -> StatusLine {
        if let Some(error) = &self.error {
            return StatusLine::Error(error.clone());
        }
        if self.is_busy() {
            return StatusLine::Busy("Loading…".to_string());
        }
        match &self.open {
            None => StatusLine::Info("No repository open".to_string()),
            Some(open) if open.history.log.truncated => {
                StatusLine::Info(format!("Showing the newest {} commits; older history is not loaded yet", group_thousands(DEFAULT_LOG_LIMIT)))
            }
            Some(open) => {
                let count = open.history.log.commits.len();
                StatusLine::Info(format!("{} commit{}", group_thousands(count), if count == 1 { "" } else { "s" }))
            }
        }
    }

    /// Draws the whole window. Kept separate from `eframe::App::ui` (which also needs an
    /// `eframe::Frame`) so tests can run it on a bare `egui::Context` without opening a window.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        // Panels are added outside-in: menu bar, tab strip and (in the Repository tab) its
        // toolbar along the top, the status bar along the bottom, then the content fills the rest.
        let chosen = egui::Panel::top("menu_bar").show(ui, |ui| menu::show(ui, &self.menu_bar)).inner;
        for command in chosen {
            self.run(command);
        }
        egui::Panel::top("tabs").show(ui, |ui| tabs::show(ui, &mut self.tab));
        if let (MainTab::Repository, Some(open)) = (self.tab, &self.open) {
            let picked = egui::Panel::top("repository_toolbar")
                .show(ui, |ui| repository::toolbar(ui, open.repo.root(), &open.refs, &open.selected_scope))
                .inner;
            // Acted on after the panel closure ends, when `open` (a borrow of `self.open`) is no
            // longer in use and `self` can be changed.
            if let Some(scope) = picked {
                self.select_scope(scope);
            }
        }
        let status = self.status_line();
        egui::Panel::bottom("status_bar").show(ui, |ui| status_bar::show(ui, &status));
        // `take` uses the scroll request once: it is only for the frame after a keyboard move.
        let scroll_to = self.scroll_to.take();
        let pick = egui::CentralPanel::default()
            .show(ui, |ui| match (self.tab, &self.open) {
                (MainTab::Repository, Some(open)) => {
                    let view = repository::BodyView {
                        history: &open.history,
                        labels: &open.labels,
                        selected: open.selected.as_deref(),
                        details: &open.details,
                        selected_file: open.selected_file.as_deref(),
                        diff: &open.diff,
                        scroll_to,
                    };
                    repository::body(ui, &view, &mut self.layout)
                }
                (MainTab::Repository, None) => {
                    repository::welcome(ui);
                    None
                }
                (MainTab::Commit, _) => {
                    placeholder::coming_later(ui, "Committing", 3);
                    None
                }
                (MainTab::Issues, _) => {
                    placeholder::coming_later(ui, "Issues", 5);
                    None
                }
                (MainTab::Projects, _) => {
                    placeholder::coming_later(ui, "Projects", 6);
                    None
                }
            })
            .inner;
        match pick {
            Some(repository::Pick::Row(row)) => self.select_row(row),
            Some(repository::Pick::File(path)) => self.select_file(path),
            Some(repository::Pick::Commit(id)) => {
                // A parent link: select it, and bring it into view if it is in the list.
                self.select_commit(id);
                self.scroll_to = self.open.as_ref().and_then(OpenRepo::selected_row);
            }
            None => {}
        }
        about::show(ui.ctx(), &mut self.about_open);
    }

    /// Selects the commit in row `row` of the history.
    fn select_row(&mut self, row: usize) {
        let id = self.open.as_ref().and_then(|open| open.history.log.commits.get(row)).map(|commit| commit.id.clone());
        if let Some(id) = id {
            self.select_commit(id);
        }
    }

    /// Selects a commit and starts reading its details in the background.
    fn select_commit(&mut self, id: String) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if open.selected.as_deref() == Some(id.as_str()) {
            return; // Already selected (and loaded or loading).
        }
        open.selected = Some(id.clone());
        open.details = DetailsState::Loading;
        open.selected_file = None;
        open.diff = DiffState::Empty;
        let job = Job::LoadDetails { repo: open.repo.clone(), id };
        self.send(job);
    }

    /// Selects one of the selected commit's changed files and starts reading its diff.
    fn select_file(&mut self, path: String) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        // The request needs the commit's parent and the file's old name (for a rename), which
        // come from the loaded details.
        let (DetailsState::Loaded(details), Some(commit)) = (&open.details, &open.selected) else {
            return;
        };
        let Some(file) = details.files.iter().find(|file| file.path == path) else {
            return;
        };
        let request =
            DiffRequest { commit: commit.clone(), parent: details.parents.first().cloned(), path: path.clone(), old_path: file.old_path.clone() };
        open.selected_file = Some(path);
        open.diff = DiffState::Loading;
        let job = Job::LoadDiff { repo: open.repo.clone(), request };
        self.send(job);
    }

    /// ↑ and ↓ move the selection through the history while nothing else wants the keys.
    fn handle_arrow_keys(&mut self, ctx: &egui::Context) {
        let busy_elsewhere = self.tab != MainTab::Repository
            || craft_menu::bar::is_open(ctx)
            || self.dialogs.is_open()
            || self.about_open
            || ctx.memory(|memory| memory.focused().is_some())
            || egui::Popup::is_any_open(ctx);
        let Some(open) = self.open.as_ref().filter(|_| !busy_elsewhere) else {
            return;
        };
        let count = open.history.log.commits.len();
        let current = open.selected_row();
        let next = ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                Some(current.map_or(0, |row| (row + 1).min(count.saturating_sub(1))))
            } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                Some(current.map_or(0, |row| row.saturating_sub(1)))
            } else {
                None
            }
        });
        if let Some(row) = next.filter(|_| count > 0) {
            self.select_row(row);
            self.scroll_to = Some(row);
        }
    }

    /// Shows any queued file dialog. Must run at the end of the frame (craft-dialogs' rule);
    /// `parent` is the window, so the dialog stays in front of it.
    fn show_dialogs(&mut self, parent: Option<&eframe::Frame>) {
        self.dialogs.show_pending(parent);
    }
}

impl eframe::App for GitCraftApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update(ctx);
        // Nothing can be unsaved yet, so a close goes straight through. Must stay last in
        // `logic` so it sees any `CancelClose` the app sends (see craft-quit's docs).
        craft_quit::on_close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.draw(ui);
        self.show_dialogs(Some(frame));
    }

    fn on_exit(&mut self) {
        // Write anything still waiting for its 500 ms delay. The window is gone, so stderr is
        // the only place left to report a failure.
        if let Err(err) = self.settings.flush() {
            eprintln!("{APP_NAME}: could not save settings: {err}");
        }
    }
}

/// The status bar text for a repository that failed to open. Most git errors already name the
/// folder; repeating it would make a long path appear twice.
fn open_failure_message(path: &std::path::Path, error: &gitcraft_git::GitError) -> String {
    let message = error.to_string();
    let shown_path = path.display().to_string();
    if message.contains(&shown_path) { message } else { format!("Could not open {shown_path}: {message}") }
}

/// `12345` → `"12,345"`.
fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::WindowPrefs;
    use crate::test_support::{TestRepo, wait_until};
    use craft_settings::Location;

    /// An app whose settings live in memory only, so tests never touch the real config file.
    fn test_app(ctx: &egui::Context) -> GitCraftApp {
        let (store, _) = SettingsStore::open(Location::Memory);
        GitCraftApp::new(ctx, store, None)
    }

    /// Runs one whole frame the way eframe would, minus the window.
    fn run_frame(app: &mut GitCraftApp, ctx: &egui::Context, input: egui::RawInput) {
        let mut output = ctx.run_ui(input, |ui| {
            app.update(ui.ctx());
            app.draw(ui);
        });
        app.show_dialogs(None);
        // There is no GPU here to upload font textures to; the suite's egui fork asserts
        // that texture updates are not silently dropped, so discard them explicitly.
        output.textures_delta.clear();
    }

    /// Runs one frame with the given window state reported by the (pretend) OS.
    fn run_frame_with_window(app: &mut GitCraftApp, ctx: &egui::Context, size: [f32; 2], maximized: bool, minimized: bool) {
        let mut input = egui::RawInput::default();
        let viewport = input.viewports.entry(egui::ViewportId::ROOT).or_default();
        viewport.inner_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()));
        viewport.maximized = Some(maximized);
        viewport.minimized = Some(minimized);
        run_frame(app, ctx, input);
    }

    /// Makes the folder picker answer `path` without showing a real dialog, then "clicks" Open
    /// and runs frames until the background work is done.
    fn open_via_dialog(app: &mut GitCraftApp, ctx: &egui::Context, path: PathBuf) {
        app.dialogs.set_override(Some(Box::new(move |_request| Answer::Paths(vec![path.clone()]))));
        app.ask_for_repository();
        run_frame(app, ctx, egui::RawInput::default()); // the dialog is "shown" at the end of this frame
        wait_until(|| {
            run_frame(app, ctx, egui::RawInput::default());
            !app.is_busy()
        });
    }

    /// Input for one frame in which Ctrl+O (⌘O on macOS) is pressed.
    fn ctrl_o() -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::O,
                physical_key: Some(egui::Key::O),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        }
    }

    /// Makes the folder picker count how often it is asked, and answer Cancel.
    fn count_dialogs(app: &mut GitCraftApp) -> std::rc::Rc<std::cell::Cell<usize>> {
        // `Rc<Cell<..>>`: a shared counter the dialog override (a closure kept by `Dialogs`) and
        // the test can both reach. Single-threaded, so no Arc or atomics needed.
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&asked);
        app.dialogs.set_override(Some(Box::new(move |_request| {
            counter.set(counter.get() + 1);
            Answer::Cancelled
        })));
        asked
    }

    #[test]
    fn ctrl_o_asks_for_a_repository() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let asked = count_dialogs(&mut app);
        run_frame(&mut app, &ctx, ctrl_o());
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn open_repository_switches_to_the_repository_tab() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let asked = count_dialogs(&mut app);
        app.tab = MainTab::Issues;
        app.run(Command::OpenRepository);
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert_eq!(app.tab, MainTab::Repository);
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn about_opens_and_shortcuts_wait_while_it_is_up() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        let asked = count_dialogs(&mut app);
        app.run(Command::About);
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert!(app.about_open);
        run_frame(&mut app, &ctx, ctrl_o());
        assert_eq!(asked.get(), 0, "Ctrl+O must not open a picker over the About window");
    }

    #[test]
    fn every_tab_draws() {
        let repo = TestRepo::with_commits(&["One"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        for tab in MainTab::ALL {
            app.tab = tab;
            run_frame(&mut app, &ctx, egui::RawInput::default());
            run_frame(&mut app, &ctx, egui::RawInput::default());
        }
    }

    #[test]
    fn window_title_shows_the_open_repository() {
        let repo = TestRepo::with_commits(&["One"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        assert_eq!(app.window_title, "GitCraft");
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        let root = app.open.as_ref().map(|open| open.repo.root().display().to_string()).unwrap();
        assert_eq!(app.window_title, format!("GitCraft — {root}"));
    }

    /// Runs frames until the background worker has nothing left to do.
    fn settle(app: &mut GitCraftApp, ctx: &egui::Context) {
        wait_until(|| {
            run_frame(app, ctx, egui::RawInput::default());
            !app.is_busy()
        });
    }

    /// `main` has Base; `feature` has Base and Feature work.
    fn repo_with_feature_branch() -> TestRepo {
        let repo = TestRepo::with_commits(&["Base"]);
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "Feature work"]);
        repo.git(&["checkout", "-q", "main"]);
        repo
    }

    fn summaries(app: &GitCraftApp) -> Vec<String> {
        app.open.as_ref().map(|open| open.history.log.commits.iter().map(|c| c.summary.clone()).collect()).unwrap_or_default()
    }

    #[test]
    fn picking_a_branch_shows_only_its_history() {
        let repo = repo_with_feature_branch();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        assert_eq!(summaries(&app).len(), 2, "all branches at first");

        app.select_scope(LogScope::Ref("refs/heads/main".into()));
        settle(&mut app, &ctx);
        assert_eq!(summaries(&app), ["Base"]);

        app.select_scope(LogScope::AllBranches);
        settle(&mut app, &ctx);
        assert_eq!(summaries(&app).len(), 2);
    }

    #[test]
    fn a_result_for_a_branch_no_longer_selected_is_ignored() {
        let repo = repo_with_feature_branch();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        // Pick main, then change your mind before its history arrives.
        app.select_scope(LogScope::Ref("refs/heads/main".into()));
        app.select_scope(LogScope::Ref("refs/heads/feature".into()));
        settle(&mut app, &ctx);
        assert_eq!(summaries(&app), ["Feature work", "Base"]);
        let open = app.open.as_ref().unwrap();
        assert_eq!(open.log_scope, open.selected_scope);
    }

    #[test]
    fn a_failed_branch_load_puts_the_dropdown_back() {
        let repo = repo_with_feature_branch();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        // The branch disappears after the list was read (deleted in another tool, say).
        repo.git(&["branch", "-D", "feature"]);
        app.select_scope(LogScope::Ref("refs/heads/feature".into()));
        settle(&mut app, &ctx);

        let open = app.open.as_ref().unwrap();
        assert_eq!(open.selected_scope, LogScope::AllBranches);
        assert_eq!(summaries(&app).len(), 2, "the old history stays");
        match app.status_line() {
            StatusLine::Error(text) => assert!(text.contains("history of feature"), "{text}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn opening_reads_the_branches() {
        let repo = repo_with_feature_branch();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        let open = app.open.as_ref().unwrap();
        let names: Vec<_> = open.refs.branches().map(|r| r.short_name().to_string()).collect();
        assert_eq!(names, ["feature", "main"]);
        assert_eq!(open.refs.head_branch(), Some("refs/heads/main"));
    }

    /// Input for one frame in which `key` is pressed.
    fn press(key: egui::Key) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        }
    }

    fn selected_summary(app: &GitCraftApp) -> Option<String> {
        let open = app.open.as_ref()?;
        open.history.log.commits.get(open.selected_row()?).map(|c| c.summary.clone())
    }

    fn loaded_message(app: &GitCraftApp) -> Option<String> {
        match &app.open.as_ref()?.details {
            DetailsState::Loaded(details) => Some(details.message.clone()),
            _ => None,
        }
    }

    #[test]
    fn selecting_a_commit_loads_its_details() {
        let repo = TestRepo::with_commits(&["First", "Second"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        assert!(matches!(app.open.as_ref().unwrap().details, DetailsState::Empty));

        app.select_row(1);
        assert!(matches!(app.open.as_ref().unwrap().details, DetailsState::Loading));
        settle(&mut app, &ctx);
        assert_eq!(loaded_message(&app).as_deref(), Some("First"));
    }

    #[test]
    fn the_last_selection_wins() {
        let repo = TestRepo::with_commits(&["First", "Second", "Third"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        // Click three rows before any details arrive; only the last one's details may show.
        app.select_row(0);
        app.select_row(1);
        app.select_row(2);
        settle(&mut app, &ctx);
        assert_eq!(selected_summary(&app).as_deref(), Some("First"));
        assert_eq!(loaded_message(&app).as_deref(), Some("First"));
    }

    #[test]
    fn arrow_keys_move_the_selection() {
        let repo = TestRepo::with_commits(&["First", "Second", "Third"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        run_frame(&mut app, &ctx, press(egui::Key::ArrowDown));
        assert_eq!(selected_summary(&app).as_deref(), Some("Third"), "↓ with nothing selected picks the newest");
        run_frame(&mut app, &ctx, press(egui::Key::ArrowDown));
        run_frame(&mut app, &ctx, press(egui::Key::ArrowDown));
        run_frame(&mut app, &ctx, press(egui::Key::ArrowDown));
        assert_eq!(selected_summary(&app).as_deref(), Some("First"), "stops at the oldest");
        run_frame(&mut app, &ctx, press(egui::Key::ArrowUp));
        assert_eq!(selected_summary(&app).as_deref(), Some("Second"));
        settle(&mut app, &ctx);
        assert_eq!(loaded_message(&app).as_deref(), Some("Second"));
    }

    #[test]
    fn arrow_keys_wait_while_about_is_open() {
        let repo = TestRepo::with_commits(&["First"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        app.run(Command::About);
        run_frame(&mut app, &ctx, press(egui::Key::ArrowDown));
        assert_eq!(selected_summary(&app), None);
    }

    #[test]
    fn a_parent_link_selects_the_parent() {
        let repo = TestRepo::with_commits(&["First", "Second"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        app.select_row(0);
        settle(&mut app, &ctx);
        let parent = match &app.open.as_ref().unwrap().details {
            DetailsState::Loaded(details) => details.parents[0].clone(),
            other => panic!("expected details, got {other:?}"),
        };
        app.select_commit(parent);
        settle(&mut app, &ctx);
        assert_eq!(selected_summary(&app).as_deref(), Some("First"));
        assert_eq!(loaded_message(&app).as_deref(), Some("First"));
    }

    #[test]
    fn switching_branch_keeps_a_selection_that_is_still_shown() {
        let repo = repo_with_feature_branch();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        let row =
            |app: &GitCraftApp, summary: &str| app.open.as_ref().unwrap().history.log.commits.iter().position(|c| c.summary == summary).unwrap();
        app.select_row(row(&app, "Base"));
        app.select_scope(LogScope::Ref("refs/heads/main".into()));
        settle(&mut app, &ctx);
        assert_eq!(selected_summary(&app).as_deref(), Some("Base"), "Base is on main too");

        app.select_scope(LogScope::AllBranches);
        settle(&mut app, &ctx);
        app.select_row(row(&app, "Feature work"));
        app.select_scope(LogScope::Ref("refs/heads/main".into()));
        settle(&mut app, &ctx);
        assert_eq!(selected_summary(&app), None, "Feature work is not on main");
        assert!(matches!(app.open.as_ref().unwrap().details, DetailsState::Empty));
    }

    #[test]
    fn the_two_panes_draw_with_details_loaded() {
        let repo = TestRepo::with_commits(&["First", "Second"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        app.select_row(0);
        settle(&mut app, &ctx);
        run_frame(&mut app, &ctx, egui::RawInput::default());
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert!(loaded_message(&app).is_some());
    }

    /// The diff on screen as (kind, text) pairs, hunk headers left out.
    fn diff_lines(app: &GitCraftApp) -> Option<Vec<(gitcraft_git::LineKind, String)>> {
        match &app.open.as_ref()?.diff {
            DiffState::Loaded(view) => Some(
                view.rows
                    .iter()
                    .filter_map(|row| match row {
                        diff_view::DiffRow::Line { kind, text, .. } => Some((*kind, text.clone())),
                        diff_view::DiffRow::Hunk(_) => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// A repository with one commit that changes two files.
    fn repo_with_file_changes() -> TestRepo {
        let repo = TestRepo::with_commits(&[]);
        repo.commit_files(&[("a.txt", "one\n"), ("b.txt", "first\n")], "Add files");
        repo.commit_files(&[("a.txt", "one\ntwo\n"), ("b.txt", "second\n")], "Change both");
        repo
    }

    #[test]
    fn selecting_a_commit_shows_its_first_file_diff() {
        let repo = repo_with_file_changes();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        app.select_row(0);
        settle(&mut app, &ctx);
        assert_eq!(app.open.as_ref().unwrap().selected_file.as_deref(), Some("a.txt"));
        use gitcraft_git::LineKind::{Added, Context, Removed};
        assert_eq!(diff_lines(&app), Some(vec![(Context, "one".into()), (Added, "two".into())]));

        app.select_file("b.txt".into());
        settle(&mut app, &ctx);
        assert_eq!(diff_lines(&app), Some(vec![(Removed, "first".into()), (Added, "second".into())]));
        // Draw it all once more, diff included.
        run_frame(&mut app, &ctx, egui::RawInput::default());
    }

    #[test]
    fn a_new_commit_selection_drops_the_old_diff() {
        let repo = repo_with_file_changes();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        app.select_row(0);
        settle(&mut app, &ctx);
        // Pick the older commit; whatever was on screen for the newer one must go.
        app.select_row(1);
        assert!(matches!(app.open.as_ref().unwrap().diff, DiffState::Empty));
        settle(&mut app, &ctx);
        let open = app.open.as_ref().unwrap();
        assert_eq!(open.selected_file.as_deref(), Some("a.txt"));
        use gitcraft_git::LineKind::Added;
        assert_eq!(diff_lines(&app), Some(vec![(Added, "one".into())]), "the root commit adds the whole file");
    }

    #[test]
    fn selecting_a_file_that_is_not_in_the_commit_does_nothing() {
        let repo = repo_with_file_changes();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        app.select_row(0);
        settle(&mut app, &ctx);
        app.select_file("missing.txt".into());
        assert_eq!(app.open.as_ref().unwrap().selected_file.as_deref(), Some("a.txt"));
    }

    #[test]
    fn product_name_is_gitcraft() {
        assert_eq!(APP_NAME, "GitCraft");
    }

    #[test]
    fn starts_with_no_repository() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        run_frame(&mut app, &ctx, egui::RawInput::default());
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert_eq!(app.status_line(), StatusLine::Info("No repository open".into()));
    }

    #[test]
    fn opening_a_repository_shows_its_commits() {
        let repo = TestRepo::with_commits(&["First", "Second", "Third"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);

        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());

        let open = app.open.as_ref().expect("a repository should be open");
        let summaries: Vec<_> = open.history.log.commits.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, ["Third", "Second", "First"]);
        assert_eq!(app.status_line(), StatusLine::Info("3 commits".into()));
        // Draw a couple more frames with the table on screen.
        run_frame(&mut app, &ctx, egui::RawInput::default());
        run_frame(&mut app, &ctx, egui::RawInput::default());
    }

    #[test]
    fn a_failed_open_keeps_the_current_repository_and_shows_why() {
        let repo = TestRepo::with_commits(&["Only"]);
        let not_a_repo = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);

        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        open_via_dialog(&mut app, &ctx, not_a_repo.path().to_path_buf());

        assert_eq!(app.open.as_ref().map(|open| open.history.log.commits.len()), Some(1));
        match app.status_line() {
            StatusLine::Error(text) => assert!(text.contains("is not a git repository"), "{text}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_repository_says_so() {
        let repo = TestRepo::with_commits(&[]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        assert_eq!(app.status_line(), StatusLine::Info("0 commits".into()));
        run_frame(&mut app, &ctx, egui::RawInput::default());
    }

    #[test]
    fn cancelling_the_dialog_changes_nothing() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        app.dialogs.set_override(Some(Box::new(|_request| Answer::Cancelled)));
        app.ask_for_repository();
        run_frame(&mut app, &ctx, egui::RawInput::default());
        run_frame(&mut app, &ctx, egui::RawInput::default());
        assert!(!app.is_busy());
        assert_eq!(app.status_line(), StatusLine::Info("No repository open".into()));
    }

    #[test]
    fn remembers_the_normal_window_size() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        run_frame_with_window(&mut app, &ctx, [1300.0, 750.0], false, false);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: false });
    }

    #[test]
    fn maximizing_keeps_the_normal_size() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        run_frame_with_window(&mut app, &ctx, [1300.0, 750.0], false, false);
        run_frame_with_window(&mut app, &ctx, [2560.0, 1400.0], true, false);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: true });
    }

    #[test]
    fn minimizing_changes_nothing() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        run_frame_with_window(&mut app, &ctx, [1300.0, 750.0], false, false);
        run_frame_with_window(&mut app, &ctx, [0.0, 0.0], false, true);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: false });
    }

    #[test]
    fn a_truncated_history_says_so() {
        let repo = TestRepo::with_commits(&["A"]);
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        open_via_dialog(&mut app, &ctx, repo.path().to_path_buf());
        // Pretend the history went past the limit.
        if let Some(open) = app.open.as_mut() {
            open.history.log.truncated = true;
        }
        assert_eq!(app.status_line(), StatusLine::Info("Showing the newest 10,000 commits; older history is not loaded yet".into()));
    }

    #[test]
    fn failure_messages_name_the_folder_once() {
        let path = std::path::Path::new("/repos/thing");
        let not_repo = gitcraft_git::GitError::NotARepository { path: path.to_path_buf() };
        assert_eq!(open_failure_message(path, &not_repo), "/repos/thing is not a git repository");

        let missing_git = gitcraft_git::GitError::NotFound;
        let message = open_failure_message(path, &missing_git);
        assert!(message.starts_with("Could not open /repos/thing: git was not found"), "{message}");
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1000), "1,000");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }
}
