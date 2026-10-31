//! Tests against real repositories made in temporary folders with the installed git.

// This whole file is test code, where a failed `unwrap` is just a failed test. (clippy.toml's
// allow-*-in-tests settings only reach `#[test]` functions, not the helpers below.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitcraft_git::{Git, GitError};

/// A throwaway repository. Both folders are deleted when this is dropped.
struct TestRepo {
    dir: tempfile::TempDir,
    /// Holds an empty file used as git's global config while building the test history.
    config_dir: tempfile::TempDir,
}

impl TestRepo {
    /// An empty repository on branch `main`.
    fn new() -> TestRepo {
        let repo = TestRepo { dir: tempfile::tempdir().unwrap(), config_dir: tempfile::tempdir().unwrap() };
        std::fs::write(repo.empty_config(), b"").unwrap();
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn empty_config(&self) -> PathBuf {
        self.config_dir.path().join("gitconfig")
    }

    /// Runs git to build the test history. The user's own git config is shut out (it could
    /// require signed commits, for example), and dates are fixed so the results are predictable.
    fn git(&self, args: &[&str]) -> String {
        let empty_config = self.empty_config();
        let output = Command::new("git")
            .args(["-c", "user.name=Test Author", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &empty_config)
            .env("GIT_AUTHOR_DATE", "@1700000000 +0000")
            .env("GIT_COMMITTER_DATE", "@1700000000 +0000")
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
}

fn git() -> Git {
    Git::locate().unwrap()
}

/// Compares paths after resolving them fully (temporary folders can be reached through
/// symlinks on Linux or short 8.3 names on Windows).
fn same_path(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

#[test]
fn opens_a_repository_from_its_top_folder_or_a_subfolder() {
    let repo = TestRepo::new();
    let sub: PathBuf = repo.path().join("src").join("deep");
    std::fs::create_dir_all(&sub).unwrap();

    let git = git();
    assert!(same_path(git.open_repo(repo.path()).unwrap().root(), repo.path()));
    assert!(same_path(git.open_repo(&sub).unwrap().root(), repo.path()));
}

#[test]
fn a_plain_folder_is_not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let err = git().open_repo(dir.path()).unwrap_err();
    assert!(matches!(err, GitError::NotARepository { .. }), "{err:?}");
}

#[test]
fn a_missing_folder_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let err = git().open_repo(&dir.path().join("nope")).unwrap_err();
    assert!(matches!(err, GitError::NoSuchFolder { .. }), "{err:?}");
}

#[test]
fn a_bare_repository_has_no_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("git").args(["init", "-q", "--bare"]).current_dir(dir.path()).status().unwrap();
    assert!(status.success());
    let err = git().open_repo(dir.path()).unwrap_err();
    assert!(matches!(err, GitError::NoWorkingTree { .. }), "{err:?}");
}

#[test]
fn an_empty_repository_has_an_empty_log() {
    let repo = TestRepo::new();
    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), 100).unwrap();
    assert!(log.commits.is_empty());
    assert!(!log.truncated);
}

#[test]
fn reads_a_linear_history_newest_first() {
    let repo = TestRepo::new();
    let first = repo.commit("First commit");
    let second = repo.commit("Second commit");

    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), 100).unwrap();
    assert_eq!(log.commits.len(), 2);
    assert!(!log.truncated);

    let newest = &log.commits[0];
    assert_eq!(newest.id, second);
    assert_eq!(newest.parents, vec![first.clone()]);
    assert_eq!(newest.summary, "Second commit");
    assert_eq!(newest.author_name, "Test Author");
    assert_eq!(newest.author_email, "test@example.com");
    assert_eq!(newest.author_time, 1_700_000_000);

    assert_eq!(log.commits[1].id, first);
    assert!(log.commits[1].parents.is_empty());
}

#[test]
fn reads_a_merge_with_both_parents() {
    let repo = TestRepo::new();
    repo.commit("Base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    let feature = repo.commit("Feature work");
    repo.git(&["checkout", "-q", "main"]);
    let main = repo.commit("Main work");
    repo.git(&["merge", "-q", "--no-ff", "feature", "-m", "Merge feature"]);

    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), 100).unwrap();
    let merge = &log.commits[0];
    assert_eq!(merge.summary, "Merge feature");
    assert!(merge.is_merge());
    assert_eq!(merge.parents, vec![main, feature]);
    assert_eq!(log.commits.len(), 4);
}

#[test]
fn stops_at_the_limit_and_says_so() {
    let repo = TestRepo::new();
    for n in 1..=5 {
        repo.commit(&format!("Commit {n}"));
    }
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();

    let log = git.log(&opened, 3).unwrap();
    assert_eq!(log.commits.len(), 3);
    assert!(log.truncated);
    assert_eq!(log.commits[0].summary, "Commit 5");

    let log = git.log(&opened, 5).unwrap();
    assert_eq!(log.commits.len(), 5);
    assert!(!log.truncated);
}

#[test]
fn keeps_unicode_names_and_messages() {
    let repo = TestRepo::new();
    repo.git(&["-c", "user.name=Zoë Müller-Łukasz", "commit", "-q", "--allow-empty", "-m", "Añadir soporte 🦀 für Ümlauts"]);
    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), 10).unwrap();
    assert_eq!(log.commits[0].author_name, "Zoë Müller-Łukasz");
    assert_eq!(log.commits[0].summary, "Añadir soporte 🦀 für Ümlauts");
}

#[test]
fn a_file_named_head_does_not_confuse_the_log() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("HEAD"), b"not a revision").unwrap();
    repo.git(&["add", "HEAD"]);
    repo.commit("Add a file called HEAD");
    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), 10).unwrap();
    assert_eq!(log.commits.len(), 1);
}
