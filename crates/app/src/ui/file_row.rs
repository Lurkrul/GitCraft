//! A clickable row for a changed file: coloured status letter, path, and extra details on the
//! right. Used by the commit details panel and the Commit tab.

/// The status letter and its look.
pub struct Letter<'a> {
    pub letter: char,
    pub color: egui::Color32,
    /// Shown on hover, such as "Modified".
    pub tooltip: &'a str,
}

/// Draws one row and returns its click response. `id_salt` must differ for each row of a list.
/// `right` fills the right end of the row (line counts, say).
pub fn show(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    letter: &Letter<'_>,
    path: &str,
    path_tooltip: Option<String>,
    selected: bool,
    right: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    // A placeholder shape, filled in once we know the row's size and whether it is hovered.
    // (Painting the highlight after the row would cover its text.)
    let background = ui.painter().add(egui::Shape::Noop);
    let row = ui
        .horizontal(|ui| {
            // Labels are not selectable here, so a click selects the file instead of text.
            ui.add(egui::Label::new(egui::RichText::new(letter.letter.to_string()).monospace().strong().color(letter.color)).selectable(false))
                .on_hover_text(letter.tooltip);
            let path = ui.add(egui::Label::new(path).truncate().selectable(false));
            if let Some(tooltip) = path_tooltip {
                path.on_hover_text(tooltip);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        })
        .response;
    let response = ui.interact(row.rect, ui.scope_id().with(id_salt), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        Some(ui.visuals().selection.bg_fill)
    } else if response.hovered() {
        Some(ui.visuals().widgets.hovered.weak_bg_fill)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().set(background, egui::epaint::RectShape::filled(row.rect.expand(1.0), 2.0, fill));
    }
    response
}

/// A label that can't be selected (so clicks go to the row).
pub fn plain(ui: &mut egui::Ui, text: egui::RichText) {
    ui.add(egui::Label::new(text).selectable(false));
}

pub fn added_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(0x81, 0xc7, 0x84) } else { egui::Color32::from_rgb(0x2e, 0x7d, 0x32) }
}

pub fn removed_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(0xe5, 0x73, 0x73) } else { egui::Color32::from_rgb(0xc6, 0x28, 0x28) }
}

pub const MODIFIED_COLOR: egui::Color32 = egui::Color32::from_rgb(0xff, 0xb7, 0x4d);
pub const RENAMED_COLOR: egui::Color32 = egui::Color32::from_rgb(0x4f, 0xc3, 0xf7);
