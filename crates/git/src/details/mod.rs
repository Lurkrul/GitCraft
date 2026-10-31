//! One commit in full: message, author and committer, and the files it changed.

mod parse;

use crate::error::{GitError, Result};
use crate::hash::parse_hash;
use crate::locate::Git;
use crate::repo::Repo;

/// Fields separated by NUL; the message (`%B`, which may span many lines) is last, so it can hold
/// anything but a NUL (git strips those from messages).
const SHOW_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ce%x00%ct%x00%B";

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    /// Moved (and perhaps edited); `ChangedFile::old_path` says from where.
    Renamed,
    /// Copied from another file; `ChangedFile::old_path` says which.
    Copied,
    /// Changed kind, such as a file becoming a symlink.
    TypeChanged,
    /// Anything else git may report (unmerged, unknown).
    Other,
}

impl FileStatus {
    /// The one-letter code git uses, for compact display.
    pub fn letter(self) -> char {
        match self {
            FileStatus::Added => 'A',
            FileStatus::Modified => 'M',
            FileStatus::Deleted => 'D',
            FileStatus::Renamed => 'R',
            FileStatus::Copied => 'C',
            FileStatus::TypeChanged => 'T',
            FileStatus::Other => '?',
        }
    }
}

/// A file a commit changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// The path after the change, relative to the repository root, with `/` separators. Bytes
    /// that aren't UTF-8 are shown as the replacement character.
    pub path: String,
    /// For renames and copies, the path it came from.
    pub old_path: Option<String>,
    pub status: FileStatus,
    /// Lines added and removed; `None` for binary files, which have no lines.
    pub lines: Option<(u64, u64)>,
}

/// A commit with everything the details panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub id: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_time: i64,
    pub committer_name: String,
    pub committer_email: String,
    pub committer_time: i64,
    /// The whole message: the summary line, then the body (if any).
    pub message: String,
    /// The files changed, compared with the first parent (for a merge too; for a root commit,
    /// every file is added).
    pub files: Vec<ChangedFile>,
}

impl Git {
    /// Reads `id` in full. `id` must be a full commit hash (as `Commit::id` is).
    pub fn commit_details(&self, repo: &Repo, id: &str) -> Result<CommitDetails> {
        // A full hash can never be mistaken for an option or a path.
        let id = parse_hash(id.as_bytes())
            .ok_or_else(|| GitError::Malformed { what: "commit id".into(), detail: format!("{id:?} is not a full commit hash") })?;

        let output = self.run(repo.root(), &["show", "-s", "--encoding=UTF-8", SHOW_FORMAT, &id, "--"])?;
        let mut details = parse::parse_show(&output)?;

        // Compare with the first parent, or with nothing for a root commit.
        let mut range: Vec<&str> = Vec::new();
        match details.parents.first() {
            Some(parent) => range.extend([parent.as_str(), id.as_str()]),
            None => range.extend(["--root", id.as_str()]),
        }
        let diff_tree = |what: &str| -> Result<Vec<u8>> {
            let mut args = vec!["diff-tree", "-r", "-z", "--no-commit-id", "--find-renames", what];
            args.extend(&range);
            args.push("--");
            self.run(repo.root(), &args)
        };
        let statuses = parse::parse_name_status(&diff_tree("--name-status")?)?;
        let counts = parse::parse_numstat(&diff_tree("--numstat")?)?;
        details.files = parse::join_files(statuses, counts);
        Ok(details)
    }
}
