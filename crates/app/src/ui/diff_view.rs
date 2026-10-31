//! One file's diff: hunk headers and numbered lines, tinted green for added and red for removed.
//! In the Commit tab, each added or removed line and each hunk also gets a checkbox.

use gitcraft_git::{FileDiff, LineKind, LineRef, MAX_DIFF_BYTES, MAX_DIFF_LINES};

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
    /// A hunk's `@@ ... @@` header, and the hunk's index in the diff.
    Hunk { header: String, index: usize },
    Line {
        kind: LineKind,
        old: Option<u32>,
        new: Option<u32>,
        text: String,
        /// Where the line is in the diff, for its checkbox.
        at: LineRef,
        /// The block (index into [`DiffView::blocks`]) an added or removed line belongs to.
        block: Option<usize>,
    },
}

/// A run of added and removed lines with no unchanged line between them: ticked or unticked as
/// one with the handle beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The block's first and last rows in [`DiffView::rows`].
    pub first_row: usize,
    pub last_row: usize,
    /// Its added and removed lines.
    pub lines: Vec<LineRef>,
}

/// A diff made ready to draw: flat rows (so only the visible ones need laying out) and the width
/// of the line-number columns. The diff itself is kept too (the Commit tab builds patches from it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffView {
    pub rows: Vec<DiffRow>,
    pub blocks: Vec<Block>,
    /// Digits in the largest line number, for sizing the number columns.
    pub number_digits: usize,
    pub diff: FileDiff,
}

impl DiffView {
    pub fn new(diff: FileDiff) -> DiffView {
        let mut rows = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut largest = 0u32;
        for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
            rows.push(DiffRow::Hunk { header: hunk.header.clone(), index: hunk_index });
            // The block being built, if the previous line was a change. A hunk header or an
            // unchanged line ends it; a "no newline" marker belongs to the line before it.
            let mut open: Option<usize> = None;
            for (line_index, line) in hunk.lines.iter().enumerate() {
                largest = largest.max(line.old_line.unwrap_or(0)).max(line.new_line.unwrap_or(0));
                let at = LineRef { hunk: hunk_index, line: line_index };
                let row = rows.len();
                let block = match line.kind {
                    LineKind::Added | LineKind::Removed => {
                        let index = *open.get_or_insert_with(|| {
                            blocks.push(Block { first_row: row, last_row: row, lines: Vec::new() });
                            blocks.len() - 1
                        });
                        if let Some(block) = blocks.get_mut(index) {
                            block.last_row = row;
                            block.lines.push(at);
                        }
                        Some(index)
                    }
                    LineKind::NoNewline => open,
                    LineKind::Context => {
                        open = None;
                        None
                    }
                };
                if let (LineKind::NoNewline, Some(block)) = (line.kind, block.and_then(|index| blocks.get_mut(index))) {
                    block.last_row = row;
                }
                rows.push(DiffRow::Line { kind: line.kind, old: line.old_line, new: line.new_line, text: line.text.replace('\t', TAB), at, block });
            }
        }
        DiffView { rows, blocks, number_digits: largest.to_string().len(), diff }
    }
}

/// The checkboxes' state, for the Commit tab's diff.
pub struct Ticks<'a> {
    /// False draws the boxes greyed out and ignores clicks (`disabled_reason` says why on hover).
    pub enabled: bool,
    pub disabled_reason: &'a str,
    /// Whether an added or removed line is ticked.
    pub is_ticked: &'a dyn Fn(LineRef) -> bool,
}

/// A click on a checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickClick {
    pub target: TickTarget,
    /// Shift was held: the range from the last clicked line to this one.
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickTarget {
    Line(LineRef),
    /// A hunk's box: all of its lines.
    Hunk(usize),
    /// A block's handle (index into [`DiffView::blocks`]): all of its lines.
    Block(usize),
}

/// How full a checkbox is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxState {
    Ticked,
    Unticked,
    Mixed,
}

/// The state of a hunk's box, from its changed lines.
fn hunk_state(diff: &FileDiff, hunk: usize, is_ticked: &dyn Fn(LineRef) -> bool) -> BoxState {
    let lines = diff.hunks.get(hunk).map(|h| h.lines.as_slice()).unwrap_or_default();
    let changed = lines.iter().enumerate().filter(|(_, line)| matches!(line.kind, LineKind::Added | LineKind::Removed));
    lines_state(changed.map(|(line, _)| LineRef { hunk, line }), is_ticked)
}

/// Ticked if all `lines` are, unticked if none (or there are none), mixed otherwise.
fn lines_state(lines: impl Iterator<Item = LineRef>, is_ticked: &dyn Fn(LineRef) -> bool) -> BoxState {
    let (mut ticked, mut unticked) = (false, false);
    for at in lines {
        if is_ticked(at) {
            ticked = true;
        } else {
            unticked = true;
        }
    }
    match (ticked, unticked) {
        (true, true) => BoxState::Mixed,
        (true, false) => BoxState::Ticked,
        _ => BoxState::Unticked,
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
/// view at the top instead of keeping the previous file's scroll position. With `ticks`, each
/// changed line and hunk gets a checkbox, and a click on one is returned.
pub fn show(
    ui: &mut egui::Ui,
    state: &DiffState,
    scroll_key: impl std::hash::Hash + std::fmt::Debug,
    ticks: Option<&Ticks<'_>>,
) -> Option<TickClick> {
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
        DiffState::Loaded(view) if view.diff.binary => {
            ui.centered_and_justified(|ui| {
                ui.weak("Binary file: there are no lines to compare.");
            });
        }
        DiffState::Loaded(view) if view.rows.is_empty() => {
            ui.centered_and_justified(|ui| {
                ui.weak("No changes to the file's contents (it was renamed, or its mode changed).");
            });
        }
        DiffState::Loaded(view) => return lines(ui, view, scroll_key, ticks),
    }
    None
}

fn lines(ui: &mut egui::Ui, view: &DiffView, scroll_key: impl std::hash::Hash + std::fmt::Debug, ticks: Option<&Ticks<'_>>) -> Option<TickClick> {
    if view.diff.truncated {
        ui.weak(format!("Showing the first {MAX_DIFF_LINES} lines of this file's changes."));
    }
    if let Some(ticks) = ticks.filter(|ticks| !ticks.enabled) {
        ui.weak(ticks.disabled_reason);
    }
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0;
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let number_width = char_width * view.number_digits.max(1) as f32;
    // [block box | line box] | old number | new number | +/- marker | text
    let box_width = if ticks.is_some() { row_height + 4.0 } else { 0.0 };
    let block_width = box_width;
    let tick_width = block_width + box_width;
    let gutter = tick_width + 2.0 * (number_width + char_width) + 2.0 * char_width;
    let colors = Colors::new(ui.visuals());
    let mut clicked = None;

    // `show_rows` lays out only the rows in view, so a 20,000-line diff scrolls smoothly.
    egui::ScrollArea::both().id_salt(("file-diff", scroll_key)).auto_shrink(false).show_rows(ui, row_height, view.rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let first = range.start;
        // The block whose checkbox the pointer is on, found when its first row is drawn.
        let mut hovered_block = None;
        for (offset, row) in view.rows.get(range).unwrap_or(&[]).iter().enumerate() {
            let (text, color, background, numbers) = match row {
                DiffRow::Hunk { header, .. } => (header.as_str(), colors.weak, Some(colors.hunk), None),
                DiffRow::Line { kind, old, new, text, .. } => {
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
            if let (Some(ticks), DiffRow::Line { block: Some(block_index), .. }) = (ticks, row)
                && let Some(block) = view.blocks.get(*block_index)
            {
                // One checkbox per block, on its first line, in a column of its own. Hovering it
                // highlights the block's rows (drawn after the first row, so they know).
                let row_index = first + offset;
                if row_index == block.first_row {
                    let box_rect =
                        egui::Rect::from_center_size(egui::pos2(rect.left() + block_width / 2.0, y), egui::vec2(row_height - 4.0, row_height - 4.0));
                    let id = ui.scope_id().with(("diff-block", row_index));
                    let sense = if ticks.enabled { egui::Sense::click() } else { egui::Sense::hover() };
                    let response = ui.interact(box_rect.expand(2.0), id, sense);
                    let response = if ticks.enabled {
                        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Tick or untick this whole block of changes")
                    } else {
                        response.on_hover_text(ticks.disabled_reason)
                    };
                    if ticks.enabled && response.hovered() {
                        hovered_block = Some(*block_index);
                    }
                    let state = lines_state(block.lines.iter().copied(), ticks.is_ticked);
                    paint_box(ui, box_rect, state, ticks.enabled, response.hovered());
                    if response.clicked() {
                        clicked = Some(TickClick { target: TickTarget::Block(*block_index), shift: false });
                    }
                }
                if hovered_block == Some(*block_index) {
                    ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.5));
                }
            }
            if let Some(ticks) = ticks {
                let target = match row {
                    DiffRow::Hunk { index, .. } => Some((TickTarget::Hunk(*index), hunk_state(&view.diff, *index, ticks.is_ticked))),
                    DiffRow::Line { kind: LineKind::Added | LineKind::Removed, at, .. } => {
                        let state = if (ticks.is_ticked)(*at) { BoxState::Ticked } else { BoxState::Unticked };
                        Some((TickTarget::Line(*at), state))
                    }
                    DiffRow::Line { .. } => None,
                };
                if let Some((target, state)) = target {
                    let box_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + block_width + box_width / 2.0, y),
                        egui::vec2(row_height - 4.0, row_height - 4.0),
                    );
                    let id = ui.scope_id().with(("diff-tick", first + offset));
                    let sense = if ticks.enabled { egui::Sense::click() } else { egui::Sense::hover() };
                    let response = ui.interact(box_rect.expand(2.0), id, sense);
                    let response = if ticks.enabled {
                        response.on_hover_cursor(egui::CursorIcon::PointingHand)
                    } else {
                        response.on_hover_text(ticks.disabled_reason)
                    };
                    paint_box(ui, box_rect, state, ticks.enabled, response.hovered());
                    if response.clicked() {
                        clicked = Some(TickClick { target, shift: ui.input(|input| input.modifiers.shift) });
                    }
                }
            }
            if let Some((kind, old, new)) = numbers {
                let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                let right_of = |columns: f32| rect.left() + tick_width + columns;
                let painter = ui.painter();
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
            let text_left = if numbers.is_some() { rect.left() + gutter } else { rect.left() + tick_width + char_width };
            painter.galley(egui::pos2(text_left, y - galley.size().y / 2.0), galley, color);
        }
    });
    clicked
}

/// Draws a checkbox: a rounded square, with a tick when ticked or a bar when part-filled. (Drawn by
/// hand because the rows are painted, not laid out as widgets.)
fn paint_box(ui: &egui::Ui, rect: egui::Rect, state: BoxState, enabled: bool, hovered: bool) {
    let visuals = ui.visuals();
    let widget = if !enabled {
        &visuals.widgets.noninteractive
    } else if hovered {
        &visuals.widgets.hovered
    } else {
        &visuals.widgets.inactive
    };
    let painter = ui.painter();
    painter.rect(rect, 2.0, widget.bg_fill, widget.bg_stroke, egui::StrokeKind::Inside);
    let mark = egui::Stroke::new(2.0, if enabled { widget.fg_stroke.color } else { visuals.weak_text_color() });
    match state {
        BoxState::Ticked => {
            let r = rect.shrink(rect.width() * 0.2);
            let points =
                vec![egui::pos2(r.left(), r.center().y), egui::pos2(r.center().x - r.width() * 0.1, r.bottom()), egui::pos2(r.right(), r.top())];
            painter.add(egui::Shape::line(points, mark));
        }
        BoxState::Mixed => {
            let r = rect.shrink(rect.width() * 0.25);
            painter.line_segment([egui::pos2(r.left(), r.center().y), egui::pos2(r.right(), r.center().y)], mark);
        }
        BoxState::Unticked => {}
    }
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
        assert_eq!(view.rows[0], DiffRow::Hunk { header: "@@ -98,3 +98,3 @@".into(), index: 0 });
        assert_eq!(
            view.rows[1],
            DiffRow::Line {
                kind: LineKind::Context,
                old: Some(98),
                new: Some(98),
                text: "    fn a() {".into(),
                at: LineRef { hunk: 0, line: 0 },
                block: None
            },
            "tabs become spaces"
        );
        assert_eq!(view.number_digits, 3, "line 100 needs three digits");
    }

    #[test]
    fn hunk_boxes_sum_up_their_lines() {
        let diff = sample();
        assert_eq!(hunk_state(&diff, 0, &|_| true), BoxState::Ticked);
        assert_eq!(hunk_state(&diff, 0, &|_| false), BoxState::Unticked);
        assert_eq!(hunk_state(&diff, 0, &|at| at.line == 1), BoxState::Mixed);
        assert_eq!(hunk_state(&diff, 7, &|_| true), BoxState::Unticked, "a missing hunk is just empty");
    }

    /// Clicks at `pos` (press and release over separate frames) and returns what `show` reported.
    fn click(state: &DiffState, ticks: &Ticks<'_>, pos: egui::Pos2, shift: bool) -> Option<TickClick> {
        let ctx = egui::Context::default();
        let modifiers = egui::Modifiers { shift, ..egui::Modifiers::NONE };
        let frames = [
            vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers }],
            vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers }],
        ];
        let mut result = None;
        for events in frames {
            let input = egui::RawInput { events, ..egui::RawInput::default() };
            let mut output = ctx.run_ui(input, |ui| {
                if let Some(click) = show(ui, state, "key", Some(ticks)) {
                    result = Some(click);
                }
            });
            output.textures_delta.clear();
        }
        result
    }

    /// The height of a diff row.
    fn row_height() -> f32 {
        let ctx = egui::Context::default();
        let mut height = 0.0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0;
        });
        output.textures_delta.clear();
        height
    }

    /// The centre of the line checkbox on row `row` (rows start at the top of the screen). The block
    /// column, as wide as a checkbox column, comes first.
    fn box_center(row: usize) -> egui::Pos2 {
        let height = row_height();
        let column = height + 4.0;
        egui::pos2(column + column / 2.0, height * (row as f32 + 0.5))
    }

    /// The centre of the block checkbox column on row `row`.
    fn block_box_center(row: usize) -> egui::Pos2 {
        let height = row_height();
        egui::pos2((height + 4.0) / 2.0, height * (row as f32 + 0.5))
    }

    /// Two blocks in one hunk: "-a +A", then context, then "+b" with a no-newline marker.
    fn two_blocks() -> FileDiff {
        use LineKind::*;
        FileDiff {
            hunks: vec![Hunk {
                header: "@@ -1,2 +1,3 @@".into(),
                lines: vec![
                    line(Removed, Some(1), None, "a"),
                    line(Added, None, Some(1), "A"),
                    line(Context, Some(2), Some(2), "keep"),
                    line(Added, None, Some(3), "b"),
                    line(NoNewline, None, None, "No newline at end of file"),
                ],
            }],
            ..FileDiff::default()
        }
    }

    #[test]
    fn blocks_are_runs_of_changes_between_unchanged_lines() {
        let view = DiffView::new(two_blocks());
        let at = |line| LineRef { hunk: 0, line };
        assert_eq!(
            view.blocks,
            [Block { first_row: 1, last_row: 2, lines: vec![at(0), at(1)] }, Block { first_row: 4, last_row: 5, lines: vec![at(3)] }]
        );
        let blocks: Vec<_> = view.rows.iter().map(|row| if let DiffRow::Line { block, .. } = row { *block } else { None }).collect();
        assert_eq!(blocks, [None, Some(0), Some(0), None, Some(1), Some(1)], "the marker belongs to its line's block");
        assert!(DiffView::new(sample()).blocks.len() == 1);
    }

    #[test]
    fn clicking_a_block_box_reports_its_block() {
        let state = DiffState::Loaded(Box::new(DiffView::new(two_blocks())));
        let is_ticked = |_: LineRef| true;
        let ticks = Ticks { enabled: true, disabled_reason: "", is_ticked: &is_ticked };
        // Row 1 is the first line of the first block, row 4 of the second.
        assert_eq!(click(&state, &ticks, block_box_center(1), false), Some(TickClick { target: TickTarget::Block(0), shift: false }));
        assert_eq!(click(&state, &ticks, block_box_center(4), false), Some(TickClick { target: TickTarget::Block(1), shift: false }));
        assert_eq!(click(&state, &ticks, block_box_center(2), false), None, "only the block's first line has the box");
        assert_eq!(click(&state, &ticks, block_box_center(3), false), None, "no box beside an unchanged line");
        let disabled = Ticks { enabled: false, disabled_reason: "why", is_ticked: &is_ticked };
        assert_eq!(click(&state, &disabled, block_box_center(1), false), None);
    }

    #[test]
    fn clicking_boxes_reports_the_line_or_hunk() {
        let state = DiffState::Loaded(Box::new(DiffView::new(sample())));
        let is_ticked = |_: LineRef| true;
        let ticks = Ticks { enabled: true, disabled_reason: "", is_ticked: &is_ticked };
        // Row 0 is the hunk header, row 1 context (no box), row 2 the removed line.
        assert_eq!(click(&state, &ticks, box_center(0), false), Some(TickClick { target: TickTarget::Hunk(0), shift: false }));
        assert_eq!(click(&state, &ticks, box_center(1), false), None, "context lines have no box");
        assert_eq!(
            click(&state, &ticks, box_center(2), true),
            Some(TickClick { target: TickTarget::Line(LineRef { hunk: 0, line: 1 }), shift: true })
        );

        let disabled = Ticks { enabled: false, disabled_reason: "why", is_ticked: &is_ticked };
        assert_eq!(click(&state, &disabled, box_center(2), false), None, "disabled boxes ignore clicks");
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
        let is_ticked = |at: LineRef| at.line == 1;
        let ticks = Ticks { enabled: true, disabled_reason: "", is_ticked: &is_ticked };
        for state in &states {
            for with_ticks in [None, Some(&ticks)] {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    show(ui, state, "key", with_ticks);
                });
                output.textures_delta.clear();
            }
        }
    }
}
