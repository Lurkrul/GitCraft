//! Talking to remotes: fetch, pull and push, and how far the checked-out branch is ahead of or
//! behind its upstream.
//!
//! These commands use the user's own git setup for credentials (credential helpers, SSH agent).
//! Git never prompts in a terminal here (`GIT_TERMINAL_PROMPT=0`); a credential manager's own
//! window still works. They can take long, so they get a longer time limit and can be cancelled.

use std::time::Duration;

use crate::command::{CancelFlag, RunOptions};
use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::repo::Repo;

/// Network commands may wait on a slow server or a credential window.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Where the checked-out branch stands against its upstream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    /// The repository's remotes (`origin`, …).
    pub remotes: Vec<String>,
    /// The checked-out branch's short name, `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Its upstream's short name (`origin/main`), if it has one that still exists.
    pub upstream: Option<String>,
    /// Commits on the branch that the upstream doesn't have (to push).
    pub ahead: usize,
    /// Commits on the upstream that the branch doesn't have (to pull).
    pub behind: usize,
}

/// How to push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushMode {
    /// An ordinary push; refused if the remote has commits the branch doesn't.
    Normal,
    /// `--force-with-lease`: replaces the remote branch, but only if it is still where the last
    /// fetch saw it (so nobody else's newer work is overwritten unseen). Plain `--force` is never
    /// used.
    ForceWithLease,
}

impl Git {
    /// Reads the remotes, the checked-out branch, its upstream and the ahead/behind counts (from
    /// what the last fetch knows; nothing is fetched).
    pub fn sync_state(&self, repo: &Repo) -> Result<SyncState> {
        let root = repo.root();
        let remotes = String::from_utf8_lossy(&self.run(root, &["remote"])?)
            .lines()
            .map(str::trim)
            .filter(|name| valid_remote(name))
            .map(str::to_string)
            .collect();
        let options = RunOptions { also_ok: &[1], ..RunOptions::default() };
        let head = self.run_with(root, &["symbolic-ref", "-q", "HEAD"], &options)?;
        let head = String::from_utf8_lossy(&head).trim().to_string();
        let Some(branch) = head.strip_prefix("refs/heads/").map(str::to_string) else {
            return Ok(SyncState { remotes, ..SyncState::default() });
        };
        let tracking = self.run(root, &["for-each-ref", "--format=%(upstream:short)%00%(upstream:track)", &head])?;
        let tracking = String::from_utf8_lossy(&tracking);
        let (upstream, track) = tracking.trim_end().split_once('\0').unwrap_or(("", ""));
        // "[gone]": the upstream was deleted on the remote (and pruned here).
        let upstream = (!upstream.is_empty() && track != "[gone]").then(|| upstream.to_string());
        let (mut ahead, mut behind) = (0, 0);
        if upstream.is_some() && self.head_commit(repo)?.is_some() {
            let counts = self.run(root, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}", "--"])?;
            let counts = String::from_utf8_lossy(&counts);
            let mut numbers = counts.split_whitespace().map(str::parse::<usize>);
            match (numbers.next(), numbers.next()) {
                (Some(Ok(a)), Some(Ok(b))) => (ahead, behind) = (a, b),
                _ => return Err(GitError::Malformed { what: "rev-list --left-right".into(), detail: format!("{:?}", counts.trim()) }),
            }
        }
        Ok(SyncState { remotes, branch: Some(branch), upstream, ahead, behind })
    }

    /// Fetches every remote, removing remote branches that were deleted there.
    pub fn fetch(&self, repo: &Repo, cancel: Option<CancelFlag>) -> Result<()> {
        self.run_with(repo.root(), &["fetch", "--all", "--prune"], &network(cancel))?;
        Ok(())
    }

    /// Pulls the checked-out branch's upstream, the way the user's git config says (merge,
    /// rebase or fast-forward only), exactly like `git pull` on the command line. A merge uses
    /// git's default message (no editor opens).
    pub fn pull(&self, repo: &Repo, cancel: Option<CancelFlag>) -> Result<()> {
        self.run_with(repo.root(), &["pull", "--no-edit"], &network(cancel))?;
        Ok(())
    }

    /// Pushes the checked-out branch to its upstream.
    pub fn push(&self, repo: &Repo, mode: PushMode, cancel: Option<CancelFlag>) -> Result<()> {
        let args: &[&str] = match mode {
            PushMode::Normal => &["push"],
            PushMode::ForceWithLease => &["push", "--force-with-lease"],
        };
        self.run_with(repo.root(), args, &network(cancel)).map_err(push_error)?;
        Ok(())
    }

    /// Publishes the checked-out branch: pushes it to `remote` under the same name and makes that
    /// its upstream.
    pub fn publish(&self, repo: &Repo, remote: &str, branch: &str, cancel: Option<CancelFlag>) -> Result<()> {
        if !valid_remote(remote) || branch.is_empty() || branch.starts_with('-') || branch.chars().any(char::is_control) {
            return Err(GitError::Malformed { what: "push".into(), detail: format!("can't publish {branch:?} to {remote:?}") });
        }
        let refspec = format!("HEAD:refs/heads/{branch}");
        self.run_with(repo.root(), &["push", "-u", remote, &refspec], &network(cancel)).map_err(push_error)?;
        Ok(())
    }

    /// How many commits the upstream has that the checked-out branch doesn't: what a force push
    /// would take off the remote (as of the last fetch).
    pub fn upstream_only_commits(&self, repo: &Repo) -> Result<usize> {
        let output = self.run(repo.root(), &["rev-list", "--count", "HEAD..@{upstream}", "--"])?;
        let text = String::from_utf8_lossy(&output);
        text.trim().parse().map_err(|_| GitError::Malformed { what: "rev-list --count".into(), detail: format!("{:?}", text.trim()) })
    }
}

fn network(cancel: Option<CancelFlag>) -> RunOptions {
    RunOptions { timeout: NETWORK_TIMEOUT, cancel, ..RunOptions::default() }
}

/// Remote names come from `git remote`; never pass one that could be read as an option.
fn valid_remote(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && !name.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Turns git's "rejected" push failures into [`GitError::PushRejected`], which says what to do.
fn push_error(error: GitError) -> GitError {
    match &error {
        GitError::CommandFailed { stderr, .. } if stderr.contains("[rejected]") || stderr.contains("[remote rejected]") => {
            GitError::PushRejected { stale: stderr.contains("stale info"), detail: stderr.clone() }
        }
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_names_that_look_like_options_are_refused() {
        assert!(valid_remote("origin") && valid_remote("my-fork"));
        for bad in ["", "-u", "a b", "x\ty"] {
            assert!(!valid_remote(bad), "{bad:?}");
        }
    }

    #[test]
    fn rejected_pushes_get_their_own_error() {
        let failed = |stderr: &str| GitError::CommandFailed { command: "git push".into(), code: Some(1), stderr: stderr.into() };
        assert!(matches!(push_error(failed(" ! [rejected]        main -> main (fetch first)")), GitError::PushRejected { stale: false, .. }));
        assert!(matches!(push_error(failed(" ! [rejected]        main -> main (stale info)")), GitError::PushRejected { stale: true, .. }));
        assert!(matches!(push_error(failed("fatal: unable to access")), GitError::CommandFailed { .. }));
    }
}
