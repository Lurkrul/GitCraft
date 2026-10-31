//! Prints a repository's history using the git layer, for trying it out by hand:
//!
//! ```sh
//! cargo run -p gitcraft-git --example log -- <folder> [limit]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use gitcraft_git::{DEFAULT_LOG_LIMIT, Git, GitError, LogScope};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let folder = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let limit = args.next().and_then(|arg| arg.to_str()?.parse().ok()).unwrap_or(DEFAULT_LOG_LIMIT);

    match run(folder, limit) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(folder: PathBuf, limit: usize) -> Result<(), GitError> {
    let git = Git::locate()?;
    println!("git {} at {}", git.version(), git.exe().display());

    let repo = git.open_repo(&folder)?;
    println!("repository: {}", repo.root().display());

    let log = git.log(&repo, &LogScope::AllBranches, limit)?;
    for commit in &log.commits {
        let merge = if commit.is_merge() { " (merge)" } else { "" };
        println!("{} {:>10} {:<20} {}{merge}", commit.short_id(), commit.author_time, commit.author_name, commit.summary);
    }
    println!("{} commit(s){}", log.commits.len(), if log.truncated { ", more not shown" } else { "" });
    Ok(())
}
