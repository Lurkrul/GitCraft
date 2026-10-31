//! Reading the commit history.

mod parse;

use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::repo::Repo;

/// How many commits [`Git::log`] reads unless told otherwise.
pub const DEFAULT_LOG_LIMIT: usize = 10_000;

/// The fields asked for, each followed by a NUL byte (`%x00`); with `-z` each commit also ends
/// with one. Git strips NUL bytes from names and messages, so they can't appear inside a field.
/// Order: hash, parent hashes (space-separated), author time (Unix seconds), author name,
/// author email, subject (the first line of the message).
const LOG_FORMAT: &str = "--format=%H%x00%P%x00%at%x00%an%x00%ae%x00%s";

/// One commit, as shown in the history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full hash in lowercase hex: 40 characters (SHA-1) or 64 (SHA-256 repositories).
    pub id: String,
    /// Parent hashes: none for a root commit, two or more for a merge.
    pub parents: Vec<String>,
    /// Author time in seconds since 1970-01-01 UTC.
    pub author_time: i64,
    pub author_name: String,
    pub author_email: String,
    /// First line of the commit message.
    pub summary: String,
}

impl Commit {
    /// The first 7 characters of the hash, as git shows it.
    pub fn short_id(&self) -> &str {
        // `get` returns `None` instead of panicking if the id were ever shorter.
        self.id.get(..7).unwrap_or(&self.id)
    }

    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }
}

/// The result of [`Git::log`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    /// Newest first.
    pub commits: Vec<Commit>,
    /// True if the history has more commits than the limit allowed.
    pub truncated: bool,
}

impl Git {
    /// Reads up to `limit` commits reachable from `HEAD`, newest first. A repository without
    /// any commits gives an empty log, not an error.
    pub fn log(&self, repo: &Repo, limit: usize) -> Result<Log> {
        if !self.has_commits(repo)? {
            return Ok(Log::default());
        }
        // Ask for one more than the limit to learn whether there are more.
        let max_count = format!("--max-count={}", limit.saturating_add(1));
        // `--encoding=UTF-8` overrides any `i18n.logOutputEncoding` in the repository's config.
        // The final `--` makes git read `HEAD` as a revision even if a file is called "HEAD".
        let output = self.run(repo.root(), &["log", "-z", "--encoding=UTF-8", &max_count, LOG_FORMAT, "HEAD", "--"])?;
        let mut commits = parse::parse_log(&output)?;
        let truncated = commits.len() > limit;
        commits.truncate(limit);
        Ok(Log { commits, truncated })
    }

    /// Whether `HEAD` points at a commit. In a fresh repository it names a branch that doesn't
    /// exist yet, and `rev-parse --verify --quiet` exits with code 1 without printing anything.
    fn has_commits(&self, repo: &Repo) -> Result<bool> {
        match self.run(repo.root(), &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]) {
            Ok(_) => Ok(true),
            Err(GitError::CommandFailed { code: Some(1), .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author_time: 0,
            author_name: String::new(),
            author_email: String::new(),
            summary: String::new(),
        }
    }

    #[test]
    fn short_id_is_seven_characters() {
        assert_eq!(commit("0123456789abcdef0123456789abcdef01234567", &[]).short_id(), "0123456");
        assert_eq!(commit("abc", &[]).short_id(), "abc");
    }

    #[test]
    fn merge_has_two_or_more_parents() {
        assert!(!commit("a", &["b"]).is_merge());
        assert!(commit("a", &["b", "c"]).is_merge());
    }
}
