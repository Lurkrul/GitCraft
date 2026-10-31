//! Linux (and other Unix) specifics.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::Command;

use crate::error::Result;

pub const GIT_EXE_NAME: &str = "git";

/// Nothing to do: there is no console window to hide on Unix.
pub fn hide_console(_command: &mut Command) {}

/// Unix paths are arbitrary bytes (not necessarily UTF-8), so they are taken as they are.
pub fn path_from_git_output(bytes: Vec<u8>) -> Result<PathBuf> {
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_non_utf8_bytes() {
        let path = path_from_git_output(b"/tmp/caf\xe9".to_vec()).unwrap();
        assert_eq!(path.into_os_string().into_vec(), b"/tmp/caf\xe9");
    }
}
