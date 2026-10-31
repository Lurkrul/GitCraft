//! The status bar along the bottom of the window.

/// What the status bar shows. `app.rs` decides; this module only draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusLine {
    /// Ordinary information, such as the commit count.
    Info(String),
    /// Work is running in the background.
    Busy(String),
    /// Something went wrong; shown in the theme's error colour.
    Error(String),
}

pub fn show(ui: &mut egui::Ui, line: &StatusLine) {
    ui.horizontal(|ui| match line {
        StatusLine::Info(text) => {
            ui.label(text);
        }
        StatusLine::Busy(text) => {
            ui.spinner();
            ui.label(text);
        }
        StatusLine::Error(text) => {
            let color = ui.visuals().error_fg_color;
            // Wrap rather than truncate: the whole message matters, and git errors can be long.
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
        }
    });
}
