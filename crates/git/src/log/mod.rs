//! Reading the commit history.

mod parse;

use crate::error::Result;
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

/// Which history [`Git::log`] reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LogScope {
    /// Every local branch, remote branch and tag, plus HEAD (which may be detached).
    #[default]
    AllBranches,
    /// One ref, by its full name (`refs/heads/main`, `refs/remotes/origin/main`).
    Ref(String),
}

impl Git {
    /// Reads up to `limit` commits in `scope`, newest first. Children always come before their
    /// parents (`--date-order`), which the commit graph relies on. A repository without any
    /// commits gives an empty log, not an error.
    pub fn log(&self, repo: &Repo, scope: &LogScope, limit: usize) -> Result<Log> {
        // Ask for one more than the limit to learn whether there are more.
        let max_count = format!("--max-count={}", limit.saturating_add(1));
        // `--encoding=UTF-8` overrides any `i18n.logOutputEncoding` in the repository's config.
        let mut args: Vec<&str> = vec!["log", "-z", "--encoding=UTF-8", "--date-order", &max_count, LOG_FORMAT];
        let ref_name;
        match scope {
            LogScope::AllBranches => {
                args.extend(["--branches", "--remotes", "--tags"]);
                // HEAD too, so a detached HEAD's commits show; but only once it has a commit,
                // because in a new repository `HEAD` names nothing and git would fail.
                if self.head_commit(repo)?.is_some() {
                    args.push("HEAD");
                }
            }
            LogScope::Ref(name) => {
                // A full ref name (starting with "refs/") can never be taken for an option.
                ref_name = crate::refs::parse_ref_name(name.as_bytes())?;
                args.push(&ref_name);
            }
        }
        // The final `--` makes git read the revisions as revisions even if files share their names.
        args.push("--");

        let output = self.run(repo.root(), &args)?;
        let mut commits = parse::parse_log(&output)?;
        let truncated = commits.len() > limit;
        commits.truncate(limit);
        Ok(Log { commits, truncated })
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
