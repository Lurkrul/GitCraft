//! Parsing `git log -z` output in the layout `LOG_FORMAT` asks for. A pure function, so every
//! odd case can be tested without running git.

use super::Commit;
use crate::error::{GitError, Result};
use crate::hash::parse_hash;

/// Fields per commit in `LOG_FORMAT`.
const FIELDS: usize = 6;

/// Parses the whole output. The output is one flat run of NUL-terminated fields, six per commit.
/// Anything that doesn't fit is an error naming the commit, never a panic.
pub fn parse_log(output: &[u8]) -> Result<Vec<Commit>> {
    // Every field ends with a NUL, so drop the last one rather than treating it as a separator
    // (otherwise `split` would yield an empty field at the end).
    let body = output.strip_suffix(b"\0").unwrap_or(output);
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let fields: Vec<&[u8]> = body.split(|byte| *byte == 0).collect();
    // `as_chunks` splits into arrays of exactly FIELDS fields, plus whatever is left over.
    let (records, leftover) = fields.as_chunks::<FIELDS>();
    if !leftover.is_empty() {
        return Err(malformed(
            "git log output",
            format!("{} fields is not a whole number of commits ({FIELDS} fields each); the output may be cut short", fields.len()),
        ));
    }
    // `collect` into `Result<Vec<_>>` stops at the first error and returns it.
    records.iter().enumerate().map(|(index, record)| parse_commit(index, record)).collect()
}

fn parse_commit(index: usize, record: &[&[u8]; FIELDS]) -> Result<Commit> {
    let what = || format!("commit {} in git log output", index + 1);
    // The record is a fixed-size array, so this pattern always matches: it names all six fields.
    let &[id, parents, time, name, email, summary] = record;

    let id = parse_hash(id).ok_or_else(|| malformed(&what(), format!("invalid commit hash {:?}", String::from_utf8_lossy(id))))?;
    let parents = parents
        .split(|byte| *byte == b' ')
        .filter(|parent| !parent.is_empty())
        .map(|parent| parse_hash(parent).ok_or_else(|| malformed(&what(), format!("invalid parent hash {:?}", String::from_utf8_lossy(parent)))))
        .collect::<Result<Vec<_>>>()?;
    let author_time = std::str::from_utf8(time)
        .ok()
        .and_then(|text| text.parse::<i64>().ok())
        .ok_or_else(|| malformed(&what(), format!("invalid author time {:?}", String::from_utf8_lossy(time))))?;

    Ok(Commit {
        id,
        parents,
        author_time,
        // Names and messages are usually UTF-8 but nothing guarantees it; invalid bytes become
        // the replacement character instead of failing the whole history.
        author_name: String::from_utf8_lossy(name).into_owned(),
        author_email: String::from_utf8_lossy(email).into_owned(),
        summary: String::from_utf8_lossy(summary).into_owned(),
    })
}

fn malformed(what: &str, detail: String) -> GitError {
    GitError::Malformed { what: what.to_string(), detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    /// Builds output exactly as git would: each field followed by a NUL.
    fn record(fields: [&[u8]; FIELDS]) -> Vec<u8> {
        let mut out = Vec::new();
        for field in fields {
            out.extend_from_slice(field);
            out.push(0);
        }
        out
    }

    fn simple(id: &str, parents: &str, summary: &str) -> Vec<u8> {
        record([id.as_bytes(), parents.as_bytes(), b"1700000000", b"Ada Lovelace", b"ada@example.com", summary.as_bytes()])
    }

    fn error_detail(output: &[u8]) -> String {
        match parse_log(output) {
            Err(GitError::Malformed { what, detail }) => format!("{what}: {detail}"),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn empty_output_is_no_commits() {
        assert_eq!(parse_log(b"").unwrap(), Vec::new());
    }

    #[test]
    fn parses_a_normal_history() {
        let mut out = simple(A, B, "Second");
        out.extend(simple(B, "", "First"));
        let commits = parse_log(&out).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, A);
        assert_eq!(commits[0].parents, vec![B.to_string()]);
        assert_eq!(commits[0].author_time, 1_700_000_000);
        assert_eq!(commits[0].author_name, "Ada Lovelace");
        assert_eq!(commits[0].author_email, "ada@example.com");
        assert_eq!(commits[0].summary, "Second");
        assert!(commits[1].parents.is_empty(), "root commit has no parents");
    }

    #[test]
    fn parses_a_merge() {
        let commits = parse_log(&simple(A, &format!("{B} {C}"), "Merge")).unwrap();
        assert_eq!(commits[0].parents, vec![B.to_string(), C.to_string()]);
    }

    #[test]
    fn accepts_output_without_the_final_nul() {
        let mut out = simple(A, "", "Only");
        out.pop();
        assert_eq!(parse_log(&out).unwrap().len(), 1);
    }

    #[test]
    fn keeps_empty_subjects_and_odd_characters() {
        let mut out = simple(A, "", "");
        out.extend(simple(B, "", "Tab\there, unit\x1fseparator, emoji 🦀"));
        let commits = parse_log(&out).unwrap();
        assert_eq!(commits[0].summary, "");
        assert_eq!(commits[1].summary, "Tab\there, unit\x1fseparator, emoji 🦀");
    }

    #[test]
    fn invalid_utf8_in_names_is_replaced_not_rejected() {
        let out = record([A.as_bytes(), b"", b"0", b"Ren\xe9", b"r@example.com", b"Caf\xe9"]);
        let commits = parse_log(&out).unwrap();
        assert_eq!(commits[0].author_name, "Ren\u{fffd}");
        assert_eq!(commits[0].summary, "Caf\u{fffd}");
    }

    #[test]
    fn accepts_sha256_hashes() {
        let long = "0123456789abcdef".repeat(4);
        assert_eq!(parse_log(&simple(&long, &long, "x")).unwrap()[0].id, long);
    }

    #[test]
    fn negative_times_are_allowed() {
        // Commits dated before 1970 exist (imported history).
        let out = record([A.as_bytes(), b"", b"-86400", b"n", b"e", b"s"]);
        assert_eq!(parse_log(&out).unwrap()[0].author_time, -86_400);
    }

    #[test]
    fn cut_short_output_is_an_error() {
        let mut out = simple(A, B, "Second");
        out.extend(b"bbbb\0");
        assert!(error_detail(&out).contains("not a whole number of commits"));
    }

    #[test]
    fn bad_hashes_are_errors_naming_the_commit() {
        let mut out = simple(A, "", "fine");
        out.extend(simple("not-a-hash", "", "bad"));
        let detail = error_detail(&out);
        assert!(detail.contains("commit 2"), "{detail}");
        assert!(detail.contains("invalid commit hash"), "{detail}");

        assert!(error_detail(&simple(&A.to_uppercase(), "", "x")).contains("invalid commit hash"));
        assert!(error_detail(&simple(&A[..39], "", "x")).contains("invalid commit hash"));
        assert!(error_detail(&simple(A, "zzz", "x")).contains("invalid parent hash"));
    }

    #[test]
    fn bad_times_are_errors() {
        let out = record([A.as_bytes(), b"", b"yesterday", b"n", b"e", b"s"]);
        assert!(error_detail(&out).contains("invalid author time"));
        let out = record([A.as_bytes(), b"", b"99999999999999999999999", b"n", b"e", b"s"]);
        assert!(error_detail(&out).contains("invalid author time"));
    }
}
