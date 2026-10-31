//! One file's changes in a commit, as hunks of numbered lines.

mod parse;
mod patch;

pub(crate) use parse::parse_patch;
pub use patch::LineRef;

use std::ops::Range;

use crate::command::RunOptions;
use crate::error::{GitError, Result};
use crate::hash::parse_hash;
use crate::locate::Git;
use crate::repo::Repo;

/// Most lines kept from one file's diff; the rest is dropped and `FileDiff::truncated` is set.
pub const MAX_DIFF_LINES: usize = 20_000;
/// Most bytes of patch accepted for one file (beyond it the diff is refused as too large).
pub const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

/// Options for every diff we parse. The user's config must not change the output, or run
/// programs: no external diff tools, no text conversion filters, standard a/ and b/ prefixes, no
/// colour, three lines of context.
pub(crate) const DIFF_FORMAT_ARGS: [&str; 6] = ["--no-ext-diff", "--no-textconv", "--no-color", "--src-prefix=a/", "--dst-prefix=b/", "-U3"];

/// What a line of a diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Unchanged, shown for context.
    Context,
    Added,
    Removed,
    /// Git's "\ No newline at end of file" marker for the line before.
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line number in the old file (context and removed lines).
    pub old_line: Option<u32>,
    /// Line number in the new file (context and added lines).
    pub new_line: Option<u32>,
    /// The text, without the leading `+`/`-`/space and without a trailing carriage return.
    pub text: String,
}

/// A run of changes with its surrounding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -1,4 +1,6 @@ fn main()` line, as git wrote it.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// One file's diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub hunks: Vec<Hunk>,
    /// A binary file: git reports only that it changed.
    pub binary: bool,
    /// More than `MAX_DIFF_LINES` lines; only the first ones are kept.
    pub truncated: bool,
    /// Git's exact output, for building a patch of chosen lines ([`FileDiff::partial_patch`]).
    /// `None` when lines can't be picked: a binary file, a cut-short diff, or output that didn't
    /// add up.
    pub source: Option<PatchSource>,
}

impl FileDiff {
    /// Whether single lines of this diff can be picked for a commit.
    pub fn can_pick_lines(&self) -> bool {
        self.source.is_some()
    }
}

/// A hunk header's numbers: `@@ -12,5 +12,7 @@` is old lines 12 to 16 and new lines 12 to 18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HunkRange {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

/// The bytes git wrote for one file's diff, with where each part is. Its hunks and lines line up
/// one for one with [`FileDiff::hunks`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSource {
    pub(crate) bytes: Vec<u8>,
    /// The file header (`diff --git`, mode lines, `---`/`+++`) before the first hunk.
    pub(crate) header: Range<usize>,
    pub(crate) hunks: Vec<SourceHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceHunk {
    pub header: HunkRange,
    /// Each line's bytes in `PatchSource::bytes`, including its line ending.
    pub lines: Vec<Range<usize>>,
}

/// Which file of which commit to diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRequest {
    /// The commit (a full hash).
    pub commit: String,
    /// Its first parent, or `None` for a root commit (then everything is added).
    pub parent: Option<String>,
    /// The file's path in the commit.
    pub path: String,
    /// For a renamed or copied file, where it came from (so git can pair the two).
    pub old_path: Option<String>,
}

impl Git {
    /// Reads one file's changes in a commit, compared with its first parent.
    pub fn file_diff(&self, repo: &Repo, request: &DiffRequest) -> Result<FileDiff> {
        let hash = |id: &str| {
            parse_hash(id.as_bytes())
                .ok_or_else(|| GitError::Malformed { what: "commit id".into(), detail: format!("{id:?} is not a full commit hash") })
        };
        let commit = hash(&request.commit)?;
        let parent = request.parent.as_deref().map(hash).transpose()?;

        let mut args: Vec<&str> = vec!["diff-tree", "-p", "--no-commit-id", "--find-renames"];
        args.extend(DIFF_FORMAT_ARGS);
        match &parent {
            Some(parent) => args.extend([parent.as_str(), commit.as_str()]),
            None => args.extend(["--root", commit.as_str()]),
        }
        // `--` ends the options: what follows are paths (literal, see `Git::run`).
        args.push("--");
        if let Some(old) = &request.old_path {
            args.push(old);
        }
        args.push(&request.path);

        let options = RunOptions { max_output: MAX_DIFF_BYTES, ..RunOptions::default() };
        let output = self.run_with(repo.root(), &args, &options)?;
        Ok(parse_patch(&output, MAX_DIFF_LINES))
    }
}
