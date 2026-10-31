//! GitCraft: the desktop app's entry point.

// Hide the console window that Windows would otherwise open next to a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod graph;
mod settings;
#[cfg(test)]
mod test_support;
mod ui;
mod worker;

use std::process::ExitCode;

use craft_settings::{Location, SettingsStore};

fn main() -> ExitCode {
    // Never fails: a missing or damaged file gives defaults, and the report says what happened.
    let (settings, report) = SettingsStore::<settings::Prefs>::open(Location::Auto);
    let status = settings::load_problem_message(&report);

    let window = &settings.get().window;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(app::APP_NAME)
            .with_inner_size(window.sanitized_size())
            .with_maximized(window.maximized)
            .with_min_inner_size(settings::MIN_WINDOW_SIZE),
        ..Default::default()
    };

    // `run_native` takes a closure that builds the app once the window exists. `move` makes the
    // closure take ownership of `settings` and `status` (they are moved into the app, not
    // copied). `Box::new(...)` puts the app on the heap behind a trait object
    // (`Box<dyn eframe::App>`), like a `std::unique_ptr<App>` to a base class in C++.
    let result = eframe::run_native(app::APP_NAME, options, Box::new(move |cc| Ok(Box::new(app::GitCraftApp::new(&cc.egui_ctx, settings, status)))));

    // A window that can't be created (no GPU driver, no display) is reported, not a panic.
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: could not start: {err}", app::APP_NAME);
            ExitCode::FAILURE
        }
    }
}
