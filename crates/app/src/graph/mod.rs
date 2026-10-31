//! The commit graph: lane assignment (pure, in `lanes`), author avatars and painting.

pub mod avatar;
pub mod lanes;
pub mod paint;

use gitcraft_git::Log;

use lanes::GraphRow;

/// A loaded history and its graph, one row per commit. Built on the background thread, so even
/// a 10,000-commit graph never holds up a frame.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub log: Log,
    pub rows: Vec<GraphRow>,
    /// The widest row, in lanes (sizes the graph column).
    pub max_width: usize,
}

impl History {
    pub fn new(log: Log) -> History {
        let rows = lanes::assign_lanes(&log.commits);
        let max_width = rows.iter().map(|row| row.width).max().unwrap_or(0);
        History { log, rows, max_width }
    }
}
