//! The working tree's changes (`git status`) and each changed file's diff against HEAD.

mod parse;

pub use parse::MAX_STATUS_ENTRIES;

use crate::command::RunOptions;
use crate::diff::{DIFF_FORMAT_ARGS, FileDiff, MAX_DIFF_BYTES, MAX_DIFF_LINES, parse_patch};
use crate::error::Result;
use crate::locate::Git;
use crate::repo::Repo;

/// One side of a file's status: how it differs in the index (staged) or the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Unmodified,
    Modified,
    /// Became a symlink or submodule, or stopped being one.
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// Part of a merge conflict.
    Unmerged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A file git already tracks (or one staged to be added).
    Tracked,
    /// A file with a merge conflict.
    Unmerged,
    /// A file git doesn't track yet (and isn't ignored).
    Untracked,
}

/// How a file differs between HEAD and the working tree, all changes taken together: what the
/// Commit tab shows next to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingChange {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Conflicted,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// The path relative to the repository's top folder, with `/` separators. Checked to stay
    /// inside the repository.
    pub path: String,
    /// For a staged rename or copy, where it came from.
    pub old_path: Option<String>,
    pub kind: EntryKind,
    /// The change already staged in the index.
    pub staged: Change,
    /// The change in the working tree not yet staged.
    pub unstaged: Change,
    /// The path is a submodule.
    pub submodule: bool,
}

impl StatusEntry {
    /// The change between HEAD and the working tree.
    pub fn change(&self) -> WorkingChange {
        match (self.kind, self.staged, self.unstaged) {
            (EntryKind::Unmerged, ..) => WorkingChange::Conflicted,
            (EntryKind::Untracked, ..) => WorkingChange::Added,
            (_, _, Change::Deleted) | (_, Change::Deleted, _) => WorkingChange::Deleted,
            (_, Change::Added, _) => WorkingChange::Added,
            (_, Change::Renamed | Change::Copied, _) => WorkingChange::Renamed,
            (_, Change::TypeChanged, _) | (_, _, Change::TypeChanged) => WorkingChange::TypeChanged,
            _ => WorkingChange::Modified,
        }
    }

    /// Something of this file is already staged (by `git add` in a terminal, say).
    pub fn has_staged(&self) -> bool {
        self.kind == EntryKind::Tracked && self.staged != Change::Unmodified
    }

    /// The file isn't in HEAD, so its diff compares it with nothing.
    pub fn is_new(&self) -> bool {
        self.change() == WorkingChange::Added
    }
}

/// The working tree's changes, in git's order (by path).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub entries: Vec<StatusEntry>,
    /// More than `MAX_STATUS_ENTRIES` changed files; only the first ones are listed.
    pub truncated: bool,
}

/// Which working-tree file to diff, and against what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingDiffRequest {
    /// The path relative to the repository's top folder (as in [`StatusEntry::path`]).
    pub path: String,
    /// The file isn't in HEAD (untracked, newly added, or HEAD has no commit yet): it is
    /// compared with nothing, so every line shows as added.
    pub new_file: bool,
}

impl Git {
    /// Reads the working tree's changes: staged, not staged and untracked files (every file in
    /// an untracked folder is listed). Submodules are included.
    pub fn status(&self, repo: &Repo) -> Result<Status> {
        let output = self.run(
            repo.root(),
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
                // A rename shows as a deleted and an added file, as `git status` shows it until
                // both are staged. Each half can then be included in a commit on its own.
                "--no-renames",
                "--ignore-submodules=none",
            ],
        )?;
        parse::parse_status(&output)
    }

    /// Reads one changed file's diff from HEAD to the working tree: staged and unstaged changes
    /// together, the way the Commit tab shows them.
    pub fn working_diff(&self, repo: &Repo, request: &WorkingDiffRequest) -> Result<FileDiff> {
        // The path came from `git status` and was checked there; check again, since it is about
        // to be joined to the repository folder.
        let path = parse::checked_path(&request.path, "?")?;
        let options = RunOptions { max_output: MAX_DIFF_BYTES, also_ok: &[1], ..RunOptions::default() };

        let output = if request.new_file {
            // Only a file can be compared with nothing; a deleted file or a folder (an untracked
            // nested repository, say) has no lines to show.
            let on_disk = repo.root().join(&path);
            match on_disk.symlink_metadata() {
                Ok(meta) if !meta.is_dir() => {}
                _ => return Ok(FileDiff::default()),
            }
            // `--no-index` compares two files outside git's records. "/dev/null" means "no
            // file" to git on every platform, Windows included. Exit code 1 means "they
            // differ", which they always do here.
            let mut args = vec!["diff", "--no-index"];
            args.extend(DIFF_FORMAT_ARGS);
            args.extend(["--", "/dev/null", path.as_str()]);
            self.run_with(repo.root(), &args, &options)?
        } else {
            let mut args = vec!["diff"];
            args.extend(DIFF_FORMAT_ARGS);
            args.extend(["HEAD", "--", path.as_str()]);
            let options = RunOptions { also_ok: &[], ..options };
            self.run_with(repo.root(), &args, &options)?
        };
        Ok(parse_patch(&output, MAX_DIFF_LINES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: EntryKind, staged: Change, unstaged: Change) -> StatusEntry {
        StatusEntry { path: "f".into(), old_path: None, kind, staged, unstaged, submodule: false }
    }

    #[test]
    fn overall_change_combines_both_sides() {
        use Change::*;
        let cases = [
            (entry(EntryKind::Untracked, Unmodified, Added), WorkingChange::Added),
            (entry(EntryKind::Unmerged, Unmerged, Unmerged), WorkingChange::Conflicted),
            (entry(EntryKind::Tracked, Unmodified, Modified), WorkingChange::Modified),
            (entry(EntryKind::Tracked, Modified, Modified), WorkingChange::Modified),
            (entry(EntryKind::Tracked, Added, Modified), WorkingChange::Added),
            (entry(EntryKind::Tracked, Unmodified, Deleted), WorkingChange::Deleted),
            (entry(EntryKind::Tracked, Deleted, Unmodified), WorkingChange::Deleted),
            (entry(EntryKind::Tracked, Renamed, Unmodified), WorkingChange::Renamed),
            (entry(EntryKind::Tracked, Unmodified, TypeChanged), WorkingChange::TypeChanged),
        ];
        for (entry, expected) in cases {
            assert_eq!(entry.change(), expected, "{entry:?}");
        }
    }

    #[test]
    fn staged_means_something_in_the_index() {
        assert!(entry(EntryKind::Tracked, Change::Modified, Change::Unmodified).has_staged());
        assert!(!entry(EntryKind::Tracked, Change::Unmodified, Change::Modified).has_staged());
        assert!(!entry(EntryKind::Untracked, Change::Unmodified, Change::Added).has_staged());
    }
}
