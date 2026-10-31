//! Parsing `git for-each-ref` output in the layout `REFS_FORMAT` asks for. Pure, so every odd case
//! can be tested without running git.

use super::{Ref, RefKind};
use crate::error::{GitError, Result};
use crate::hash::parse_hash;

/// Longest ref name accepted. Git has no fixed limit, but a longer name is a damaged repository.
const MAX_REF_NAME: usize = 4096;

/// Parses every line. Symbolic refs (such as `refs/remotes/origin/HEAD`, which only says which
/// branch the remote considers its default) are skipped: they aren't branches of their own.
pub fn parse_refs(output: &[u8]) -> Result<Vec<Ref>> {
    let mut refs = Vec::new();
    for (index, line) in output.split(|byte| *byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if let Some(parsed) = parse_line(index, line)? {
            refs.push(parsed);
        }
    }
    Ok(refs)
}

fn parse_line(index: usize, line: &[u8]) -> Result<Option<Ref>> {
    let what = || format!("ref {} in git for-each-ref output", index + 1);
    let fields: Vec<&[u8]> = line.split(|byte| *byte == 0).collect();
    let &[name, object, peeled, symref] = fields.as_slice() else {
        return Err(malformed(&what(), format!("expected 4 fields, found {}", fields.len())));
    };
    if !symref.is_empty() {
        return Ok(None);
    }
    let name = parse_ref_name(name).map_err(|_| malformed(&what(), format!("invalid ref name {:?}", String::from_utf8_lossy(name))))?;
    let Some(kind) = kind_of(&name) else {
        // Only the three namespaces were asked for, so anything else is unexpected output.
        return Err(malformed(&what(), format!("unexpected ref {name:?}")));
    };
    // For an annotated tag, `peeled` is the commit it points at; for everything else it's empty.
    let target_bytes = if peeled.is_empty() { object } else { peeled };
    let target = parse_hash(target_bytes).ok_or_else(|| malformed(&what(), format!("invalid hash {:?}", String::from_utf8_lossy(target_bytes))))?;
    Ok(Some(Ref { name, kind, target }))
}

fn kind_of(name: &str) -> Option<RefKind> {
    if name.starts_with("refs/heads/") {
        Some(RefKind::Branch)
    } else if name.starts_with("refs/remotes/") {
        Some(RefKind::RemoteBranch)
    } else if name.starts_with("refs/tags/") {
        Some(RefKind::Tag)
    } else {
        None
    }
}

/// A full ref name from git: UTF-8 (git allows other bytes, but such names can't be shown or
/// typed and are vanishingly rare), starting with `refs/`, no control characters, sane length.
/// Starting with `refs/` also guarantees it can never be mistaken for a command-line option.
pub fn parse_ref_name(bytes: &[u8]) -> Result<String> {
    let invalid = || malformed("ref name", format!("{:?} is not a usable ref name", String::from_utf8_lossy(bytes)));
    let name = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if name.len() > MAX_REF_NAME || !name.starts_with("refs/") || name.len() == "refs/".len() || name.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(name.to_string())
}

fn malformed(what: &str, detail: String) -> GitError {
    GitError::Malformed { what: what.to_string(), detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn line(name: &str, object: &str, peeled: &str, symref: &str) -> String {
        format!("{name}\0{object}\0{peeled}\0{symref}\n")
    }

    #[test]
    fn parses_each_kind() {
        let out = [
            line("refs/heads/main", A, "", ""),
            line("refs/remotes/origin/main", A, "", ""),
            line("refs/tags/light", A, "", ""),
            line("refs/tags/v1", B, A, ""),
        ]
        .concat();
        let refs = parse_refs(out.as_bytes()).unwrap();
        let kinds: Vec<_> = refs.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RefKind::Branch, RefKind::RemoteBranch, RefKind::Tag, RefKind::Tag]);
        // The annotated tag points at the commit, not at the tag object.
        assert!(refs.iter().all(|r| r.target == A));
    }

    #[test]
    fn skips_symbolic_refs() {
        let out = [line("refs/remotes/origin/HEAD", A, "", "refs/remotes/origin/main"), line("refs/remotes/origin/main", A, "", "")].concat();
        let refs = parse_refs(out.as_bytes()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "refs/remotes/origin/main");
    }

    #[test]
    fn keeps_unicode_names_and_accepts_crlf() {
        let out = line("refs/heads/función/ñ", A, "", "").replace('\n', "\r\n");
        assert_eq!(parse_refs(out.as_bytes()).unwrap()[0].name, "refs/heads/función/ñ");
    }

    #[test]
    fn empty_output_is_no_refs() {
        assert!(parse_refs(b"").unwrap().is_empty());
        assert!(parse_refs(b"\n").unwrap().is_empty());
    }

    #[test]
    fn rejects_damaged_lines() {
        let bad = |out: String| matches!(parse_refs(out.as_bytes()), Err(GitError::Malformed { .. }));
        assert!(bad("refs/heads/main\0".to_string() + A + "\n"), "too few fields");
        assert!(bad(line("refs/heads/main", "not-a-hash", "", "")));
        assert!(bad(line("refs/notes/commits", A, "", "")), "namespace not asked for");
        assert!(bad(line("heads/main", A, "", "")), "not a full ref name");
        assert!(bad(line("refs/heads/bad\u{7}name", A, "", "")), "control character");
        assert!(bad(line(&format!("refs/heads/{}", "x".repeat(MAX_REF_NAME)), A, "", "")), "too long");
    }

    #[test]
    fn ref_names_must_look_like_refs() {
        assert!(parse_ref_name(b"refs/heads/main").is_ok());
        assert!(parse_ref_name(b"refs/").is_err());
        assert!(parse_ref_name(b"--all").is_err());
        assert!(parse_ref_name(b"refs/heads/\xff").is_err());
    }
}
