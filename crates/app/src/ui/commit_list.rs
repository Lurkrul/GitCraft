//! The commit history: the graph, then each commit's author avatar, branch and tag labels and summary.

use egui_extras::{Column, TableBuilder};

use crate::graph::History;
use crate::graph::{avatar, paint};
use crate::ui::ref_labels::{self, LabelMap};

/// Rows are tall enough for an avatar.
const MIN_ROW_HEIGHT: f32 = 24.0;

/// Draws the history with the `selected` commit highlighted, scrolling to row `scroll_to` if
/// given. Returns the row the user clicked, if any. Only the rows on screen are laid out and
/// painted, so a history of thousands of commits scrolls smoothly.
pub fn show(ui: &mut egui::Ui, history: &History, labels: &LabelMap, selected: Option<&str>, scroll_to: Option<usize>) -> Option<usize> {
    let commits = &history.log.commits;
    let row_height = (ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.spacing().item_spacing.y).max(MIN_ROW_HEIGHT);
    let mut clicked = None;

    // No gap between rows, so graph lines run on unbroken from one row to the next.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(paint::column_width(history.max_width)))
        .column(Column::remainder().clip(true));
    if let Some(row) = scroll_to {
        table = table.scroll_to_row(row, None);
    }
    table.body(|body| {
        body.rows(row_height, commits.len(), |mut row| {
            let index = row.index();
            // `get` instead of `[index]`: out of range gives `None`, never a panic.
            let (Some(commit), Some(graph_row)) = (commits.get(index), history.rows.get(index)) else {
                return;
            };
            row.set_selected(selected == Some(commit.id.as_str()));
            row.col(|ui| {
                paint::paint_row(ui.painter(), ui.max_rect(), graph_row, commit);
            });
            row.col(|ui| {
                // The author's avatar comes first, then the branch and tag labels, then the summary.
                avatar::show(ui, &commit.author_name, &commit.author_email);
                if let Some(commit_labels) = labels.get(&commit.id) {
                    ref_labels::show(ui, commit_labels, paint::lane_color(graph_row.color));
                }
                // Not selectable: selectable text would take the click instead of the row.
                ui.add(egui::Label::new(&commit.summary).truncate().selectable(false));
            });
            if row.response().clicked() {
                clicked = Some(index);
            }
        });
    });
    clicked
}
