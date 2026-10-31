//! The commit history table.

use chrono::{DateTime, Local, TimeZone};
use egui_extras::{Column, TableBuilder};
use gitcraft_git::Commit;

/// Draws `commits` as a table: hash, summary, author, date. Only the rows on screen are laid
/// out, so a history of thousands of commits scrolls smoothly.
pub fn show(ui: &mut egui::Ui, commits: &[Commit]) {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().item_spacing.y;
    let hash_width = ui.fonts_mut(|fonts| fonts.glyph_width(&egui::TextStyle::Monospace.resolve(ui.style()), '0')) * 9.0;

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(hash_width))
        .column(Column::remainder().at_least(160.0).clip(true))
        .column(Column::initial(160.0).at_least(60.0).clip(true))
        .column(Column::initial(130.0).at_least(60.0).clip(true))
        .header(row_height, |mut header| {
            for title in ["Hash", "Summary", "Author", "Date"] {
                header.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(row_height, commits.len(), |mut row| {
                // `get` instead of `commits[i]`: an index out of range gives `None`, never a panic.
                let Some(commit) = commits.get(row.index()) else {
                    return;
                };
                row.col(|ui| {
                    ui.monospace(commit.short_id()).on_hover_text(&commit.id);
                });
                row.col(|ui| {
                    ui.add(egui::Label::new(&commit.summary).truncate());
                });
                row.col(|ui| {
                    ui.add(egui::Label::new(&commit.author_name).truncate()).on_hover_text(&commit.author_email);
                });
                row.col(|ui| {
                    ui.label(format_time(commit.author_time, &Local));
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
