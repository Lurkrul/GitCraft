//! Tests against real repositories made in temporary folders with the installed git.

// This whole file is test code, where a failed `unwrap` is just a failed test. (clippy.toml's
// allow-*-in-tests settings only reach `#[test]` functions, not the helpers below.)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gitcraft_git::{
    CommitRequest, DiffRequest, EntryKind, FileStatus, Git, GitError, Head, LineKind, LogScope, PartialFile, RefKind, WorkingChange,
    WorkingDiffRequest,
};

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
        // Commits made through `Git` use the machine's own config; these repository settings
        // override anything there that would change the results (identity, signing, hooks).
        for (key, value) in [
            ("user.name", "Test Author"),
            ("user.email", "test@example.com"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
            ("core.autocrlf", "false"),
        ] {
            repo.git(&["config", key, value]);
        }
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
fn git_paths_point_into_the_git_folder_even_in_a_worktree() {
    let repo = TestRepo::new();
    repo.commit("Root");
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let path = git.git_path(&opened, "gitcraft/prestage").unwrap();
    assert_eq!(path, opened.root().join(".git").join("gitcraft").join("prestage"));

    // A linked worktree has its own git folder (its `.git` is a file pointing there).
    let worktree_dir = tempfile::tempdir().unwrap();
    let worktree = worktree_dir.path().join("wt");
    repo.git(&["worktree", "add", "-q", "-b", "side", worktree.to_str().unwrap()]);
    let opened_worktree = git.open_repo(&worktree).unwrap();
    let in_worktree = git.git_path(&opened_worktree, "gitcraft/prestage").unwrap();
    assert!(in_worktree.ends_with(Path::new("worktrees").join("wt").join("gitcraft").join("prestage")), "{}", in_worktree.display());
    assert!(in_worktree.is_absolute());

    for bad in ["", "../escape", "/abs"] {
        assert!(git.git_path(&opened, bad).is_err(), "{bad:?}");
    }
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

fn write(repo: &TestRepo, path: &str, contents: &str) {
    let full = repo.path().join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, contents).unwrap();
}

#[test]
fn status_lists_staged_unstaged_and_untracked_files() {
    let repo = TestRepo::new();
    for name in ["edited.txt", "staged.txt", "gone.txt"] {
        write(&repo, name, "original\n");
    }
    repo.git(&["add", "."]);
    repo.commit("Root");

    write(&repo, "edited.txt", "changed\n");
    write(&repo, "staged.txt", "changed\n");
    repo.git(&["add", "staged.txt"]);
    std::fs::remove_file(repo.path().join("gone.txt")).unwrap();
    write(&repo, "added.txt", "new\n");
    repo.git(&["add", "added.txt"]);
    write(&repo, "new folder/untracked file.txt", "new\n");

    let git = git();
    let status = git.status(&git.open_repo(repo.path()).unwrap()).unwrap();
    let mut summary: Vec<_> = status.entries.iter().map(|e| (e.path.as_str(), e.change(), e.has_staged())).collect();
    summary.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(
        summary,
        [
            ("added.txt", WorkingChange::Added, true),
            ("edited.txt", WorkingChange::Modified, false),
            ("gone.txt", WorkingChange::Deleted, false),
            ("new folder/untracked file.txt", WorkingChange::Added, false),
            ("staged.txt", WorkingChange::Modified, true),
        ]
    );
    let untracked = status.entries.iter().find(|e| e.path.starts_with("new folder")).unwrap();
    assert_eq!(untracked.kind, EntryKind::Untracked);
}

#[test]
fn a_clean_tree_has_no_changes() {
    let repo = TestRepo::new();
    write(&repo, "a.txt", "a\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    let git = git();
    let status = git.status(&git.open_repo(repo.path()).unwrap()).unwrap();
    assert!(status.entries.is_empty() && !status.truncated);
}

fn working(path: &str, new_file: bool) -> WorkingDiffRequest {
    WorkingDiffRequest { path: path.into(), new_file }
}

fn changed_lines(diff: &gitcraft_git::FileDiff) -> Vec<(LineKind, String)> {
    diff.hunks.iter().flat_map(|h| &h.lines).filter(|l| l.kind != LineKind::Context).map(|l| (l.kind, l.text.clone())).collect()
}

#[test]
fn working_diffs_show_staged_and_unstaged_changes_together() {
    let repo = TestRepo::new();
    write(&repo, "notes.txt", "one\ntwo\nthree\n");
    write(&repo, "gone.txt", "bye\n");
    repo.git(&["add", "."]);
    repo.commit("Root");

    // One change staged, a second one not: both show, compared with HEAD.
    write(&repo, "notes.txt", "ONE\ntwo\nthree\n");
    repo.git(&["add", "notes.txt"]);
    write(&repo, "notes.txt", "ONE\ntwo\nTHREE\n");
    std::fs::remove_file(repo.path().join("gone.txt")).unwrap();
    write(&repo, "brand new.txt", "hello\nworld\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let diff = git.working_diff(&opened, &working("notes.txt", false)).unwrap();
    let expected = [(LineKind::Removed, "one"), (LineKind::Added, "ONE"), (LineKind::Removed, "three"), (LineKind::Added, "THREE")];
    assert_eq!(changed_lines(&diff), expected.map(|(k, t)| (k, t.to_string())));

    let diff = git.working_diff(&opened, &working("gone.txt", false)).unwrap();
    assert_eq!(changed_lines(&diff), [(LineKind::Removed, "bye".to_string())]);

    let diff = git.working_diff(&opened, &working("brand new.txt", true)).unwrap();
    assert_eq!(changed_lines(&diff), [(LineKind::Added, "hello".to_string()), (LineKind::Added, "world".to_string())]);
    assert_eq!(diff.hunks[0].lines[0].new_line, Some(1));
}

#[test]
fn working_diffs_work_before_the_first_commit() {
    let repo = TestRepo::new();
    write(&repo, "first.txt", "a\n");
    repo.git(&["add", "first.txt"]);
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let status = git.status(&opened).unwrap();
    assert_eq!(status.entries.len(), 1);
    assert!(status.entries[0].is_new());
    let diff = git.working_diff(&opened, &working("first.txt", true)).unwrap();
    assert_eq!(changed_lines(&diff), [(LineKind::Added, "a".to_string())]);
}

#[test]
fn a_new_file_that_has_gone_has_no_diff() {
    let repo = TestRepo::new();
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let diff = git.working_diff(&opened, &working("never-existed.txt", true)).unwrap();
    assert!(diff.hunks.is_empty());
}

#[test]
fn working_diff_paths_are_literal_and_stay_inside() {
    let repo = TestRepo::new();
    write(&repo, "weird[1].txt", "a\n");
    write(&repo, "weird1.txt", "decoy\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    write(&repo, "weird[1].txt", "b\n");
    write(&repo, "weird1.txt", "decoy changed\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let diff = git.working_diff(&opened, &working("weird[1].txt", false)).unwrap();
    assert_eq!(changed_lines(&diff), [(LineKind::Removed, "a".to_string()), (LineKind::Added, "b".to_string())]);

    let escape = git.working_diff(&opened, &working("../outside.txt", true));
    assert!(matches!(escape, Err(GitError::Malformed { .. })), "{escape:?}");
}

fn commit_request(include: &[&str], exclude: &[&str], message: &str) -> CommitRequest {
    CommitRequest {
        include: include.iter().map(|p| p.to_string()).collect(),
        exclude: exclude.iter().map(|p| p.to_string()).collect(),
        partial: Vec::new(),
        message: message.into(),
        amend: false,
    }
}

/// Reads `path`'s working diff and builds a patch of the lines `pick` chooses.
fn pick_lines(git: &Git, opened: &gitcraft_git::Repo, path: &str, new_file: bool, pick: impl Fn(&str) -> bool) -> PartialFile {
    let diff = git.working_diff(opened, &working(path, new_file)).unwrap();
    assert!(diff.can_pick_lines(), "{path}: lines should be pickable");
    let patch = diff.partial_patch(|at| pick(&diff.hunks[at.hunk].lines[at.line].text)).unwrap().expect("something picked");
    PartialFile { path: path.into(), patch }
}

/// A file's contents in HEAD.
fn in_head(repo: &TestRepo, path: &str) -> String {
    repo.git(&["show", &format!("HEAD:{path}")])
}

/// Lines "1" to "12", with the given lines' text replaced (by line number).
fn numbered_with(changes: &[(usize, &str)]) -> String {
    (1..=12usize)
        .map(|n| match changes.iter().find(|(line, _)| *line == n) {
            Some((_, text)) => format!("{text}\n"),
            None => format!("{n}\n"),
        })
        .collect()
}

fn numbered_lines() -> String {
    numbered_with(&[])
}

#[test]
fn commits_only_the_picked_lines() {
    let repo = TestRepo::new();
    write(&repo, "f.txt", &numbered_lines());
    write(&repo, "other.txt", "o\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    // Two hunks: line 2 and line 11 changed, plus a new line after 6 in the middle hunk.
    let edited = numbered_with(&[(2, "two"), (6, "6\nsix and a half"), (11, "eleven")]);
    write(&repo, "f.txt", &edited);
    write(&repo, "other.txt", "o2\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    // Take "two" (and the removal of "2"), and the new line; leave "11" alone.
    let file = pick_lines(&git, &opened, "f.txt", false, |text| ["2", "two", "six and a half"].contains(&text));
    let request = CommitRequest { partial: vec![file], ..commit_request(&["other.txt"], &[], "Part of f") };
    git.commit(&opened, &request).unwrap();

    let expected = numbered_with(&[(2, "two"), (6, "6\nsix and a half")]);
    assert_eq!(in_head(&repo, "f.txt") + "\n", expected);
    assert_eq!(in_head(&repo, "other.txt"), "o2");
    // The file on disk is untouched, and the rest is still a change, not staged.
    assert_eq!(std::fs::read_to_string(repo.path().join("f.txt")).unwrap(), edited);
    let status = git.status(&opened).unwrap();
    let left: Vec<_> = status.entries.iter().map(|e| (e.path.as_str(), e.has_staged())).collect();
    assert_eq!(left, [("f.txt", false)]);
    let rest = git.working_diff(&opened, &working("f.txt", false)).unwrap();
    assert_eq!(changed_lines(&rest), [(LineKind::Removed, "11".to_string()), (LineKind::Added, "eleven".to_string())]);
}

#[test]
fn picked_lines_keep_crlf_endings() {
    let repo = TestRepo::new();
    write(&repo, "w.txt", "a\r\nb\r\nc\r\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    write(&repo, "w.txt", "a\r\nB\r\nc\r\nd\r\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let file = pick_lines(&git, &opened, "w.txt", false, |text| text == "d");
    git.commit(&opened, &CommitRequest { partial: vec![file], ..commit_request(&[], &[], "Add d") }).unwrap();
    let blob = Command::new("git").args(["cat-file", "blob", "HEAD:w.txt"]).current_dir(repo.path()).output().unwrap().stdout;
    assert_eq!(blob, b"a\r\nb\r\nc\r\nd\r\n");
}

#[test]
fn part_of_a_new_file_before_the_first_commit() {
    let repo = TestRepo::new();
    write(&repo, "new.txt", "keep\nskip\nkeep too\n");
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let file = pick_lines(&git, &opened, "new.txt", true, |text| text != "skip");
    git.commit(&opened, &CommitRequest { partial: vec![file], ..commit_request(&[], &[], "First") }).unwrap();
    assert_eq!(in_head(&repo, "new.txt"), "keep\nkeep too");
    assert_eq!(std::fs::read_to_string(repo.path().join("new.txt")).unwrap(), "keep\nskip\nkeep too\n");
}

#[test]
fn part_of_a_deleted_file_keeps_the_rest() {
    let repo = TestRepo::new();
    write(&repo, "d.txt", "one\ntwo\nthree\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    std::fs::remove_file(repo.path().join("d.txt")).unwrap();

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let file = pick_lines(&git, &opened, "d.txt", false, |text| text == "two");
    git.commit(&opened, &CommitRequest { partial: vec![file], ..commit_request(&[], &[], "Drop two") }).unwrap();
    assert_eq!(in_head(&repo, "d.txt"), "one\nthree");
    assert!(!repo.path().join("d.txt").exists(), "still deleted on disk");
}

#[test]
fn picked_lines_apply_after_the_file_changed_again_and_ignore_earlier_staging() {
    let repo = TestRepo::new();
    write(&repo, "f.txt", &numbered_lines());
    repo.git(&["add", "."]);
    repo.commit("Root");
    write(&repo, "f.txt", &numbered_with(&[(3, "three")]));
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let file = pick_lines(&git, &opened, "f.txt", false, |text| text == "3" || text == "three");

    // Meanwhile: something else is staged in a terminal, and the file is edited again.
    write(&repo, "f.txt", &numbered_with(&[(3, "three"), (12, "twelve")]));
    repo.git(&["add", "f.txt"]);

    git.commit(&opened, &CommitRequest { partial: vec![file], ..commit_request(&[], &[], "Three") }).unwrap();
    assert_eq!(in_head(&repo, "f.txt") + "\n", numbered_with(&[(3, "three")]), "only the picked change");
}

#[test]
fn blank_lines_survive_a_suppress_blank_empty_config() {
    let repo = TestRepo::new();
    repo.git(&["config", "diff.suppressBlankEmpty", "true"]);
    write(&repo, "b.txt", "a\n\nb\n");
    repo.git(&["add", "."]);
    repo.commit("Root");
    write(&repo, "b.txt", "a\n\nB\nc\n");
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let file = pick_lines(&git, &opened, "b.txt", false, |text| text == "c");
    git.commit(&opened, &CommitRequest { partial: vec![file], ..commit_request(&[], &[], "c") }).unwrap();
    assert_eq!(in_head(&repo, "b.txt"), "a\n\nb\nc");
}

#[test]
fn a_path_listed_twice_is_refused() {
    let repo = TestRepo::new();
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let request = CommitRequest { partial: vec![PartialFile { path: "a".into(), patch: Vec::new() }], ..commit_request(&["a"], &[], "x") };
    assert!(matches!(git.commit(&opened, &request), Err(GitError::Malformed { .. })));
}

/// The files changed by HEAD's commit.
fn files_in_head(repo: &TestRepo) -> Vec<String> {
    let mut files: Vec<String> = repo.git(&["show", "--name-only", "--format=", "HEAD"]).lines().map(str::to_string).collect();
    files.sort();
    files
}

#[test]
fn commits_only_the_included_files() {
    let repo = TestRepo::new();
    write(&repo, "keep.txt", "1\n");
    write(&repo, "gone.txt", "1\n");
    write(&repo, "staged elsewhere.txt", "1\n");
    repo.git(&["add", "."]);
    repo.commit("Root");

    write(&repo, "keep.txt", "2\n");
    std::fs::remove_file(repo.path().join("gone.txt")).unwrap();
    write(&repo, "new [1].txt", "new\n");
    write(&repo, "staged elsewhere.txt", "2\n");
    repo.git(&["add", "staged elsewhere.txt"]); // staged in a terminal, then unticked in GitCraft
    write(&repo, "not ticked.txt", "x\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let request = commit_request(&["keep.txt", "gone.txt", "new [1].txt"], &["staged elsewhere.txt", "not ticked.txt"], "Pick some\n\nDetails here.");
    let id = git.commit(&opened, &request).unwrap();

    assert_eq!(id, repo.git(&["rev-parse", "HEAD"]));
    assert_eq!(files_in_head(&repo), ["gone.txt", "keep.txt", "new [1].txt"]);
    assert_eq!(repo.git(&["log", "-1", "--format=%B"]), "Pick some\n\nDetails here.");
    // The left-out changes are still on disk, and no longer staged.
    let status = git.status(&opened).unwrap();
    let left: Vec<_> = status.entries.iter().map(|e| (e.path.as_str(), e.has_staged())).collect();
    assert_eq!(left, [("staged elsewhere.txt", false), ("not ticked.txt", false)]);
    assert_eq!(std::fs::read_to_string(repo.path().join("staged elsewhere.txt")).unwrap(), "2\n");
}

#[test]
fn the_first_commit_can_leave_files_out() {
    let repo = TestRepo::new();
    write(&repo, "a.txt", "a\n");
    write(&repo, "b.txt", "b\n");
    repo.git(&["add", "."]);
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    git.commit(&opened, &commit_request(&["a.txt"], &["b.txt"], "First")).unwrap();
    assert_eq!(files_in_head(&repo), ["a.txt"]);
    let status = git.status(&opened).unwrap();
    assert_eq!(status.entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["b.txt"]);
    assert!(!status.unborn);
}

#[test]
fn amend_replaces_the_last_commit() {
    let repo = TestRepo::new();
    write(&repo, "a.txt", "a\n");
    repo.git(&["add", "."]);
    let first = repo.commit("Root");
    write(&repo, "b.txt", "b\n");
    repo.git(&["add", "."]);
    repo.commit("Second, with a typo");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let info = git.amend_info(&opened).unwrap();
    assert_eq!(info.message, "Second, with a typo");
    assert_eq!(info.pushed_to, None);

    write(&repo, "c.txt", "c\n");
    let request = CommitRequest { amend: true, ..commit_request(&["c.txt"], &[], "Second") };
    git.commit(&opened, &request).unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD~1"]), first, "replaced, not added");
    assert_eq!(files_in_head(&repo), ["b.txt", "c.txt"]);
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]), "Second");
}

#[test]
fn amend_info_knows_a_pushed_commit() {
    let origin = TestRepo::new();
    origin.commit("Root");
    let clone_dir = tempfile::tempdir().unwrap();
    let clone_path = clone_dir.path().join("clone");
    origin.git(&["clone", "-q", origin.path().to_str().unwrap(), clone_path.to_str().unwrap()]);
    let git = git();
    let opened = git.open_repo(&clone_path).unwrap();
    assert_eq!(git.amend_info(&opened).unwrap().pushed_to.as_deref(), Some("refs/remotes/origin/main"));
}

#[test]
fn a_failing_hook_stops_the_commit_and_says_why() {
    let repo = TestRepo::new();
    repo.commit("Root");
    let hook = repo.path().join(".git").join("hooks").join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'lint failed: tabs in a.txt' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    write(&repo, "a.txt", "a\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    let before = repo.git(&["rev-parse", "HEAD"]);
    match git.commit(&opened, &commit_request(&["a.txt"], &[], "Blocked")) {
        Err(GitError::CommandFailed { command, stderr, .. }) => {
            assert_eq!(command, "git commit");
            assert!(stderr.contains("lint failed"), "{stderr}");
        }
        other => panic!("expected the hook to stop it, got {other:?}"),
    }
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn an_empty_message_is_refused_before_running_git() {
    let repo = TestRepo::new();
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    assert!(matches!(git.commit(&opened, &commit_request(&[], &[], "  \n ")), Err(GitError::Malformed { .. })));
}

#[test]
fn discard_puts_files_back_as_committed() {
    let repo = TestRepo::new();
    write(&repo, "edited.txt", "original\n");
    write(&repo, "deleted.txt", "keep me\n");
    write(&repo, "staged edit.txt", "original\n");
    repo.git(&["add", "."]);
    repo.commit("Root");

    write(&repo, "edited.txt", "changed\n");
    std::fs::remove_file(repo.path().join("deleted.txt")).unwrap();
    write(&repo, "staged edit.txt", "changed\n");
    repo.git(&["add", "staged edit.txt"]);
    write(&repo, "new staged.txt", "new\n");
    repo.git(&["add", "new staged.txt"]);
    write(&repo, "other.txt", "untouched\n");

    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    for path in ["edited.txt", "deleted.txt", "staged edit.txt", "new staged.txt"] {
        git.discard(&opened, path).unwrap();
    }
    let read = |name: &str| std::fs::read_to_string(repo.path().join(name)).ok();
    assert_eq!(read("edited.txt").as_deref(), Some("original\n"));
    assert_eq!(read("deleted.txt").as_deref(), Some("keep me\n"));
    assert_eq!(read("staged edit.txt").as_deref(), Some("original\n"));
    assert_eq!(read("new staged.txt"), None, "a newly added file is removed");
    let status = git.status(&opened).unwrap();
    assert_eq!(status.entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["other.txt"], "only the untouched file is left");
}

#[test]
fn discard_before_the_first_commit_unstages() {
    let repo = TestRepo::new();
    write(&repo, "first.txt", "a\n");
    repo.git(&["add", "first.txt"]);
    let git = git();
    let opened = git.open_repo(repo.path()).unwrap();
    git.discard(&opened, "first.txt").unwrap();
    let status = git.status(&opened).unwrap();
    assert_eq!(status.entries[0].kind, EntryKind::Untracked);
    assert!(git.discard(&opened, "../outside").is_err());
}
