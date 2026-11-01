//! The toolbar's big boxes, like GitHub Desktop's: a label on top, the value larger underneath, and (for a dropdown) a small arrow at the right edge. Clicking a dropdown box opens its list
//! in a popup below it.

/// Width of each box.
pub const BOX_WIDTH: f32 = 230.0;
/// Most of a list shown before it scrolls.
const LIST_HEIGHT: f32 = 420.0;
/// The label's and the value's text size, relative to normal body text.
const CAPTION_SCALE: f32 = 0.95;
const VALUE_SCALE: f32 = 1.2;

/// Draws a box and returns its response (clicked, hovered…). A dropdown box gets a small arrow at
/// its right edge.
pub fn toolbar_box(ui: &mut egui::Ui, caption: &str, value: &str, dropdown_arrow: bool, enabled: bool) -> egui::Response {
    let padding = egui::vec2(10.0, 6.0);
    // Sized from the body text, so they follow the user's zoom: the label a little smaller than
    // normal text (egui's "small" is too small to read comfortably here), the value larger.
    let body = egui::TextStyle::Body.resolve(ui.style());
    let caption_font = egui::FontId::new(body.size * CAPTION_SCALE, body.family.clone());
    let value_font = egui::FontId::new(body.size * VALUE_SCALE, body.family);
    let height = ui.fonts_mut(|fonts| fonts.row_height(&caption_font) + fonts.row_height(&value_font)) + 2.0 * padding.y + 3.0;
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(BOX_WIDTH, height), sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = if enabled { ui.style().interact(&response) } else { &ui.visuals().widgets.noninteractive };
    let painter = ui.painter();
    painter.rect(rect, 4.0, visuals.weak_bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);

    let inner = rect.shrink2(padding);
    let trailing_width = if dropdown_arrow { 16.0 } else { 0.0 };
    let text_width = (inner.width() - trailing_width).max(0.0);
    let weak = ui.visuals().weak_text_color();
    // The label in normal text colour (the "weak" grey was too close to the box's background), the
    // value in the strongest; a disabled box greys both.
    let (caption_color, value_color) = if enabled { (ui.visuals().text_color(), ui.visuals().strong_text_color()) } else { (weak, weak) };
    // Long names are cut with "…" so the box keeps its size.
    let caption_galley = one_line(ui, caption, caption_font, caption_color, text_width);
    let value_galley = one_line(ui, value, value_font, value_color, text_width);
    let caption_height = caption_galley.size().y;
    painter.galley(inner.left_top(), caption_galley, caption_color);
    painter.galley(inner.left_top() + egui::vec2(0.0, caption_height + 2.0), value_galley, value_color);
    if dropdown_arrow {
        // Drawn, not a text glyph: egui's built-in fonts have no "▾".
        let center = egui::pos2(inner.right() - 5.0, rect.center().y);
        let points = vec![center + egui::vec2(-5.0, -2.5), center + egui::vec2(5.0, -2.5), center + egui::vec2(0.0, 3.5)];
        painter.add(egui::Shape::convex_polygon(points, weak, egui::Stroke::NONE));
    }
    response
}

/// A single line of text, cut with "…" to fit `width`.
fn one_line(ui: &egui::Ui, text: &str, font: egui::FontId, color: egui::Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// A dropdown box: the box, and its list in a popup below while open. `list` draws the list and
/// returns what was picked; picking closes the popup.
pub fn dropdown<R>(ui: &mut egui::Ui, caption: &str, value: &str, hover: &str, list: impl FnOnce(&mut egui::Ui) -> Option<R>) -> Option<R> {
    let response = toolbar_box(ui, caption, value, true, true).on_hover_text(hover);
    let mut picked = None;
    // `CloseOnClickOutside`: right-clicking an item (for its context menu) mustn't close the list;
    // a pick closes it explicitly.
    egui::Popup::menu(&response).width(BOX_WIDTH).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
            picked = list(ui);
        });
        if picked.is_some() {
            ui.close();
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_draw_with_long_and_short_text() {
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let response = toolbar_box(ui, "Current repository", &"a-very-long-repository-name".repeat(5), true, true);
                assert!((response.rect.width() - BOX_WIDTH).abs() < 0.5, "the box keeps its width");
                toolbar_box(ui, "Push", "Up to date", false, false);
                let picked: Option<u8> = dropdown(ui, "Current branch", "main", "hover", |_| None);
                assert_eq!(picked, None);
            });
            output.textures_delta.clear();
        }
    }
}
