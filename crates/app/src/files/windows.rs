//! Windows: Explorer and the shell's "Open with" dialog.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TRASH_NAME: &str = "Recycle Bin";
pub const FILE_MANAGER_NAME: &str = "Explorer";
pub const HAS_SYSTEM_OPEN_WITH: bool = true;

/// A program in the Windows folder, by full path, so a same-named program in the current folder
/// or on the PATH can't be run instead.
fn system_program(relative: &str) -> PathBuf {
    let windows = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windows.join(relative)
}

/// `explorer.exe /select,"<path>"` opens the folder with the file selected. Explorer reads its
/// own command line, so the argument is passed exactly as written (`raw_arg`), with the path in
/// quotes. Windows paths can't contain `"`, so the quoting can't be broken out of.
pub fn reveal_command(path: &Path) -> Command {
    let mut command = Command::new(system_program("explorer.exe"));
    command.raw_arg(format!("/select,\"{}\"", path.display()));
    command
}

/// Explorer opens a file with the default program for its type, as a double-click would. (No
/// `cmd /c start`: cmd would read `&` and `^` in a file name as commands.)
pub fn open_command(path: &Path) -> Command {
    let mut command = Command::new(system_program("explorer.exe"));
    command.arg(path);
    command
}

/// The shell's "Open with" dialog: `rundll32 shell32.dll,OpenAs_RunDLL <path>`.
pub fn open_with_dialog_command(path: &Path) -> Option<Command> {
    let mut command = Command::new(system_program(r"System32\rundll32.exe"));
    command.arg("shell32.dll,OpenAs_RunDLL").arg(path);
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_use_full_system_paths() {
        let command = reveal_command(Path::new(r"C:\repo\a b.txt"));
        assert!(Path::new(command.get_program()).is_absolute());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [r#"/select,"C:\repo\a b.txt""#]);
        let dialog = open_with_dialog_command(Path::new(r"C:\x.txt")).unwrap();
        assert!(dialog.get_program().to_string_lossy().ends_with(r"System32\rundll32.exe"));
    }
}
