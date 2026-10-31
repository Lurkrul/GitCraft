//! Parsing a unified diff (`git diff-tree -p` output for one file). Pure, so every odd case can be
//! tested without running git. It never fails: a line it doesn't understand is skipped, because a
//! diff that is shown slightly incomplete is better than none.
//!
//! Alongside the readable lines it keeps git's exact bytes ([`PatchSource`]), so a patch of chosen
//! lines can be rebuilt later. That copy is only kept when the diff is complete and every hunk's
//! line counts match its header: a patch is only ever built from output we fully understood.

use std::ops::Range;

use super::{DiffLine, FileDiff, Hunk, HunkRange, LineKind, PatchSource, SourceHunk};

/// Longest line kept, in characters; longer lines (minified files) are cut with "…".
const MAX_LINE_CHARS: usize = 2_000;

pub fn parse_patch(output: &[u8], max_lines: usize) -> FileDiff {
    let mut diff = FileDiff::default();
    let mut kept = 0usize;
    // Where the next old and new lines are, inside the current hunk.
    let (mut old_line, mut new_line) = (0u32, 0u32);
    // The raw-bytes side: where the file header ends, each hunk's line ranges, and whether
    // everything so far was understood.
    let mut header_end = None;
    let mut source_hunks: Vec<SourceHunk> = Vec::new();
    let mut understood = true;

    let mut offset = 0usize;
    // `split_inclusive` keeps each line's '\n', so `range` covers the line's exact bytes.
    for raw in output.split_inclusive(|&byte| byte == b'\n') {
        let range = offset..offset + raw.len();
        offset = range.end;
        let bytes = raw.strip_suffix(b"\n").unwrap_or(raw);
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        let line = String::from_utf8_lossy(bytes);

        if line.starts_with("diff --git ") && !diff.hunks.is_empty() {
            break; // A second file: we asked for one, so stop rather than mix them.
        }
        if let Some(header) = hunk_range(&line) {
            old_line = header.old_start;
            new_line = header.new_start;
            header_end.get_or_insert(range.start);
            diff.hunks.push(Hunk { header: line.to_string(), lines: Vec::new() });
            source_hunks.push(SourceHunk { header, lines: Vec::new() });
            continue;
        }
        let (Some(hunk), Some(source)) = (diff.hunks.last_mut(), source_hunks.last_mut()) else {
            // Before the first hunk: file headers. Only "binary" matters to us.
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                diff.binary = true;
            }
            continue;
        };

        let (kind, body) = match line.chars().next() {
            Some(' ') => (LineKind::Context, line.get(1..).unwrap_or("")),
            Some('+') => (LineKind::Added, line.get(1..).unwrap_or("")),
            Some('-') => (LineKind::Removed, line.get(1..).unwrap_or("")),
            Some('\\') => (LineKind::NoNewline, line.get(1..).unwrap_or("").trim_start()),
            // An empty line at the very end is just the output ending; anywhere else it is
            // something we don't understand, so no patch is built from this diff.
            _ => {
                if range.end < output.len() || !line.is_empty() {
                    understood = false;
                }
                continue;
            }
        };

        if kept >= max_lines {
            diff.truncated = true;
            break;
        }
        kept += 1;

        let (old, new) = match kind {
            LineKind::Context => (Some(old_line), Some(new_line)),
            LineKind::Removed => (Some(old_line), None),
            LineKind::Added => (None, Some(new_line)),
            LineKind::NoNewline => (None, None),
        };
        if old.is_some() {
            old_line = old_line.saturating_add(1);
        }
        if new.is_some() {
            new_line = new_line.saturating_add(1);
        }
        hunk.lines.push(DiffLine { kind, old_line: old, new_line: new, text: clip(body) });
        source.lines.push(range);
    }

    let counts_match =
        diff.hunks.iter().zip(&source_hunks).all(|(hunk, source)| counts_of(hunk) == (source.header.old_count, source.header.new_count));
    if let Some(header_end) = header_end
        && understood
        && counts_match
        && !diff.truncated
        && !diff.binary
    {
        diff.source = Some(PatchSource { bytes: output.to_vec(), header: 0..header_end, hunks: source_hunks });
    }
    diff
}

/// Reads a hunk header's numbers. A count may be left out (`@@ -1 +1 @@`), and then it is 1.
fn hunk_range(line: &str) -> Option<HunkRange> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let parse = |range: &str| -> Option<(u32, u32)> {
        match range.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((range.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = parse(old)?;
    let (new_start, new_count) = parse(new)?;
    Some(HunkRange { old_start, old_count, new_start, new_count })
}

/// How many old-file and new-file lines a parsed hunk holds.
fn counts_of(hunk: &Hunk) -> (u32, u32) {
    let count = |wanted: &[LineKind]| hunk.lines.iter().filter(|line| wanted.contains(&line.kind)).count();
    let old = count(&[LineKind::Context, LineKind::Removed]);
    let new = count(&[LineKind::Context, LineKind::Added]);
    (u32::try_from(old).unwrap_or(u32::MAX), u32::try_from(new).unwrap_or(u32::MAX))
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", text.get(..cut).unwrap_or(text)),
        None => text.to_string(),
    }
}

/// The bytes of one raw line (for tests and the patch builder).
pub(crate) fn raw_line<'a>(bytes: &'a [u8], range: &Range<usize>) -> &'a [u8] {
    bytes.get(range.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,4 +1,5 @@ fn main() {
 use std::io;
-let x = 1;
+let x = 2;
+let y = 3;
 fn main() {}
 // end
@@ -20,2 +21,2 @@
-old tail
+new tail
 last
";

    fn kinds(hunk: &Hunk) -> Vec<LineKind> {
        hunk.lines.iter().map(|l| l.kind).collect()
    }

    #[test]
    fn parses_hunks_and_numbers_lines() {
        let diff = parse_patch(PATCH.as_bytes(), 1000);
        assert!(!diff.binary && !diff.truncated);
        assert_eq!(diff.hunks.len(), 2);

        let first = &diff.hunks[0];
        assert_eq!(first.header, "@@ -1,4 +1,5 @@ fn main() {");
        assert_eq!(first.lines.len(), 6);
        use LineKind::{Added, Context, Removed};
        assert_eq!(kinds(first), [Context, Removed, Added, Added, Context, Context]);
        let numbers: Vec<_> = first.lines.iter().map(|l| (l.old_line, l.new_line)).collect();
        assert_eq!(numbers, [(Some(1), Some(1)), (Some(2), None), (None, Some(2)), (None, Some(3)), (Some(3), Some(4)), (Some(4), Some(5))]);
        assert_eq!(first.lines[1].text, "let x = 1;");

        let second = &diff.hunks[1];
        assert_eq!(second.lines[0].old_line, Some(20));
        assert_eq!(second.lines[1].new_line, Some(21));
    }

    #[test]
    fn keeps_the_exact_bytes_of_every_line() {
        let diff = parse_patch(PATCH.as_bytes(), 1000);
        let source = diff.source.as_ref().expect("a complete diff keeps its source");
        let header = std::str::from_utf8(&source.bytes[source.header.clone()]).unwrap();
        assert!(header.starts_with("diff --git ") && header.ends_with("+++ b/src/main.rs\n"), "{header:?}");
        assert_eq!(source.hunks.len(), 2);
        assert_eq!(raw_line(&source.bytes, &source.hunks[0].lines[1]), b"-let x = 1;\n");
        assert_eq!(source.hunks[1].header, HunkRange { old_start: 20, old_count: 2, new_start: 21, new_count: 2 });
    }

    #[test]
    fn hunk_headers_with_and_without_counts() {
        assert_eq!(hunk_range("@@ -1 +1 @@"), Some(HunkRange { old_start: 1, old_count: 1, new_start: 1, new_count: 1 }));
        assert_eq!(hunk_range("@@ -0,0 +1,3 @@ x"), Some(HunkRange { old_start: 0, old_count: 0, new_start: 1, new_count: 3 }));
        assert_eq!(hunk_range("@@ garbage @@"), None);
        assert_eq!(hunk_range("@@ -1,x +1 @@"), None);
        assert_eq!(hunk_range("not a header"), None);
    }

    #[test]
    fn a_new_file_starts_at_line_one() {
        let patch = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        let numbers: Vec<_> = diff.hunks[0].lines.iter().map(|l| l.new_line).collect();
        assert_eq!(numbers, [Some(1), Some(2)]);
        assert!(diff.source.is_some());
    }

    #[test]
    fn binary_files_have_no_hunks() {
        let patch = "diff --git a/i.png b/i.png\nindex 1..2 100644\nBinary files a/i.png and b/i.png differ\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        assert!(diff.binary);
        assert!(diff.hunks.is_empty());
        assert!(diff.source.is_none());
    }

    #[test]
    fn no_newline_markers_and_crlf() {
        let patch = "@@ -1 +1 @@\r\n-old\r\n\\ No newline at end of file\r\n+new\r\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        let lines = &diff.hunks[0].lines;
        assert_eq!(lines[0].text, "old", "carriage return removed");
        assert_eq!(lines[1].kind, LineKind::NoNewline);
        assert_eq!(lines[1].text, "No newline at end of file");
        assert_eq!(lines[2].new_line, Some(1), "the marker doesn't use up a line number");
        let source = diff.source.as_ref().unwrap();
        assert_eq!(raw_line(&source.bytes, &source.hunks[0].lines[0]), b"-old\r\n", "the raw copy keeps the carriage return");
    }

    #[test]
    fn stops_at_the_line_limit() {
        let mut patch = String::from("@@ -1,100 +1,100 @@\n");
        for i in 0..100 {
            patch.push_str(&format!(" line {i}\n"));
        }
        let diff = parse_patch(patch.as_bytes(), 10);
        assert!(diff.truncated);
        assert_eq!(diff.hunks[0].lines.len(), 10);
        assert!(diff.source.is_none(), "no patches from a cut-short diff");
    }

    #[test]
    fn counts_that_disagree_with_the_header_mean_no_source() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-a\n+b\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        assert_eq!(diff.hunks[0].lines.len(), 2, "still shown");
        assert!(diff.source.is_none());
    }

    #[test]
    fn a_stray_line_inside_the_output_means_no_source() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-a\n\n+b\n c\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        assert!(diff.source.is_none());
    }

    #[test]
    fn long_lines_are_cut() {
        let patch = format!("@@ -1 +1 @@\n+{}\n", "x".repeat(MAX_LINE_CHARS + 50));
        let diff = parse_patch(patch.as_bytes(), 10);
        let text = &diff.hunks[0].lines[0].text;
        assert_eq!(text.chars().count(), MAX_LINE_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn only_the_first_file_is_used() {
        let patch = format!("{PATCH}diff --git a/other b/other\n@@ -1 +1 @@\n-a\n+b\n");
        let diff = parse_patch(patch.as_bytes(), 1000);
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.source.as_ref().map(|s| s.hunks.len()), Some(2));
    }

    #[test]
    fn garbage_and_empty_input_never_fail() {
        assert_eq!(parse_patch(b"", 10), FileDiff::default());
        assert_eq!(parse_patch(b"\xff\xfe random bytes \n+ not in a hunk\n", 10), FileDiff::default());
        let diff = parse_patch("@@ -1 +1 @@\n+caf\u{e9} \u{1f980}\n".as_bytes(), 10);
        assert_eq!(diff.hunks[0].lines[0].text, "café 🦀");
    }

    #[test]
    fn invalid_utf8_keeps_its_raw_bytes() {
        let patch = b"@@ -1 +1 @@\n-\xff\n+ok\n";
        let diff = parse_patch(patch, 10);
        let source = diff.source.as_ref().unwrap();
        assert_eq!(raw_line(&source.bytes, &source.hunks[0].lines[0]), b"-\xff\n");
    }
}
