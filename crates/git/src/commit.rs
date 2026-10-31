//! Making a commit from a chosen set of working-tree changes, or throwing a file's changes away.

use std::time::Duration;

use crate::command::RunOptions;
use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::repo::Repo;
use crate::status::checked_path;

/// Hooks (pre-commit, commit-msg) can run test suites, so a commit gets longer than the usual
/// 60 seconds.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// What to commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRequest {
    /// Changed files to include as they are on disk (new, edited and deleted files alike).
    pub include: Vec<String>,
    /// Changed files to leave out: whatever is staged for them is unstaged first. Their files on
    /// disk are not touched.
    pub exclude: Vec<String>,
    /// Files committed in part: only the changes in each patch (from
    /// [`crate::FileDiff::partial_patch`]). Their files on disk are not touched.
    pub partial: Vec<PartialFile>,
    /// The full message: summary, then a blank line and the description.
    pub message: String,
    /// Replace the last commit instead of adding a new one.
    pub amend: bool,
}

/// Part of a file's changes to commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialFile {
    pub path: String,
    /// A patch against the file's version in HEAD, holding only the chosen lines.
    pub patch: Vec<u8>,
}

/// What the last commit is, for the Amend option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendInfo {
    /// Its message, to start the new one from.
    pub message: String,
    /// A remote branch that already contains it (such as `refs/remotes/origin/main`), if any.
    /// Amending it then rewrites published history.
    pub pushed_to: Option<String>,
}

impl Git {
    /// Commits exactly the chosen changes: the left-out files are unstaged, the included ones
    /// staged as they are on disk, then `git commit` runs (hooks included). Returns the new
    /// commit's id. Paths go to git through stdin, so any number of them works.
    ///
    /// Files that are neither included nor excluded keep whatever is staged for them. If git
    /// refuses the commit (a hook fails, say), the staging done so far stays, as it would on
    /// the command line.
    pub fn commit(&self, repo: &Repo, request: &CommitRequest) -> Result<String> {
        if request.message.trim().is_empty() {
            return Err(GitError::Malformed { what: "commit message".into(), detail: "the message is empty".into() });
        }
        let include = path_list(&request.include)?;
        let exclude = path_list(&request.exclude)?;
        let partial_paths: Vec<String> = request.partial.iter().map(|file| file.path.clone()).collect();
        let mut seen = std::collections::HashSet::new();
        if let Some(twice) = request.include.iter().chain(&request.exclude).chain(&partial_paths).find(|path| !seen.insert(path.as_str())) {
            return Err(GitError::Malformed { what: "commit request".into(), detail: format!("{twice:?} is listed more than once") });
        }
        let partial = path_list(&partial_paths)?;
        let root = repo.root();
        let quick = RunOptions::default();

        if let Some(paths) = exclude {
            self.unstage(repo, paths)?;
        }
        if let Some(paths) = include {
            // `-A` stages deletions too.
            self.run_with_input(root, &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"], &quick, paths)?;
        }
        if let Some(paths) = partial {
            // The patches are made against HEAD, so the index must hold HEAD's version first.
            self.unstage(repo, paths)?;
            // One `git apply` for every file: it applies all of them or none. `--cached` changes
            // only the index, never the files on disk. `--whitespace=nowarn` stops the user's
            // `apply.whitespace` setting from rewriting or refusing lines.
            let patches: Vec<u8> = request.partial.iter().flat_map(|file| file.patch.iter().copied()).collect();
            self.run_with_input(root, &["apply", "--cached", "--whitespace=nowarn", "-"], &quick, patches)?;
        }

        let mut args = vec!["commit", "-q", "-F", "-"];
        if request.amend {
            args.push("--amend");
        }
        let options = RunOptions { timeout: COMMIT_TIMEOUT, ..RunOptions::default() };
        self.run_with_input(root, &args, &options, request.message.clone().into_bytes())?;

        let head = self.run(root, &["rev-parse", "HEAD"])?;
        Ok(String::from_utf8_lossy(&head).trim().to_string())
    }

    /// The last commit's message and whether it has been pushed.
    pub fn amend_info(&self, repo: &Repo) -> Result<AmendInfo> {
        let root = repo.root();
        let message = self.run(root, &["log", "-1", "--encoding=UTF-8", "--format=%B", "HEAD", "--"])?;
        let remotes = self.run(root, &["branch", "-r", "--contains", "HEAD", "--format=%(refname)"])?;
        // `origin/HEAD` is only an alias for another remote branch, so it is skipped.
        let pushed_to =
            String::from_utf8_lossy(&remotes).lines().map(str::trim).find(|line| !line.is_empty() && !line.ends_with("/HEAD")).map(str::to_string);
        Ok(AmendInfo { message: String::from_utf8_lossy(&message).trim_end().to_string(), pushed_to })
    }

    /// Puts a tracked file back the way it is in the last commit, both on disk and in the index:
    /// edits are undone, a deleted file comes back, and a newly added file is removed. Before the
    /// first commit there is nothing to go back to, so the file is just unstaged.
    ///
    /// This throws away the file's changes for good; the app moves the file to the Recycle Bin
    /// first and asks before calling it. Untracked files aren't git's to restore: the app deals
    /// with them.
    pub fn discard(&self, repo: &Repo, path: &str) -> Result<()> {
        checked_path(path, "?")?;
        if self.has_commits(repo)? {
            self.run(repo.root(), &["restore", "--source=HEAD", "--staged", "--worktree", "--", path])?;
        } else {
            self.run(repo.root(), &["rm", "--cached", "-q", "--ignore-unmatch", "--", path])?;
        }
        Ok(())
    }

    /// Puts the files' index entries back to HEAD's (`paths` as from [`path_list`]): staged changes
    /// are dropped, files HEAD doesn't have leave the index. The files on disk are not touched.
    fn unstage(&self, repo: &Repo, paths: Vec<u8>) -> Result<()> {
        let quick = RunOptions::default();
        if self.has_commits(repo)? {
            self.run_with_input(repo.root(), &["reset", "-q", "--pathspec-from-file=-", "--pathspec-file-nul"], &quick, paths)?;
        } else {
            // No HEAD to reset to: take the files out of the index instead.
            let args = ["rm", "--cached", "-q", "--ignore-unmatch", "--pathspec-from-file=-", "--pathspec-file-nul"];
            self.run_with_input(repo.root(), &args, &quick, paths)?;
        }
        Ok(())
    }

    /// Whether HEAD points at a commit (false on a branch with no commits yet).
    fn has_commits(&self, repo: &Repo) -> Result<bool> {
        // `--verify -q` exits with 1, printing nothing, when HEAD has no commit.
        let options = RunOptions { also_ok: &[1], ..RunOptions::default() };
        let output = self.run_with(repo.root(), &["rev-parse", "--verify", "-q", "HEAD"], &options)?;
        Ok(!output.is_empty())
    }
}

/// The paths as NUL-separated stdin for `--pathspec-file-nul`, or `None` if there are none.
/// Each is checked to stay inside the repository first.
fn path_list(paths: &[String]) -> Result<Option<Vec<u8>>> {
    if paths.is_empty() {
        return Ok(None);
    }
    let mut list = Vec::new();
    for path in paths {
        checked_path(path, "?")?;
        list.extend_from_slice(path.as_bytes());
        list.push(0);
    }
    Ok(Some(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lists_are_nul_separated_and_checked() {
        assert_eq!(path_list(&[]).unwrap(), None);
        assert_eq!(path_list(&["a b.txt".into(), "c/d".into()]).unwrap(), Some(b"a b.txt\0c/d\0".to_vec()));
        assert!(path_list(&["../escape".into()]).is_err());
    }
}
