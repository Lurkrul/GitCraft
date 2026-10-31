//! Help → About GitCraft.

use crate::app::APP_NAME;

/// Shows the About window while `open` is true; its Close button and Esc set it to false.
pub fn show(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    let modal = egui::Modal::new(egui::Id::unique("gitcraft-about")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.vertical_centered(|ui| {
            ui.heading(APP_NAME);
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(6.0);
            ui.label("Git, GitHub Issues and GitHub Projects in one window.");
            ui.label("Licensed under MIT or Apache-2.0.");
            ui.add_space(6.0);
            if ui.button("Close").clicked() {
                ui.close();
            }
        });
    });
    // A click outside the window, Esc, or the Close button.
    if modal.should_close() {
        *open = false;
    }
}
