//! The git layer's error type.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Shorthand used throughout the crate: `Result<T>` means `Result<T, GitError>`.
pub type Result<T> = std::result::Result<T, GitError>;

/// Everything that can go wrong in the git layer. Each variant's message (its `Display`) is
/// written for the user and says what to do about it where possible.
#[derive(Debug)]
pub enum GitError {
    /// No `git` executable on the `PATH`.
    NotFound,
    /// A `git` was found but did not behave like git.
    Unusable { exe: PathBuf, reason: String },
    /// Starting git or reading its output failed at the operating-system level.
    Io { context: String, source: io::Error },
    /// The chosen folder does not exist (or is not a folder).
    NoSuchFolder { path: PathBuf },
    /// The folder is not inside a git repository.
    NotARepository { path: PathBuf },
    /// A bare repository, or the inside of a `.git` folder: there is no working tree to show.
    NoWorkingTree { path: PathBuf },
    /// Git refuses to work in a repository owned by another user (its `safe.directory` check).
    UnsafeRepository { path: PathBuf },
    /// Git ran and reported an error. `command` is only the subcommand (such as `git log`),
    /// never its full arguments, so URLs or other sensitive values never reach the UI or logs.
    CommandFailed { command: String, code: Option<i32>, stderr: String },
    /// Git did not finish in time and was stopped.
    TimedOut { command: String, after: Duration },
    /// Git produced more output than we are willing to hold in memory.
    OutputTooLarge { command: String, limit: usize },
    /// Git's output did not have the shape we asked for.
    Malformed { what: String, detail: String },
    /// Something GitCraft can't do (yet, or in this case). The text is the whole message.
    Unsupported(String),
    /// A branch name git doesn't allow.
    InvalidBranchName { name: String },
    /// The user stopped the command.
    Cancelled { command: String },
    /// The remote refused a push: it has commits the branch doesn't (`stale`: for a force push,
    /// it changed since the last fetch). `detail` is git's message.
    PushRejected { stale: bool, detail: String },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "git was not found. Install git and make sure it is on your PATH, then restart GitCraft."),
            Self::Unusable { exe, reason } => write!(f, "{} does not work as git: {reason}", exe.display()),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::NoSuchFolder { path } => write!(f, "{} does not exist or is not a folder", path.display()),
            Self::NotARepository { path } => write!(f, "{} is not a git repository", path.display()),
            Self::NoWorkingTree { path } => {
                write!(f, "{} has no working tree (a bare repository or a .git folder); open the project folder instead", path.display())
            }
            Self::UnsafeRepository { path } => write!(
                f,
                "git refuses to open {} because it is owned by another user. If you trust it, run: git config --global --add safe.directory \"{}\"",
                path.display(),
                // git expects forward slashes here on every platform.
                path.display().to_string().replace('\\', "/")
            ),
            Self::CommandFailed { command, code, stderr } => {
                match code {
                    Some(code) => write!(f, "{command} failed (exit code {code})")?,
                    None => write!(f, "{command} was stopped by the system")?,
                }
                if stderr.is_empty() { Ok(()) } else { write!(f, ": {stderr}") }
            }
            Self::TimedOut { command, after } => write!(f, "{command} did not finish within {} seconds and was stopped", after.as_secs()),
            Self::OutputTooLarge { command, limit } => write!(f, "{command} produced more than {} MiB of output", limit / (1024 * 1024)),
            Self::Malformed { what, detail } => write!(f, "unexpected output from git ({what}): {detail}"),
            Self::Unsupported(message) => write!(f, "{message}"),
            Self::Cancelled { command } => write!(f, "{command} was cancelled"),
            Self::PushRejected { stale: false, .. } => write!(
                f,
                "The remote has commits that your branch doesn't have, so the push was refused. Pull first to bring them in (or use Force Push… if you mean to replace them)."
            ),
            Self::PushRejected { stale: true, .. } => write!(
                f,
                "The remote branch changed since your last fetch, so the force push was refused (nothing was overwritten). Fetch, look at what changed, then try again."
            ),
            Self::InvalidBranchName { name } => write!(
                f,
                "\"{name}\" can't be a branch name. Branch names can't be empty, start with \"-\", contain spaces, \"..\" or any of \
                 ~ ^ : ? * [ \\, or end with \"/\" or \".lock\"."
            ),
        }
    }
}

// Implementing `std::error::Error` lets callers treat this like any other error (for example
// `Box<dyn Error>`). `source` exposes the underlying OS error, where there is one.
impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_repository_message_gives_the_fix() {
        let message = GitError::UnsafeRepository { path: PathBuf::from(r"E:\repos\thing") }.to_string();
        assert!(message.contains(r#"safe.directory "E:/repos/thing""#), "{message}");
    }

    #[test]
    fn command_failed_message_includes_stderr_and_code() {
        let err = GitError::CommandFailed { command: "git log".into(), code: Some(128), stderr: "fatal: bad revision".into() };
        assert_eq!(err.to_string(), "git log failed (exit code 128): fatal: bad revision");
    }
}
