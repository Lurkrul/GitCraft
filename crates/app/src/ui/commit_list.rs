//! The commit history: the graph, then each commit's author avatar, branch and tag labels and summary.

use chrono::{DateTime, Local, TimeZone};
use egui_extras::{Column, TableBuilder};

use crate::graph::History;
use crate::graph::{avatar, paint};
use crate::ui::ref_labels::{self, LabelMap};

/// Rows are tall enough for an avatar.
const MIN_ROW_HEIGHT: f32 = 24.0;

/// Draws the history. Only the rows on screen are laid out and painted, so a history of
/// thousands of commits scrolls smoothly.
pub fn show(ui: &mut egui::Ui, history: &History, labels: &LabelMap) {
    let commits = &history.log.commits;
    let row_height = (ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().item_spacing.y).max(MIN_ROW_HEIGHT);

    // No gap between rows, so graph lines run on unbroken from one row to the next.
    ui.spacing_mut().item_spacing.y = 0.0;
    // Just the graph and the summary: author, date and hash belong to the commit details panel.
    TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(paint::column_width(history.max_width)))
        .column(Column::remainder().clip(true))
        .body(|body| {
            body.rows(row_height, commits.len(), |mut row| {
                let index = row.index();
                // `get` instead of `[index]`: out of range gives `None`, never a panic.
                let (Some(commit), Some(graph_row)) = (commits.get(index), history.rows.get(index)) else {
                    return;
                };
                row.col(|ui| {
                    paint::paint_row(ui.painter(), ui.max_rect(), graph_row, commit);
                });
                row.col(|ui| {
                    // The author's avatar comes first, then the branch and tag labels, then the summary.
                    avatar::show(ui, &commit.author_name, &commit.author_email);
                    if let Some(commit_labels) = labels.get(&commit.id) {
                        ref_labels::show(ui, commit_labels, paint::lane_color(graph_row.color));
                    }
                    // Until the details panel exists, hovering the summary shows who, when and which.
                    let details = format!("{}\n{}  ·  {}", commit.author_name, format_time(commit.author_time, &Local), commit.short_id());
                    ui.add(egui::Label::new(&commit.summary).truncate()).on_hover_text(details);
                });
            });
        });
}

/// Formats a Unix time as `2026-10-31 14:05` in the given time zone (the user's local zone in
/// the app; a fixed one in tests). Times outside chrono's range show as a dash.
///
/// Generic over the time zone type `Tz` (like a C++ template parameter) so tests can pass `Utc`.
pub fn format_time<Tz: TimeZone>(unix_seconds: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    match DateTime::from_timestamp(unix_seconds, 0) {
        Some(utc) => utc.with_timezone(zone).format("%Y-%m-%d %H:%M").to_string(),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    #[test]
    fn formats_in_the_given_zone() {
        assert_eq!(format_time(1_700_000_000, &Utc), "2023-11-14 22:13");
        let plus_ten = FixedOffset::east_opt(10 * 3600).unwrap();
        assert_eq!(format_time(1_700_000_000, &plus_ten), "2023-11-15 08:13");
    }

    #[test]
    fn handles_old_and_impossible_times() {
        assert_eq!(format_time(-86_400, &Utc), "1969-12-31 00:00");
        assert_eq!(format_time(i64::MAX, &Utc), "—");
    }
}
