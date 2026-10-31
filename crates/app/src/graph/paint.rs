//! Drawing one row of the commit graph with egui's painter.

use egui::epaint::CubicBezierShape;
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};
use gitcraft_git::Commit;

use super::lanes::{GraphRow, Segment};

/// Horizontal space per lane, in points.
pub const LANE_WIDTH: f32 = 16.0;
/// The graph column never grows past this many lanes; wider graphs are clipped at the right.
pub const MAX_SHOWN_LANES: usize = 16;
const LINE_WIDTH: f32 = 2.0;
const COMMIT_DOT_RADIUS: f32 = 5.0;
const MERGE_DOT_RADIUS: f32 = 3.5;

/// Lane (and avatar) colours: distinct, and readable against both the dark theme and dark text.
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x4f, 0xc3, 0xf7), // light blue
    Color32::from_rgb(0x81, 0xc7, 0x84), // green
    Color32::from_rgb(0xff, 0xb7, 0x4d), // orange
    Color32::from_rgb(0xe5, 0x73, 0x73), // red
    Color32::from_rgb(0xba, 0x68, 0xc8), // purple
    Color32::from_rgb(0xff, 0xd5, 0x4f), // yellow
    Color32::from_rgb(0x4d, 0xb6, 0xac), // teal
    Color32::from_rgb(0xf0, 0x62, 0x92), // pink
];
/// The colour for a palette index (lane colours and author colours both use it).
pub fn lane_color(index: usize) -> Color32 {
    PALETTE.get(index % PALETTE.len()).copied().unwrap_or(Color32::GRAY)
}

/// The width of the graph column for a graph `lanes` wide (capped at `MAX_SHOWN_LANES`).
pub fn column_width(lanes: usize) -> f32 {
    LANE_WIDTH * lanes.clamp(1, MAX_SHOWN_LANES) as f32
}

/// Draws one row of the graph inside `rect` (one table cell): the lines and the commit's dot
/// (smaller for a merge). Returns the dot's area.
pub fn paint_row(painter: &Painter, rect: Rect, row: &GraphRow, commit: &Commit) -> Rect {
    let x = |lane: usize| rect.left() + LANE_WIDTH * (lane as f32 + 0.5);
    let middle = rect.center().y;

    for segment in &row.upper {
        line(painter, segment, pos2(x(segment.from), rect.top()), pos2(x(segment.to), middle));
    }
    for segment in &row.lower {
        line(painter, segment, pos2(x(segment.from), middle), pos2(x(segment.to), rect.bottom()));
    }

    let center = pos2(x(row.lane), middle);
    let radius = if commit.is_merge() { MERGE_DOT_RADIUS } else { COMMIT_DOT_RADIUS };
    painter.circle_filled(center, radius, lane_color(row.color));
    Rect::from_center_size(center, egui::Vec2::splat(radius * 2.0))
}

/// A straight line within a lane, or a smooth S-curve when it moves between lanes.
fn line(painter: &Painter, segment: &Segment, start: Pos2, end: Pos2) {
    let stroke = Stroke::new(LINE_WIDTH, lane_color(segment.color));
    if segment.from == segment.to {
        painter.line_segment([start, end], stroke);
    } else {
        // Control points straight above and below the ends make the curve leave and arrive
        // vertically, like a railway switch.
        let bend = (end.y - start.y) / 2.0;
        let points = [start, pos2(start.x, start.y + bend), pos2(end.x, end.y - bend), end];
        painter.add(CubicBezierShape::from_points_stroke(points, false, Color32::TRANSPARENT, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_cycle() {
        assert_eq!(lane_color(0), lane_color(PALETTE.len()));
        assert_ne!(lane_color(0), lane_color(1));
    }

    #[test]
    fn column_width_is_capped() {
        assert_eq!(column_width(0), LANE_WIDTH);
        assert_eq!(column_width(3), LANE_WIDTH * 3.0);
        assert_eq!(column_width(500), LANE_WIDTH * MAX_SHOWN_LANES as f32);
    }

    #[test]
    fn paints_without_a_window() {
        let commits = vec![
            Commit {
                id: "m".into(),
                parents: vec!["a".into(), "b".into()],
                author_time: 0,
                author_name: "A B".into(),
                author_email: String::new(),
                summary: String::new(),
            },
            Commit {
                id: "a".into(),
                parents: vec![],
                author_time: 0,
                author_name: "Ada".into(),
                author_email: String::new(),
                summary: String::new(),
            },
        ];
        let rows = super::super::lanes::assign_lanes(&commits);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(60.0, 24.0));
            for (row, commit) in rows.iter().zip(&commits) {
                let dot = paint_row(ui.painter(), rect, row, commit);
                assert!(rect.contains_rect(dot));
            }
        });
        output.textures_delta.clear();
    }
}
