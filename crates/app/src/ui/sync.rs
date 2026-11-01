//! The toolbar's sync button: fetch, pull, push or publish, whichever fits now, like GitHub
//! Desktop's. While a network command runs it shows what is happening and a Cancel button.

use gitcraft_git::SyncState;

use crate::ui::toolbar_box::toolbar_box;

/// A network command, run on the network worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetOp {
    Fetch,
    Pull,
    Push,
    /// Push a branch that has no upstream yet, and make the pushed branch its upstream.
    Publish {
        remote: String,
        branch: String,
    },
    /// `--force-with-lease` (asked first).
    ForcePush,
    /// Clone a submodule and check out its recorded commit.
    SubmoduleInit {
        path: String,
    },
    /// Check out a submodule's recorded commit (asked first).
    SubmoduleUpdate {
        path: String,
    },
}

impl NetOp {
    /// What the status shows while it runs ("Pushing…").
    pub fn running_label(&self) -> &'static str {
        match self {
            NetOp::Fetch => "Fetching…",
            NetOp::Pull => "Pulling…",
            NetOp::Push | NetOp::ForcePush => "Pushing…",
            NetOp::Publish { .. } => "Publishing…",
            NetOp::SubmoduleInit { .. } => "Initialising submodule…",
            NetOp::SubmoduleUpdate { .. } => "Updating submodule…",
        }
    }

    /// The start of an error message ("Could not push").
    pub fn failed(&self) -> &'static str {
        match self {
            NetOp::Fetch => "Could not fetch",
            NetOp::Pull => "Could not pull",
            NetOp::Push => "Could not push",
            NetOp::ForcePush => "Could not force push",
            NetOp::Publish { .. } => "Could not publish the branch",
            NetOp::SubmoduleInit { .. } => "Could not initialise the submodule",
            NetOp::SubmoduleUpdate { .. } => "Could not update the submodule",
        }
    }
}

/// What the box offers now: the command, its title ("Push"), the status under it ("Push 2
/// commits") and a hover text. `None` for the command when nothing can be synced (the title and
/// status then say why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub op: Option<NetOp>,
    pub title: String,
    pub status: String,
    pub tooltip: String,
}

/// The remote a new branch is published to: `origin` if there is one, otherwise the first.
pub fn publish_remote(sync: &SyncState) -> Option<&str> {
    sync.remotes.iter().find(|remote| *remote == "origin").or(sync.remotes.first()).map(String::as_str)
}

/// "1 commit", "3 commits".
fn commits(count: usize) -> String {
    if count == 1 { "1 commit".to_string() } else { format!("{count} commits") }
}

/// What the box should offer for this state. Pure, so each case can be tested.
pub fn suggestion(sync: Option<&SyncState>) -> Suggestion {
    let none =
        |title: &str, status: &str, tooltip: &str| Suggestion { op: None, title: title.into(), status: status.into(), tooltip: tooltip.into() };
    let Some(sync) = sync else {
        return none("Sync", "Reading the remotes…", "Reading the remotes…");
    };
    let Some(remote) = publish_remote(sync) else {
        return none("No remote", "Nothing to sync with", "This repository has no remote to sync with (add one with git remote add).");
    };
    let Some(branch) = &sync.branch else {
        return none("Detached HEAD", "Check out a branch", "Check out a branch to sync it.");
    };
    let Some(upstream) = &sync.upstream else {
        return Suggestion {
            op: Some(NetOp::Publish { remote: remote.to_string(), branch: branch.clone() }),
            title: "Publish branch".into(),
            status: format!("Not on {remote} yet"),
            tooltip: format!("Push {branch} to {remote} for the first time, and keep it in sync from now on."),
        };
    };
    let counts = format!("{} to push, {} to pull (as of the last fetch).", sync.ahead, sync.behind);
    if sync.behind > 0 {
        let also_push = if sync.ahead > 0 { format!(", then {} to push", sync.ahead) } else { String::new() };
        Suggestion {
            op: Some(NetOp::Pull),
            title: "Pull".into(),
            status: format!("Pull {}{also_push}", commits(sync.behind)),
            tooltip: format!("Bring in the new commits from {upstream}. {counts}"),
        }
    } else if sync.ahead > 0 {
        Suggestion {
            op: Some(NetOp::Push),
            title: "Push".into(),
            status: format!("Push {}", commits(sync.ahead)),
            tooltip: format!("Send your commits to {upstream}. {counts}"),
        }
    } else {
        Suggestion {
            op: Some(NetOp::Fetch),
            title: format!("Fetch {remote}"),
            status: "Up to date".into(),
            tooltip: format!("Check {remote} for new commits. {upstream} is up to date as of the last fetch."),
        }
    }
}

/// What was clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPick {
    Run(NetOp),
    Cancel,
}

/// Draws the Push/Pull box: the action on top and its status below. While `running`, it shows what
/// is happening with a spinner, and a click cancels it.
pub fn sync_box(ui: &mut egui::Ui, sync: Option<&SyncState>, running: Option<&NetOp>) -> Option<SyncPick> {
    if let Some(op) = running {
        let response = toolbar_box(ui, op.running_label(), "Click to cancel", false, true).on_hover_text("Stop it (nothing half-done is kept)");
        let spinner = egui::Rect::from_center_size(egui::pos2(response.rect.right() - 18.0, response.rect.center().y), egui::vec2(16.0, 16.0));
        ui.put(spinner, egui::Spinner::new());
        return response.clicked().then_some(SyncPick::Cancel);
    }
    let suggestion = suggestion(sync);
    let enabled = suggestion.op.is_some();
    let response = toolbar_box(ui, &suggestion.title, &suggestion.status, false, enabled);
    let response = if enabled { response.on_hover_text(&suggestion.tooltip) } else { response.on_disabled_hover_text(&suggestion.tooltip) };
    if response.clicked() { suggestion.op.map(SyncPick::Run) } else { None }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn state(upstream: Option<&str>, ahead: usize, behind: usize) -> SyncState {
        SyncState { remotes: vec!["origin".into()], branch: Some("main".into()), upstream: upstream.map(str::to_string), ahead, behind }
    }

    #[test]
    fn suggests_what_fits() {
        let fetch = suggestion(Some(&state(Some("origin/main"), 0, 0)));
        assert_eq!((fetch.op, fetch.title.as_str(), fetch.status.as_str()), (Some(NetOp::Fetch), "Fetch origin", "Up to date"));
        let pull = suggestion(Some(&state(Some("origin/main"), 1, 3)));
        assert_eq!((pull.op, pull.title.as_str()), (Some(NetOp::Pull), "Pull"), "pull comes first: a push would be refused");
        assert_eq!(pull.status, "Pull 3 commits, then 1 to push");
        let push = suggestion(Some(&state(Some("origin/main"), 1, 0)));
        assert_eq!((push.op, push.title.as_str(), push.status.as_str()), (Some(NetOp::Push), "Push", "Push 1 commit"));
        let publish = suggestion(Some(&state(None, 0, 0)));
        assert_eq!(publish.op, Some(NetOp::Publish { remote: "origin".into(), branch: "main".into() }));
        assert_eq!((publish.title.as_str(), publish.status.as_str()), ("Publish branch", "Not on origin yet"));
    }

    #[test]
    fn says_why_nothing_can_sync() {
        assert_eq!(suggestion(None).op, None);
        let no_remote = SyncState { remotes: vec![], ..state(None, 0, 0) };
        assert_eq!(suggestion(Some(&no_remote)).title, "No remote");
        let detached = SyncState { branch: None, ..state(None, 0, 0) };
        assert_eq!(suggestion(Some(&detached)).title, "Detached HEAD");
    }

    #[test]
    fn publishes_to_origin_or_the_first_remote() {
        let fork = SyncState { remotes: vec!["fork".into(), "origin".into()], ..state(None, 0, 0) };
        assert_eq!(publish_remote(&fork), Some("origin"));
        let only = SyncState { remotes: vec!["upstream".into()], ..state(None, 0, 0) };
        assert_eq!(publish_remote(&only), Some("upstream"));
    }

    #[test]
    fn draws_idle_and_running() {
        let ctx = egui::Context::default();
        let synced = state(Some("origin/main"), 1, 0);
        for running in [None, Some(&NetOp::Push)] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert_eq!(sync_box(ui, Some(&synced), running), None);
            });
            output.textures_delta.clear();
        }
    }
}
