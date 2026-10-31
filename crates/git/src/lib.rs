//! GitCraft's git layer.
//!
//! Runs the system `git` executable as a subprocess and parses its machine-readable output. There
//! is no UI here, so everything can be tested without a window. All functions block until git
//! finishes, so the app calls them from a background thread, never the UI thread.
//!
//! ```no_run
//! # fn main() -> Result<(), gitcraft_git::GitError> {
//! let git = gitcraft_git::Git::locate()?;
//! let repo = git.open_repo(std::path::Path::new("."))?;
//! let log = git.log(&repo, gitcraft_git::DEFAULT_LOG_LIMIT)?;
//! for commit in &log.commits {
//!     println!("{} {}", commit.short_id(), commit.summary);
//! }
//! # Ok(())
//! # }
//! ```

mod command;
mod error;
mod locate;
mod log;
mod platform;
mod repo;

pub use command::RunOptions;
pub use error::{GitError, Result};
pub use locate::Git;
pub use log::{Commit, DEFAULT_LOG_LIMIT, Log};
pub use repo::Repo;
