//! Building a patch of chosen lines: for committing part of a file (`git apply --cached`), or for
//! discarding part of it (`git apply` on the working tree).
//!
//! The diff is HEAD → working tree. The patch applies to HEAD's version of the file and takes only
//! the chosen changes, the same way GitHub Desktop does it:
//! - a chosen added line is kept, an unchosen one is left out;
//! - a chosen removed line is kept, an unchosen one becomes a context line (it stays in the file);
//! - context lines are kept, hunks with nothing chosen are left out, and every hunk header is
//!   recalculated.
//!
//! Lines are copied from git's exact bytes, so line endings and odd encodings survive untouched.

use super::parse::raw_line;
use super::{FileDiff, LineKind, PatchSource};
use crate::error::{GitError, Result};

/// One line of a diff: the index of its hunk in [`FileDiff::hunks`] and of the line in that hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineRef {
    pub hunk: usize,
    pub line: usize,
}

/// What a diff line became in the patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kept {
    Context,
    Removed,
    Added,
}

/// Which sides of the file have already ended (a "no newline at end of file" line was passed).
#[derive(Default)]
struct Ended {
    old: bool,
    new: bool,
}

/// Which way a patch goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// HEAD's version → HEAD's version plus the chosen changes (for `git apply --cached`).
    Commit,
    /// The working file → the working file without the chosen changes (for `git apply` on the
    /// working tree).
    Discard,
}

impl FileDiff {
    /// A patch with only the added and removed lines `selected` says yes to (it is asked only about
    /// those). Returns `None` when nothing is chosen.
    ///
    /// Fails if lines can't be picked in this diff (see [`FileDiff::can_pick_lines`]), or if the
    /// choice can't be written as a valid patch: when a file's last line has no newline, its
    /// removed and added versions have to be chosen together.
    pub fn partial_patch(&self, selected: impl Fn(LineRef) -> bool) -> Result<Option<Vec<u8>>> {
        let source = self.pickable()?;
        let Some((hunks, keeps_lines)) = self.build_hunks(source, &selected, Direction::Commit)? else {
            return Ok(None);
        };
        let mut patch = file_header(source, keeps_lines)?;
        patch.extend_from_slice(&hunks);
        Ok(Some(patch))
    }

    /// A patch that takes the chosen changes back out of the working file, for `git apply` on the
    /// working tree: a chosen added line is removed, a chosen removed line is put back, and every
    /// other change stays as it is. Returns `None` when nothing is chosen.
    ///
    /// Fails like [`FileDiff::partial_patch`], and for a deleted file (there is no file on disk to
    /// edit: discard the whole file instead).
    pub fn discard_patch(&self, selected: impl Fn(LineRef) -> bool) -> Result<Option<Vec<u8>>> {
        let source = self.pickable()?;
        let Some((hunks, _)) = self.build_hunks(source, &selected, Direction::Discard)? else {
            return Ok(None);
        };
        let mut patch = working_file_header(source)?;
        patch.extend_from_slice(&hunks);
        Ok(Some(patch))
    }

    fn pickable(&self) -> Result<&PatchSource> {
        self.source.as_ref().ok_or_else(|| {
            GitError::Unsupported("Single lines can't be picked in this file's changes (a binary file, or too many changes to show).".into())
        })
    }

    /// The hunks of a patch in `direction`, and whether the resulting file keeps any lines; `None`
    /// when no line is chosen. "Old" and "new" below are the patch's own sides: for a commit HEAD's
    /// version and the result, for a discard the working file and the result.
    fn build_hunks(&self, source: &PatchSource, selected: &dyn Fn(LineRef) -> bool, direction: Direction) -> Result<Option<(Vec<u8>, bool)>> {
        let mut hunks_out = Vec::new();
        // How many more lines the new file has than the old one, before the current hunk.
        let mut delta: i64 = 0;
        // Whether the resulting file keeps any lines (decides whether a deletion stays one).
        let mut keeps_lines = false;

        for (hunk_index, (hunk, source_hunk)) in self.hunks.iter().zip(&source.hunks).enumerate() {
            let mut body = Vec::new();
            let (mut old_count, mut new_count) = (0u32, 0u32);
            let mut changed = false;
            // What happened to the line just before, for a "no newline" marker that follows it.
            let mut previous: Option<Kept> = None;
            let mut ended = Ended::default();

            for (line_index, (line, range)) in hunk.lines.iter().zip(&source_hunk.lines).enumerate() {
                let raw = raw_line(&source.bytes, range);
                if line.kind == LineKind::NoNewline {
                    // The marker belongs to the line before; it goes wherever that line went.
                    if let Some(kept) = previous {
                        push_line(&mut body, raw);
                        match kept {
                            Kept::Context => (ended.old, ended.new) = (true, true),
                            Kept::Removed => ended.old = true,
                            Kept::Added => ended.new = true,
                        }
                    }
                    continue;
                }
                let chosen = matches!(line.kind, LineKind::Added | LineKind::Removed) && selected(LineRef { hunk: hunk_index, line: line_index });
                let kept = match (direction, line.kind, chosen) {
                    (_, LineKind::Context | LineKind::NoNewline, _) => Some(Kept::Context),
                    // Committing works on HEAD's version, where removed lines still are.
                    (Direction::Commit, LineKind::Removed, true) => Some(Kept::Removed),
                    (Direction::Commit, LineKind::Removed, false) => Some(Kept::Context), // stays in the file
                    (Direction::Commit, LineKind::Added, true) => Some(Kept::Added),
                    (Direction::Commit, LineKind::Added, false) => None, // left out
                    // Discarding works on the working file, where added lines are and removed
                    // lines aren't.
                    (Direction::Discard, LineKind::Added, true) => Some(Kept::Removed), // taken out
                    (Direction::Discard, LineKind::Added, false) => Some(Kept::Context), // stays
                    (Direction::Discard, LineKind::Removed, true) => Some(Kept::Added), // put back
                    (Direction::Discard, LineKind::Removed, false) => None,             // stays gone
                };
                previous = kept;
                let Some(kept) = kept else { continue };

                let side_ended = match kept {
                    Kept::Context => ended.old || ended.new,
                    Kept::Removed => ended.old,
                    Kept::Added => ended.new,
                };
                if side_ended {
                    let verb = match direction {
                        Direction::Commit => "ticked",
                        Direction::Discard => "discarded",
                    };
                    return Err(GitError::Unsupported(format!(
                        "The file's last line has no newline at the end, so its removed and added versions must be {verb} together."
                    )));
                }
                // The line's text with the marker of what it became (' ', '-' or '+').
                let marker = match kept {
                    Kept::Context => {
                        old_count += 1;
                        new_count += 1;
                        b' '
                    }
                    Kept::Removed => {
                        old_count += 1;
                        changed = true;
                        b'-'
                    }
                    Kept::Added => {
                        new_count += 1;
                        changed = true;
                        b'+'
                    }
                };
                body.push(marker);
                push_line(&mut body, raw.get(1..).unwrap_or_default());
            }

            keeps_lines |= new_count > 0;
            if !changed {
                continue;
            }
            // Where the hunk starts in the new file. A count of 0 means the start is the line
            // *before* the (empty) range, as in "@@ -5,0 +6,2 @@".
            let old_start = i64::from(match direction {
                Direction::Commit => source_hunk.header.old_start,
                // The working file is the diff's new side.
                Direction::Discard => source_hunk.header.new_start,
            });
            let first_old = if old_count == 0 { old_start + 1 } else { old_start };
            let first_new = first_old + delta;
            let new_start = if new_count == 0 { first_new - 1 } else { first_new }.max(0);
            hunks_out.extend_from_slice(format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@\n").as_bytes());
            hunks_out.extend_from_slice(&body);
            delta += i64::from(new_count) - i64::from(old_count);
        }

        Ok((!hunks_out.is_empty()).then_some((hunks_out, keeps_lines)))
    }
}

/// The header of a patch on the working file: the file's name (from the diff's `+++` line) on both
/// sides, with no mode lines, so it reads as an ordinary edit even for a new, untracked file.
fn working_file_header(source: &PatchSource) -> Result<Vec<u8>> {
    let header = source.bytes.get(source.header.clone()).unwrap_or_default();
    let new_name = header
        .split_inclusive(|&byte| byte == b'\n')
        .find_map(|line| line.strip_prefix(b"+++ "))
        .map(|name| name.strip_suffix(b"\n").unwrap_or(name))
        .map(|name| name.strip_suffix(b"\r").unwrap_or(name))
        .unwrap_or_default();
    if new_name == b"/dev/null" {
        return Err(GitError::Unsupported("This file was deleted, so there are no lines on disk to discard. Discard the whole file instead.".into()));
    }
    let old_name = if let Some(rest) = new_name.strip_prefix(b"b/") {
        [b"a/".as_slice(), rest].concat()
    } else if let Some(rest) = new_name.strip_prefix(b"\"b/") {
        [b"\"a/".as_slice(), rest].concat()
    } else {
        return Err(GitError::Malformed { what: "diff header".into(), detail: "the file's name has an unexpected form".into() });
    };
    let mut out = Vec::new();
    for part in [b"diff --git ".as_slice(), &old_name, b" ", new_name, b"\n--- ", &old_name, b"\n+++ ", new_name, b"\n"] {
        out.extend_from_slice(part);
    }
    Ok(out)
}

/// Appends a line, making sure it ends with a newline (git's output always does, but the last line
/// of a cut-off buffer might not).
fn push_line(out: &mut Vec<u8>, line: &[u8]) {
    out.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        out.push(b'\n');
    }
}

/// The patch's file header. A deleted file that keeps some of its lines is no longer a deletion,
/// so its header becomes an ordinary edit's: no "deleted file" line, and `+++` names the file.
fn file_header(source: &PatchSource, keeps_lines: bool) -> Result<Vec<u8>> {
    let header = source.bytes.get(source.header.clone()).unwrap_or_default();
    let lines: Vec<&[u8]> = header.split_inclusive(|&byte| byte == b'\n').collect();
    let is_deletion = lines.iter().any(|line| line.starts_with(b"deleted file mode "));
    if !is_deletion || !keeps_lines {
        return Ok(header.to_vec());
    }

    let mut out = Vec::new();
    for line in &lines {
        if line.starts_with(b"deleted file mode ") || line.starts_with(b"index ") {
            // The index line names the deleted state (an all-zero id); git apply doesn't need it.
            continue;
        }
        if line.starts_with(b"+++ ") {
            out.extend_from_slice(&new_side_name(&lines)?);
        } else {
            out.extend_from_slice(line);
        }
    }
    Ok(out)
}

/// `+++ b/path` built from the `--- a/path` line (git quotes unusual names: `--- "a/x\ty"`).
fn new_side_name(lines: &[&[u8]]) -> Result<Vec<u8>> {
    let old = lines.iter().find_map(|line| line.strip_prefix(b"--- ")).unwrap_or_default();
    let mut out = b"+++ ".to_vec();
    if let Some(rest) = old.strip_prefix(b"a/") {
        out.extend_from_slice(b"b/");
        out.extend_from_slice(rest);
    } else if let Some(rest) = old.strip_prefix(b"\"a/") {
        out.extend_from_slice(b"\"b/");
        out.extend_from_slice(rest);
    } else {
        return Err(GitError::Malformed { what: "diff header".into(), detail: "the old file's name has an unexpected form".into() });
    }
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::parse_patch;
    use super::*;

    const HEADER: &str = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n";

    fn diff(body: &str) -> FileDiff {
        let diff = parse_patch(format!("{HEADER}{body}").as_bytes(), 1000);
        assert!(diff.can_pick_lines(), "test diff must be well formed");
        diff
    }

    /// The patch's text after the file header.
    fn hunks(patch: Option<Vec<u8>>) -> String {
        let patch = String::from_utf8(patch.expect("a patch")).unwrap();
        patch.strip_prefix(HEADER).unwrap_or(&patch).to_string()
    }

    fn only(lines: &'static [(usize, usize)]) -> impl Fn(LineRef) -> bool {
        move |at| lines.contains(&(at.hunk, at.line))
    }

    const TWO_HUNKS: &str = "@@ -1,4 +1,5 @@
 a
-b
+B
+B2
 c
 d
@@ -10,3 +11,3 @@
 x
-y
+Y
 z
";

    #[test]
    fn everything_chosen_gives_the_same_hunks() {
        let patch = diff(TWO_HUNKS).partial_patch(|_| true).unwrap();
        assert_eq!(hunks(patch), TWO_HUNKS);
    }

    #[test]
    fn nothing_chosen_gives_no_patch() {
        assert_eq!(diff(TWO_HUNKS).partial_patch(|_| false).unwrap(), None);
    }

    #[test]
    fn unchosen_removals_become_context_and_unchosen_additions_go() {
        // Only "+B" in the first hunk: "b" stays, "B2" is left out, the second hunk is dropped.
        let patch = diff(TWO_HUNKS).partial_patch(only(&[(0, 2)])).unwrap();
        assert_eq!(hunks(patch), "@@ -1,4 +1,5 @@\n a\n b\n+B\n c\n d\n");
    }

    #[test]
    fn later_hunks_move_by_what_earlier_hunks_changed() {
        // First hunk: remove "b" only (one line shorter). Second hunk: everything.
        let patch = diff(TWO_HUNKS).partial_patch(only(&[(0, 1), (1, 1), (1, 2)])).unwrap();
        assert_eq!(hunks(patch), "@@ -1,4 +1,3 @@\n a\n-b\n c\n d\n@@ -10,3 +9,3 @@\n x\n-y\n+Y\n z\n");
    }

    #[test]
    fn a_dropped_hunk_does_not_move_later_ones() {
        let patch = diff(TWO_HUNKS).partial_patch(only(&[(1, 1), (1, 2)])).unwrap();
        assert_eq!(hunks(patch), "@@ -10,3 +10,3 @@\n x\n-y\n+Y\n z\n");
    }

    #[test]
    fn crlf_and_odd_bytes_are_copied_exactly() {
        let body = b"@@ -1,2 +1,2 @@\r\n a\r\n-\xff\r\n+new\r\n";
        let diff = parse_patch(&[HEADER.as_bytes(), body].concat(), 100);
        let patch = diff.partial_patch(|at| at.line == 2).unwrap().unwrap();
        assert!(patch.ends_with(b"@@ -1,2 +1,3 @@\n a\r\n \xff\r\n+new\r\n"), "{:?}", String::from_utf8_lossy(&patch));
    }

    #[test]
    fn pure_insertions_and_deletions_get_the_right_starts() {
        let body = "@@ -5,0 +6,2 @@\n+one\n+two\n@@ -20,2 +21,0 @@\n-gone1\n-gone2\n";
        let diff = diff(body);
        // Only the deletion: the insertion before it is left out, so nothing shifts.
        assert_eq!(hunks(diff.partial_patch(|at| at.hunk == 1).unwrap()), "@@ -20,2 +19,0 @@\n-gone1\n-gone2\n");
        // Only the insertion.
        assert_eq!(hunks(diff.partial_patch(|at| at.hunk == 0 && at.line == 0).unwrap()), "@@ -5,0 +6,1 @@\n+one\n");
    }

    #[test]
    fn a_new_file_keeps_only_the_chosen_lines() {
        let patch = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+one\n+two\n+three\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        let out = String::from_utf8(diff.partial_patch(|at| at.line != 1).unwrap().unwrap()).unwrap();
        assert_eq!(out, "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+three\n");
    }

    #[test]
    fn a_partly_kept_deletion_becomes_an_edit() {
        let patch =
            "diff --git a/d.txt b/d.txt\ndeleted file mode 100644\nindex 3333333..0000000\n--- a/d.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-one\n-two\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        let out = String::from_utf8(diff.partial_patch(|at| at.line == 0).unwrap().unwrap()).unwrap();
        assert_eq!(out, "diff --git a/d.txt b/d.txt\n--- a/d.txt\n+++ b/d.txt\n@@ -1,2 +1,1 @@\n-one\n two\n");
        // Every line chosen: still a deletion.
        let all = String::from_utf8(diff.partial_patch(|_| true).unwrap().unwrap()).unwrap();
        assert!(all.contains("deleted file mode") && all.contains("+++ /dev/null"), "{all}");
    }

    #[test]
    fn quoted_names_keep_their_quotes() {
        let lines: Vec<&[u8]> = vec![b"--- \"a/we\\tird\"\n"];
        assert_eq!(new_side_name(&lines).unwrap(), b"+++ \"b/we\\tird\"\n");
        assert!(new_side_name(&[b"--- /dev/null\n"]).is_err());
    }

    #[test]
    fn no_newline_markers_follow_their_line() {
        // The old last line had no newline; the new file adds a line after it.
        let body = "@@ -1,1 +1,2 @@\n-last\n\\ No newline at end of file\n+last\n+more\n";
        let diff = diff(body);
        // All chosen: the patch is unchanged.
        assert_eq!(hunks(diff.partial_patch(|_| true).unwrap()), body);
        // Only the removal and its replacement (not "more"): fine, the marker comes along.
        assert_eq!(hunks(diff.partial_patch(only(&[(0, 0), (0, 2)])).unwrap()), "@@ -1,1 +1,1 @@\n-last\n\\ No newline at end of file\n+last\n");
        // Only "+more": "last" would stay unterminated with a line after it. Refused.
        let err = diff.partial_patch(only(&[(0, 3)])).unwrap_err();
        assert!(err.to_string().contains("ticked together"), "{err}");
    }

    #[test]
    fn a_marker_after_a_dropped_line_is_dropped() {
        let body = "@@ -1 +1 @@\n-a\n+b\n\\ No newline at end of file\n";
        let out = hunks(diff(body).partial_patch(only(&[(0, 0)])).unwrap());
        assert_eq!(out, "@@ -1,1 +0,0 @@\n-a\n");
    }

    // --- Discarding (a patch on the working file) ---

    /// The discard patch's text after its header, checking the header names the file on both sides.
    fn discard_hunks(patch: Option<Vec<u8>>) -> String {
        let patch = String::from_utf8(patch.expect("a patch")).unwrap();
        let header = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n";
        patch.strip_prefix(header).unwrap_or_else(|| panic!("unexpected header: {patch}")).to_string()
    }

    #[test]
    fn discarding_an_added_line_takes_it_out_and_keeps_the_rest() {
        // "+B" is taken out; "B2" stays (context), and the removal of "b" stays removed.
        let patch = diff(TWO_HUNKS).discard_patch(only(&[(0, 2)])).unwrap();
        assert_eq!(discard_hunks(patch), "@@ -1,5 +1,4 @@\n a\n-B\n B2\n c\n d\n");
    }

    #[test]
    fn discarding_a_removed_line_puts_it_back() {
        let patch = diff(TWO_HUNKS).discard_patch(only(&[(0, 1)])).unwrap();
        assert_eq!(discard_hunks(patch), "@@ -1,5 +1,6 @@\n a\n+b\n B\n B2\n c\n d\n");
    }

    #[test]
    fn discarding_a_later_hunk_uses_working_file_line_numbers() {
        // The second hunk starts at line 11 of the working file (the first hunk added a line).
        let patch = diff(TWO_HUNKS).discard_patch(|at| at.hunk == 1).unwrap();
        assert_eq!(discard_hunks(patch), "@@ -11,3 +11,3 @@\n x\n+y\n-Y\n z\n");
        // Discarding both hunks: the second moves up by what the first took out.
        let both = diff(TWO_HUNKS).discard_patch(|_| true).unwrap();
        assert_eq!(discard_hunks(both), "@@ -1,5 +1,4 @@\n a\n+b\n-B\n-B2\n c\n d\n@@ -11,3 +10,3 @@\n x\n+y\n-Y\n z\n");
        assert_eq!(diff(TWO_HUNKS).discard_patch(|_| false).unwrap(), None);
    }

    #[test]
    fn discarding_lines_of_a_new_file_edits_it() {
        let patch = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+one\n+two\n+three\n";
        let out = String::from_utf8(parse_patch(patch.as_bytes(), 100).discard_patch(|at| at.line == 1).unwrap().unwrap()).unwrap();
        assert_eq!(out, "diff --git a/n b/n\n--- a/n\n+++ b/n\n@@ -1,3 +1,2 @@\n one\n-two\n three\n");
    }

    #[test]
    fn a_deleted_file_has_no_lines_to_discard() {
        let patch = "diff --git a/d b/d\ndeleted file mode 100644\n--- a/d\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-gone\n";
        let err = parse_patch(patch.as_bytes(), 100).discard_patch(|_| true).unwrap_err();
        assert!(err.to_string().contains("deleted"), "{err}");
    }

    #[test]
    fn discarding_keeps_crlf_and_handles_no_newline_markers() {
        let body = b"@@ -1,2 +1,2 @@\r\n a\r\n-old\r\n+new\r\n";
        let crlf = parse_patch(&[HEADER.as_bytes(), body].concat(), 100);
        let out = crlf.discard_patch(|at| at.line == 2).unwrap().unwrap();
        assert!(out.ends_with(b"@@ -1,2 +1,1 @@\n a\r\n-new\r\n"), "{:?}", String::from_utf8_lossy(&out));

        let body = "@@ -1,1 +1,2 @@\n-last\n\\ No newline at end of file\n+last\n+more\n";
        let marked = diff(body);
        assert_eq!(discard_hunks(marked.discard_patch(only(&[(0, 3)])).unwrap()), "@@ -1,2 +1,1 @@\n last\n-more\n");
        let err = marked.discard_patch(only(&[(0, 0)])).unwrap_err();
        assert!(err.to_string().contains("discarded together"), "{err}");
    }

    #[test]
    fn unpickable_diffs_are_an_error() {
        let binary = FileDiff { binary: true, ..FileDiff::default() };
        assert!(matches!(binary.partial_patch(|_| true), Err(GitError::Unsupported(_))));
    }
}
