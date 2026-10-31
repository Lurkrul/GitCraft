//! Picking single lines of a file for the next commit.
//!
//! A line is identified by a [`LineKey`] that doesn't depend on where it sits in the *new* file,
//! so unticked lines stay unticked when the file is edited elsewhere and its diff is read again:
//! - a removed line by its line number in the old file (HEAD's version, which doesn't move);
//! - an added line by the old-file line it is inserted before, its place in that run of added
//!   lines, and its text.

use std::collections::{BTreeMap, BTreeSet};

use gitcraft_git::{FileDiff, LineKind, LineRef, PartialFile};

/// A changed line's identity, stable across edits elsewhere in the file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineKey {
    pub removed: bool,
    /// Removed: its old line number. Added: the old line it comes before.
    pub anchor: u32,
    /// Added: its place in the run of added lines at that spot (0 for the first).
    pub offset: u32,
    pub text: String,
}

impl LineKey {
    /// The same spot, ignoring the text: an edited line still matches its earlier self.
    fn spot(&self) -> (bool, u32, u32) {
        (self.removed, self.anchor, self.offset)
    }
}

/// Every added and removed line of `diff`, in order, with its key.
pub fn line_keys(diff: &FileDiff) -> Vec<(LineRef, LineKey)> {
    let mut keys = Vec::new();
    for (hunk_index, hunk) in diff.hunks.iter().enumerate() {
        // The old line number following each position, found from the end backwards.
        let mut next_old = vec![None; hunk.lines.len()];
        let mut following = None;
        for (index, line) in hunk.lines.iter().enumerate().rev() {
            next_old[index] = following;
            if line.old_line.is_some() {
                following = line.old_line;
            }
        }
        let mut last_old: Option<u32> = None;
        let mut run = 0u32;
        for (index, line) in hunk.lines.iter().enumerate() {
            let at = LineRef { hunk: hunk_index, line: index };
            match line.kind {
                LineKind::Context => {
                    last_old = line.old_line;
                    run = 0;
                }
                LineKind::Removed => {
                    let anchor = line.old_line.unwrap_or(0);
                    keys.push((at, LineKey { removed: true, anchor, offset: 0, text: line.text.clone() }));
                    last_old = line.old_line;
                    run = 0;
                }
                LineKind::Added => {
                    // Before the next old line; at the end of a hunk, after the last one; in a
                    // hunk with no old lines at all (a new file), at 0.
                    let anchor = next_old.get(index).copied().flatten().or(last_old.map(|n| n.saturating_add(1))).unwrap_or(0);
                    keys.push((at, LineKey { removed: false, anchor, offset: run, text: line.text.clone() }));
                    run = run.saturating_add(1);
                }
                LineKind::NoNewline => {}
            }
        }
    }
    keys
}

/// A file committed in part: its diff and the lines left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partial {
    /// The diff the choice was made on (the patch is built from it).
    pub diff: FileDiff,
    /// The diff's changed lines, by position.
    keys: BTreeMap<LineRef, LineKey>,
    unticked: BTreeSet<LineKey>,
}

impl Partial {
    /// Starts from `diff` with the given lines unticked.
    pub fn new(diff: FileDiff, unticked: BTreeSet<LineKey>) -> Partial {
        let keys = line_keys(&diff).into_iter().collect();
        Partial { diff, keys, unticked }
    }

    pub fn is_ticked(&self, at: LineRef) -> bool {
        self.keys.get(&at).is_none_or(|key| !self.unticked.contains(key))
    }

    /// The positions of the unticked lines (for drawing).
    pub fn unticked_lines(&self) -> BTreeSet<LineRef> {
        self.keys.iter().filter(|(_, key)| self.unticked.contains(key)).map(|(at, _)| *at).collect()
    }

    pub fn set(&mut self, lines: &[LineRef], ticked: bool) {
        for at in lines {
            if let Some(key) = self.keys.get(at) {
                if ticked {
                    self.unticked.remove(key);
                } else {
                    self.unticked.insert(key.clone());
                }
            }
        }
    }

    /// No line is left out.
    pub fn all_ticked(&self) -> bool {
        !self.keys.values().any(|key| self.unticked.contains(key))
    }

    /// Every line is left out.
    pub fn none_ticked(&self) -> bool {
        self.keys.values().all(|key| self.unticked.contains(key))
    }

    /// Moves the choice onto a freshly read diff of the same file. Unticked lines that are still
    /// there stay unticked; so does a line edited in place (same spot, new text), so a change the
    /// user left out isn't slipped in. Lines that are new elsewhere start ticked, like any change.
    pub fn remap(&mut self, diff: FileDiff) {
        let new_keys: BTreeMap<LineRef, LineKey> = line_keys(&diff).into_iter().collect();
        let old_keys: BTreeSet<&LineKey> = self.keys.values().collect();
        let present: BTreeSet<&LineKey> = new_keys.values().collect();
        let vanished_spots: BTreeSet<_> = self.unticked.iter().filter(|key| !present.contains(key)).map(LineKey::spot).collect();

        let mut unticked = BTreeSet::new();
        for key in new_keys.values() {
            let kept = self.unticked.contains(key);
            let edited = !old_keys.contains(key) && vanished_spots.contains(&key.spot());
            if kept || edited {
                unticked.insert(key.clone());
            }
        }
        self.diff = diff;
        self.keys = new_keys;
        self.unticked = unticked;
    }

    /// The patch of the ticked lines, or `None` if nothing is ticked.
    pub fn patch(&self, path: &str) -> Result<Option<PartialFile>, String> {
        let patch = self.diff.partial_patch(|at| self.is_ticked(at)).map_err(|error| format!("{path}: {error}"))?;
        Ok(patch.map(|patch| PartialFile { path: path.to_string(), patch }))
    }
}

/// The changed lines between two lines (inclusive, in either order), for a Shift+click.
pub fn range(diff: &FileDiff, from: LineRef, to: LineRef) -> Vec<LineRef> {
    let (low, high) = if from <= to { (from, to) } else { (to, from) };
    line_keys(diff).into_iter().map(|(at, _)| at).filter(|at| (low..=high).contains(at)).collect()
}

/// A hunk's changed lines.
pub fn hunk_lines(diff: &FileDiff, hunk: usize) -> Vec<LineRef> {
    line_keys(diff).into_iter().map(|(at, _)| at).filter(|at| at.hunk == hunk).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::{DiffLine, Hunk};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
        DiffLine { kind, old_line: old, new_line: new, text: text.into() }
    }

    /// Old lines 10-13; "b" (11) replaced by "B1", "B2"; "x" added at the end.
    fn sample(added_text: &str) -> FileDiff {
        use LineKind::*;
        FileDiff {
            hunks: vec![Hunk {
                header: "@@ -10,4 +10,6 @@".into(),
                lines: vec![
                    line(Context, Some(10), Some(10), "a"),
                    line(Removed, Some(11), None, "b"),
                    line(Added, None, Some(11), added_text),
                    line(Added, None, Some(12), "B2"),
                    line(Context, Some(12), Some(13), "c"),
                    line(Context, Some(13), Some(14), "d"),
                    line(Added, None, Some(15), "x"),
                ],
            }],
            ..FileDiff::default()
        }
    }

    fn at(line: usize) -> LineRef {
        LineRef { hunk: 0, line }
    }

    #[test]
    fn keys_use_old_file_positions() {
        let keys: Vec<_> = line_keys(&sample("B1")).into_iter().map(|(at, key)| (at.line, key.removed, key.anchor, key.offset)).collect();
        assert_eq!(keys, [(1, true, 11, 0), (2, false, 12, 0), (3, false, 12, 1), (6, false, 14, 0)]);
    }

    #[test]
    fn a_new_file_anchors_at_zero() {
        let diff = FileDiff {
            hunks: vec![Hunk {
                header: "@@ -0,0 +1,2 @@".into(),
                lines: vec![line(LineKind::Added, None, Some(1), "p"), line(LineKind::Added, None, Some(2), "q")],
            }],
            ..FileDiff::default()
        };
        let keys: Vec<_> = line_keys(&diff).into_iter().map(|(_, key)| (key.anchor, key.offset)).collect();
        assert_eq!(keys, [(0, 0), (0, 1)]);
    }

    #[test]
    fn ticking_and_unticking() {
        let mut partial = Partial::new(sample("B1"), BTreeSet::new());
        assert!(partial.all_ticked() && !partial.none_ticked());
        partial.set(&[at(2)], false);
        assert!(!partial.is_ticked(at(2)) && partial.is_ticked(at(3)));
        assert!(partial.is_ticked(at(0)), "context lines count as ticked");
        assert_eq!(partial.unticked_lines(), BTreeSet::from([at(2)]));
        partial.set(&[at(1), at(3), at(6)], false);
        assert!(partial.none_ticked());
    }

    #[test]
    fn unticked_lines_survive_a_refresh() {
        let mut partial = Partial::new(sample("B1"), BTreeSet::new());
        partial.set(&[at(6)], false); // leave out "x"
        // The file is read again: the same changes, now one line further down in the new file.
        let mut moved = sample("B1");
        for line in &mut moved.hunks[0].lines {
            line.new_line = line.new_line.map(|n| n + 1);
        }
        partial.remap(moved);
        assert!(!partial.is_ticked(at(6)));
        assert!(partial.is_ticked(at(2)));
    }

    #[test]
    fn an_unticked_line_edited_in_place_stays_unticked() {
        let mut partial = Partial::new(sample("B1"), BTreeSet::new());
        partial.set(&[at(2)], false);
        partial.remap(sample("B1 edited"));
        assert!(!partial.is_ticked(at(2)), "same spot, new text: still left out");
        assert!(partial.is_ticked(at(3)));
    }

    #[test]
    fn ranges_and_hunks_cover_changed_lines_only() {
        let diff = sample("B1");
        assert_eq!(range(&diff, at(6), at(2)), [at(2), at(3), at(6)]);
        assert_eq!(hunk_lines(&diff, 0), [at(1), at(2), at(3), at(6)]);
        assert!(hunk_lines(&diff, 5).is_empty());
    }

    #[test]
    fn patches_need_a_pickable_diff() {
        let partial = Partial::new(sample("B1"), BTreeSet::new());
        let error = partial.patch("f.txt").unwrap_err();
        assert!(error.starts_with("f.txt: "), "{error}");
    }
}
