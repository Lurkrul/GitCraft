//! Lane assignment for the commit graph: a pure function from commits to lanes.
//!
//! The commits come newest first with every child before its parents (git's `--date-order`
//! guarantees this). We walk them keeping a list of active lanes, each waiting for one commit:
//!
//! 1. A commit takes the leftmost lane waiting for it, or a new lane if nothing waits for it
//!    (a branch tip). Other lanes waiting for the same commit end here: their branches started
//!    from this commit, so their lines converge into its dot.
//! 2. Its lane then waits for its first parent. Each further parent (a merge) joins a lane that
//!    already waits for that parent, or gets a new lane.
//!
//! Each row records the line segments that cross its top half (from the row above into the
//! row's middle) and its bottom half (from the middle down to the row below). Lanes never move
//! sideways, so a lane just passing through is a straight vertical line, and freed lanes are
//! reused, keeping the graph narrow.

use gitcraft_git::Commit;

/// A line within one half of a row, from lane `from` to lane `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub from: usize,
    pub to: usize,
    /// Colour index; the painter maps it to a palette entry.
    pub color: usize,
}

/// How to draw one commit's row of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// The lane the commit's dot is in.
    pub lane: usize,
    /// The dot's colour index.
    pub color: usize,
    /// Segments from the top edge of the row to its middle.
    pub upper: Vec<Segment>,
    /// Segments from the middle of the row to its bottom edge.
    pub lower: Vec<Segment>,
    /// Lanes needed to draw this row (one more than the rightmost lane used).
    pub width: usize,
}

/// A lane in use: the commit it is heading for, and its colour.
#[derive(Debug, Clone)]
struct Lane {
    waiting_for: String,
    color: usize,
}

/// Assigns every commit a lane and works out the lines between rows. One row per commit, in the
/// same order. Commits whose parents are not in the list (a truncated history, or one branch's
/// history shown on its own) simply leave their lane running off the bottom.
pub fn assign_lanes(commits: &[Commit]) -> Vec<GraphRow> {
    // `Option<Lane>` per slot: `None` is a free slot that a new lane may reuse.
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut colors = 0..; // An endless counter: each new lane takes the next colour index.
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        // Lanes waiting for this commit, left to right.
        let waiting: Vec<usize> = active(&lanes).filter(|(_, l)| l.waiting_for == commit.id).map(|(i, _)| i).collect();

        // Top half: every active lane comes down from the row above; lanes waiting for this
        // commit bend into its dot, the others pass straight by.
        let upper: Vec<Segment> = active(&lanes)
            .map(|(i, l)| {
                let to = if l.waiting_for == commit.id { waiting.first().copied().unwrap_or(i) } else { i };
                Segment { from: i, to, color: l.color }
            })
            .collect();

        // Step 1: the commit's lane. If nothing waits for it, it is a branch tip: a new lane with
        // nothing above it.
        let (lane, color) = match waiting.first() {
            Some(&first) => (first, color_of(&lanes, first)),
            None => {
                let color = colors.next().unwrap_or(0);
                (place(&mut lanes, commit.id.clone(), color), color)
            }
        };
        // The converging lanes end at this commit.
        for &i in waiting.iter().skip(1) {
            free(&mut lanes, i);
        }

        // Step 2: the lanes leaving the commit.
        match commit.parents.first() {
            Some(first_parent) => {
                if let Some(slot) = lanes.get_mut(lane) {
                    *slot = Some(Lane { waiting_for: first_parent.clone(), color });
                }
            }
            None => free(&mut lanes, lane), // A root commit: its line ends here.
        }
        // Merge parents: a line from the dot to the lane that will reach each parent.
        let mut merge_lines: Vec<Segment> = Vec::new();
        let mut opened_here: Vec<usize> = Vec::new();
        for parent in commit.parents.iter().skip(1) {
            // Only the slot number is kept, so the look-up's borrow of `lanes` ends before
            // `place` needs to change it.
            let existing = active(&lanes).find(|(_, l)| &l.waiting_for == parent).map(|(i, _)| i);
            let target = match existing {
                Some(i) => i,
                None => {
                    let i = place(&mut lanes, parent.clone(), colors.next().unwrap_or(0));
                    opened_here.push(i);
                    i
                }
            };
            let segment = Segment { from: lane, to: target, color: color_of(&lanes, target) };
            // The same parent listed twice (possible in hand-made history) gets one line.
            if target != lane && !merge_lines.contains(&segment) {
                merge_lines.push(segment);
            }
        }

        // Bottom half: every lane still active continues down (the commit's own lane from its
        // dot, the others straight by), except lanes opened for a merge here, which start at the
        // dot and are drawn by the merge lines.
        let mut lower: Vec<Segment> =
            active(&lanes).filter(|(i, _)| !opened_here.contains(i)).map(|(i, l)| Segment { from: i, to: i, color: l.color }).collect();
        lower.extend(merge_lines);

        // Free slots at the right edge are dropped so later rows stay narrow.
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }

        let width = upper.iter().chain(&lower).map(|s| s.from.max(s.to) + 1).fold(lane + 1, usize::max);
        rows.push(GraphRow { lane, color, upper, lower, width });
    }
    rows
}

/// The lanes in use, with their slot numbers.
fn active(lanes: &[Option<Lane>]) -> impl Iterator<Item = (usize, &Lane)> {
    lanes.iter().enumerate().filter_map(|(i, slot)| slot.as_ref().map(|lane| (i, lane)))
}

fn color_of(lanes: &[Option<Lane>], i: usize) -> usize {
    lanes.get(i).and_then(Option::as_ref).map_or(0, |lane| lane.color)
}

fn free(lanes: &mut [Option<Lane>], i: usize) {
    if let Some(slot) = lanes.get_mut(i) {
        *slot = None;
    }
}

/// Starts a lane waiting for `commit` in the leftmost free slot (or a new one at the right) and
/// returns its slot number.
fn place(lanes: &mut Vec<Option<Lane>>, commit: String, color: usize) -> usize {
    let lane = Some(Lane { waiting_for: commit, color });
    match lanes.iter().position(Option::is_none) {
        Some(i) => {
            if let Some(slot) = lanes.get_mut(i) {
                *slot = lane;
            }
            i
        }
        None => {
            lanes.push(lane);
            lanes.len() - 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Commits from (id, parents) pairs; ids are short for readability (the graph doesn't care).
    fn history(spec: &[(&str, &[&str])]) -> Vec<Commit> {
        spec.iter()
            .map(|(id, parents)| Commit {
                id: id.to_string(),
                parents: parents.iter().map(|p| p.to_string()).collect(),
                author_time: 0,
                author_name: String::new(),
                author_email: String::new(),
                summary: String::new(),
            })
            .collect()
    }

    fn seg(from: usize, to: usize, color: usize) -> Segment {
        Segment { from, to, color }
    }

    #[test]
    fn a_straight_history_is_one_lane() {
        let rows = assign_lanes(&history(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]));
        assert!(rows.iter().all(|r| r.lane == 0 && r.color == 0 && r.width == 1));
        assert_eq!(rows[0].upper, [], "the tip has nothing above it");
        assert_eq!(rows[0].lower, [seg(0, 0, 0)]);
        assert_eq!(rows[1].upper, [seg(0, 0, 0)]);
        assert_eq!(rows[2].lower, [], "the root has nothing below it");
    }

    #[test]
    fn a_merge_opens_a_lane_that_converges_at_the_fork_point() {
        //   m        merge of a (first parent) and b
        //   | \
        //   a  |
        //   |  b
        //   | /
        //   c
        let rows = assign_lanes(&history(&[("m", &["a", "b"]), ("a", &["c"]), ("b", &["c"]), ("c", &[])]));
        let lanes: Vec<_> = rows.iter().map(|r| r.lane).collect();
        assert_eq!(lanes, [0, 0, 1, 0]);

        assert_eq!(rows[0].lower, [seg(0, 0, 0), seg(0, 1, 1)], "straight down to a, across to b");
        assert_eq!(rows[1].upper, [seg(0, 0, 0), seg(1, 1, 1)]);
        assert_eq!(rows[2].color, 1, "b keeps the colour of the line that reached it");
        assert_eq!(rows[3].upper, [seg(0, 0, 0), seg(1, 0, 1)], "b's lane bends into c");
        assert_eq!(rows[3].lower, []);
        assert!(rows.iter().all(|r| r.width <= 2));
    }

    #[test]
    fn two_branch_tips_from_one_commit() {
        // x and y both branch from p; neither is a merge.
        let rows = assign_lanes(&history(&[("x", &["p"]), ("y", &["p"]), ("p", &[])]));
        assert_eq!(rows.iter().map(|r| r.lane).collect::<Vec<_>>(), [0, 1, 0]);
        assert_eq!(rows[1].upper, [seg(0, 0, 0)], "y is a new tip: only x's lane passes by");
        assert_ne!(rows[0].color, rows[1].color, "each tip gets its own colour");
        assert_eq!(rows[2].upper, [seg(0, 0, 0), seg(1, 0, 1)]);
    }

    #[test]
    fn a_merge_parent_already_waited_for_joins_that_lane() {
        // t is a tip whose parent is b; then m merges a and b. b already has a lane (from t),
        // so m's merge line goes to it instead of opening a third lane.
        let rows = assign_lanes(&history(&[("t", &["b"]), ("m", &["a", "b"]), ("a", &["r"]), ("b", &["r"]), ("r", &[])]));
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].lower, [seg(0, 0, 0), seg(1, 1, 1), seg(1, 0, 0)]);
        assert!(rows.iter().all(|r| r.width <= 2), "{rows:?}");
    }

    #[test]
    fn an_octopus_merge_fans_out() {
        let rows = assign_lanes(&history(&[("m", &["a", "b", "c"]), ("a", &["r"]), ("b", &["r"]), ("c", &["r"]), ("r", &[])]));
        assert_eq!(rows[0].lower, [seg(0, 0, 0), seg(0, 1, 1), seg(0, 2, 2)]);
        assert_eq!(rows[4].upper, [seg(0, 0, 0), seg(1, 0, 1), seg(2, 0, 2)]);
        assert_eq!(rows[4].width, 3);
    }

    #[test]
    fn freed_lanes_are_reused() {
        // Two short side branches one after the other should both use lane 1.
        let rows = assign_lanes(&history(&[("m2", &["c", "s2"]), ("s2", &["c"]), ("c", &["m1"]), ("m1", &["a", "s1"]), ("s1", &["a"]), ("a", &[])]));
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[4].lane, 1);
        assert!(rows.iter().all(|r| r.width <= 2));
    }

    #[test]
    fn parents_outside_the_list_run_off_the_bottom() {
        // A truncated history: b's parent isn't loaded.
        let rows = assign_lanes(&history(&[("a", &["b"]), ("b", &["missing"])]));
        assert_eq!(rows[1].lower, [seg(0, 0, 0)]);
    }

    #[test]
    fn a_duplicated_parent_draws_one_line() {
        let rows = assign_lanes(&history(&[("m", &["a", "b", "b"]), ("a", &[]), ("b", &[])]));
        assert_eq!(rows[0].lower, [seg(0, 0, 0), seg(0, 1, 1)]);
    }

    #[test]
    fn every_segment_stays_inside_the_row_width() {
        let rows =
            assign_lanes(&history(&[("m", &["a", "b"]), ("t", &["b"]), ("a", &["c"]), ("b", &["c", "d"]), ("c", &["e"]), ("d", &["e"]), ("e", &[])]));
        for row in &rows {
            assert!(row.lane < row.width);
            for s in row.upper.iter().chain(&row.lower) {
                assert!(s.from < row.width && s.to < row.width, "{row:?}");
            }
        }
    }

    #[test]
    fn no_commits_no_rows() {
        assert!(assign_lanes(&[]).is_empty());
    }
}
