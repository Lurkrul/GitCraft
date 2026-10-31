//! The GitCraft window: per-frame logic and drawing.

use std::time::Instant;

use craft_settings::{SaveStatus, SettingsStore};

use crate::settings::Prefs;

/// The product name, used for the window title and messages.
pub const APP_NAME: &str = "GitCraft";

/// All of the app's state. eframe owns it and calls `logic` and `ui` on it every frame.
pub struct GitCraftApp {
    settings: SettingsStore<Prefs>,
    /// Shown in the status bar; the latest problem worth telling the user about.
    status: Option<String>,
}

impl GitCraftApp {
    /// `settings` is moved in: the app owns the store from now on, and nothing else can use it.
    pub fn new(settings: SettingsStore<Prefs>, status: Option<String>) -> Self {
        Self { settings, status }
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
            self.status = Some(format!("Could not save settings: {err}"));
        }
        // egui only repaints on input, so without this an idle window would never save.
        if let Some(deadline) = tick.next_deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
    }

    /// Draws the whole window. Kept separate from `eframe::App::ui` (which also needs an
    /// `eframe::Frame`) so tests can run it on a bare `egui::Context` without opening a window.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.label(self.status.as_deref().unwrap_or("No repository open"));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading(APP_NAME);
            });
        });
    }
}

impl eframe::App for GitCraftApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.remember_window(ctx);
        self.save_settings_when_due(ctx);
        // Nothing can be unsaved yet, so a close goes straight through. Must stay last in
        // `logic` so it sees any `CancelClose` the app sends (see craft-quit's docs).
        craft_quit::on_close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self) {
        // Write anything still waiting for its 500 ms delay. The window is gone, so stderr is
        // the only place left to report a failure.
        if let Err(err) = self.settings.flush() {
            eprintln!("{APP_NAME}: could not save settings: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::WindowPrefs;
    use craft_settings::Location;

    /// An app whose settings live in memory only, so tests never touch the real config file.
    fn test_app() -> GitCraftApp {
        let (store, _) = SettingsStore::open(Location::Memory);
        GitCraftApp::new(store, None)
    }

    /// Runs one `logic` pass with the given window state reported by the (pretend) OS.
    fn run_logic(app: &mut GitCraftApp, ctx: &egui::Context, size: [f32; 2], maximized: bool, minimized: bool) {
        let mut input = egui::RawInput::default();
        let viewport = input.viewports.entry(egui::ViewportId::ROOT).or_default();
        viewport.inner_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()));
        viewport.maximized = Some(maximized);
        viewport.minimized = Some(minimized);
        let mut output = ctx.run_ui(input, |ui| app.remember_window(ui.ctx()));
        output.textures_delta.clear(); // no GPU in tests; see `draws_a_frame_without_a_window`
    }

    #[test]
    fn product_name_is_gitcraft() {
        assert_eq!(APP_NAME, "GitCraft");
    }

    #[test]
    fn draws_a_frame_without_a_window() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        // Two frames: egui lays some widgets out in the first pass and settles in the second.
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.draw(ui));
            assert!(!output.shapes.is_empty(), "the window should draw something");
            // There is no GPU here to upload font textures to; the suite's egui fork asserts
            // that texture updates are not silently dropped, so discard them explicitly.
            output.textures_delta.clear();
        }
    }

    #[test]
    fn remembers_the_normal_window_size() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        run_logic(&mut app, &ctx, [1300.0, 750.0], false, false);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: false });
    }

    #[test]
    fn maximizing_keeps_the_normal_size() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        run_logic(&mut app, &ctx, [1300.0, 750.0], false, false);
        run_logic(&mut app, &ctx, [2560.0, 1400.0], true, false);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: true });
    }

    #[test]
    fn minimizing_changes_nothing() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        run_logic(&mut app, &ctx, [1300.0, 750.0], false, false);
        run_logic(&mut app, &ctx, [0.0, 0.0], false, true);
        assert_eq!(app.settings.get().window, WindowPrefs { width: 1300.0, height: 750.0, maximized: false });
    }
}
