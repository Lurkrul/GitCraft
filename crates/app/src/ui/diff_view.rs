//! One file's diff: hunk headers and numbered lines, tinted green for added and red for removed.

use gitcraft_git::{FileDiff, LineKind, MAX_DIFF_BYTES, MAX_DIFF_LINES};

/// Spaces a tab is shown as (egui's fonts have no tab glyph).
const TAB: &str = "    ";

/// What the diff area has to show.
#[derive(Debug, Default)]
pub enum DiffState {
    /// No file selected.
    #[default]
    Empty,
    Loading,
    Loaded(Box<DiffView>),
    Failed(String),
}

/// A row of the diff as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// A hunk's `@@ ... @@` header.
    Hunk(String),
    Line {
        kind: LineKind,
        old: Option<u32>,
        new: Option<u32>,
        text: String,
    },
}

/// A diff made ready to draw: flat rows (so only the visible ones need laying out) and the width
/// of the line-number columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffView {
    pub rows: Vec<DiffRow>,
    pub binary: bool,
    pub truncated: bool,
    /// Digits in the largest line number, for sizing the number columns.
    pub number_digits: usize,
}

impl DiffView {
    pub fn new(diff: FileDiff) -> DiffView {
        let mut rows = Vec::new();
        let mut largest = 0u32;
        for hunk in diff.hunks {
            rows.push(DiffRow::Hunk(hunk.header));
            for line in hunk.lines {
                largest = largest.max(line.old_line.unwrap_or(0)).max(line.new_line.unwrap_or(0));
                rows.push(DiffRow::Line { kind: line.kind, old: line.old_line, new: line.new_line, text: line.text.replace('\t', TAB) });
            }
        }
        DiffView { rows, binary: diff.binary, truncated: diff.truncated, number_digits: largest.to_string().len() }
    }
}

/// The message shown when a diff couldn't be read, explaining "too large" plainly.
pub fn failure_message(error: &gitcraft_git::GitError) -> String {
    match error {
        gitcraft_git::GitError::OutputTooLarge { .. } => {
            format!("This file's changes are too large to show (over {} MiB).", MAX_DIFF_BYTES / (1024 * 1024))
        }
        other => format!("Could not read this file's changes: {other}"),
    }
}

/// Draws the diff. `scroll_key` identifies what is shown (commit and file): a new key starts the
/// view at the top instead of keeping the previous file's scroll position.
pub fn show(ui: &mut egui::Ui, state: &DiffState, scroll_key: impl std::hash::Hash + std::fmt::Debug) {
    match state {
        DiffState::Empty => {
            ui.centered_and_justified(|ui| {
                ui.weak("Select a file to see its changes.");
            });
        }
        DiffState::Loading => {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
        }
        DiffState::Failed(message) => {
            let color = ui.visuals().error_fg_color;
            ui.add(egui::Label::new(egui::RichText::new(message).color(color)).wrap());
        }
        DiffState::Loaded(view) if view.binary => {
            ui.centered_and_justified(|ui| {
                ui.weak("Binary file: there are no lines to compare.");
            });
        }
        DiffState::Loaded(view) if view.rows.is_empty() => {
            ui.centered_and_justified(|ui| {
                ui.weak("No changes to the file's contents (it was renamed, or its mode changed).");
            });
        }
        DiffState::Loaded(view) => lines(ui, view, scroll_key),
    }
}

fn lines(ui: &mut egui::Ui, view: &DiffView, scroll_key: impl std::hash::Hash + std::fmt::Debug) {
    if view.truncated {
        ui.weak(format!("Showing the first {MAX_DIFF_LINES} lines of this file's changes."));
    }
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0;
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let number_width = char_width * view.number_digits.max(1) as f32;
    // Old number | new number | +/- marker | text
    let gutter = 2.0 * (number_width + char_width) + 2.0 * char_width;
    let colors = Colors::new(ui.visuals());

    // `show_rows` lays out only the rows in view, so a 20,000-line diff scrolls smoothly.
    egui::ScrollArea::both().id_salt(("file-diff", scroll_key)).auto_shrink(false).show_rows(ui, row_height, view.rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in view.rows.get(range).unwrap_or(&[]) {
            let (text, color, background, numbers) = match row {
                DiffRow::Hunk(header) => (header.as_str(), colors.weak, Some(colors.hunk), None),
                DiffRow::Line { kind, old, new, text } => {
                    let (color, background) = match kind {
                        LineKind::Added => (colors.text, Some(colors.added)),
                        LineKind::Removed => (colors.text, Some(colors.removed)),
                        LineKind::Context => (colors.text, None),
                        LineKind::NoNewline => (colors.weak, None),
                    };
                    (text.as_str(), color, background, Some((*kind, *old, *new)))
                }
            };
            let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
            let width = (gutter + galley.size().x).max(ui.available_width());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_height), egui::Sense::hover());
            let painter = ui.painter();
            if let Some(background) = background {
                painter.rect_filled(rect, 0.0, background);
            }
            let y = rect.center().y;
            if let Some((kind, old, new)) = numbers {
                let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                let right_of = |columns: f32| rect.left() + columns;
                painter.text(egui::pos2(right_of(number_width), y), egui::Align2::RIGHT_CENTER, number(old), font.clone(), colors.weak);
                painter.text(
                    egui::pos2(right_of(2.0 * number_width + char_width), y),
                    egui::Align2::RIGHT_CENTER,
                    number(new),
                    font.clone(),
                    colors.weak,
                );
                let marker = match kind {
                    LineKind::Added => "+",
                    LineKind::Removed => "-",
                    LineKind::Context | LineKind::NoNewline => "",
                };
                painter.text(
                    egui::pos2(right_of(2.0 * (number_width + char_width)), y),
                    egui::Align2::LEFT_CENTER,
                    marker,
                    font.clone(),
                    colors.weak,
                );
            }
            let text_left = if numbers.is_some() { rect.left() + gutter } else { rect.left() + char_width };
            painter.galley(egui::pos2(text_left, y - galley.size().y / 2.0), galley, color);
        }
    });
}

/// The diff's colours for the current theme.
struct Colors {
    text: egui::Color32,
    weak: egui::Color32,
    added: egui::Color32,
    removed: egui::Color32,
    hunk: egui::Color32,
}

impl Colors {
    fn new(visuals: &egui::Visuals) -> Colors {
        let (added, removed) = if visuals.dark_mode {
            (egui::Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 48), egui::Color32::from_rgba_unmultiplied(0xf8, 0x51, 0x49, 48))
        } else {
            (egui::Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 40), egui::Color32::from_rgba_unmultiplied(0xcf, 0x22, 0x2e, 36))
        };
        Colors { text: visuals.text_color(), weak: visuals.weak_text_color(), added, removed, hunk: visuals.faint_bg_color }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::{DiffLine, Hunk};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
        DiffLine { kind, old_line: old, new_line: new, text: text.into() }
    }

    fn sample() -> FileDiff {
        FileDiff {
            hunks: vec![Hunk {
                header: "@@ -98,3 +98,3 @@".into(),
                lines: vec![
                    line(LineKind::Context, Some(98), Some(98), "\tfn a() {"),
                    line(LineKind::Removed, Some(99), None, "old"),
                    line(LineKind::Added, None, Some(99), "new"),
                    line(LineKind::Context, Some(100), Some(100), "}"),
                ],
            }],
            binary: false,
            truncated: false,
            source: None,
        }
    }

    #[test]
    fn flattens_hunks_into_rows() {
        let view = DiffView::new(sample());
        assert_eq!(view.rows.len(), 5);
        assert_eq!(view.rows[0], DiffRow::Hunk("@@ -98,3 +98,3 @@".into()));
        assert_eq!(
            view.rows[1],
            DiffRow::Line { kind: LineKind::Context, old: Some(98), new: Some(98), text: "    fn a() {".into() },
            "tabs become spaces"
        );
        assert_eq!(view.number_digits, 3, "line 100 needs three digits");
    }

    #[test]
    fn explains_oversized_diffs() {
        let message = failure_message(&gitcraft_git::GitError::OutputTooLarge { command: "git diff-tree".into(), limit: MAX_DIFF_BYTES });
        assert!(message.contains("too large"), "{message}");
    }

    #[test]
    fn draws_every_state() {
        let binary = FileDiff { binary: true, ..FileDiff::default() };
        let truncated = FileDiff { truncated: true, ..sample() };
        let states = [
            DiffState::Empty,
            DiffState::Loading,
            DiffState::Failed("nope".into()),
            DiffState::Loaded(Box::new(DiffView::new(binary))),
            DiffState::Loaded(Box::new(DiffView::new(FileDiff::default()))),
            DiffState::Loaded(Box::new(DiffView::new(sample()))),
            DiffState::Loaded(Box::new(DiffView::new(truncated))),
        ];
        let ctx = egui::Context::default();
        for state in &states {
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show(ui, state, "key"));
                output.textures_delta.clear();
            }
        }
    }
}
