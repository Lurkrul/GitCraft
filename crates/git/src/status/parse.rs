//! Parsing `git status --porcelain=v2 -z` output. Pure, so every record type can be tested
//! without running git.
//!
//! Records are NUL-terminated. Each starts with a type character:
//! - `1 XY sub mH mI mW hH hI path`: a changed tracked file
//! - `2 XY sub mH mI mW hH hI Xscore path` NUL `origPath`: a renamed or copied file
//! - `u XY sub m1 m2 m3 mW h1 h2 h3 path`: a file with a merge conflict
//! - `? path`: an untracked file; `! path`: an ignored file (skipped)
//! - `# ...`: headers (skipped)

use std::path::{Component, Path};

use super::{Change, EntryKind, Status, StatusEntry};
use crate::error::{GitError, Result};

/// Most entries kept; a working tree with more (an unignored `node_modules`, say) is cut off and
/// `Status::truncated` is set.
pub const MAX_STATUS_ENTRIES: usize = 50_000;

pub fn parse_status(output: &[u8]) -> Result<Status> {
    let text = String::from_utf8_lossy(output);
    let mut records = text.split('\0').filter(|record| !record.is_empty());
    let mut status = Status::default();

    while let Some(record) = records.next() {
        if status.entries.len() >= MAX_STATUS_ENTRIES {
            status.truncated = true;
            break;
        }
        let entry = match record.as_bytes().first() {
            Some(b'1') => tracked(record, 9, None)?,
            Some(b'2') => {
                // The original path is the next NUL-separated record.
                let old = records.next().ok_or_else(|| malformed(record, "a rename without its original path"))?;
                tracked(record, 10, Some(old))?
            }
            Some(b'u') => unmerged(record)?,
            Some(b'?') => StatusEntry {
                path: checked_path(record.get(2..).unwrap_or(""), record)?,
                old_path: None,
                kind: EntryKind::Untracked,
                staged: Change::Unmodified,
                unstaged: Change::Added,
                submodule: false,
            },
            // Headers, ignored files, and any record type a future git adds.
            _ => continue,
        };
        status.entries.push(entry);
    }
    Ok(status)
}

/// A `1` (ordinary, `fields` = 9) or `2` (rename/copy, `fields` = 10) record. The path is the
/// last field and may contain spaces, so the record is split into exactly `fields` parts.
fn tracked(record: &str, fields: usize, old_path: Option<&str>) -> Result<StatusEntry> {
    let parts: Vec<&str> = record.splitn(fields, ' ').collect();
    let (Some(xy), Some(sub), Some(path)) = (parts.get(1), parts.get(2), parts.get(fields - 1)) else {
        return Err(malformed(record, "too few fields"));
    };
    let (staged, unstaged) = changes(xy).ok_or_else(|| malformed(record, "unknown status letters"))?;
    Ok(StatusEntry {
        path: checked_path(path, record)?,
        old_path: old_path.map(|old| checked_path(old, record)).transpose()?,
        kind: EntryKind::Tracked,
        staged,
        unstaged,
        submodule: sub.starts_with('S'),
    })
}

/// A `u` record: 11 fields, the path last.
fn unmerged(record: &str) -> Result<StatusEntry> {
    let parts: Vec<&str> = record.splitn(11, ' ').collect();
    let (Some(sub), Some(path)) = (parts.get(2), parts.get(10)) else {
        return Err(malformed(record, "too few fields"));
    };
    Ok(StatusEntry {
        path: checked_path(path, record)?,
        old_path: None,
        kind: EntryKind::Unmerged,
        staged: Change::Unmerged,
        unstaged: Change::Unmerged,
        submodule: sub.starts_with('S'),
    })
}

/// The two status letters: what changed in the index (staged) and in the working tree (not yet
/// staged). `.` means unchanged.
fn changes(xy: &str) -> Option<(Change, Change)> {
    let mut letters = xy.chars();
    let (x, y) = (letters.next()?, letters.next()?);
    if letters.next().is_some() {
        return None;
    }
    Some((change(x)?, change(y)?))
}

fn change(letter: char) -> Option<Change> {
    Some(match letter {
        '.' => Change::Unmodified,
        'M' => Change::Modified,
        'T' => Change::TypeChanged,
        'A' => Change::Added,
        'D' => Change::Deleted,
        'R' => Change::Renamed,
        'C' => Change::Copied,
        'U' => Change::Unmerged,
        _ => return None,
    })
}

/// A path from git, checked before it is ever joined to the repository folder: it must be
/// relative and stay inside the repository. Uses the platform's own rules, so `a:b` is a fine
/// file name on Linux but a drive on Windows.
pub(super) fn checked_path(path: &str, record: &str) -> Result<String> {
    let inside = Path::new(path).components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if path.is_empty() || !inside {
        return Err(malformed(record, "a path outside the repository"));
    }
    Ok(path.to_string())
}

fn malformed(record: &str, detail: &str) -> GitError {
    // Only the record type goes in the message, not the whole record (it holds file names).
    let kind = record.chars().next().unwrap_or('?');
    GitError::Malformed { what: "git status output".into(), detail: format!("record type {kind:?}: {detail}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "1234567890123456789012345678901234567890";

    fn ordinary(xy: &str, path: &str) -> String {
        format!("1 {xy} N... 100644 100644 100644 {HASH} {HASH} {path}\0")
    }

    #[test]
    fn reads_every_record_type() {
        let output = [
            "# branch.oid abc\0".to_string(),
            ordinary(".M", "src/main.rs"),
            ordinary("A.", "new file.txt"),
            format!("2 R. N... 100644 100644 100644 {HASH} {HASH} R100 renamed.rs\0old name.rs\0"),
            format!("u UU N... 100644 100644 100644 100644 {HASH} {HASH} {HASH} conflict.txt\0"),
            "? notes/todo.md\0".to_string(),
            "! target/ignored\0".to_string(),
        ]
        .concat();
        let status = parse_status(output.as_bytes()).unwrap();
        let summary: Vec<_> = status.entries.iter().map(|e| (e.path.as_str(), e.kind, e.staged, e.unstaged)).collect();
        assert_eq!(
            summary,
            [
                ("src/main.rs", EntryKind::Tracked, Change::Unmodified, Change::Modified),
                ("new file.txt", EntryKind::Tracked, Change::Added, Change::Unmodified),
                ("renamed.rs", EntryKind::Tracked, Change::Renamed, Change::Unmodified),
                ("conflict.txt", EntryKind::Unmerged, Change::Unmerged, Change::Unmerged),
                ("notes/todo.md", EntryKind::Untracked, Change::Unmodified, Change::Added),
            ]
        );
        assert_eq!(status.entries[2].old_path.as_deref(), Some("old name.rs"));
        assert!(!status.truncated);
    }

    #[test]
    fn marks_submodules() {
        let output = format!("1 .M SC.. 160000 160000 160000 {HASH} {HASH} vendor/lib\0");
        assert!(parse_status(output.as_bytes()).unwrap().entries[0].submodule);
    }

    #[test]
    fn empty_output_means_a_clean_tree() {
        assert_eq!(parse_status(b"").unwrap(), Status::default());
    }

    #[test]
    fn rejects_paths_outside_the_repository() {
        let mut paths = vec!["../secret", "a/../../b", "/etc/passwd"];
        if cfg!(windows) {
            paths.extend(["C:/Windows", r"\\server\share", r"a\..\..\b"]);
        }
        for path in paths {
            assert!(parse_status(format!("? {path}\0").as_bytes()).is_err(), "{path}");
        }
    }

    #[test]
    fn colons_are_fine_in_linux_file_names() {
        // Linux allows it; on Windows a single letter before a colon is a drive.
        assert!(parse_status(b"? notes:2026.txt\0").is_ok());
        assert_eq!(parse_status(b"? C:file\0").is_ok(), !cfg!(windows));
    }

    #[test]
    fn rejects_broken_records() {
        assert!(parse_status(b"1 .M\0").is_err(), "too short");
        assert!(parse_status(ordinary("XY", "a").as_bytes()).is_err(), "unknown letters");
        let rename = format!("2 R. N... 100644 100644 100644 {HASH} {HASH} R100 new\0");
        assert!(parse_status(rename.as_bytes()).is_err(), "rename without its original path");
    }

    #[test]
    fn stops_at_the_entry_limit() {
        let output = "? f\0".repeat(MAX_STATUS_ENTRIES + 3);
        let status = parse_status(output.as_bytes()).unwrap();
        assert_eq!(status.entries.len(), MAX_STATUS_ENTRIES);
        assert!(status.truncated);
    }
}
