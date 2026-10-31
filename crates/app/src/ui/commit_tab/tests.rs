use super::*;
use gitcraft_git::{Change, EntryKind};

fn entry(path: &str, kind: EntryKind, staged: Change, unstaged: Change) -> StatusEntry {
    StatusEntry { path: path.into(), old_path: None, kind, staged, unstaged, submodule: false }
}

fn status(entries: Vec<StatusEntry>) -> Status {
    Status { entries, truncated: false, branch: Some("main".into()), unborn: false }
}

fn unstaged(path: &str) -> StatusEntry {
    entry(path, EntryKind::Tracked, Change::Unmodified, Change::Modified)
}

fn sample() -> Changes {
    let mut changes = Changes::default();
    changes.set_status(status(vec![
        unstaged("a.txt"),
        entry("b.txt", EntryKind::Untracked, Change::Unmodified, Change::Added),
        entry("c.txt", EntryKind::Unmerged, Change::Unmerged, Change::Unmerged),
    ]));
    changes.selected = Some("a.txt".into());
    changes
}

/// A request that must be possible.
fn request(changes: &Changes) -> CommitRequest {
    changes.commit_request().expect("not blocked").expect("patches build")
}

#[test]
fn counts_files() {
    assert_eq!(count_label(1), "1 changed file");
    assert_eq!(count_label(3), "3 changed files");
}

#[test]
fn everything_starts_ticked_when_nothing_is_staged() {
    let changes = sample();
    assert!(["a.txt", "b.txt", "c.txt"].iter().all(|path| changes.tick(path) == Tick::All));
}

#[test]
fn new_changes_start_ticked_whatever_is_staged() {
    let mut changes = Changes::default();
    changes.set_status(status(vec![
        entry("fully.txt", EntryKind::Tracked, Change::Modified, Change::Unmodified),
        entry("partly.txt", EntryKind::Tracked, Change::Modified, Change::Modified),
        unstaged("unstaged.txt"),
        entry("new.txt", EntryKind::Untracked, Change::Unmodified, Change::Added),
    ]));
    for path in ["fully.txt", "partly.txt", "unstaged.txt", "new.txt"] {
        assert_eq!(changes.tick(path), Tick::All, "{path}");
    }
}

#[test]
fn refreshes_keep_choices_and_forget_files_that_are_gone() {
    let mut changes = sample();
    changes.ticks.insert("a.txt".into(), Tick::Nothing);
    changes.set_status(status(vec![unstaged("a.txt"), unstaged("d.txt")]));
    assert_eq!(changes.tick("a.txt"), Tick::Nothing, "kept");
    assert_eq!(changes.tick("d.txt"), Tick::All, "a new file starts ticked");
    assert_eq!(changes.ticks.len(), 2, "b and c are gone");
}

#[test]
fn commit_requests_follow_the_ticks() {
    let mut changes = sample();
    assert_eq!(changes.blocker(), Some("Write a summary first."));
    assert_eq!(changes.commit_request(), None);

    changes.summary = "  Fix login  ".into();
    changes.description = "Long story.\n\n".into();
    changes.ticks.insert("b.txt".into(), Tick::Nothing);
    let request = request(&changes);
    assert_eq!(request.include, ["a.txt", "c.txt"]);
    assert_eq!(request.exclude, ["b.txt"]);
    assert!(request.partial.is_empty());
    assert_eq!(request.message, "Fix login\n\nLong story.");
    assert!(!request.amend);

    changes.set_all(Tick::Nothing);
    assert_eq!(changes.blocker(), Some("Tick at least one file to commit."));
    changes.amend = true;
    assert_eq!(changes.blocker(), Some("Reading the last commit…"));
    changes.amend_info = Some(AmendInfo { message: "Old".into(), pushed_to: None });
    assert_eq!(changes.blocker(), None, "an amend can change only the message");
    changes.committing = true;
    assert_eq!(changes.blocker(), Some("Committing…"));
}

#[test]
fn a_partly_staged_file_is_committed_whole_when_ticked() {
    let mut changes = Changes::default();
    changes.set_status(status(vec![entry("partly.txt", EntryKind::Tracked, Change::Modified, Change::Modified)]));
    changes.summary = "All of it".into();
    let request = request(&changes);
    assert_eq!(request.include, ["partly.txt"], "the ticks decide, not the index");
}

#[test]
fn amend_starts_from_the_last_message_unless_one_is_written() {
    let info = AmendInfo { message: "Old summary\n\nOld body\nline 2".into(), pushed_to: None };
    let mut changes = sample();
    changes.amend = true;
    changes.take_amend_info(info.clone());
    assert_eq!((changes.summary.as_str(), changes.description.as_str()), ("Old summary", "Old body\nline 2"));

    let mut written = sample();
    written.amend = true;
    written.summary = "Mine".into();
    written.take_amend_info(info);
    assert_eq!(written.summary, "Mine");
}

#[test]
fn a_finished_commit_clears_the_box() {
    let mut changes = sample();
    changes.summary = "x".into();
    changes.description = "y".into();
    changes.amend = true;
    changes.committing = true;
    changes.commit_error = Some("old".into());
    changes.committed();
    assert!(changes.summary.is_empty() && changes.description.is_empty() && !changes.amend && !changes.committing);
    assert_eq!(changes.commit_error, None);
}

#[test]
fn splits_messages() {
    assert_eq!(split_message("One line"), ("One line".into(), String::new()));
    assert_eq!(split_message("Sum\n\nBody\n"), ("Sum".into(), "Body".into()));
    assert_eq!(split_message("Sum\r\n\r\nBody"), ("Sum".into(), "Body".into()));
}

#[test]
fn button_names_the_branch() {
    let mut s = status(vec![]);
    assert_eq!(commit_label(&s, false), "Commit to main");
    assert_eq!(commit_label(&s, true), "Amend last commit");
    s.branch = None;
    assert_eq!(commit_label(&s, false), "Commit (detached HEAD)");
}

// --- Picking lines ---

/// `a.txt`'s diff: two hunks, each replacing one line.
const PATCH: &str = "diff --git a/a.txt b/a.txt
index 1111111..2222222 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
@@ -10,3 +10,3 @@
 ten
-eleven
+ELEVEN
 twelve
";

/// `sample()` with a.txt selected and its diff loaded.
fn with_diff(patch: &str) -> Changes {
    let mut changes = sample();
    changes.diff_loaded("a.txt", FileDiff::parse(patch.as_bytes()));
    changes
}

fn line(hunk: usize, line: usize) -> LineRef {
    LineRef { hunk, line }
}

fn click_line(changes: &mut Changes, hunk: usize, at: usize, shift: bool) {
    changes.click_tick(TickClick { target: TickTarget::Line(line(hunk, at)), shift });
}

#[test]
fn unticking_a_line_makes_the_file_partial() {
    let mut changes = with_diff(PATCH);
    click_line(&mut changes, 0, 2, false); // "+TWO"
    assert_eq!(changes.tick("a.txt"), Tick::Lines);
    assert!(!changes.line_ticked("a.txt", line(0, 2)));
    assert!(changes.line_ticked("a.txt", line(0, 1)), "the removal of \"two\" is still ticked");

    // Ticking it again: every line is ticked, so the file is simply ticked.
    click_line(&mut changes, 0, 2, false);
    assert_eq!(changes.tick("a.txt"), Tick::All);
    assert!(changes.partial.is_empty());
}

#[test]
fn hunk_boxes_tick_and_untick_the_whole_hunk() {
    let mut changes = with_diff(PATCH);
    changes.click_tick(TickClick { target: TickTarget::Hunk(1), shift: false });
    assert_eq!(changes.tick("a.txt"), Tick::Lines);
    assert!(!changes.line_ticked("a.txt", line(1, 1)) && !changes.line_ticked("a.txt", line(1, 2)));
    assert!(changes.line_ticked("a.txt", line(0, 1)));
    // A part-ticked hunk becomes fully ticked.
    click_line(&mut changes, 1, 1, false);
    changes.click_tick(TickClick { target: TickTarget::Hunk(1), shift: false });
    assert_eq!(changes.tick("a.txt"), Tick::All);
}

/// One hunk with two blocks: "two" → "TWO", then "four" → "FOUR", "FOUR2".
const TWO_BLOCKS: &str = "diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,5 +1,6 @@
 one
-two
+TWO
 three
-four
+FOUR
+FOUR2
 five
";

#[test]
fn block_handles_tick_and_untick_one_block() {
    let mut changes = with_diff(TWO_BLOCKS);
    changes.click_tick(TickClick { target: TickTarget::Block(1), shift: false });
    assert_eq!(changes.tick("a.txt"), Tick::Lines);
    for at in [line(0, 4), line(0, 5), line(0, 6)] {
        assert!(!changes.line_ticked("a.txt", at), "{at:?} is in the second block");
    }
    assert!(changes.line_ticked("a.txt", line(0, 1)) && changes.line_ticked("a.txt", line(0, 2)), "the first block is untouched");
    // Part-ticked block: one click ticks it fully again.
    click_line(&mut changes, 0, 5, false);
    changes.click_tick(TickClick { target: TickTarget::Block(1), shift: false });
    assert_eq!(changes.tick("a.txt"), Tick::All);
    // A block that doesn't exist changes nothing.
    changes.click_tick(TickClick { target: TickTarget::Block(9), shift: false });
    assert_eq!(changes.tick("a.txt"), Tick::All);
}

#[test]
fn shift_click_sets_a_range() {
    let mut changes = with_diff(PATCH);
    click_line(&mut changes, 0, 1, false); // untick "-two"; the range starts here
    click_line(&mut changes, 1, 1, true); // untick through "-eleven"
    for at in [line(0, 1), line(0, 2), line(1, 1)] {
        assert!(!changes.line_ticked("a.txt", at), "{at:?}");
    }
    assert!(changes.line_ticked("a.txt", line(1, 2)), "past the range");
}

#[test]
fn ticking_a_line_of_an_unticked_file_ticks_only_that_line() {
    let mut changes = with_diff(PATCH);
    changes.set_tick("a.txt", Tick::Nothing);
    click_line(&mut changes, 1, 2, false);
    assert_eq!(changes.tick("a.txt"), Tick::Lines);
    assert!(changes.line_ticked("a.txt", line(1, 2)));
    assert!(!changes.line_ticked("a.txt", line(0, 1)));
}

#[test]
fn the_file_box_clears_picked_lines() {
    let mut changes = with_diff(PATCH);
    click_line(&mut changes, 0, 2, false);
    changes.set_tick("a.txt", Tick::All);
    assert!(changes.partial.is_empty());
    assert!(changes.line_ticked("a.txt", line(0, 2)));
}

#[test]
fn line_boxes_are_blocked_while_committing_and_for_cut_short_diffs() {
    let mut busy = with_diff(PATCH);
    busy.committing = true;
    click_line(&mut busy, 0, 2, false);
    assert_eq!(busy.tick("a.txt"), Tick::All);

    let mut cut = sample();
    // A cut-short diff, as the parser gives it: no source to build patches from.
    cut.diff_loaded("a.txt", FileDiff { truncated: true, source: None, ..FileDiff::parse(PATCH.as_bytes()) });
    let DiffState::Loaded(view) = &cut.diff else { panic!("loaded") };
    assert!(cut.line_ticks_blocker(&view.diff).is_some());
}

#[test]
fn picked_lines_go_into_the_commit_as_a_patch() {
    let mut changes = with_diff(PATCH);
    changes.summary = "Part".into();
    changes.click_tick(TickClick { target: TickTarget::Hunk(1), shift: false }); // leave out "eleven"
    let request = request(&changes);
    assert_eq!(request.include, ["b.txt", "c.txt"]);
    assert_eq!(request.partial.len(), 1);
    let patch = String::from_utf8(request.partial[0].patch.clone()).unwrap();
    assert!(patch.contains("+TWO") && !patch.contains("ELEVEN"), "{patch}");
}

#[test]
fn a_patch_that_cant_be_built_is_an_error_not_a_crash() {
    // The old last line has no newline: ticking only the new line after it can't be a patch.
    let patch = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,2 @@\n-last\n\\ No newline at end of file\n+last\n+more\n";
    let mut changes = with_diff(patch);
    changes.summary = "x".into();
    changes.set_tick("a.txt", Tick::Nothing);
    click_line(&mut changes, 0, 3, false); // only "+more"
    let error = changes.commit_request().expect("not blocked").expect_err("refused");
    assert!(error.starts_with("a.txt: ") && error.contains("ticked together"), "{error}");
}

#[test]
fn a_refreshed_diff_keeps_unticked_lines() {
    let mut changes = with_diff(PATCH);
    click_line(&mut changes, 1, 2, false); // leave out "+ELEVEN"
    // The file is edited near the top (a new line after "one") and read again.
    let edited = PATCH.replace("@@ -1,3 +1,3 @@\n one\n", "@@ -1,3 +1,4 @@\n one\n+new top\n").replace("+10,3", "+11,3");
    changes.diff_loaded("a.txt", FileDiff::parse(edited.as_bytes()));
    assert_eq!(changes.tick("a.txt"), Tick::Lines);
    // "+ELEVEN" is still line 2 of the second hunk, and still unticked; the new line is ticked.
    assert!(!changes.line_ticked("a.txt", line(1, 2)));
    assert!(changes.line_ticked("a.txt", line(0, 1)));
}

#[test]
fn a_diff_that_can_no_longer_be_split_leaves_the_file_out() {
    let mut changes = with_diff(PATCH);
    click_line(&mut changes, 1, 2, false);
    changes.diff_loaded("a.txt", FileDiff { binary: true, ..FileDiff::default() });
    assert_eq!(changes.tick("a.txt"), Tick::Nothing);
    assert!(changes.partial.is_empty());
}

#[test]
fn draws_every_state() {
    let failed = Changes { error: Some("nope".into()), ..Changes::default() };
    let mut clean = Changes::default();
    clean.set_status(status(vec![]));
    let mut busy = sample();
    busy.committing = true;
    busy.commit_error = Some("hook failed".into());
    let mut partial = with_diff(PATCH);
    click_line(&mut partial, 0, 2, false);
    let mut states = [Changes::default(), failed, clean, sample(), busy, with_diff(PATCH), partial];
    let ctx = egui::Context::default();
    let mut ratio = split::DEFAULT_CHANGES_RATIO;
    for changes in &mut states {
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(ui, changes, &mut ratio, &[PathBuf::from("editor")]);
            });
            output.textures_delta.clear();
        }
    }
}
