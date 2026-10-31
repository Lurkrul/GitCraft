//! Two panes with a draggable divider, built on craft-ui's split geometry and resize handle
//! (pointer drags, arrow keys when focused, and screen-reader support).

use craft_ui::layout::{ResizeHandle, ResizeHandleStyle, SplitAxis, SplitSize, split_rect};

/// Thickness of the divider, in points.
const DIVIDER: f32 = 6.0;

/// History and details, side by side: the history's share of the width to start with.
pub const DEFAULT_HISTORY_RATIO: f32 = 0.55;
/// Details above the diff: the details' share of the height to start with.
pub const DEFAULT_DETAILS_RATIO: f32 = 0.4;
/// Commit tab: the changed-files list's share of the width to start with.
pub const DEFAULT_CHANGES_RATIO: f32 = 0.3;

/// How a split is laid out.
pub struct Split<'a> {
    /// A stable id for the divider (independent of the label).
    pub id: egui::Id,
    /// `Horizontal`: first pane on the left. `Vertical`: first pane on top.
    pub axis: SplitAxis,
    /// Neither pane gets smaller than this along the axis (unless the space itself is too small).
    pub min_pane: f32,
    /// What screen readers announce for the divider.
    pub label: &'a str,
}

/// Fills the rest of `ui` with two panes. `ratio` is the first pane's share of the space;
/// dragging the divider (or focusing it and pressing the arrow keys) changes it.
pub fn two_panes(ui: &mut egui::Ui, split: &Split<'_>, ratio: &mut f32, first: impl FnOnce(&mut egui::Ui), second: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.available_rect_before_wrap();
    let size = SplitSize::Ratio(sane_ratio(*ratio));
    // `split_rect` refuses only impossible geometry (such as a zero-area window while minimized);
    // then there is nothing to draw.
    let Some(layout) = split_rect(rect, split.axis, size, DIVIDER, split.min_pane) else {
        return;
    };

    // Each pane is a child `Ui` limited to its rectangle.
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.first), |ui| {
        ui.set_clip_rect(layout.first);
        first(ui);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.second), |ui| {
        ui.set_clip_rect(layout.second);
        second(ui);
    });

    // A thin line marks the divider; the handle lights it up on hover, drag and focus.
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    match split.axis {
        SplitAxis::Horizontal => ui.painter().vline(layout.divider.center().x, layout.divider.y_range(), stroke),
        SplitAxis::Vertical => ui.painter().hline(layout.divider.x_range(), layout.divider.center().y, stroke),
    };
    let visuals = ui.visuals();
    let style =
        ResizeHandleStyle { hovered: visuals.widgets.hovered.bg_fill, dragged: visuals.selection.bg_fill, focused: visuals.selection.stroke.color };
    let resize = ResizeHandle::new(split.id, split.axis, layout.divider, split.label, style).show(ui);
    if resize.delta != 0.0
        && let Some(SplitSize::Ratio(new_ratio)) = layout.resized(size, resize.delta, split.min_pane)
    {
        *ratio = new_ratio;
    }

    ui.advance_cursor_after_rect(rect);
}

/// A ratio that is safe to lay out with: settings files and drags can produce nonsense.
pub fn sane_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() { ratio.clamp(0.1, 0.9) } else { 0.5 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_are_kept_sane() {
        assert_eq!(sane_ratio(0.5), 0.5);
        assert_eq!(sane_ratio(-3.0), 0.1);
        assert_eq!(sane_ratio(7.0), 0.9);
        assert_eq!(sane_ratio(f32::NAN), 0.5);
    }

    fn panes(axis: SplitAxis) -> (egui::Rect, egui::Rect) {
        let ctx = egui::Context::default();
        let mut ratio = 0.5;
        let split = Split { id: egui::Id::unique("split-test"), axis, min_pane: 50.0, label: "test" };
        let (mut first_rect, mut second_rect) = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                two_panes(ui, &split, &mut ratio, |ui| first_rect = ui.max_rect(), |ui| second_rect = ui.max_rect());
            });
            output.textures_delta.clear();
        }
        assert_eq!(ratio, 0.5, "no drag, no change");
        (first_rect, second_rect)
    }

    #[test]
    fn horizontal_puts_the_panes_side_by_side() {
        let (left, right) = panes(SplitAxis::Horizontal);
        assert!(left.right() < right.left(), "{left:?} {right:?}");
        assert!((left.width() - right.width()).abs() < 1.0, "half each");
    }

    #[test]
    fn vertical_stacks_the_panes() {
        let (top, bottom) = panes(SplitAxis::Vertical);
        assert!(top.bottom() < bottom.top(), "{top:?} {bottom:?}");
        assert!((top.height() - bottom.height()).abs() < 1.0, "half each");
    }
}
