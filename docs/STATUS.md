# Status

Last updated: 2026-10-31

## Current milestone: 1, Skeleton

Plan (approved 2026-10-31): a Cargo workspace (`crates/app`, `crates/git`); licence MIT OR
Apache-2.0, matching craft-libs.

| Step | What | State |
|---|---|---|
| 1 | Workspace scaffold, lints, craft-libs pin, empty window | done |
| 4a | Settings: remember window size and maximized state (done early, on request) | done |
| 2 | `gitcraft-git`: locate git, run commands, validate repo, parse log | done |
| 3 | Worker thread, folder picker, commit list, status bar | done |
| 4b | Settings: remember and reopen the last repository | moved to R7 below |

## Done

- Workspace with CLAUDE.md lints, craft-libs pinned at `c39e435cc34417843793df240a0b38bac970179b`,
  egui/winit fork patches copied from craft-libs.
- `gitcraft` app opens a "GitCraft" window; closing goes through `craft-quit`.
- Window size and maximized state are saved with `craft-settings` (`%APPDATA%\gitcraft\config.toml`
  on Windows) and restored on the next launch. The normal size is kept while maximized, so
  un-maximizing returns to it. Saved sizes are clamped (480×320 to 16384) and non-numbers fall back
  to defaults. Settings problems (damaged file, read-only folder, failed save) show in the status bar.
- Verified on Windows: resize → restart keeps size; maximize → restart opens maximized, and restoring
  returns to the earlier size.
- `gitcraft-git` crate (std only, no UI):
  - `Git::locate()` finds `git` (`git.exe` only on Windows) on the PATH, skipping relative PATH
    entries, and checks `git --version`.
  - `Git::run`: no shell, no prompts (`GIT_TERMINAL_PROMPT=0`), English messages, no optional
    locks, no console window on Windows, 60 s timeout, 64 MiB output cap, stdout and stderr read on
    separate threads. Error messages name only the subcommand, never the full arguments.
  - `Git::open_repo` resolves the top of the working tree. Specific errors for a missing folder,
    not a repository, a bare repository or `.git` folder, and dubious ownership (`safe.directory`,
    with the command to fix it).
  - `Git::log(repo, limit)` reads HEAD's history (`-z`, NUL-separated fields). An empty repo gives
    an empty log, and `truncated` is set when there are more commits than the limit. The parser
    validates hashes (SHA-1 and SHA-256), parents and times; invalid UTF-8 in names is replaced.
  - Try it by hand: `cargo run -p gitcraft-git --example log -- <folder> [limit]`.
- App, step 3:
  - `worker.rs`: one background thread owns `Git`. `Job::Open(path)` gives back `Event::Opened` or
    `Event::Failed` and wakes the UI. A stopped thread is detected (closed channel) and reported.
  - Folder picker via `craft-dialogs`: answers are handled in `logic`, dialogs shown at the end of `ui`.
  - Commit table (`egui_extras`): hash (full hash on hover), summary, author (email on hover) and
    local date. Columns are resizable and only visible rows are laid out.
  - Status bar: errors in the error colour, a spinner while loading, the commit count, and a
    truncation notice past 10,000 commits. A failed open keeps the current repository on screen.
  - Verified on Windows: opened this repo through the real folder picker; opening a non-repository
    showed the error and kept the history. A 12,000-commit repository loads 10,000 in about 0.3 s
    (debug build).

## UI rework (approved 2026-10-31; brings Milestone 2 forward)

Decisions: initials avatars for now (GitHub avatars opt-in after sign-in, Milestone 4); right pane
shows commit info, then the changed files, then the selected file's diff; the branch dropdown only
filters (no checkout until Milestone 3); toolbar has the branch dropdown at the far left, then the
repository name (full path on hover and in the window title).

| Step | What | State |
|---|---|---|
| R1 | Commit step 3 | done (`f22430c`) |
| R2 | Window shell: menu bar (craft-menu), tabs (craft-ui), repository toolbar | done |
| R3 | Refs + branch dropdown filtering the history | done |
| R4 | Lane assignment (pure, tested), graph drawing, ref labels, initials avatars | done |
| R5 | Two panes (craft-ui split), commit selection, details panel | next |
| R6 | Diff parsing and diff view | |
| R7 | Reopen last repository; remember divider and branch per repository | |

R2 details:
- File menu: Open Repository… (Ctrl+O), then Exit (added by craft-menu's layout). Help: About GitCraft.
- Tabs: Repository, Issues and Projects (the last two are placeholders until Milestones 5 and 6).
- Repository toolbar: branch dropdown ("All branches" only for now), then the repository name.
- Window title: "GitCraft — <path>" while a repository is open.
- Shortcuts wait while a menu, dialog or the About window is open.
- Verified on Windows: Ctrl+O, the File and Help menus, About and Close, Exit, and tab switching.

R3 details:
- Git layer: `refs()` (branches, remote branches, tags with annotated tags peeled, HEAD:
  branch/unborn/detached; `origin/HEAD` symrefs skipped), and `log(repo, scope, limit)` with
  `LogScope::AllBranches` (branches, remotes, tags and HEAD, `--date-order`) or `LogScope::Ref`.
  Ref names are validated (must start with `refs/`, no control characters) before reaching git.
- Dropdown: All branches, then Local (current branch marked), then Remote. Choosing one reloads the
  history in the background. Late results for a branch or repository the user has left are
  ignored. A failed load (branch deleted meanwhile) puts the dropdown back and shows the error.
- Opening a repository now shows all branches (it used to show only HEAD's history).
- Commit tab added as the first tab (placeholder until Milestone 3); the app starts on Repository.

R4 details:
- `graph/lanes.rs`: `assign_lanes(&[Commit]) -> Vec<GraphRow>`, a pure function. Each row has the
  commit's lane and colour, plus the segments crossing its top and bottom halves. Lanes never move
  sideways and freed lanes are reused. Unit tests cover linear history, a merge with its fork point,
  two tips from one commit, a merge parent that is already waited for, an octopus merge, lane
  reuse, truncated history, duplicated parents, and segments staying inside the row width.
- `graph/paint.rs`: straight lines within a lane, S-curves (cubic Béziers) between lanes, an
  8-colour palette, a dot per commit (smaller for merges). The column
  is capped at 16 lanes.
- Lanes are computed on the worker thread (`graph::History`), not in the UI.
- Avatars (initials) sit before each summary, not in the graph. Each author keeps one colour,
  from an FNV-1a hash of their email (top bits; the low bits barely mix).
- Labels beside the summary: the checked-out branch first (bold), then local branches (filled in
  the lane colour), remote branches (outlined) and tags (grey). A detached HEAD gets a "HEAD" label.
- The list shows only the graph and summaries (with labels). Author, date and hash move to the
  details panel (R5); until then, hovering a summary shows them.
- Verified on Windows with a demo repository (two feature branches, a merge, a tag, a remote).

## Open questions

- Submodules (Milestone 3): proposed design given to the user on 2026-10-31, awaiting a decision.

## To verify on Fedora (deferred; batched for later)

- Window opens under Wayland; close works.
- Window size restore. Wayland compositors may ignore the requested size or maximized state.

## Known issues / notes

- The window and taskbar icon is eframe's default (the egui logo). GitCraft needs its own generic
  icon (Polish milestone; list it in ATTRIBUTION.md).
- No way to load history past the first 10,000 commits yet.

- The log follows HEAD only. Milestone 2 (graph, branch filter) will decide on `--all` and
  `--topo-order`.
- Running git on a timeout leaves its reader threads blocked if a grandchild process (such as ssh)
  keeps the pipes open. That only matters for network commands (Milestone 3), so revisit then.

- The `[patch.crates-io]` lines in the root `Cargo.toml` must be updated whenever the craft-libs rev
  changes.
- The suite's egui fork panics if a frame's `TexturesDelta` is dropped unhandled; headless tests
  must call `output.textures_delta.clear()`.
- Window position is not saved (size only). It can't be set on Wayland anyway; it could be added for
  Windows if wanted.
- Set `GITCRAFT_CONFIG_DIR` to use a different settings folder (empty means don't save at all), handy for testing.
- Fonts: egui's default fonts for now; craft-fonts is not wired in yet.
