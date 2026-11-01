//! A clickable row for a changed file: coloured status letter, path, and extra details on the
//! right. Used by the commit details panel and the Commit tab.

/// The status letter and its look.
pub struct Letter<'a> {
    pub letter: char,
    pub color: egui::Color32,
    /// Shown on hover, such as "Modified".
    pub tooltip: &'a str,
}

/// Where the status letter goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetterSide {
    /// Before the path (the commit details panel).
    Left,
    /// At the right end of the row (the Commit tab, where the checkbox comes first).
    Right,
}

/// Draws one row and returns its click response. `id_salt` must differ for each row of a list.
/// `right` fills the right end of the row (line counts, say). The path is cut with "…" if the
/// row is too narrow, so whatever is on the right always stays visible.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    letter: &Letter<'_>,
    side: LetterSide,
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
            if side == LetterSide::Left {
                letter_label(ui, letter);
            }
            // The right end is laid out first (right to left), then the path takes what is left.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Keep the right end clear of a vertical scroll bar, which floats over the list.
                ui.add_space(scroll_bar_gap(ui));
                if side == LetterSide::Right {
                    letter_label(ui, letter);
                }
                right(ui);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Labels are not selectable here, so a click selects the file instead of text.
                    let path = ui.add(egui::Label::new(path).truncate().selectable(false));
                    if let Some(tooltip) = path_tooltip {
                        path.on_hover_text(tooltip);
                    }
                });
            });
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

/// Room left at the right end of a row for a floating scroll bar at its widest (when hovered).
fn scroll_bar_gap(ui: &egui::Ui) -> f32 {
    let scroll = &ui.spacing().scroll;
    scroll.bar_width + scroll.bar_outer_margin
}

fn letter_label(ui: &mut egui::Ui, letter: &Letter<'_>) -> egui::Response {
    ui.add(egui::Label::new(egui::RichText::new(letter.letter.to_string()).monospace().strong().color(letter.color)).selectable(false))
        .on_hover_text(letter.tooltip)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The letter at the right end must stop short of where a scroll bar would be drawn.
    #[test]
    fn right_letter_leaves_room_for_the_scroll_bar() {
        let ctx = egui::Context::default();
        let mut checked = false;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let letter = Letter { letter: 'M', color: MODIFIED_COLOR, tooltip: "Modified" };
            let available_right = ui.max_rect().right();
            // The letter's width, measured by drawing it once on its own.
            let letter_width = letter_label(ui, &letter).rect.width();
            let mut content_left = f32::NEG_INFINITY;
            show(ui, "row", &letter, LetterSide::Right, "src/main.rs", None, false, |ui| {
                // Laying out right to left, the cursor sits just left of what is placed so far
                // (the gap, then the letter), so this is where the letter starts.
                content_left = ui.cursor().right();
            });
            let gap = scroll_bar_gap(ui);
            assert!(gap > 0.0 && letter_width > 0.0);
            // The gap, the letter and the spacing after it must all fit right of the cursor.
            let limit = available_right - gap - letter_width - ui.spacing().item_spacing.x;
            assert!(content_left <= limit + 0.5, "letter starts at {content_left}, must start by {limit}");
            checked = true;
        });
        output.textures_delta.clear();
        assert!(checked);
    }
}
