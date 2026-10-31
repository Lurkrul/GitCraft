//! Pre-staging: GitCraft's own record of which changes are left out of the next commit, kept in
//! the repository's git folder (`.git/gitcraft/prestage`) so it survives restarts. It never
//! touches git's index; a terminal user's `git commit` doesn't see it.
//!
//! Only the exceptions are stored: files unticked as a whole, and single unticked lines. Anything
//! changed that isn't recorded starts ticked.
//!
//! The file is plain text, one record per line, fields separated by tabs (shown here as `→`):
//!
//! ```text
//! gitcraft-prestage 1
//! file→src/main.rs
//! line→src/lib.rs→r→12→0→9f86d081884c7d65
//! ```
//!
//! `file` leaves a whole file out. `line` leaves out one line: `r` removed or `a` added, its anchor
//! and offset (see [`LineKey`]), and a hash of its text. In paths, `%`, tab, newline and carriage
//! return are written as `%25`, `%09`, `%0A` and `%0D`. The file is untrusted input: it is
//! size-checked and every record validated; a damaged file is moved aside, never half-read.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

/// Where the file lives, relative to the repository's git folder.
pub const FILE_NAME: &str = "gitcraft/prestage";
const HEADER: &str = "gitcraft-prestage 1";
/// Larger files are refused as damaged.
const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// More records than this are refused as damaged.
const MAX_RECORDS: usize = 1_000_000;

/// A changed line's identity, stable across edits elsewhere in the file:
/// - a removed line by its line number in the old file (HEAD's version, which doesn't move);
/// - an added line by the old-file line it is inserted before, and its place in that run.
///
/// Plus a hash of its text, so a different line at the same spot is told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineKey {
    pub removed: bool,
    /// Removed: its old line number. Added: the old line it comes before.
    pub anchor: u32,
    /// Added: its place in the run of added lines at that spot (0 for the first).
    pub offset: u32,
    /// [`text_hash`] of the line's text.
    pub hash: u64,
}

/// FNV-1a, 64 bits: a fixed, simple hash (unlike std's, it never changes between Rust versions,
/// so saved hashes stay valid).
pub fn text_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// What is left out of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// The whole file.
    File,
    /// These lines.
    Lines(BTreeSet<LineKey>),
}

/// Everything left out, by path (relative to the repository's top folder).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prestage {
    pub files: BTreeMap<String, Saved>,
}

impl Prestage {
    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut out = format!("{HEADER}\n");
        for (path, saved) in &self.files {
            let path = escape(path);
            match saved {
                Saved::File => out.push_str(&format!("file\t{path}\n")),
                Saved::Lines(keys) => {
                    for key in keys {
                        let kind = if key.removed { 'r' } else { 'a' };
                        out.push_str(&format!("line\t{path}\t{kind}\t{}\t{}\t{:016x}\n", key.anchor, key.offset, key.hash));
                    }
                }
            }
        }
        out
    }

    /// Reads the file's text. Any record that doesn't fit the format makes the whole file
    /// damaged: better to start fresh than to commit something on a half-read record.
    pub fn parse(text: &str) -> Result<Prestage, String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim_end) != Some(HEADER) {
            return Err("it doesn't start with the expected header (written by another version?)".into());
        }
        let mut prestage = Prestage::default();
        for (index, line) in lines.enumerate() {
            if index >= MAX_RECORDS {
                return Err(format!("it has more than {MAX_RECORDS} records"));
            }
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let number = index + 2;
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                ["file", path] => {
                    prestage.files.insert(unescape(path).ok_or(format!("line {number}: bad path"))?, Saved::File);
                }
                ["line", path, kind, anchor, offset, hash] => {
                    let path = unescape(path).ok_or(format!("line {number}: bad path"))?;
                    let removed = match *kind {
                        "r" => true,
                        "a" => false,
                        _ => return Err(format!("line {number}: unknown line kind")),
                    };
                    let number_field = |text: &str| text.parse::<u32>().map_err(|_| format!("line {number}: bad number"));
                    let hash = (hash.len() == 16).then(|| u64::from_str_radix(hash, 16).ok()).flatten().ok_or(format!("line {number}: bad hash"))?;
                    let key = LineKey { removed, anchor: number_field(anchor)?, offset: number_field(offset)?, hash };
                    match prestage.files.entry(path).or_insert_with(|| Saved::Lines(BTreeSet::new())) {
                        Saved::Lines(keys) => {
                            keys.insert(key);
                        }
                        // A whole file left out already covers its lines.
                        Saved::File => {}
                    }
                }
                _ => return Err(format!("line {number}: not a record GitCraft knows")),
            }
        }
        Ok(prestage)
    }
}

/// Paths can hold any character but NUL; the few that would break the format are escaped.
fn escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' => out.push_str("%25"),
            '\t' => out.push_str("%09"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            other => out.push(other),
        }
    }
    out
}

/// The reverse of [`escape`]; `None` for an escape it doesn't know, or an empty path.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('%') {
        out.push_str(rest.get(..at)?);
        let code = rest.get(at + 1..at + 3)?;
        out.push(match code {
            "25" => '%',
            "09" => '\t',
            "0A" => '\n',
            "0D" => '\r',
            _ => return None,
        });
        rest = rest.get(at + 3..)?;
    }
    out.push_str(rest);
    (!out.is_empty()).then_some(out)
}

/// Reads the pre-stage file. A missing file is an empty pre-stage. A damaged one (too big, not
/// text, or not in the format) is moved aside to `prestage.damaged` so it is kept for a look but
/// no longer read, and the error says so.
pub fn load(path: &Path) -> Result<Prestage, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Prestage::default()),
        Err(error) => return Err(format!("Could not read GitCraft's saved ticks ({}): {error}", path.display())),
    };
    let problem = if metadata.len() > MAX_BYTES {
        format!("it is larger than {} MiB", MAX_BYTES / (1024 * 1024))
    } else {
        let bytes = std::fs::read(path).map_err(|error| format!("Could not read GitCraft's saved ticks ({}): {error}", path.display()))?;
        match String::from_utf8(bytes).map_err(|_| "it isn't text".to_string()).and_then(|text| Prestage::parse(&text)) {
            Ok(prestage) => return Ok(prestage),
            Err(problem) => problem,
        }
    };
    let aside = damaged_path(path);
    let kept = match std::fs::rename(path, &aside) {
        Ok(()) => format!("it was moved to {}", aside.display()),
        Err(error) => format!("it could not be moved aside ({error}) and will be replaced"),
    };
    Err(format!("GitCraft's saved ticks were damaged ({problem}); every change starts ticked again, and {kept}."))
}

/// Where a damaged file is moved.
fn damaged_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    name.push(".damaged");
    path.with_file_name(name)
}

/// Writes the pre-stage file in one step: a temporary file next to it, then a rename over the old
/// one, so a crash mid-save leaves either the old file or the new one, never half of one. An
/// empty pre-stage removes the file instead.
pub fn save(path: &Path, prestage: &Prestage) -> io::Result<()> {
    if prestage.files.is_empty() {
        return match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, prestage.to_text())?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(removed: bool, anchor: u32, offset: u32, text: &str) -> LineKey {
        LineKey { removed, anchor, offset, hash: text_hash(text) }
    }

    fn sample() -> Prestage {
        let mut files = BTreeMap::new();
        files.insert("debug.log".to_string(), Saved::File);
        files.insert("odd\tname%.txt".to_string(), Saved::Lines(BTreeSet::from([key(true, 12, 0, "old"), key(false, 13, 1, "new")])));
        Prestage { files }
    }

    #[test]
    fn hashes_are_fixed() {
        // FNV-1a's published test values: they must never change, or saved files stop matching.
        assert_eq!(text_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(text_hash("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn text_round_trips() {
        let text = sample().to_text();
        assert!(text.starts_with("gitcraft-prestage 1\n"));
        assert!(text.contains("file\tdebug.log\n"));
        assert!(text.contains("odd%09name%25.txt\tr\t12\t0\t"), "{text}");
        assert_eq!(Prestage::parse(&text).unwrap(), sample());
        assert_eq!(Prestage::parse(&text.replace('\n', "\r\n")).unwrap(), sample(), "CRLF line endings are fine");
    }

    #[test]
    fn damaged_text_is_refused() {
        let good = sample().to_text();
        let cases = [
            String::new(),
            "something else\nfile\ta\n".to_string(),
            format!("{good}file\n"),
            format!("{good}line\ta\tx\t1\t0\t0000000000000000\n"),
            format!("{good}line\ta\tr\t-1\t0\t0000000000000000\n"),
            format!("{good}line\ta\tr\t1\t0\t123\n"),
            format!("{good}file\tbad%zz\n"),
            format!("{good}file\t\n"),
            format!("{good}mystery\tx\n"),
        ];
        for text in cases {
            assert!(Prestage::parse(&text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_whole_file_covers_its_lines() {
        let text = format!("{HEADER}\nfile\ta\nline\ta\tr\t1\t0\t{:016x}\n", 7);
        assert_eq!(Prestage::parse(&text).unwrap().files["a"], Saved::File);
    }

    #[test]
    fn save_and_load_round_trip_and_empty_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitcraft").join("prestage");
        assert_eq!(load(&path).unwrap(), Prestage::default(), "no file yet");
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap(), sample());
        save(&path, &Prestage::default()).unwrap();
        assert!(!path.exists());
        save(&path, &Prestage::default()).unwrap();
    }

    #[test]
    fn a_damaged_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prestage");
        std::fs::write(&path, b"\xff\xfe not text").unwrap();
        let message = load(&path).unwrap_err();
        assert!(message.contains("damaged") && message.contains("prestage.damaged"), "{message}");
        assert!(!path.exists() && dir.path().join("prestage.damaged").exists());
        assert_eq!(load(&path).unwrap(), Prestage::default(), "then it starts fresh");
    }
}
