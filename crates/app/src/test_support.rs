//! Helpers shared by the app's tests (compiled only for `cargo test`).

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// A throwaway repository with some commits. Deleted when dropped.
pub struct TestRepo {
    dir: tempfile::TempDir,
    config_dir: tempfile::TempDir,
}

impl TestRepo {
    /// A repository on `main` with one empty commit per message, oldest first.
    pub fn with_commits(messages: &[&str]) -> TestRepo {
        let repo = TestRepo { dir: tempfile::tempdir().unwrap(), config_dir: tempfile::tempdir().unwrap() };
        std::fs::write(repo.config_dir.path().join("gitconfig"), b"").unwrap();
        repo.git(&["init", "-q", "-b", "main"]);
        for message in messages {
            repo.git(&["commit", "-q", "--allow-empty", "-m", message]);
        }
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs git with the user's own config shut out (it could require signed commits).
    pub fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(["-c", "user.name=Test Author", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.config_dir.path().join("gitconfig"))
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Calls `condition` until it returns true, failing the test if that takes over 20 seconds.
pub fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for the background worker");
        std::thread::sleep(Duration::from_millis(5));
    }
}
