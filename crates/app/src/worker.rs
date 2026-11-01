//! The background thread that runs git, so the UI thread never waits on it.
//!
//! The UI sends a [`Job`] down one channel; the worker runs it and sends an [`Event`] back up
//! another, then wakes the UI. The UI collects finished events each frame with [`Worker::poll`],
//! which never blocks. Channels here are `std::sync::mpsc`: a thread-safe queue with one
//! receiving end, like a `std::queue` guarded by a mutex and condition variable in C++.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use gitcraft_git::{
    AmendInfo, CancelFlag, CommitDetails, DEFAULT_LOG_LIMIT, DiffRequest, FileDiff, Git, GitError, LogScope, PushMode, Refs, Repo, Status, SyncState,
    WorkingDiffRequest,
};

use crate::files;
use crate::graph::History;
use crate::prestage::{self, Prestage};
use crate::ui::branches::BranchOp;
use crate::ui::commit_tab::CommitPlan;
use crate::ui::sync::NetOp;

/// Called by the worker after each event, to make the UI repaint (and so poll) right away.
/// `Arc<dyn Fn ...>` is a shared, thread-safe function object, like `std::shared_ptr<std::function<void()>>`.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Work for the background thread.
#[derive(Debug)]
pub enum Job {
    /// Open the repository containing this folder and read its refs and its history (all
    /// branches).
    Open(PathBuf),
    /// Read the history of an open repository again, in another scope (such as one branch).
    LoadLog { repo: Repo, scope: LogScope },
    /// Read the branches and the history (in `scope`) again, after they changed (a commit, a
    /// branch created or checked out…), without reopening the repository.
    Reload { repo: Repo, scope: LogScope },
    /// Create, check out, rename or delete a local branch.
    Branch { repo: Repo, op: BranchOp },
    /// Count the commits only this branch reaches (lost if it is deleted), for the warning.
    CountOnlyOn { repo: Repo, name: String },
    /// Read the remotes, upstream and ahead/behind counts for the sync button.
    LoadSync { repo: Repo },
    /// Fetch, pull or push (sent to the network worker, so local work isn't held up). Setting
    /// `cancel` stops it.
    Net { repo: Repo, op: NetOp, cancel: CancelFlag },
    /// Count the upstream's commits the branch doesn't have, for the force-push warning.
    CountUpstreamOnly { repo: Repo },
    /// Read one commit in full (message, people, changed files) for the details panel.
    LoadDetails { repo: Repo, id: String },
    /// Read one changed file's diff for the diff view.
    LoadDiff { repo: Repo, request: DiffRequest },
    /// Read the working tree's changes for the Commit tab.
    LoadStatus { repo: Repo },
    /// Read one changed file's diff from HEAD to the working tree.
    LoadWorkingDiff { repo: Repo, request: WorkingDiffRequest },
    /// Commit the chosen changes (patches for partly ticked files are built here, from fresh diffs).
    Commit { repo: Repo, plan: CommitPlan },
    /// Find and read the repository's pre-stage file.
    LoadPrestage { repo: Repo },
    /// Write the pre-stage file (at the path found by `LoadPrestage`).
    SavePrestage { path: PathBuf, prestage: Prestage },
    /// Read the last commit's message and whether it was pushed, for Amend.
    LoadAmendInfo { repo: Repo },
    /// Throw away a changed file's changes: the file goes to the Recycle Bin first, then (if git
    /// tracks it) the committed version is restored.
    Discard { repo: Repo, path: String, tracked: bool },
    /// Throw away some lines' changes: a copy of the file goes to the Recycle Bin first, then the
    /// patch (from `FileDiff::discard_patch`) is applied to the working tree.
    DiscardLines { repo: Repo, path: String, patch: Vec<u8> },
    /// Add a line to the repository's `.gitignore`.
    Ignore { repo: Repo, line: String },
    /// Start another program on a file (file manager, default program, "Open with").
    Launch(Launch),
    /// See which of these folders (the recent repositories) are missing. Done here, not on the UI
    /// thread, because a folder on a disconnected network drive can take seconds to answer.
    CheckFolders(Vec<PathBuf>),
}

/// A program to start on a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Reveal(PathBuf),
    Open(PathBuf),
    /// The system's own "Open with" dialog (Windows).
    SystemOpenWith(PathBuf),
    OpenWith {
        program: PathBuf,
        path: PathBuf,
    },
}

/// What the background thread reports back. Each [`Job`] produces exactly one event. Events say
/// which repository and scope they are for, so the UI can ignore ones that arrive after the user
/// has already moved on.
#[derive(Debug)]
pub enum Event {
    Opened {
        repo: Repo,
        refs: Refs,
        history: History,
    },
    OpenFailed {
        path: PathBuf,
        error: GitError,
    },
    LogLoaded {
        repo: Repo,
        scope: LogScope,
        history: History,
    },
    LogFailed {
        repo: Repo,
        scope: LogScope,
        error: GitError,
    },
    Reloaded {
        repo: Repo,
        scope: LogScope,
        result: Result<(Refs, History), GitError>,
    },
    BranchChanged {
        repo: Repo,
        op: BranchOp,
        result: Result<(), GitError>,
    },
    OnlyOnCounted {
        repo: Repo,
        name: String,
        result: Result<usize, GitError>,
    },
    SyncLoaded {
        repo: Repo,
        result: Result<SyncState, GitError>,
    },
    NetDone {
        repo: Repo,
        op: NetOp,
        result: Result<(), GitError>,
    },
    UpstreamOnlyCounted {
        repo: Repo,
        result: Result<usize, GitError>,
    },
    DetailsLoaded {
        repo: Repo,
        id: String,
        result: Result<CommitDetails, GitError>,
    },
    DiffLoaded {
        repo: Repo,
        request: DiffRequest,
        result: Result<FileDiff, GitError>,
    },
    StatusLoaded {
        repo: Repo,
        result: Result<Status, GitError>,
    },
    WorkingDiffLoaded {
        repo: Repo,
        request: WorkingDiffRequest,
        result: Result<FileDiff, GitError>,
    },
    /// The new commit's id, or why git refused.
    Committed {
        repo: Repo,
        result: Result<String, GitError>,
    },
    AmendInfoLoaded {
        repo: Repo,
        result: Result<AmendInfo, GitError>,
    },
    /// A discard or ignore finished; the message says what went wrong, if anything.
    FilesChanged {
        repo: Repo,
        result: Result<(), String>,
    },
    /// A program was started, or couldn't be.
    Launched {
        result: Result<(), String>,
    },
    /// Which of the checked folders are missing.
    FoldersChecked {
        missing: BTreeSet<PathBuf>,
    },
    /// The pre-stage file's location (if git could say) and contents, or why it couldn't be read.
    PrestageLoaded {
        repo: Repo,
        path: Option<PathBuf>,
        result: Result<Prestage, String>,
    },
    /// The pre-stage file was written, or why not.
    PrestageSaved {
        result: Result<(), String>,
    },
}

impl Job {
    /// Small background chores the user doesn't wait for: they don't make the app look busy.
    fn is_quiet(&self) -> bool {
        matches!(self, Job::CheckFolders(_) | Job::SavePrestage { .. })
    }
}

impl Event {
    /// The events of quiet jobs (see `Job::is_quiet`).
    fn is_quiet(&self) -> bool {
        matches!(self, Event::FoldersChecked { .. } | Event::PrestageSaved { .. })
    }
}

/// The UI's handle on the background thread. Dropping it closes the job channel, which ends the
/// thread once it finishes its current job.
pub struct Worker {
    jobs: Sender<Job>,
    events: Receiver<Event>,
    /// Jobs sent whose events haven't come back yet (quiet ones counted apart).
    pending: usize,
    quiet_pending: usize,
    /// False once the thread has gone away (it should only stop if it panics).
    alive: bool,
}

impl Worker {
    /// Starts the background thread. Fails only if the OS can't create a thread.
    pub fn spawn(wake: Wake) -> std::io::Result<Worker> {
        let (job_sender, job_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        // `move` hands the receiving end of the job channel and the sending end of the event
        // channel to the new thread; the UI keeps the other two ends.
        thread::Builder::new().name("gitcraft-git".into()).spawn(move || run(job_receiver, event_sender, wake))?;
        Ok(Worker { jobs: job_sender, events: event_receiver, pending: 0, quiet_pending: 0, alive: true })
    }

    /// Queues a job. Returns false if the worker has stopped.
    pub fn send(&mut self, job: Job) -> bool {
        let quiet = job.is_quiet();
        if self.jobs.send(job).is_ok() {
            if quiet {
                self.quiet_pending += 1;
            } else {
                self.pending += 1;
            }
            true
        } else {
            self.alive = false;
            false
        }
    }

    /// Takes every event that has arrived since the last call, without waiting.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if event.is_quiet() {
                        self.quiet_pending = self.quiet_pending.saturating_sub(1);
                    } else {
                        self.pending = self.pending.saturating_sub(1);
                    }
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The thread has ended, so nothing still pending will ever arrive.
                    self.alive = false;
                    self.pending = 0;
                    self.quiet_pending = 0;
                    break;
                }
            }
        }
        events
    }

    /// True while a job the user waits for is running or queued.
    pub fn is_busy(&self) -> bool {
        self.pending > 0
    }

    /// True while any job, quiet ones included, is running or queued (for tests).
    #[cfg(test)]
    pub fn has_work(&self) -> bool {
        self.pending > 0 || self.quiet_pending > 0
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }
}

/// The thread's main loop.
fn run(jobs: Receiver<Job>, events: Sender<Event>, wake: Wake) {
    // Found on first use and then reused. If git is missing, every job reports it, so installing
    // git while GitCraft is open works without a restart.
    let mut git: Option<Git> = None;

    // Iterating a `Receiver` waits for each job in turn and ends when the UI drops its `Sender`.
    for job in jobs {
        let event = match job {
            Job::Open(path) => match open(&mut git, &path) {
                Ok((repo, refs, history)) => Event::Opened { repo, refs, history },
                Err(error) => Event::OpenFailed { path, error },
            },
            Job::LoadLog { repo, scope } => match located(&mut git).and_then(|git| git.log(&repo, &scope, DEFAULT_LOG_LIMIT)) {
                Ok(log) => Event::LogLoaded { repo, scope, history: History::new(log) },
                Err(error) => Event::LogFailed { repo, scope, error },
            },
            Job::Reload { repo, scope } => {
                let result = located(&mut git).and_then(|git| {
                    let refs = git.refs(&repo)?;
                    let log = git.log(&repo, &scope, DEFAULT_LOG_LIMIT)?;
                    Ok((refs, History::new(log)))
                });
                Event::Reloaded { repo, scope, result }
            }
            Job::Branch { repo, op } => {
                let result = located(&mut git).and_then(|git| match &op {
                    BranchOp::Create { name, start, checkout } => git.create_branch(&repo, name, start.as_deref(), *checkout),
                    BranchOp::Switch { name } => git.switch_branch(&repo, name),
                    BranchOp::Rename { old, new } => git.rename_branch(&repo, old, new),
                    BranchOp::Delete { name } => git.delete_branch(&repo, name),
                });
                Event::BranchChanged { repo, op, result }
            }
            Job::LoadSync { repo } => {
                let result = located(&mut git).and_then(|git| git.sync_state(&repo));
                Event::SyncLoaded { repo, result }
            }
            Job::Net { repo, op, cancel } => {
                let result = located(&mut git).and_then(|git| match &op {
                    NetOp::Fetch => git.fetch(&repo, Some(cancel)),
                    NetOp::Pull => git.pull(&repo, Some(cancel)),
                    NetOp::Push => git.push(&repo, PushMode::Normal, Some(cancel)),
                    NetOp::ForcePush => git.push(&repo, PushMode::ForceWithLease, Some(cancel)),
                    NetOp::Publish { remote, branch } => git.publish(&repo, remote, branch, Some(cancel)),
                });
                Event::NetDone { repo, op, result }
            }
            Job::CountUpstreamOnly { repo } => {
                let result = located(&mut git).and_then(|git| git.upstream_only_commits(&repo));
                Event::UpstreamOnlyCounted { repo, result }
            }
            Job::CountOnlyOn { repo, name } => {
                let result = located(&mut git).and_then(|git| git.commits_only_on(&repo, &name));
                Event::OnlyOnCounted { repo, name, result }
            }
            Job::LoadDetails { repo, id } => {
                let result = located(&mut git).and_then(|git| git.commit_details(&repo, &id));
                Event::DetailsLoaded { repo, id, result }
            }
            Job::LoadDiff { repo, request } => {
                let result = located(&mut git).and_then(|git| git.file_diff(&repo, &request));
                Event::DiffLoaded { repo, request, result }
            }
            Job::LoadStatus { repo } => {
                let result = located(&mut git).and_then(|git| git.status(&repo));
                Event::StatusLoaded { repo, result }
            }
            Job::LoadWorkingDiff { repo, request } => {
                let result = located(&mut git).and_then(|git| git.working_diff(&repo, &request));
                Event::WorkingDiffLoaded { repo, request, result }
            }
            Job::Commit { repo, plan } => {
                let result = located(&mut git).and_then(|git| commit(git, &repo, plan));
                Event::Committed { repo, result }
            }
            Job::LoadPrestage { repo } => match located(&mut git).and_then(|git| git.git_path(&repo, prestage::FILE_NAME)) {
                Ok(path) => {
                    let result = prestage::load(&path);
                    Event::PrestageLoaded { repo, path: Some(path), result }
                }
                Err(error) => {
                    Event::PrestageLoaded { repo, path: None, result: Err(format!("Could not find where to keep GitCraft's ticks: {error}")) }
                }
            },
            Job::SavePrestage { path, prestage } => {
                let result =
                    prestage::save(&path, &prestage).map_err(|error| format!("Could not save GitCraft's ticks ({}): {error}", path.display()));
                Event::PrestageSaved { result }
            }
            Job::LoadAmendInfo { repo } => {
                let result = located(&mut git).and_then(|git| git.amend_info(&repo));
                Event::AmendInfoLoaded { repo, result }
            }
            Job::Discard { repo, path, tracked } => {
                let result = discard(&mut git, &repo, &path, tracked);
                Event::FilesChanged { repo, result }
            }
            Job::DiscardLines { repo, path, patch } => {
                let result = discard_lines(&mut git, &repo, &path, patch);
                Event::FilesChanged { repo, result }
            }
            Job::Ignore { repo, line } => {
                let result = files::add_to_gitignore(repo.root(), &line).map(|_| ()).map_err(|error| format!("Could not update .gitignore: {error}"));
                Event::FilesChanged { repo, result }
            }
            Job::Launch(launch) => Event::Launched { result: start(&launch) },
            Job::CheckFolders(paths) => Event::FoldersChecked { missing: paths.into_iter().filter(|path| !path.is_dir()).collect() },
        };
        if events.send(event).is_err() {
            break; // The UI has gone away.
        }
        wake();
    }
}

/// Commits a plan: each partly ticked file's diff is read again and its picks moved onto it, so
/// the patch matches the file as it is now; then the whole request goes to git.
fn commit(git: &Git, repo: &Repo, plan: CommitPlan) -> Result<String, GitError> {
    let mut request = plan.request;
    for file in plan.partial {
        let diff = git.working_diff(repo, &WorkingDiffRequest { path: file.path.clone(), new_file: file.new_file })?;
        if !diff.can_pick_lines() {
            return Err(GitError::Unsupported(format!(
                "{}: single lines can no longer be picked in this file (too many changes to show). Tick or untick the whole file.",
                file.path
            )));
        }
        match file.picks.patch(&diff, &file.path).map_err(GitError::Unsupported)? {
            Some(partial) => request.partial.push(partial),
            // Nothing ticked after all: leave the file out.
            None => request.exclude.push(file.path),
        }
    }
    git.commit(repo, &request)
}

/// Discards some lines of one file. A copy of the file goes to the Recycle Bin first; if that
/// fails, nothing is changed.
fn discard_lines(git: &mut Option<Git>, repo: &Repo, path: &str, patch: Vec<u8>) -> Result<(), String> {
    let git = located(git).map_err(|error| error.to_string())?;
    let full = files::full_path(repo.root(), path);
    files::copy_to_trash(&full)
        .map_err(|error| format!("Could not put a copy of {path} in the {}, so nothing was changed: {error}", files::TRASH_NAME))?;
    git.discard_lines(repo, patch).map_err(|error| format!("Could not discard the lines in {path} (the file may have changed meanwhile): {error}"))
}

/// Discards one file's changes. The file on disk goes to the Recycle Bin first; if that fails,
/// nothing is changed. Only then does git put back the committed version (for a tracked file).
fn discard(git: &mut Option<Git>, repo: &Repo, path: &str, tracked: bool) -> Result<(), String> {
    let git = located(git).map_err(|error| error.to_string())?;
    let full = files::full_path(repo.root(), path);
    // `symlink_metadata` doesn't follow a symlink, so a link is recycled itself, not its target.
    if full.symlink_metadata().is_ok() {
        files::move_to_trash(&full)
            .map_err(|error| format!("Could not move {path} to the {}, so nothing was changed: {error}", files::TRASH_NAME))?;
    }
    if tracked {
        git.discard(repo, path)
            .map_err(|error| format!("{path} was moved to the {}, but putting back the committed version failed: {error}", files::TRASH_NAME))?;
    }
    Ok(())
}

/// Starts a program on a file, without waiting for it.
fn start(launch: &Launch) -> Result<(), String> {
    let (result, what) = match launch {
        Launch::Reveal(path) => (files::reveal(path), format!("show it in {}", files::FILE_MANAGER_NAME)),
        Launch::Open(path) => (files::open(path), "open it".to_string()),
        Launch::SystemOpenWith(path) => (files::open_with_system_dialog(path), "show the \"Open with\" dialog".to_string()),
        Launch::OpenWith { program, path } => (files::open_with(program, path), format!("open it with {}", program.display())),
    };
    result.map_err(|error| format!("Could not {what}: {error}"))
}

fn open(git: &mut Option<Git>, path: &std::path::Path) -> Result<(Repo, Refs, History), GitError> {
    let git = located(git)?;
    let repo = git.open_repo(path)?;
    let refs = git.refs(&repo)?;
    let log = git.log(&repo, &LogScope::AllBranches, DEFAULT_LOG_LIMIT)?;
    // The graph is laid out here, on the background thread, not in the UI.
    Ok((repo, refs, History::new(log)))
}

/// The cached `Git`, locating it first if needed.
fn located(cache: &mut Option<Git>) -> Result<&Git, GitError> {
    if cache.is_none() {
        *cache = Some(Git::locate()?);
    }
    // Just filled in above, so this is never `NotFound` in practice; it keeps the code panic-free.
    cache.as_ref().ok_or(GitError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestRepo, wait_until};

    fn worker() -> Worker {
        Worker::spawn(Arc::new(|| {})).unwrap()
    }

    /// Polls until one event arrives (or fails the test after a timeout).
    fn next_event(worker: &mut Worker) -> Event {
        let mut received = None;
        wait_until(|| {
            received = worker.poll().into_iter().next();
            received.is_some()
        });
        received.unwrap()
    }

    #[test]
    fn opens_a_repository_in_the_background() {
        let repo = TestRepo::with_commits(&["First", "Second"]);
        let mut worker = worker();
        assert!(worker.send(Job::Open(repo.path().to_path_buf())));
        assert!(worker.is_busy());

        match next_event(&mut worker) {
            Event::Opened { history, refs, .. } => {
                let log = history.log;
                let summaries: Vec<_> = log.commits.iter().map(|c| c.summary.as_str()).collect();
                assert_eq!(summaries, ["Second", "First"]);
                assert_eq!(refs.head_branch(), Some("refs/heads/main"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(!worker.is_busy());
        assert!(worker.is_alive());
    }

    #[test]
    fn reports_failures_with_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = worker();
        worker.send(Job::Open(dir.path().to_path_buf()));
        match next_event(&mut worker) {
            Event::OpenFailed { path, error: GitError::NotARepository { .. } } => assert_eq!(path, dir.path()),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn handles_jobs_in_order() {
        let first = TestRepo::with_commits(&["A"]);
        let second = TestRepo::with_commits(&["B1", "B2"]);
        let mut worker = worker();
        worker.send(Job::Open(first.path().to_path_buf()));
        worker.send(Job::Open(second.path().to_path_buf()));

        let counts: Vec<usize> = (0..2)
            .map(|_| match next_event(&mut worker) {
                Event::Opened { history, .. } => history.log.commits.len(),
                other => panic!("unexpected event: {other:?}"),
            })
            .collect();
        assert_eq!(counts, [1, 2]);
    }

    #[test]
    fn reloads_the_log_for_one_branch() {
        let repo = TestRepo::with_commits(&["Base"]);
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "Feature work"]);
        repo.git(&["checkout", "-q", "main"]);
        let mut worker = worker();
        worker.send(Job::Open(repo.path().to_path_buf()));
        let Event::Opened { repo: opened, history, .. } = next_event(&mut worker) else {
            panic!("expected Opened");
        };
        assert_eq!(history.log.commits.len(), 2, "all branches");
        assert_eq!(history.rows.len(), 2, "one graph row per commit");

        let scope = LogScope::Ref("refs/heads/main".into());
        worker.send(Job::LoadLog { repo: opened.clone(), scope: scope.clone() });
        match next_event(&mut worker) {
            Event::LogLoaded { repo, scope: loaded, history } => {
                let log = history.log;
                assert_eq!(repo, opened);
                assert_eq!(loaded, scope);
                assert_eq!(log.commits.iter().map(|c| c.summary.as_str()).collect::<Vec<_>>(), ["Base"]);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        worker.send(Job::LoadLog { repo: opened, scope: LogScope::Ref("refs/heads/gone".into()) });
        assert!(matches!(next_event(&mut worker), Event::LogFailed { .. }));
    }

    #[test]
    fn wakes_the_ui_after_each_event() {
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        let mut worker = Worker::spawn(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        worker.send(Job::Open(dir.path().to_path_buf()));
        next_event(&mut worker);
        wait_until(|| woken.load(std::sync::atomic::Ordering::SeqCst) == 1);
    }
}
