//! Submodules: other repositories kept inside this one at a recorded commit.
//!
//! Git keeps a submodule as one entry in the index (mode `160000`, a "gitlink") naming the commit
//! the parent repository records for it. Its files live in its own repository in that folder,
//! which may be at another commit (out of date), have local changes, or not be cloned at all yet.

use crate::command::{CancelFlag, RunOptions};
use crate::error::{GitError, Result};
use crate::hash::parse_hash;
use crate::locate::Git;
use crate::repo::Repo;
use crate::status::checked_path;

/// Most submodules listed (a sanity limit on the index's contents).
const MAX_SUBMODULES: usize = 1_000;
/// Initialising clones, which can take a while.
const CLONE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// One submodule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    /// Its folder, relative to the parent's top folder (`/` separators).
    pub path: String,
    /// The commit the parent records for it (in the index).
    pub recorded: String,
    /// The commit checked out in its folder, or `None` if it isn't initialised (not cloned).
    pub checked_out: Option<String>,
    /// It has uncommitted changes of its own.
    pub dirty: bool,
}

impl Submodule {
    pub fn initialized(&self) -> bool {
        self.checked_out.is_some()
    }

    /// Checked out at a different commit than the parent records.
    pub fn out_of_date(&self) -> bool {
        self.checked_out.as_ref().is_some_and(|commit| *commit != self.recorded)
    }
}

impl Git {
    /// Lists the repository's submodules, with what each has checked out.
    pub fn submodules(&self, repo: &Repo) -> Result<Vec<Submodule>> {
        let index = self.run(repo.root(), &["ls-files", "--stage", "-z"])?;
        let mut submodules = Vec::new();
        for (path, recorded) in parse_gitlinks(&index)? {
            let folder = repo.root().join(&path);
            // An initialised submodule has its own `.git` (a file pointing into the parent's git
            // folder, or a folder in older setups).
            let checked_out = if folder.join(".git").exists() {
                let head = self.run(&folder, &["rev-parse", "HEAD"])?;
                parse_hash(String::from_utf8_lossy(&head).trim().as_bytes())
            } else {
                None
            };
            let dirty = match checked_out {
                Some(_) => !self.run(&folder, &["status", "--porcelain", "-z", "--ignore-submodules=none"])?.is_empty(),
                None => false,
            };
            submodules.push(Submodule { path, recorded, checked_out, dirty });
        }
        Ok(submodules)
    }

    /// Initialises (clones) the submodule at `path` and checks out its recorded commit. Clones
    /// over the network, so it can be cancelled.
    pub fn init_submodule(&self, repo: &Repo, path: &str, cancel: Option<CancelFlag>) -> Result<()> {
        let path = checked_path(path, "submodule")?;
        let options = RunOptions { timeout: CLONE_TIMEOUT, cancel, ..RunOptions::default() };
        self.run_with(repo.root(), &["submodule", "update", "--init", "--", &path], &options)?;
        Ok(())
    }

    /// Checks out the parent's recorded commit in the submodule at `path` (fetching it first if
    /// the submodule doesn't have it). Commits made in the submodule stay on their branch; git
    /// refuses if local changes there would be overwritten. The app asks first.
    pub fn update_submodule(&self, repo: &Repo, path: &str, cancel: Option<CancelFlag>) -> Result<()> {
        let path = checked_path(path, "submodule")?;
        let options = RunOptions { timeout: CLONE_TIMEOUT, cancel, ..RunOptions::default() };
        self.run_with(repo.root(), &["submodule", "update", "--", &path], &options)?;
        Ok(())
    }
}

/// The gitlinks in `git ls-files --stage -z` output: `<mode> <hash> <stage>\t<path>` records,
/// NUL-separated. Only mode `160000` entries (submodules) are kept, at stage 0.
fn parse_gitlinks(output: &[u8]) -> Result<Vec<(String, String)>> {
    let malformed = |detail: String| GitError::Malformed { what: "ls-files --stage".into(), detail };
    let mut links = Vec::new();
    for record in output.split(|&byte| byte == 0).filter(|record| !record.is_empty()) {
        let text = String::from_utf8_lossy(record);
        let (meta, path) = text.split_once('\t').ok_or_else(|| malformed("a record without a path".into()))?;
        let mut fields = meta.split(' ');
        let (Some(mode), Some(hash), Some(stage)) = (fields.next(), fields.next(), fields.next()) else {
            return Err(malformed(format!("{meta:?} is not mode, hash and stage")));
        };
        if mode != "160000" || stage != "0" {
            continue;
        }
        let hash = parse_hash(hash.as_bytes()).ok_or_else(|| malformed(format!("{hash:?} is not a hash")))?;
        links.push((checked_path(path, "submodule")?, hash));
        if links.len() > MAX_SUBMODULES {
            return Err(malformed(format!("more than {MAX_SUBMODULES} submodules")));
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_gitlinks() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let output = format!("100644 {a} 0\tsrc/main.rs\x00160000 {b} 0\tvendor/lib\x00160000 {b} 0\tdeps/with space\x00");
        let links = parse_gitlinks(output.as_bytes()).unwrap();
        assert_eq!(links, [("vendor/lib".to_string(), b.clone()), ("deps/with space".to_string(), b)]);
    }

    #[test]
    fn refuses_odd_records() {
        assert!(parse_gitlinks(b"160000 nothash 0\tx\x00").is_err());
        assert!(parse_gitlinks(b"no tab here\x00").is_err());
        let escape = format!("160000 {} 0\t../outside\x00", "a".repeat(40));
        assert!(parse_gitlinks(escape.as_bytes()).is_err());
        assert_eq!(parse_gitlinks(b"").unwrap(), []);
    }

    #[test]
    fn states() {
        let sub = |checked_out: Option<&str>| Submodule {
            path: "lib".into(),
            recorded: "a".repeat(40),
            checked_out: checked_out.map(str::to_string),
            dirty: false,
        };
        assert!(!sub(None).initialized());
        assert!(!sub(Some(&"a".repeat(40))).out_of_date());
        assert!(sub(Some(&"b".repeat(40))).out_of_date());
    }
}
