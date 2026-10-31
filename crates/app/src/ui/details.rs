//! The commit details panel (right pane of the Repository tab).

use chrono::Local;
use gitcraft_git::{ChangedFile, CommitDetails, FileStatus};

use crate::graph::avatar;
use crate::ui::time::format_time;

/// What the panel has to show.
#[derive(Debug, Default)]
pub enum DetailsState {
    /// No commit selected.
    #[default]
    Empty,
    /// The selected commit's details are being read.
    Loading,
    Loaded(Box<CommitDetails>),
    /// Reading them failed; the message says why.
    Failed(String),
}

/// Draws the panel. `expanded` is whether "More info" is open (the user's choice, kept from
/// commit to commit). Returns a commit to select if the user clicked one of the parents.
pub fn show(ui: &mut egui::Ui, state: &DetailsState, expanded: &mut bool) -> Option<String> {
    match state {
        DetailsState::Empty => {
            centered(ui, |ui| {
                ui.weak("Select a commit to see its details.");
            });
            None
        }
        DetailsState::Loading => {
            centered(ui, |ui| {
                ui.spinner();
            });
            None
        }
        DetailsState::Failed(message) => {
            let color = ui.visuals().error_fg_color;
            ui.add(egui::Label::new(egui::RichText::new(message).color(color)).wrap());
            None
        }
        DetailsState::Loaded(details) => {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                // Room on the right so the line counts never sit under the scroll bar.
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin { left: 8, right: 16, top: 0, bottom: 8 })
                        .show(ui, |ui| loaded(ui, details, expanded))
                        .inner
                })
                .inner
        }
    }
}

fn centered(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        contents(ui);
    });
}

/// Description lines shown before "More info" is opened.
const COLLAPSED_LINES: usize = 3;

fn loaded(ui: &mut egui::Ui, details: &CommitDetails, expanded: &mut bool) -> Option<String> {
    ui.add_space(4.0);
    let (summary, body) = split_message(&details.message);
    ui.add(egui::Label::new(egui::RichText::new(summary).strong().size(15.0)).wrap().selectable(true));
    ui.add_space(4.0);

    // Who and which, always shown.
    ui.horizontal(|ui| {
        avatar::show(ui, &details.author_name, &details.author_email);
        ui.label(&details.author_name).on_hover_text(&details.author_email);
        ui.weak("·");
        ui.add(egui::Label::new(egui::RichText::new(short_hash(&details.id)).monospace()).selectable(true)).on_hover_text(&details.id);
    });

    if !body.is_empty() {
        ui.add_space(4.0);
        if *expanded {
            ui.add(egui::Label::new(body).wrap().selectable(true));
        } else {
            ui.label(clipped_text(ui, &preview(body), COLLAPSED_LINES));
        }
    }

    ui.add_space(4.0);
    let toggle = if *expanded { "Less info" } else { "More info" };
    if ui.link(toggle).clicked() {
        *expanded = !*expanded;
    }

    let clicked_parent = if *expanded { more_info(ui, details) } else { None };

    ui.add_space(8.0);
    ui.separator();
    files(ui, details);
    clicked_parent
}

/// The description for the collapsed view: blank lines dropped, so all of the few lines shown
/// carry text. The expanded view shows the message exactly as written.
fn preview(body: &str) -> String {
    body.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// `text` wrapped to the panel's width but cut to `rows` lines, ending in "…" when cut.
fn clipped_text(ui: &egui::Ui, text: &str, rows: usize) -> egui::text::LayoutJob {
    let format = egui::TextFormat { font_id: egui::TextStyle::Body.resolve(ui.style()), color: ui.visuals().text_color(), ..Default::default() };
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), format);
    job.wrap = egui::text::TextWrapping { max_width: ui.available_width(), max_rows: rows, break_anywhere: false, overflow_character: Some('…') };
    job
}

/// The first 7 characters of a hash, as git shows it.
fn short_hash(id: &str) -> &str {
    id.get(..7).unwrap_or(id)
}

/// The facts behind "More info": date, committer (if someone else), full hash and parents.
/// Returns a parent to select if one was clicked.
fn more_info(ui: &mut egui::Ui, details: &CommitDetails) -> Option<String> {
    let mut clicked_parent = None;
    ui.add_space(6.0);
    egui::Grid::new("commit-facts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.weak("Date");
        ui.label(format_time(details.author_time, &Local));
        ui.end_row();

        // Shown only when someone else committed it (a rebase, a cherry-pick, a web merge).
        if details.committer_email != details.author_email || details.committer_name != details.author_name {
            ui.weak("Committer");
            ui.horizontal(|ui| {
                avatar::show(ui, &details.committer_name, &details.committer_email);
                ui.label(&details.committer_name).on_hover_text(&details.committer_email);
                ui.weak(format_time(details.committer_time, &Local));
            });
            ui.end_row();
        }

        ui.weak("Commit");
        ui.add(egui::Label::new(egui::RichText::new(&details.id).monospace()).selectable(true));
        ui.end_row();

        if !details.parents.is_empty() {
            ui.weak(if details.parents.len() > 1 { "Parents" } else { "Parent" });
            ui.horizontal(|ui| {
                for parent in &details.parents {
                    if ui.link(egui::RichText::new(short_hash(parent)).monospace()).on_hover_text("Select this commit").clicked() {
                        clicked_parent = Some(parent.clone());
                    }
                }
            });
            ui.end_row();
        }
    });
    clicked_parent
}

/// The summary (first line) and the body (the rest, without the blank line between them).
pub fn split_message(message: &str) -> (&str, &str) {
    match message.split_once('\n') {
        Some((summary, rest)) => (summary.trim_end(), rest.trim()),
        None => (message.trim_end(), ""),
    }
}

/// "3 files changed, +12 -4", counting only text files (binary files have no lines).
pub fn files_summary(files: &[ChangedFile]) -> String {
    let (added, removed) = files.iter().filter_map(|f| f.lines).fold((0u64, 0u64), |(a, r), (fa, fr)| (a.saturating_add(fa), r.saturating_add(fr)));
    let noun = if files.len() == 1 { "file" } else { "files" };
    format!("{} {noun} changed, +{added} -{removed}", files.len())
}

fn files(ui: &mut egui::Ui, details: &CommitDetails) {
    if details.files.is_empty() {
        ui.weak("No files changed.");
        return;
    }
    ui.label(files_summary(&details.files));
    if details.parents.len() > 1 {
        ui.weak("Compared with the first parent.");
    }
    ui.add_space(4.0);
    for file in &details.files {
        ui.horizontal(|ui| {
            let (letter_color, tooltip) = status_style(ui, file.status);
            ui.label(egui::RichText::new(file.status.letter().to_string()).monospace().strong().color(letter_color)).on_hover_text(tooltip);
            let path = ui.add(egui::Label::new(&file.path).truncate());
            if let Some(old) = &file.old_path {
                path.on_hover_text(format!("from {old}"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match file.lines {
                Some((added, removed)) => {
                    ui.label(egui::RichText::new(format!("-{removed}")).color(removed_color(ui)));
                    ui.label(egui::RichText::new(format!("+{added}")).color(added_color(ui)));
                }
                None => {
                    ui.weak("binary");
                }
            });
        });
    }
}

fn added_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(0x81, 0xc7, 0x84) } else { egui::Color32::from_rgb(0x2e, 0x7d, 0x32) }
}

fn removed_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(0xe5, 0x73, 0x73) } else { egui::Color32::from_rgb(0xc6, 0x28, 0x28) }
}

fn status_style(ui: &egui::Ui, status: FileStatus) -> (egui::Color32, &'static str) {
    match status {
        FileStatus::Added => (added_color(ui), "Added"),
        FileStatus::Deleted => (removed_color(ui), "Deleted"),
        FileStatus::Modified => (egui::Color32::from_rgb(0xff, 0xb7, 0x4d), "Modified"),
        FileStatus::Renamed => (egui::Color32::from_rgb(0x4f, 0xc3, 0xf7), "Renamed"),
        FileStatus::Copied => (egui::Color32::from_rgb(0x4f, 0xc3, 0xf7), "Copied"),
        FileStatus::TypeChanged => (ui.visuals().text_color(), "Type changed"),
        FileStatus::Other => (ui.visuals().weak_text_color(), "Other change"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(lines: Option<(u64, u64)>) -> ChangedFile {
        ChangedFile { path: "f".into(), old_path: None, status: FileStatus::Modified, lines }
    }

    #[test]
    fn splits_summary_from_body() {
        assert_eq!(split_message("Summary\n\nBody one\nBody two"), ("Summary", "Body one\nBody two"));
        assert_eq!(split_message("Only a summary"), ("Only a summary", ""));
        assert_eq!(split_message(""), ("", ""));
    }

    #[test]
    fn the_collapsed_description_is_cut_to_three_lines() {
        let ctx = egui::Context::default();
        let long = "A line of description that goes on for a while.\n".repeat(10);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let job = clipped_text(ui, &long, COLLAPSED_LINES);
            assert_eq!(job.wrap.max_rows, 3);
            assert_eq!(job.wrap.overflow_character, Some('…'));
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            assert_eq!(galley.rows.len(), 3);
            assert!(galley.elided, "the cut is marked");
        });
        output.textures_delta.clear();
    }

    #[test]
    fn the_preview_skips_blank_lines() {
        assert_eq!(preview("One\n\n- two\n  \nthree"), "One\n- two\nthree");
        assert_eq!(preview(""), "");
    }

    #[test]
    fn short_hashes() {
        assert_eq!(short_hash(&"a".repeat(40)), "aaaaaaa");
        assert_eq!(short_hash("abc"), "abc");
    }

    #[test]
    fn summarises_changed_files() {
        assert_eq!(files_summary(&[file(Some((3, 1))), file(Some((2, 0))), file(None)]), "3 files changed, +5 -1");
        assert_eq!(files_summary(&[file(Some((1, 1)))]), "1 file changed, +1 -1");
    }

    #[test]
    fn draws_every_state() {
        let details = CommitDetails {
            id: "a".repeat(40),
            parents: vec!["b".repeat(40), "c".repeat(40)],
            author_name: "Ada".into(),
            author_email: "ada@example.com".into(),
            author_time: 0,
            committer_name: "Grace".into(),
            committer_email: "grace@example.com".into(),
            committer_time: 0,
            message: "Summary\n\nBody".into(),
            files: vec![file(Some((1, 2))), file(None)],
        };
        let states = [DetailsState::Empty, DetailsState::Loading, DetailsState::Failed("nope".into()), DetailsState::Loaded(Box::new(details))];
        let ctx = egui::Context::default();
        for state in &states {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    for mut expanded in [false, true] {
                        assert_eq!(show(ui, state, &mut expanded), None, "nothing clicked without input");
                    }
                });
                output.textures_delta.clear();
            }
        }
    }
}
