//! Tests against real repositories made in temporary folders with the installed git.

// This whole file is test code, where a failed `unwrap` is just a failed test. (clippy.toml's
// allow-*-in-tests settings only reach `#[test]` functions, not the helpers below.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitcraft_git::{DiffRequest, FileStatus, Git, GitError, Head, LineKind, LogScope, RefKind};

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

#[test]
fn commit_details_show_message_people_and_files() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("readme.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.path().join("logo.bin"), [0u8, 1, 2, 3, 0]).unwrap();
    repo.git(&["add", "."]);
    let root = repo.commit("Initial import");

    std::fs::write(repo.path().join("readme.txt"), "one\n2\nthree\n").unwrap();
    repo.git(&["mv", "logo.bin", "icon.bin"]);
    repo.git(&["add", "."]);
    repo.git(&["commit", "-q", "-m", "Edit readme\n\nAnd rename the logo."]);
    let second = repo.git(&["rev-parse", "HEAD"]);

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();

    let details = git.commit_details(&opened, &second).unwrap();
    assert_eq!(details.id, second);
    assert_eq!(details.parents, std::slice::from_ref(&root));
    assert_eq!(details.message, "Edit readme\n\nAnd rename the logo.");
    assert_eq!((details.author_name.as_str(), details.author_email.as_str()), ("Test Author", "test@example.com"));
    assert_eq!(details.author_time, 1_700_000_000);
    assert_eq!(details.committer_name, "Test Author");

    let files: Vec<_> = details.files.iter().map(|f| (f.status, f.path.as_str(), f.old_path.as_deref(), f.lines)).collect();
    assert_eq!(files, [(FileStatus::Renamed, "icon.bin", Some("logo.bin"), None), (FileStatus::Modified, "readme.txt", None, Some((2, 1)))]);

    // A root commit: every file is added.
    let details = git.commit_details(&opened, &root).unwrap();
    assert!(details.parents.is_empty());
    assert!(details.files.iter().all(|f| f.status == FileStatus::Added), "{:?}", details.files);
    assert_eq!(details.files.len(), 2);
}

#[test]
fn merge_details_compare_with_the_first_parent() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    repo.git(&["add", "."]);
    repo.commit("Base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.path().join("feature.txt"), "feature\n").unwrap();
    repo.git(&["add", "."]);
    repo.commit("Feature");
    repo.git(&["checkout", "-q", "main"]);
    std::fs::write(repo.path().join("main.txt"), "main\n").unwrap();
    repo.git(&["add", "."]);
    repo.commit("Main");
    repo.git(&["merge", "-q", "--no-ff", "feature", "-m", "Merge feature"]);
    let merge = repo.git(&["rev-parse", "HEAD"]);

    let git = git();
    let details = git.commit_details(&git.open_repo(repo.path()).unwrap(), &merge).unwrap();
    assert_eq!(details.parents.len(), 2);
    // Against the first parent (main), the merge brought in the feature's file.
    let paths: Vec<_> = details.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["feature.txt"]);
}

#[test]
fn commit_details_need_a_full_hash() {
    let repo = TestRepo::new();
    repo.commit("One");
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    assert!(matches!(git.commit_details(&opened, "HEAD"), Err(GitError::Malformed { .. })));
    assert!(matches!(git.commit_details(&opened, "--output=x"), Err(GitError::Malformed { .. })));
    // Well-formed but not in this repository: git's own error.
    assert!(matches!(git.commit_details(&opened, &"0".repeat(40)), Err(GitError::CommandFailed { .. })));
}

fn diff_request(commit: &str, parent: Option<&str>, path: &str, old_path: Option<&str>) -> DiffRequest {
    DiffRequest { commit: commit.into(), parent: parent.map(Into::into), path: path.into(), old_path: old_path.map(Into::into) }
}

#[test]
fn file_diffs_for_edits_new_files_renames_and_binaries() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("notes.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(repo.path().join("old-name.txt"), "unchanged content\n").unwrap();
    std::fs::write(repo.path().join("pic.bin"), [0u8, 1, 2]).unwrap();
    repo.git(&["add", "."]);
    let root = repo.commit("Root");

    std::fs::write(repo.path().join("notes.txt"), "one\n2\nthree\nfour\n").unwrap();
    repo.git(&["mv", "old-name.txt", "new-name.txt"]);
    std::fs::write(repo.path().join("pic.bin"), [0u8, 9, 9, 9]).unwrap();
    repo.git(&["add", "."]);
    let second = repo.commit("Second");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();

    // An edit: one line changed, one added.
    let diff = git.file_diff(&opened, &diff_request(&second, Some(&root), "notes.txt", None)).unwrap();
    assert_eq!(diff.hunks.len(), 1);
    let changes: Vec<_> = diff.hunks[0].lines.iter().filter(|l| l.kind != LineKind::Context).map(|l| (l.kind, l.text.as_str())).collect();
    assert_eq!(changes, [(LineKind::Removed, "two"), (LineKind::Added, "2"), (LineKind::Added, "four")]);

    // A root commit: the whole file is added.
    let diff = git.file_diff(&opened, &diff_request(&root, None, "notes.txt", None)).unwrap();
    assert!(diff.hunks[0].lines.iter().all(|l| l.kind == LineKind::Added));
    assert_eq!(diff.hunks[0].lines.len(), 3);

    // A pure rename has no line changes, and asking with both paths is fine.
    let diff = git.file_diff(&opened, &diff_request(&second, Some(&root), "new-name.txt", Some("old-name.txt"))).unwrap();
    assert!(diff.hunks.is_empty() && !diff.binary);

    // A binary file.
    let diff = git.file_diff(&opened, &diff_request(&second, Some(&root), "pic.bin", None)).unwrap();
    assert!(diff.binary);
}

#[test]
fn file_names_that_look_like_pathspec_magic_are_taken_literally() {
    let repo = TestRepo::new();
    // As a pattern, "weird[1].txt" would also match "weird1.txt" ([1] is a character class).
    // Taken literally it names only itself. (Brackets work on every platform; ":" and "*" can't
    // appear in Windows file names.)
    let name = "weird[1].txt";
    std::fs::write(repo.path().join(name), "a\n").unwrap();
    std::fs::write(repo.path().join("weird1.txt"), "decoy\n").unwrap();
    repo.git(&["add", "."]);
    let root = repo.commit("Root");

    let git = git();
    let diff = git.file_diff(&git.open_repo(repo.path()).unwrap(), &diff_request(&root, None, name, None)).unwrap();
    let texts: Vec<_> = diff.hunks.iter().flat_map(|h| &h.lines).map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["a"], "only the named file, not the decoy");
}

#[test]
fn oversized_diffs_are_refused() {
    let repo = TestRepo::new();
    let big = "x".repeat(100).repeat(50_000); // ~5 MB on one line
    std::fs::write(repo.path().join("big.txt"), &big).unwrap();
    repo.git(&["add", "."]);
    let root = repo.commit("Big");
    let git = git();
    let err = git.file_diff(&git.open_repo(repo.path()).unwrap(), &diff_request(&root, None, "big.txt", None)).unwrap_err();
    assert!(matches!(err, GitError::OutputTooLarge { .. }), "{err:?}");
}

#[test]
fn file_diffs_need_full_hashes() {
    let repo = TestRepo::new();
    let root = repo.commit("Root");
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    assert!(matches!(git.file_diff(&opened, &diff_request("HEAD", None, "x", None)), Err(GitError::Malformed { .. })));
    assert!(matches!(git.file_diff(&opened, &diff_request(&root, Some("--cached"), "x", None)), Err(GitError::Malformed { .. })));
}
