//! Linux (and other Unix desktops): `xdg-open` from the freedesktop.org tools.

use std::path::Path;
use std::process::Command;

pub const TRASH_NAME: &str = "Trash";
pub const FILE_MANAGER_NAME: &str = "file manager";
pub const HAS_SYSTEM_OPEN_WITH: bool = false;

/// Opens the folder containing the file. (Selecting the file itself needs a file-manager-specific
/// D-Bus call; the folder works everywhere.)
pub fn reveal_command(path: &Path) -> Command {
    let folder = path.parent().unwrap_or(path);
    let mut command = Command::new("xdg-open");
    command.arg(folder);
    command
}

/// Opens the file with the default program for its type.
pub fn open_command(path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(path);
    command
}

/// There is no common "Open with" dialog on Linux; the app asks for a program with a file picker.
pub fn open_with_dialog_command(_path: &Path) -> Option<Command> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_opens_the_folder() {
        let command = reveal_command(Path::new("/repo/src/main.rs"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["/repo/src"]);
    }
}
