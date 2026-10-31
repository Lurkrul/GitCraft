//! Tests against real repositories made in temporary folders with the installed git.

// This whole file is test code, where a failed `unwrap` is just a failed test. (clippy.toml's
// allow-*-in-tests settings only reach `#[test]` functions, not the helpers below.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitcraft_git::{Git, GitError, Head, LogScope, RefKind};

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
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 100).unwrap();
    assert!(log.commits.is_empty());
    assert!(!log.truncated);
}

#[test]
fn reads_a_linear_history_newest_first() {
    let repo = TestRepo::new();
    let first = repo.commit("First commit");
    let second = repo.commit("Second commit");

    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 100).unwrap();
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
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 100).unwrap();
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

    let log = git.log(&opened, &LogScope::AllBranches, 3).unwrap();
    assert_eq!(log.commits.len(), 3);
    assert!(log.truncated);
    assert_eq!(log.commits[0].summary, "Commit 5");

    let log = git.log(&opened, &LogScope::AllBranches, 5).unwrap();
    assert_eq!(log.commits.len(), 5);
    assert!(!log.truncated);
}

#[test]
fn keeps_unicode_names_and_messages() {
    let repo = TestRepo::new();
    repo.git(&["-c", "user.name=Zoë Müller-Łukasz", "commit", "-q", "--allow-empty", "-m", "Añadir soporte 🦀 für Ümlauts"]);
    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 10).unwrap();
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
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 10).unwrap();
    assert_eq!(log.commits.len(), 1);
}

/// A repository with `main` (Base, Main work) and `feature` (Base, Feature work), on `main`.
fn two_branches() -> (TestRepo, String, String, String) {
    let repo = TestRepo::new();
    let base = repo.commit("Base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    let feature = repo.commit("Feature work");
    repo.git(&["checkout", "-q", "main"]);
    let main = repo.commit("Main work");
    (repo, base, main, feature)
}

#[test]
fn reads_branches_tags_and_head() {
    let (repo, base, main, feature) = two_branches();
    repo.git(&["tag", "-a", "v1.0", "-m", "Release", &base]);
    repo.git(&["tag", "light", &feature]);

    let git = git();
    let refs = git.refs(&git.open_repo(repo.path()).unwrap()).unwrap();

    let names: Vec<_> = refs.refs.iter().map(|r| (r.kind, r.short_name(), r.target.as_str())).collect();
    assert_eq!(
        names,
        [
            (RefKind::Branch, "feature", feature.as_str()),
            (RefKind::Branch, "main", main.as_str()),
            (RefKind::Tag, "light", feature.as_str()),
            // The annotated tag points at the commit, not at the tag object.
            (RefKind::Tag, "v1.0", base.as_str()),
        ]
    );
    assert_eq!(refs.head, Head::Branch { name: "refs/heads/main".into(), commit: main.clone() });
    assert_eq!(refs.pointing_at(&main).map(|r| r.short_name()).collect::<Vec<_>>(), ["main"]);
}

#[test]
fn reads_remote_branches_of_a_clone() {
    let (origin, _, _, _) = two_branches();
    let parent = tempfile::tempdir().unwrap();
    let clone = parent.path().join("clone");
    let status = Command::new("git").arg("clone").arg("-q").arg(origin.path()).arg(&clone).status().unwrap();
    assert!(status.success());

    let git = git();
    let refs = git.refs(&git.open_repo(&clone).unwrap()).unwrap();
    let remotes: Vec<_> = refs.remote_branches().map(|r| r.short_name()).collect();
    // origin/HEAD is a symbolic ref, not a branch, so it is left out.
    assert_eq!(remotes, ["origin/feature", "origin/main"]);
    assert_eq!(refs.branches().map(|r| r.short_name()).collect::<Vec<_>>(), ["main"]);
}

#[test]
fn head_can_be_detached_or_unborn() {
    let (repo, base, _, _) = two_branches();
    repo.git(&["checkout", "-q", "--detach", &base]);
    let git = git();
    let refs = git.refs(&git.open_repo(repo.path()).unwrap()).unwrap();
    assert_eq!(refs.head, Head::Detached { commit: base });
    assert_eq!(refs.head_branch(), None);

    let empty = TestRepo::new();
    let refs = git.refs(&git.open_repo(empty.path()).unwrap()).unwrap();
    assert!(refs.refs.is_empty());
    assert_eq!(refs.head, Head::Unborn { name: "refs/heads/main".into() });
}

#[test]
fn log_can_show_all_branches_or_one() {
    let (repo, base, main, feature) = two_branches();
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let ids = |scope: &LogScope| git.log(&opened, scope, 100).unwrap().commits.into_iter().map(|c| c.id).collect::<Vec<_>>();

    let all = ids(&LogScope::AllBranches);
    assert_eq!(all.len(), 3);
    assert!(all.contains(&main) && all.contains(&feature));
    assert_eq!(all.last(), Some(&base), "parents come after their children");

    assert_eq!(ids(&LogScope::Ref("refs/heads/feature".into())), [feature, base.clone()]);
    assert_eq!(ids(&LogScope::Ref("refs/heads/main".into())), [main, base]);
}

#[test]
fn all_branches_includes_a_detached_head() {
    let (repo, _, _, _) = two_branches();
    repo.git(&["checkout", "-q", "--detach"]);
    let detached = repo.commit("Experiment on a detached HEAD");
    let git = git();
    let log = git.log(&git.open_repo(repo.path()).unwrap(), &LogScope::AllBranches, 100).unwrap();
    // Every test commit has the same timestamp, so check membership rather than position.
    assert!(log.commits.iter().any(|c| c.id == detached));
}

#[test]
fn a_scope_must_be_a_full_ref_name() {
    let (repo, _, _, _) = two_branches();
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    // Something that looks like an option is refused before git ever sees it.
    let err = git.log(&opened, &LogScope::Ref("--output=x".into()), 10).unwrap_err();
    assert!(matches!(err, GitError::Malformed { .. }), "{err:?}");
    // A branch that doesn't exist is git's error, reported as such.
    let err = git.log(&opened, &LogScope::Ref("refs/heads/missing".into()), 10).unwrap_err();
    assert!(matches!(err, GitError::CommandFailed { .. }), "{err:?}");
}
