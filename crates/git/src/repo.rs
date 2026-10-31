//! Opening a repository.

use std::path::{Path, PathBuf};

use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::platform;

/// An open repository: a folder git has confirmed is the top of a working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// The top folder of the working tree.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Git {
    /// Opens the repository containing `path`. `path` may be the top folder or any folder inside
    /// the working tree; the returned [`Repo`] always points at the top.
    pub fn open_repo(&self, path: &Path) -> Result<Repo> {
        if !path.is_dir() {
            return Err(GitError::NoSuchFolder { path: path.to_path_buf() });
        }
        let mut output = self.run(path, &["rev-parse", "--show-toplevel"])?;
        // The path ends with a newline (on Windows possibly "\r\n").
        while output.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            output.pop();
        }
        if output.is_empty() {
            return Err(GitError::Malformed { what: "rev-parse --show-toplevel".into(), detail: "empty path".into() });
        }
        Ok(Repo { root: platform::path_from_git_output(output)? })
    }
}
