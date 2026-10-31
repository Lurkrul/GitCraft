//! Windows specifics.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use crate::error::{GitError, Result};

/// Only `git.exe` is accepted. A `git.cmd` or `git.bat` would be run through `cmd.exe`, which
/// re-parses the arguments and is not safe for untrusted ones.
pub const GIT_EXE_NAME: &str = "git.exe";

/// Win32 `CREATE_NO_WINDOW`: git is a console program, and without this flag every git command
/// started from a GUI app would flash a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hide_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Git for Windows prints paths as UTF-8 with forward slashes (`E:/repos/thing`).
pub fn path_from_git_output(bytes: Vec<u8>) -> Result<PathBuf> {
    let text = String::from_utf8(bytes).map_err(|_| GitError::Malformed { what: "path".into(), detail: "not valid UTF-8".into() })?;
    // Rebuilding from components turns the forward slashes into backslashes.
    Ok(PathBuf::from(text).components().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_forward_slashes() {
        let path = path_from_git_output(b"E:/repos/thing".to_vec()).unwrap();
        assert_eq!(path.to_str(), Some(r"E:\repos\thing"));
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(matches!(path_from_git_output(vec![b'E', b':', 0xff]), Err(GitError::Malformed { .. })));
    }
}
