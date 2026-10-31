//! Parsing `git show` and `git diff-tree -z` output. Pure, so every odd case can be tested
//! without running git.

use super::{ChangedFile, CommitDetails, FileStatus};
use crate::error::{GitError, Result};
use crate::hash::parse_hash;

/// Fields in `SHOW_FORMAT`, the message included.
const SHOW_FIELDS: usize = 9;

pub fn parse_show(output: &[u8]) -> Result<CommitDetails> {
    let what = "git show output";
    // `splitn` stops after the 8th NUL, so NUL-free messages arrive whole in the last field.
    let fields: Vec<&[u8]> = output.splitn(SHOW_FIELDS, |byte| *byte == 0).collect();
    let &[id, parents, author_name, author_email, author_time, committer_name, committer_email, committer_time, message] = fields.as_slice() else {
        return Err(malformed(what, format!("expected {SHOW_FIELDS} fields, found {}", fields.len())));
    };
    let hash = |bytes: &[u8]| parse_hash(bytes).ok_or_else(|| malformed(what, format!("invalid hash {:?}", String::from_utf8_lossy(bytes))));
    let time = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|t| t.parse::<i64>().ok())
            .ok_or_else(|| malformed(what, format!("invalid time {:?}", String::from_utf8_lossy(bytes))))
    };
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();

    Ok(CommitDetails {
        id: hash(id)?,
        parents: parents.split(|b| *b == b' ').filter(|p| !p.is_empty()).map(hash).collect::<Result<_>>()?,
        author_name: text(author_name),
        author_email: text(author_email),
        author_time: time(author_time)?,
        committer_name: text(committer_name),
        committer_email: text(committer_email),
        committer_time: time(committer_time)?,
        // Git ends the message with a newline, and the format adds one more.
        message: text(message).trim_end().to_string(),
        files: Vec::new(),
    })
}

/// One `--name-status` entry: the status and the path(s).
#[derive(Debug, PartialEq, Eq)]
pub struct StatusEntry {
    pub status: FileStatus,
    pub path: String,
    pub old_path: Option<String>,
}

/// `X\0path\0`, or for renames and copies `R<score>\0old\0new\0`.
pub fn parse_name_status(output: &[u8]) -> Result<Vec<StatusEntry>> {
    let what = "git diff-tree --name-status output";
    let mut fields = output.split(|byte| *byte == 0).filter(|f| !f.is_empty());
    let mut entries = Vec::new();
    while let Some(code) = fields.next() {
        let status = match code.first() {
            Some(b'A') => FileStatus::Added,
            Some(b'M') => FileStatus::Modified,
            Some(b'D') => FileStatus::Deleted,
            Some(b'R') => FileStatus::Renamed,
            Some(b'C') => FileStatus::Copied,
            Some(b'T') => FileStatus::TypeChanged,
            Some(b'U' | b'X') => FileStatus::Other,
            _ => return Err(malformed(what, format!("unknown status {:?}", String::from_utf8_lossy(code)))),
        };
        let mut next_path = || fields.next().map(path_text).ok_or_else(|| malformed(what, "a status without a path".to_string()));
        let entry = if matches!(status, FileStatus::Renamed | FileStatus::Copied) {
            let old_path = next_path()?;
            StatusEntry { status, path: next_path()?, old_path: Some(old_path) }
        } else {
            StatusEntry { status, path: next_path()?, old_path: None }
        };
        entries.push(entry);
    }
    Ok(entries)
}

/// One `--numstat` entry: lines added and removed (`None` for binary), and the new path.
#[derive(Debug, PartialEq, Eq)]
pub struct CountEntry {
    pub lines: Option<(u64, u64)>,
    pub path: String,
}

/// `added\tremoved\tpath\0`, or for renames `added\tremoved\t\0old\0new\0`; binary files show
/// `-` for both counts.
pub fn parse_numstat(output: &[u8]) -> Result<Vec<CountEntry>> {
    let what = "git diff-tree --numstat output";
    let mut fields = output.split(|byte| *byte == 0);
    let mut entries = Vec::new();
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue; // the final NUL
        }
        let mut parts = record.splitn(3, |byte| *byte == b'\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(malformed(what, format!("unexpected entry {:?}", String::from_utf8_lossy(record))));
        };
        let lines = match (added, removed) {
            (b"-", b"-") => None,
            _ => Some((
                count(added).ok_or_else(|| malformed(what, "invalid count".into()))?,
                count(removed).ok_or_else(|| malformed(what, "invalid count".into()))?,
            )),
        };
        // An empty path means a rename: the old and new paths follow as their own fields.
        let path = if path.is_empty() {
            let _old = fields.next();
            fields.next().filter(|p| !p.is_empty()).map(path_text).ok_or_else(|| malformed(what, "a rename without its paths".to_string()))?
        } else {
            path_text(path)
        };
        entries.push(CountEntry { lines, path });
    }
    Ok(entries)
}

/// Puts each file's line counts with its status. Both lists come from the same diff in the same
/// order; a count is only used if its path matches, so a mismatch loses counts, never mixes them.
pub fn join_files(statuses: Vec<StatusEntry>, counts: Vec<CountEntry>) -> Vec<ChangedFile> {
    let mut counts = counts.into_iter();
    statuses
        .into_iter()
        .map(|entry| {
            let lines = counts.next().filter(|c| c.path == entry.path).and_then(|c| c.lines);
            ChangedFile { path: entry.path, old_path: entry.old_path, status: entry.status, lines }
        })
        .collect()
}

fn count(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

fn path_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn malformed(what: &str, detail: String) -> GitError {
    GitError::Malformed { what: what.to_string(), detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn show(parents: &str, message: &str) -> Vec<u8> {
        format!("{A}\0{parents}\0Ada\0ada@example.com\01700000000\0Grace\0grace@example.com\01700000100\0{message}\n").into_bytes()
    }

    #[test]
    fn parses_a_commit_with_a_multi_line_message() {
        let details = parse_show(&show(B, "Summary line\n\nBody line one\nBody line two\n")).unwrap();
        assert_eq!(details.id, A);
        assert_eq!(details.parents, [B]);
        assert_eq!((details.author_name.as_str(), details.author_time), ("Ada", 1_700_000_000));
        assert_eq!((details.committer_name.as_str(), details.committer_time), ("Grace", 1_700_000_100));
        assert_eq!(details.message, "Summary line\n\nBody line one\nBody line two");
    }

    #[test]
    fn root_commits_and_empty_messages() {
        let details = parse_show(&show("", "")).unwrap();
        assert!(details.parents.is_empty());
        assert_eq!(details.message, "");
    }

    #[test]
    fn rejects_damaged_show_output() {
        assert!(parse_show(b"short\0output").is_err());
        assert!(parse_show(&show("zzz", "x")).is_err(), "bad parent hash");
        let bad_time = format!("{A}\0\0a\0b\0soon\0c\0d\01\0msg").into_bytes();
        assert!(parse_show(&bad_time).is_err());
    }

    #[test]
    fn parses_name_status_including_renames() {
        let out = b"M\0src/lib.rs\0R087\0old name.rs\0new name.rs\0A\0docs/\xc3\xa9t\xc3\xa9.md\0D\0gone.txt\0";
        let entries = parse_name_status(out).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0], StatusEntry { status: FileStatus::Modified, path: "src/lib.rs".into(), old_path: None });
        assert_eq!(entries[1], StatusEntry { status: FileStatus::Renamed, path: "new name.rs".into(), old_path: Some("old name.rs".into()) });
        assert_eq!(entries[2].path, "docs/été.md");
        assert_eq!(entries[3].status, FileStatus::Deleted);
    }

    #[test]
    fn rejects_unknown_statuses_and_missing_paths() {
        assert!(parse_name_status(b"Z\0file\0").is_err());
        assert!(parse_name_status(b"M\0").is_err());
        assert!(parse_name_status(b"R100\0only-old\0").is_err());
    }

    #[test]
    fn parses_numstat_including_binary_and_renames() {
        let out = b"3\t1\tsrc/lib.rs\0-\t-\timage.png\x000\t2\t\0old.rs\0new.rs\0";
        let entries = parse_numstat(out).unwrap();
        assert_eq!(entries[0], CountEntry { lines: Some((3, 1)), path: "src/lib.rs".into() });
        assert_eq!(entries[1], CountEntry { lines: None, path: "image.png".into() });
        assert_eq!(entries[2], CountEntry { lines: Some((0, 2)), path: "new.rs".into() });
    }

    #[test]
    fn rejects_damaged_numstat() {
        assert!(parse_numstat(b"3\tsrc\0").is_err());
        assert!(parse_numstat(b"x\t1\tfile\0").is_err());
        assert!(parse_numstat(b"1\t1\t\0only-old\0").is_err());
    }

    #[test]
    fn joins_counts_by_path() {
        let statuses = vec![
            StatusEntry { status: FileStatus::Modified, path: "a".into(), old_path: None },
            StatusEntry { status: FileStatus::Added, path: "b".into(), old_path: None },
        ];
        let counts = vec![CountEntry { lines: Some((1, 2)), path: "a".into() }, CountEntry { lines: Some((5, 0)), path: "not-b".into() }];
        let files = join_files(statuses, counts);
        assert_eq!(files[0].lines, Some((1, 2)));
        assert_eq!(files[1].lines, None, "a mismatched path loses its count rather than taking another file's");
    }

    #[test]
    fn empty_diffs_are_fine() {
        assert!(parse_name_status(b"").unwrap().is_empty());
        assert!(parse_numstat(b"").unwrap().is_empty());
    }
}
