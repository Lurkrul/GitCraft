//! Branches, remote branches, tags and HEAD.

mod parse;

pub(crate) use parse::parse_ref_name;

use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::repo::Repo;

/// One line per ref, fields separated by NUL: the full name, the object it names, the commit an
/// annotated tag points at (empty otherwise), and the target of a symbolic ref (empty otherwise).
/// Ref names can't contain NUL or newlines (git's own rules), so the split is unambiguous.
const REFS_FORMAT: &str = "--format=%(refname)%00%(objectname)%00%(*objectname)%00%(symref)";

/// What kind of ref this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    /// A local branch (`refs/heads/...`).
    Branch,
    /// A remote-tracking branch (`refs/remotes/<remote>/...`).
    RemoteBranch,
    /// A tag (`refs/tags/...`).
    Tag,
}

/// A branch or tag and the commit it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// The full name, such as `refs/heads/main`. Safe to pass to git as a revision.
    pub name: String,
    pub kind: RefKind,
    /// The commit (for an annotated tag, the commit the tag points at).
    pub target: String,
}

impl Ref {
    /// The name as people write it: `main`, `origin/main`, `v1.0`.
    pub fn short_name(&self) -> &str {
        let prefix = match self.kind {
            RefKind::Branch => "refs/heads/",
            RefKind::RemoteBranch => "refs/remotes/",
            RefKind::Tag => "refs/tags/",
        };
        self.name.strip_prefix(prefix).unwrap_or(&self.name)
    }
}

/// Where HEAD is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// On a branch that has commits.
    Branch { name: String, commit: String },
    /// On a branch with no commits yet (a new repository).
    Unborn { name: String },
    /// Detached: checked out at a commit rather than a branch.
    Detached { commit: String },
}

/// Every branch, remote branch and tag, plus HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refs {
    /// Sorted by kind (branches, then remote branches, then tags), then by name.
    pub refs: Vec<Ref>,
    pub head: Head,
}

impl Refs {
    pub fn branches(&self) -> impl Iterator<Item = &Ref> {
        self.refs.iter().filter(|r| r.kind == RefKind::Branch)
    }

    pub fn remote_branches(&self) -> impl Iterator<Item = &Ref> {
        self.refs.iter().filter(|r| r.kind == RefKind::RemoteBranch)
    }

    /// The full name of the branch HEAD is on, if it is on one.
    pub fn head_branch(&self) -> Option<&str> {
        match &self.head {
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
            Head::Detached { .. } => None,
        }
    }

    /// The refs pointing at `commit`, in the same order as `refs`.
    pub fn pointing_at<'a>(&'a self, commit: &'a str) -> impl Iterator<Item = &'a Ref> {
        self.refs.iter().filter(move |r| r.target == commit)
    }
}

impl Git {
    /// Reads all branches, remote branches and tags, and where HEAD is.
    pub fn refs(&self, repo: &Repo) -> Result<Refs> {
        let output = self.run(repo.root(), &["for-each-ref", REFS_FORMAT, "refs/heads", "refs/remotes", "refs/tags"])?;
        let mut refs = parse::parse_refs(&output)?;
        refs.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
        Ok(Refs { refs, head: self.head(repo)? })
    }

    fn head(&self, repo: &Repo) -> Result<Head> {
        // `symbolic-ref -q HEAD` prints the branch, or exits with 1 when HEAD is detached.
        let branch = match self.run(repo.root(), &["symbolic-ref", "-q", "HEAD"]) {
            Ok(output) => Some(parse_ref_name(trim_newline(&output))?),
            Err(GitError::CommandFailed { code: Some(1), .. }) => None,
            Err(err) => return Err(err),
        };
        let commit = self.head_commit(repo)?;
        match (branch, commit) {
            (Some(name), Some(commit)) => Ok(Head::Branch { name, commit }),
            (Some(name), None) => Ok(Head::Unborn { name }),
            (None, Some(commit)) => Ok(Head::Detached { commit }),
            (None, None) => Err(GitError::Malformed { what: "HEAD".into(), detail: "detached, but not at a commit".into() }),
        }
    }

    /// The commit HEAD points at, or `None` in a repository without commits.
    pub(crate) fn head_commit(&self, repo: &Repo) -> Result<Option<String>> {
        match self.run(repo.root(), &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]) {
            Ok(output) => {
                let line = trim_newline(&output);
                crate::hash::parse_hash(line).map(Some).ok_or_else(|| GitError::Malformed {
                    what: "rev-parse HEAD".into(),
                    detail: format!("invalid hash {:?}", String::from_utf8_lossy(line)),
                })
            }
            // Exit code 1 with no output: HEAD names a branch that has no commits yet.
            Err(GitError::CommandFailed { code: Some(1), .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

/// `output` without its trailing newline (and carriage return, on Windows).
fn trim_newline(output: &[u8]) -> &[u8] {
    let output = output.strip_suffix(b"\n").unwrap_or(output);
    output.strip_suffix(b"\r").unwrap_or(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> Ref {
        Ref { name: format!("refs/heads/{name}"), kind: RefKind::Branch, target: "a".repeat(40) }
    }

    #[test]
    fn short_names_drop_the_prefix() {
        assert_eq!(branch("feature/login").short_name(), "feature/login");
        let remote = Ref { name: "refs/remotes/origin/main".into(), kind: RefKind::RemoteBranch, target: String::new() };
        assert_eq!(remote.short_name(), "origin/main");
        let tag = Ref { name: "refs/tags/v1.0".into(), kind: RefKind::Tag, target: String::new() };
        assert_eq!(tag.short_name(), "v1.0");
    }

    #[test]
    fn head_branch_for_each_state() {
        let refs = |head| Refs { refs: vec![branch("main")], head };
        assert_eq!(refs(Head::Branch { name: "refs/heads/main".into(), commit: "a".repeat(40) }).head_branch(), Some("refs/heads/main"));
        assert_eq!(refs(Head::Unborn { name: "refs/heads/main".into() }).head_branch(), Some("refs/heads/main"));
        assert_eq!(refs(Head::Detached { commit: "a".repeat(40) }).head_branch(), None);
    }

    #[test]
    fn trims_newlines() {
        assert_eq!(trim_newline(b"x\r\n"), b"x");
        assert_eq!(trim_newline(b"x\n"), b"x");
        assert_eq!(trim_newline(b"x"), b"x");
    }
}
