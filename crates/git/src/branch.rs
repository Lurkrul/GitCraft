//! Creating, checking out, renaming and deleting local branches.
//!
//! Branch names here are short names as people write them (`feature/login`, not
//! `refs/heads/feature/login`). Every name is checked before it reaches git: it can't start with
//! `-` (so it is never read as an option) and must pass git's own `check-ref-format` rules.

use crate::error::{GitError, Result};
use crate::hash::parse_hash;
use crate::locate::Git;
use crate::refs::parse_ref_name;
use crate::repo::Repo;

/// Longest branch name accepted (git's own limit is the file system's; this keeps messages sane).
const MAX_BRANCH_NAME: usize = 250;

impl Git {
    /// Checks that `name` is a valid new branch name, by git's own rules (no spaces, no `..`, no
    /// `~ ^ : ? * [ \`, not ending in `.lock` or `/`, …).
    pub fn check_branch_name(&self, repo: &Repo, name: &str) -> Result<()> {
        quick_check(name)?;
        let full = format!("refs/heads/{name}");
        match self.run(repo.root(), &["check-ref-format", &full]) {
            Ok(_) => Ok(()),
            // Exit code 1, with no message: git says the name is not valid.
            Err(GitError::CommandFailed { code: Some(1), .. }) => Err(invalid(name)),
            Err(error) => Err(error),
        }
    }

    /// Creates the branch `name` at `start` (a full ref name such as `refs/heads/main`, or a full
    /// commit hash; `None` for where HEAD is), and checks it out if `checkout` is set (uncommitted
    /// changes come along, as with `git switch -c`).
    pub fn create_branch(&self, repo: &Repo, name: &str, start: Option<&str>, checkout: bool) -> Result<()> {
        self.check_branch_name(repo, name)?;
        let start = start.map(checked_start).transpose()?;
        let mut args = if checkout { vec!["switch", "-c", name] } else { vec!["branch", name] };
        if let Some(start) = &start {
            args.push(start);
        }
        self.run(repo.root(), &args)?;
        Ok(())
    }

    /// Checks out the local branch `name`. Uncommitted changes come along when git can carry
    /// them; if they would be overwritten, git refuses and nothing changes (its message says
    /// which files).
    pub fn switch_branch(&self, repo: &Repo, name: &str) -> Result<()> {
        quick_check(name)?;
        // `--no-guess`: never quietly create a branch from a remote one of the same name.
        self.run(repo.root(), &["switch", "--no-guess", name])?;
        Ok(())
    }

    /// Renames the local branch `old` to `new` (the checked-out branch included).
    pub fn rename_branch(&self, repo: &Repo, old: &str, new: &str) -> Result<()> {
        quick_check(old)?;
        self.check_branch_name(repo, new)?;
        self.run(repo.root(), &["branch", "-m", old, new])?;
        Ok(())
    }

    /// How many commits only the branch `name` reaches: the ones that would be lost (left
    /// unreachable) if it were deleted. Every other branch, remote branch, tag and HEAD count as
    /// keeping a commit.
    pub fn commits_only_on(&self, repo: &Repo, name: &str) -> Result<usize> {
        quick_check(name)?;
        let full = format!("refs/heads/{name}");
        // For `--branches`, git matches `--exclude` patterns without the `refs/heads/` prefix.
        // Branch names can't hold glob characters (`* ? [`), so the name matches only itself.
        let exclude = format!("--exclude={name}");
        let output = self.run(repo.root(), &["rev-list", "--count", &full, "--not", &exclude, "--branches", "--remotes", "--tags", "HEAD", "--"])?;
        let text = String::from_utf8_lossy(&output);
        text.trim().parse().map_err(|_| GitError::Malformed { what: "rev-list --count".into(), detail: format!("{:?} is not a number", text.trim()) })
    }

    /// Deletes the local branch `name`, merged or not. The app asks first, naming the commits that
    /// would be lost (see [`Git::commits_only_on`]). Git refuses to delete the checked-out branch.
    pub fn delete_branch(&self, repo: &Repo, name: &str) -> Result<()> {
        quick_check(name)?;
        self.run(repo.root(), &["branch", "-D", name])?;
        Ok(())
    }
}

/// The checks that need no git: something to name, not an option, no control characters.
fn quick_check(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.len() > MAX_BRANCH_NAME || name.chars().any(char::is_control) {
        return Err(invalid(name));
    }
    Ok(())
}

fn invalid(name: &str) -> GitError {
    GitError::InvalidBranchName { name: name.to_string() }
}

/// A start point: a full ref name (never read as an option, since it starts with `refs/`) or a
/// full commit hash.
fn checked_start(start: &str) -> Result<String> {
    if let Some(hash) = parse_hash(start.as_bytes()) {
        return Ok(hash);
    }
    parse_ref_name(start.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_checks_refuse_options_and_junk() {
        for bad in ["", "-f", "--delete", "a\nb", &"x".repeat(MAX_BRANCH_NAME + 1)] {
            assert!(quick_check(bad).is_err(), "{bad:?}");
        }
        assert!(quick_check("feature/login").is_ok());
    }

    #[test]
    fn start_points_are_full_refs_or_hashes() {
        assert_eq!(checked_start("refs/heads/main").unwrap(), "refs/heads/main");
        assert!(checked_start(&"a".repeat(40)).is_ok());
        assert!(checked_start("main").is_err(), "short names could be read as anything");
        assert!(checked_start("-x").is_err());
    }
}
