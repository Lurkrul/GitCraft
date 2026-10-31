//! Finding the `git` executable and checking that it works.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::command::{RunOptions, run_process};
use crate::error::{GitError, Result};
use crate::platform;

/// A working `git` executable. Every git command in the crate is a method on this.
///
/// `Clone` is cheap (a path and a short string), so a background thread can have its own copy.
#[derive(Debug, Clone)]
pub struct Git {
    exe: PathBuf,
    version: String,
}

impl Git {
    /// Finds `git` on the `PATH` and checks that it runs.
    pub fn locate() -> Result<Git> {
        let exe = find_in_path(std::env::var_os("PATH").as_deref()).ok_or(GitError::NotFound)?;
        Git::with_executable(exe)
    }

    /// Uses a specific `git` executable, after checking that `git --version` works.
    pub fn with_executable(exe: PathBuf) -> Result<Git> {
        let mut command = Command::new(&exe);
        command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        platform::hide_console(&mut command);

        let unusable = |reason: String| GitError::Unusable { exe: exe.clone(), reason };
        let output = run_process(command, &RunOptions::default(), "git --version").map_err(|err| unusable(err.to_string()))?;
        if !output.status.success() {
            return Err(unusable(format!("`--version` failed: {}", String::from_utf8_lossy(&output.stderr).trim())));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let version = text.trim().strip_prefix("git version ").ok_or_else(|| unusable(format!("unexpected version output {:?}", text.trim())))?;
        Ok(Git { exe, version: version.to_string() })
    }

    /// The executable in use.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// Git's version string, such as `2.54.0.windows.1`.
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// Looks through a `PATH`-style list of folders for the git executable.
///
/// Relative entries (such as `.`) are skipped on purpose: git commands run inside the user's
/// repositories, and a relative entry would let a repository supply its own "git" program.
/// Resolving the full path here also means `Command` never searches for the program itself.
pub(crate) fn find_in_path(path_var: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path_var?).filter(|dir| dir.is_absolute()).map(|dir| dir.join(platform::GIT_EXE_NAME)).find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn path_list(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn finds_git_in_a_later_folder() {
        let empty = tempfile::tempdir().unwrap();
        let with_git = tempfile::tempdir().unwrap();
        let fake_git = with_git.path().join(platform::GIT_EXE_NAME);
        std::fs::write(&fake_git, b"").unwrap();

        let missing = empty.path().join("does-not-exist");
        let found = find_in_path(Some(&path_list(&[&missing, empty.path(), with_git.path()])));
        assert_eq!(found, Some(fake_git));
    }

    #[test]
    fn skips_relative_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(platform::GIT_EXE_NAME), b"").unwrap();
        // A relative entry that would find the file if it were followed from inside `dir`.
        let relative = Path::new(".");
        assert_eq!(find_in_path(Some(&path_list(&[relative]))), None);
    }

    #[test]
    fn nothing_on_an_empty_or_missing_path() {
        assert_eq!(find_in_path(None), None);
        assert_eq!(find_in_path(Some(OsStr::new(""))), None);
    }

    #[test]
    fn locates_the_real_git() {
        let git = Git::locate().unwrap();
        assert!(git.exe().is_absolute());
        assert!(git.version().chars().next().is_some_and(|c| c.is_ascii_digit()), "{}", git.version());
    }

    #[test]
    fn a_non_git_program_is_unusable() {
        // A file that exists but isn't a runnable git.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join(platform::GIT_EXE_NAME);
        std::fs::write(&fake, b"not a program").unwrap();
        assert!(matches!(Git::with_executable(fake), Err(GitError::Unusable { .. })));
    }
}
