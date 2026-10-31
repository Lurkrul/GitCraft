//! Parsing a unified diff (`git diff-tree -p` output for one file). Pure, so every odd case can be
//! tested without running git. It never fails: a line it doesn't understand is skipped, because a
//! diff that is shown slightly incomplete is better than none.

use super::{DiffLine, FileDiff, Hunk, LineKind};

/// Longest line kept, in characters; longer lines (minified files) are cut with "…".
const MAX_LINE_CHARS: usize = 2_000;

pub fn parse_patch(output: &[u8], max_lines: usize) -> FileDiff {
    let text = String::from_utf8_lossy(output);
    let mut diff = FileDiff::default();
    let mut kept = 0usize;
    // Where the next old and new lines are, inside the current hunk.
    let (mut old_line, mut new_line) = (0u32, 0u32);

    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if line.starts_with("diff --git ") && !diff.hunks.is_empty() {
            break; // A second file: we asked for one, so stop rather than mix them.
        }
        if let Some((old_start, new_start)) = hunk_start(line) {
            old_line = old_start;
            new_line = new_start;
            diff.hunks.push(Hunk { header: line.to_string(), lines: Vec::new() });
            continue;
        }
        let Some(hunk) = diff.hunks.last_mut() else {
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
            // An empty line is the end of the output (or a context line whose space was lost).
            _ => continue,
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
    }
    diff
}

/// The old and new starting line numbers of a hunk header: `@@ -12,5 +12,7 @@ ...` gives
/// (12, 12). A count may be left out (`@@ -1 +1 @@`).
fn hunk_start(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let start = |range: &str| range.split(',').next()?.parse::<u32>().ok();
    Some((start(old)?, start(new)?))
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", text.get(..cut).unwrap_or(text)),
        None => text.to_string(),
    }
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
    fn hunk_headers_without_counts() {
        assert_eq!(hunk_start("@@ -1 +1 @@"), Some((1, 1)));
        assert_eq!(hunk_start("@@ -0,0 +1,3 @@"), Some((0, 1)));
        assert_eq!(hunk_start("@@ garbage @@"), None);
        assert_eq!(hunk_start("not a header"), None);
    }

    #[test]
    fn a_new_file_starts_at_line_one() {
        let patch = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        let numbers: Vec<_> = diff.hunks[0].lines.iter().map(|l| l.new_line).collect();
        assert_eq!(numbers, [Some(1), Some(2)]);
    }

    #[test]
    fn binary_files_have_no_hunks() {
        let patch = "diff --git a/i.png b/i.png\nindex 1..2 100644\nBinary files a/i.png and b/i.png differ\n";
        let diff = parse_patch(patch.as_bytes(), 100);
        assert!(diff.binary);
        assert!(diff.hunks.is_empty());
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
        assert_eq!(parse_patch(patch.as_bytes(), 1000).hunks.len(), 2);
    }

    #[test]
    fn garbage_and_empty_input_never_fail() {
        assert_eq!(parse_patch(b"", 10), FileDiff::default());
        assert_eq!(parse_patch(b"\xff\xfe random bytes \n+ not in a hunk\n", 10), FileDiff::default());
        let diff = parse_patch("@@ -1 +1 @@\n+caf\u{e9} \u{1f980}\n".as_bytes(), 10);
        assert_eq!(diff.hunks[0].lines[0].text, "café 🦀");
    }
}
