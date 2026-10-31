//! Working with the user's files outside git: the Recycle Bin, `.gitignore`, and opening files in
//! other programs. Everything here runs on the background worker, never the UI thread.
//!
//! The platform-specific parts (how to show a file in the file manager, how to open it) are in
//! `windows.rs` and `unix.rs`; only one of them is compiled.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
use unix as platform;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What the platform calls the place deleted files go.
pub const TRASH_NAME: &str = platform::TRASH_NAME;
/// What the platform calls its file manager, for the "Show in …" menu item.
pub const FILE_MANAGER_NAME: &str = platform::FILE_MANAGER_NAME;
/// Whether the platform has its own "Open with" dialog (Windows does; Linux has no common one).
pub const HAS_SYSTEM_OPEN_WITH: bool = platform::HAS_SYSTEM_OPEN_WITH;

/// A repository-relative path (`/`-separated, as git writes it) as a full path on this platform.
pub fn full_path(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(relative.split('/').filter(|part| !part.is_empty()));
    path
}

/// The extension of a path's file name (`rs` for `src/main.rs`), if it has one. A dotfile such as
/// `.env` has none.
pub fn extension_of(relative: &str) -> Option<String> {
    let name = relative.rsplit('/').next()?;
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty() && !extension.is_empty()).then(|| extension.to_string())
}

/// The `.gitignore` line that ignores exactly this file: anchored to the repository root with a
/// leading `/`, and with the characters `.gitignore` treats specially escaped.
pub fn ignore_file_line(relative: &str) -> String {
    format!("/{}", escape_pattern(relative))
}

/// The `.gitignore` line that ignores every file with this extension, anywhere.
pub fn ignore_extension_line(extension: &str) -> String {
    format!("*.{}", escape_pattern(extension))
}

/// Backslash-escapes what `.gitignore` would read as a pattern: wildcards and brackets, and
/// trailing spaces (which git would drop). Both callers put `/` or `*.` first, so a `#` (comment)
/// or `!` (negation) can never start the line.
fn escape_pattern(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let trailing = escaped.len() - escaped.trim_end_matches(' ').len();
    if trailing > 0 {
        escaped.truncate(escaped.len() - trailing);
        escaped.push_str(&"\\ ".repeat(trailing));
    }
    escaped
}

/// `existing` with `line` added at the end, or `None` if it already has that line. Keeps the
/// file's line endings (CRLF or LF) and makes sure the new line starts on a line of its own.
pub fn with_line_added(existing: &str, line: &str) -> Option<String> {
    if existing.lines().any(|current| current.trim_end_matches('\r') == line) {
        return None;
    }
    let newline = if existing.contains("\r\n") { "\r\n" } else { "\n" };
    let mut updated = existing.to_string();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push_str(newline);
    }
    updated.push_str(line);
    updated.push_str(newline);
    Some(updated)
}

/// Adds `line` to the `.gitignore` in the repository's top folder (creating it if needed).
/// Returns false if the line was already there. The file is replaced in one step (written next
/// to it, then renamed over it), so a crash or full disk can't leave it half written.
pub fn add_to_gitignore(root: &Path, line: &str) -> io::Result<bool> {
    let path = root.join(".gitignore");
    let existing = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, ".gitignore is not valid UTF-8 text"))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let Some(updated) = with_line_added(&existing, line) else {
        return Ok(false);
    };
    let temporary = root.join(".gitignore.gitcraft-tmp");
    std::fs::write(&temporary, updated)?;
    std::fs::rename(&temporary, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })?;
    Ok(true)
}

/// Moves a file to the Recycle Bin / Trash. Fails rather than deleting for good: a file that
/// can't be recycled (on some network drives, say) is left where it is.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash_impl(path)
}

/// Puts a copy of the file in the Recycle Bin / Trash and leaves the file itself where it is: the
/// safety net before part of it is thrown away. The copy keeps the file's name, so it is easy to
/// find there.
pub fn copy_to_trash(path: &Path) -> Result<(), String> {
    let name = path.file_name().ok_or("the file has no name")?;
    let unique = format!(
        "gitcraft-copy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let folder = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let copy = folder.join(name);
    let result = std::fs::copy(path, &copy).map_err(|error| error.to_string()).and_then(|_| move_to_trash(&copy));
    // Whatever happened, the scratch folder goes (the copy is in the bin, or wasn't made).
    let _ = std::fs::remove_file(&copy);
    let _ = std::fs::remove_dir(&folder);
    result
}

#[cfg(not(test))]
fn trash_impl(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|error| error.to_string())
}

/// Tests must not fill the real Recycle Bin: the file is moved into a scratch folder instead.
#[cfg(test)]
fn trash_impl(path: &Path) -> Result<(), String> {
    let bin = std::env::temp_dir().join("gitcraft-test-trash");
    std::fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
    let unique =
        format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    std::fs::rename(path, bin.join(unique)).map_err(|error| error.to_string())
}

/// Shows the file in the platform's file manager (selected, where the platform allows it).
pub fn reveal(path: &Path) -> io::Result<()> {
    launch(platform::reveal_command(path))
}

/// Opens the file with the default program for its type.
pub fn open(path: &Path) -> io::Result<()> {
    launch(platform::open_command(path))
}

/// Shows the platform's own "Open with" dialog for the file (Windows only).
pub fn open_with_system_dialog(path: &Path) -> io::Result<()> {
    match platform::open_with_dialog_command(path) {
        Some(command) => launch(command),
        None => Err(io::Error::new(io::ErrorKind::Unsupported, "this system has no \"Open with\" dialog")),
    }
}

/// Opens the file with a program the user picked.
pub fn open_with(program: &Path, path: &Path) -> io::Result<()> {
    let mut command = Command::new(program);
    command.arg(path);
    launch(command)
}

/// Starts a program without waiting for it. A thread waits for it instead, so it is cleaned up
/// when it exits (on Linux an unwaited child would linger as a "zombie" until GitCraft quits).
fn launch(mut command: Command) -> io::Result<()> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_paths_use_the_platform_separator() {
        let root = Path::new("repo");
        assert_eq!(full_path(root, "src/ui/main.rs"), root.join("src").join("ui").join("main.rs"));
    }

    #[test]
    fn extensions() {
        assert_eq!(extension_of("src/main.rs").as_deref(), Some("rs"));
        assert_eq!(extension_of("archive.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of(".env"), None, "a dotfile has no extension");
        assert_eq!(extension_of("Makefile"), None);
        assert_eq!(extension_of("dir.d/file"), None, "only the file name counts");
        assert_eq!(extension_of("trailing."), None);
    }

    #[test]
    fn ignore_lines_are_anchored_and_escaped() {
        assert_eq!(ignore_file_line("build/out.log"), "/build/out.log");
        assert_eq!(ignore_file_line("weird[1]*.txt"), r"/weird\[1\]\*.txt");
        assert_eq!(ignore_file_line("#notes"), "/#notes", "the leading slash already stops a comment");
        assert_eq!(ignore_extension_line("log"), "*.log");
        assert_eq!(escape_pattern("name  "), r"name\ \ ");
    }

    #[test]
    fn lines_are_added_once_with_the_file_line_endings() {
        assert_eq!(with_line_added("", "/a").as_deref(), Some("/a\n"));
        assert_eq!(with_line_added("target\n", "/a").as_deref(), Some("target\n/a\n"));
        assert_eq!(with_line_added("target", "/a").as_deref(), Some("target\n/a\n"), "starts on its own line");
        assert_eq!(with_line_added("target\r\n", "/a").as_deref(), Some("target\r\n/a\r\n"));
        assert_eq!(with_line_added("x\n/a\r\n", "/a"), None, "already there");
    }

    #[test]
    fn gitignore_is_created_or_extended() {
        let dir = tempfile::tempdir().unwrap();
        assert!(add_to_gitignore(dir.path(), "/a.log").unwrap());
        assert!(add_to_gitignore(dir.path(), "*.tmp").unwrap());
        assert!(!add_to_gitignore(dir.path(), "/a.log").unwrap(), "no duplicate");
        assert_eq!(std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(), "/a.log\n*.tmp\n");
        assert!(!dir.path().join(".gitignore.gitcraft-tmp").exists());
    }

    #[test]
    fn a_gitignore_that_is_not_text_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(add_to_gitignore(dir.path(), "/a").is_err());
        assert_eq!(std::fs::read(dir.path().join(".gitignore")).unwrap(), [0xff, 0xfe, 0x00]);
    }

    #[test]
    fn trashing_moves_the_file_away() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gone.txt");
        std::fs::write(&file, "x").unwrap();
        move_to_trash(&file).unwrap();
        assert!(!file.exists());
        assert!(move_to_trash(&file).is_err(), "a missing file is an error, not a silent success");
    }
}
