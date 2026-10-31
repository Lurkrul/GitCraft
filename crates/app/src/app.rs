//! The GitCraft window: state, per-frame logic and drawing.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use craft_dialogs::{Answer, Dialogs, FileDialog};
use craft_settings::{SaveStatus, SettingsStore};
use gitcraft_git::{DEFAULT_LOG_LIMIT, Log, Repo};

use crate::settings::Prefs;
use crate::ui::status_bar::StatusLine;
use crate::ui::{commit_list, status_bar};
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
    log: Log,
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
        let sent = self.worker.as_mut().is_some_and(|worker| worker.send(Job::Open(path)));
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
        let Some(worker) = self.worker.as_mut() else {
            return;
        };
        for event in worker.poll() {
            match event {
                Event::Opened { repo, log } => {
                    self.open = Some(OpenRepo { repo, log });
                    self.error = None;
                }
                // The repository already on screen (if any) stays; the error explains what happened.
                Event::Failed { path, error } => self.error = Some(open_failure_message(&path, &error)),
            }
        }
        if !worker.is_alive() {
            self.worker = None;
            self.error = Some(WORKER_STOPPED.to_string());
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
        self.handle_dialog_answers();
        self.handle_worker_events();
        self.save_settings_when_due(ctx);
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
            Some(open) if open.log.truncated => {
                StatusLine::Info(format!("Showing the newest {} commits; older history is not loaded yet", group_thousands(DEFAULT_LOG_LIMIT)))
            }
            Some(open) => {
                let count = open.log.commits.len();
                StatusLine::Info(format!("{} commit{}", group_thousands(count), if count == 1 { "" } else { "s" }))
            }
        }
    }

    /// Draws the whole window. Kept separate from `eframe::App::ui` (which also needs an
    /// `eframe::Frame`) so tests can run it on a bare `egui::Context` without opening a window.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open repository…").clicked() {
                    self.ask_for_repository();
                }
                if let Some(open) = &self.open {
                    ui.separator();
                    ui.label(open.repo.root().display().to_string());
                }
            });
        });
        let status = self.status_line();
        egui::Panel::bottom("status_bar").show(ui, |ui| status_bar::show(ui, &status));
        egui::CentralPanel::default().show(ui, |ui| match &self.open {
            Some(open) if open.log.commits.is_empty() => {
                ui.centered_and_justified(|ui| {
                    ui.label("This repository has no commits yet.");
                });
            }
            Some(open) => commit_list::show(ui, &open.log.commits),
            None => self.draw_welcome(ui),
        });
    }

    /// Shown while no repository is open.
    fn draw_welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading(APP_NAME);
            ui.label("Open a repository to see its history.");
            ui.add_space(8.0);
            if ui.button("Open repository…").clicked() {
                self.ask_for_repository();
            }
        });
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
        let summaries: Vec<_> = open.log.commits.iter().map(|c| c.summary.as_str()).collect();
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

        assert_eq!(app.open.as_ref().map(|open| open.log.commits.len()), Some(1));
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
            open.log.truncated = true;
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
