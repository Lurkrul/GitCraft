//! The background thread that runs git, so the UI thread never waits on it.
//!
//! The UI sends a [`Job`] down one channel; the worker runs it and sends an [`Event`] back up
//! another, then wakes the UI. The UI collects finished events each frame with [`Worker::poll`],
//! which never blocks. Channels here are `std::sync::mpsc`: a thread-safe queue with one
//! receiving end, like a `std::queue` guarded by a mutex and condition variable in C++.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use gitcraft_git::{CommitDetails, DEFAULT_LOG_LIMIT, DiffRequest, FileDiff, Git, GitError, LogScope, Refs, Repo, Status, WorkingDiffRequest};

use crate::graph::History;

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
    /// Read one commit in full (message, people, changed files) for the details panel.
    LoadDetails { repo: Repo, id: String },
    /// Read one changed file's diff for the diff view.
    LoadDiff { repo: Repo, request: DiffRequest },
    /// Read the working tree's changes for the Commit tab.
    LoadStatus { repo: Repo },
    /// Read one changed file's diff from HEAD to the working tree.
    LoadWorkingDiff { repo: Repo, request: WorkingDiffRequest },
}

/// What the background thread reports back. Each [`Job`] produces exactly one event. Events say
/// which repository and scope they are for, so the UI can ignore ones that arrive after the user
/// has already moved on.
#[derive(Debug)]
pub enum Event {
    Opened { repo: Repo, refs: Refs, history: History },
    OpenFailed { path: PathBuf, error: GitError },
    LogLoaded { repo: Repo, scope: LogScope, history: History },
    LogFailed { repo: Repo, scope: LogScope, error: GitError },
    DetailsLoaded { repo: Repo, id: String, result: Result<CommitDetails, GitError> },
    DiffLoaded { repo: Repo, request: DiffRequest, result: Result<FileDiff, GitError> },
    StatusLoaded { repo: Repo, result: Result<Status, GitError> },
    WorkingDiffLoaded { repo: Repo, request: WorkingDiffRequest, result: Result<FileDiff, GitError> },
}

/// The UI's handle on the background thread. Dropping it closes the job channel, which ends the
/// thread once it finishes its current job.
pub struct Worker {
    jobs: Sender<Job>,
    events: Receiver<Event>,
    /// Jobs sent whose events haven't come back yet.
    pending: usize,
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
        Ok(Worker { jobs: job_sender, events: event_receiver, pending: 0, alive: true })
    }

    /// Queues a job. Returns false if the worker has stopped.
    pub fn send(&mut self, job: Job) -> bool {
        if self.jobs.send(job).is_ok() {
            self.pending += 1;
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
                    self.pending = self.pending.saturating_sub(1);
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The thread has ended, so nothing still pending will ever arrive.
                    self.alive = false;
                    self.pending = 0;
                    break;
                }
            }
        }
        events
    }

    /// True while a job is running or queued.
    pub fn is_busy(&self) -> bool {
        self.pending > 0
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
        };
        if events.send(event).is_err() {
            break; // The UI has gone away.
        }
        wake();
    }
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
