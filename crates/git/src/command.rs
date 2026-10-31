//! Running git safely: fixed arguments (never a shell), no prompts, a time limit and an output
//! size limit.

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{GitError, Result};
use crate::locate::Git;
use crate::platform;

/// Limits for one git command.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Git is stopped if it runs longer than this.
    pub timeout: Duration,
    /// Most stdout bytes accepted; beyond this the command fails with `OutputTooLarge`.
    pub max_output: usize,
    /// Exit codes besides 0 that mean success. `git diff --no-index` exits with 1 when the files
    /// differ, which is exactly the case we want.
    pub also_ok: &'static [i32],
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(60), max_output: 64 * 1024 * 1024, also_ok: &[] }
    }
}

/// Most stderr bytes kept (the rest is read and thrown away so git never blocks on it).
const MAX_STDERR: usize = 64 * 1024;
/// Most stderr characters put into an error message.
const MAX_STDERR_MESSAGE: usize = 2000;
/// How often we check whether git has finished.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Options placed before every subcommand so the output doesn't depend on the user's git config:
/// no pager, no colour codes, paths not escaped, no signature checks mixed into `log`.
const GLOBAL_ARGS: &[&str] =
    &["--no-pager", "-c", "color.ui=false", "-c", "core.quotepath=off", "-c", "log.showSignature=false", "-c", "core.pager=cat"];

/// What a finished process gave back.
pub(crate) struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Git {
    /// Runs `git <args>` in `cwd` with the default limits and returns its stdout.
    /// A non-zero exit becomes an error carrying git's message.
    pub fn run(&self, cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
        self.run_with(cwd, args, &RunOptions::default())
    }

    /// Like [`Git::run`], with explicit limits.
    pub fn run_with(&self, cwd: &Path, args: &[&str], options: &RunOptions) -> Result<Vec<u8>> {
        self.run_inner(cwd, args, options, None)
    }

    /// Like [`Git::run_with`], feeding `input` to git's stdin (a commit message, or a list of
    /// paths for `--pathspec-from-file=-`, which has no command-line length limit).
    pub fn run_with_input(&self, cwd: &Path, args: &[&str], options: &RunOptions, input: Vec<u8>) -> Result<Vec<u8>> {
        self.run_inner(cwd, args, options, Some(input))
    }

    fn run_inner(&self, cwd: &Path, args: &[&str], options: &RunOptions, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let mut command = Command::new(self.exe());
        command
            .args(GLOBAL_ARGS)
            .args(args)
            .current_dir(cwd)
            // Never wait for a username or password typed into a terminal nobody can see.
            .env("GIT_TERMINAL_PROMPT", "0")
            // English messages, so the few we recognise (see `classify_failure`) can be matched.
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            // Read-only commands shouldn't take the index lock, so they never block the user's
            // own git commands running at the same time.
            .env("GIT_OPTIONAL_LOCKS", "0")
            // Paths we pass are file names, never pathspec patterns: a file called ":x" or "*.rs"
            // means exactly that file.
            .env("GIT_LITERAL_PATHSPECS", "1")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        platform::hide_console(&mut command);

        let name = command_name(args);
        let output = run_process(command, options, &name, input)?;
        let accepted = output.status.code().is_some_and(|code| options.also_ok.contains(&code));
        if output.status.success() || accepted { Ok(output.stdout) } else { Err(classify_failure(name, cwd, output.status.code(), &output.stderr)) }
    }
}

/// "git log" for `["log", "-z", ...]`. Only the subcommand is kept for messages: later commands
/// may carry URLs, and nothing that could hold a credential may reach the UI or a log.
fn command_name(args: &[&str]) -> String {
    match args.first() {
        Some(subcommand) => format!("git {subcommand}"),
        None => "git".to_string(),
    }
}

/// Turns git's error messages that we know into specific errors the user can act on.
fn classify_failure(command: String, cwd: &Path, code: Option<i32>, stderr: &[u8]) -> GitError {
    let text = String::from_utf8_lossy(stderr);
    let path = cwd.to_path_buf();
    if text.contains("not a git repository") {
        GitError::NotARepository { path }
    } else if text.contains("detected dubious ownership") {
        GitError::UnsafeRepository { path }
    } else if text.contains("must be run in a work tree") {
        GitError::NoWorkingTree { path }
    } else {
        let trimmed = text.trim();
        // Count characters, not bytes, so a multi-byte character is never cut in half.
        let stderr = match trimmed.char_indices().nth(MAX_STDERR_MESSAGE) {
            Some((cut, _)) => format!("{}…", &trimmed[..cut]),
            None => trimmed.to_string(),
        };
        GitError::CommandFailed { command, code, stderr }
    }
}

/// Starts `command`, collects its output within the limits and waits for it to exit.
///
/// stdout and stderr are each read on their own thread. Reading them one after the other could
/// deadlock: git would block writing to a full stderr pipe while we wait on stdout.
pub(crate) fn run_process(mut command: Command, options: &RunOptions, name: &str, input: Option<Vec<u8>>) -> Result<ProcessOutput> {
    let io_error = |what: &str, source: io::Error| GitError::Io { context: format!("{name}: {what}"), source };

    let mut child = command.spawn().map_err(|e| io_error("could not start", e))?;
    // `take()` moves each pipe out of `child`, so the reader thread can own it.
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        stop(&mut child);
        return Err(io_error("could not read output", io::Error::other("output pipes missing")));
    };

    // `move` gives each closure ownership of its pipe; the thread returns what it read, and
    // `join` below hands that back (like std::thread + std::future in C++).
    let max_output = options.max_output;
    let stdout_reader = thread::spawn(move || read_capped(stdout, max_output, false));
    let stderr_reader = thread::spawn(move || read_capped(stderr, MAX_STDERR, true));
    // stdin is written on its own thread too, for the same reason. Dropping the pipe when done
    // tells git the input has ended. A write error (git exited early) shows up in git's own exit
    // code, so it is ignored here.
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }

    let deadline = Instant::now() + options.timeout;
    let status = loop {
        // The deadline is checked before `try_wait`, so a zero timeout always times out (tests
        // rely on this).
        if Instant::now() >= deadline {
            stop(&mut child);
            // Killing git closes its pipes, so the reader threads finish on their own.
            return Err(GitError::TimedOut { command: name.to_string(), after: options.timeout });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(e) => {
                stop(&mut child);
                return Err(io_error("could not wait for git", e));
            }
        }
    };

    let (stdout, too_large) = join_reader(stdout_reader).map_err(|e| io_error("could not read output", e))?;
    let (stderr, _) = join_reader(stderr_reader).map_err(|e| io_error("could not read errors", e))?;
    // Checked before the exit status: when we stop reading, git fails with a broken pipe, and
    // "too much output" is the real reason.
    if too_large {
        return Err(GitError::OutputTooLarge { command: name.to_string(), limit: options.max_output });
    }
    Ok(ProcessOutput { status, stdout, stderr })
}

/// Kills and reaps the child. Errors are ignored: it may already have exited.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads up to `max` bytes. Returns the bytes and whether there was more. With `drain`, the rest
/// is read and discarded; without it, the pipe is closed early (git then stops with an error).
fn read_capped(reader: impl Read, max: usize, drain: bool) -> io::Result<(Vec<u8>, bool)> {
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    let mut limited = reader.take(limit);
    let mut buffer = Vec::new();
    limited.read_to_end(&mut buffer)?;
    let over = buffer.len() > max;
    buffer.truncate(max);
    if over && drain {
        io::copy(&mut limited.into_inner(), &mut io::sink())?;
    }
    Ok((buffer, over))
}

/// Waits for a reader thread. A thread that panicked becomes an error rather than a crash.
fn join_reader(handle: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>) -> io::Result<(Vec<u8>, bool)> {
    handle.join().map_err(|_| io::Error::other("reader thread stopped unexpectedly"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git() -> Git {
        Git::locate().unwrap()
    }

    #[test]
    fn runs_git_and_returns_stdout() {
        let out = git().run(&std::env::temp_dir(), &["--version"]).unwrap();
        assert!(String::from_utf8_lossy(&out).starts_with("git version "));
    }

    #[test]
    fn failure_carries_code_and_message() {
        let err = git().run(&std::env::temp_dir(), &["no-such-subcommand"]).unwrap_err();
        match err {
            GitError::CommandFailed { command, code, stderr } => {
                assert_eq!(command, "git no-such-subcommand");
                assert!(code.is_some_and(|c| c != 0));
                assert!(stderr.contains("not a git command"), "{stderr}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn output_over_the_limit_is_an_error() {
        let options = RunOptions { max_output: 5, ..RunOptions::default() };
        let err = git().run_with(&std::env::temp_dir(), &["--version"], &options).unwrap_err();
        assert!(matches!(err, GitError::OutputTooLarge { limit: 5, .. }), "{err:?}");
    }

    #[test]
    fn a_command_past_its_time_limit_is_stopped() {
        let options = RunOptions { timeout: Duration::ZERO, ..RunOptions::default() };
        let err = git().run_with(&std::env::temp_dir(), &["--version"], &options).unwrap_err();
        assert!(matches!(err, GitError::TimedOut { .. }), "{err:?}");
    }

    #[test]
    fn command_name_keeps_only_the_subcommand() {
        assert_eq!(command_name(&["push", "https://user:secret@example.com/repo"]), "git push");
        assert_eq!(command_name(&[]), "git");
    }

    #[test]
    fn known_messages_become_specific_errors() {
        let cwd = Path::new("/some/dir");
        let classify = |stderr: &str| classify_failure("git x".into(), cwd, Some(128), stderr.as_bytes());
        assert!(matches!(classify("fatal: not a git repository (or any of the parent directories): .git"), GitError::NotARepository { .. }));
        assert!(matches!(classify("fatal: detected dubious ownership in repository at '/some/dir'"), GitError::UnsafeRepository { .. }));
        assert!(matches!(classify("fatal: this operation must be run in a work tree"), GitError::NoWorkingTree { .. }));
        assert!(matches!(classify("fatal: something else"), GitError::CommandFailed { .. }));
    }

    #[test]
    fn long_stderr_is_shortened_without_splitting_characters() {
        let stderr = "é".repeat(MAX_STDERR_MESSAGE + 10);
        let GitError::CommandFailed { stderr, .. } = classify_failure("git x".into(), Path::new("."), Some(1), stderr.as_bytes()) else {
            panic!("expected CommandFailed");
        };
        assert_eq!(stderr.chars().count(), MAX_STDERR_MESSAGE + 1); // plus the ellipsis
    }

    #[test]
    fn read_capped_reports_overflow() {
        assert_eq!(read_capped(&b"hello"[..], 5, false).unwrap(), (b"hello".to_vec(), false));
        assert_eq!(read_capped(&b"hello!"[..], 5, true).unwrap(), (b"hello".to_vec(), true));
    }
}
