//! Two panes side by side with a draggable divider, built on craft-ui's split geometry and
//! resize handle (pointer drags, arrow keys when focused, and screen-reader support).

use craft_ui::layout::{ResizeHandle, ResizeHandleStyle, SplitAxis, SplitSize, split_rect};

/// Width of the divider, in points.
const DIVIDER: f32 = 6.0;
/// Neither pane gets narrower than this (unless the window itself is too narrow).
const MIN_PANE: f32 = 220.0;

/// The share of the width the left pane gets when nothing else has been chosen.
pub const DEFAULT_RATIO: f32 = 0.55;

/// Fills the rest of `ui` with a left and a right pane. `ratio` is the left pane's share of the
/// width; dragging the divider (or focusing it and pressing ←/→) changes it.
pub fn two_panes(ui: &mut egui::Ui, id: egui::Id, ratio: &mut f32, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.available_rect_before_wrap();
    let size = SplitSize::Ratio(sane_ratio(*ratio));
    // `split_rect` refuses only impossible geometry (such as a zero-area window while minimized);
    // then there is nothing to draw.
    let Some(layout) = split_rect(rect, SplitAxis::Horizontal, size, DIVIDER, MIN_PANE) else {
        return;
    };

    // Each pane is a child `Ui` limited to its rectangle.
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.first), |ui| {
        ui.set_clip_rect(layout.first);
        left(ui);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.second), |ui| {
        ui.set_clip_rect(layout.second);
        right(ui);
    });

    // A thin line marks the divider; the handle lights it up on hover, drag and focus.
    let line_x = layout.divider.center().x;
    ui.painter().vline(line_x, layout.divider.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
    let visuals = ui.visuals();
    let style =
        ResizeHandleStyle { hovered: visuals.widgets.hovered.bg_fill, dragged: visuals.selection.bg_fill, focused: visuals.selection.stroke.color };
    let resize = ResizeHandle::new(id, SplitAxis::Horizontal, layout.divider, "Resize the history and details panes", style).show(ui);
    if resize.delta != 0.0
        && let Some(SplitSize::Ratio(new_ratio)) = layout.resized(size, resize.delta, MIN_PANE)
    {
        *ratio = new_ratio;
    }

    ui.advance_cursor_after_rect(rect);
}

/// A ratio that is safe to lay out with: settings files and drags can produce nonsense.
pub fn sane_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() { ratio.clamp(0.1, 0.9) } else { DEFAULT_RATIO }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_are_kept_sane() {
        assert_eq!(sane_ratio(0.5), 0.5);
        assert_eq!(sane_ratio(-3.0), 0.1);
        assert_eq!(sane_ratio(7.0), 0.9);
        assert_eq!(sane_ratio(f32::NAN), DEFAULT_RATIO);
    }

    #[test]
    fn both_panes_draw_side_by_side() {
        let ctx = egui::Context::default();
        let mut ratio = 0.5;
        let (mut left_rect, mut right_rect) = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                two_panes(ui, egui::Id::unique("split-test"), &mut ratio, |ui| left_rect = ui.max_rect(), |ui| right_rect = ui.max_rect());
            });
            output.textures_delta.clear();
        }
        assert!(left_rect.right() < right_rect.left(), "{left_rect:?} {right_rect:?}");
        assert!((left_rect.width() - right_rect.width()).abs() < 1.0, "half each");
        assert_eq!(ratio, 0.5, "no drag, no change");
    }
}
