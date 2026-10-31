# Status

Last updated: 2026-10-31

## Milestones 1 and 2 are done. Current: Milestone 3, Git actions (see below)

Plan (approved 2026-10-31): a Cargo workspace (`crates/app`, `crates/git`); licence MIT OR
Apache-2.0, matching craft-libs.

| Step | What | State |
|---|---|---|
| 1 | Workspace scaffold, lints, craft-libs pin, empty window | done |
| 4a | Settings: remember window size and maximized state (done early, on request) | done |
| 2 | `gitcraft-git`: locate git, run commands, validate repo, parse log | done |
| 3 | Worker thread, folder picker, commit list, status bar | done |
| 4b | Settings: remember and reopen the last repository | done in R7 below |

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
| R5 | Two panes (craft-ui split), commit selection, details panel | done |
| R6 | Diff parsing and diff view | done |
| R7 | Reopen last repository; remember divider and branch per repository | done |

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

R5 details:
- Git layer: `commit_details(repo, id)` reads the full message, author and committer (with times),
  parents, and changed files with status and +/- line counts (`None` for binary files), from
  `git show` and `git diff-tree -z` (`--name-status` and `--numstat`, joined by path). A merge is
  compared with its first parent; a root commit shows every file as added. The id must be a full hash.
- Two panes via craft-ui `split_rect` and `ResizeHandle` (drag, or focus and press the arrow keys).
  The ratio lives in memory for now; R7 saves it.
- Clicking a row selects it; Up/Down move the selection when nothing else has focus and keep it in
  view. Details load in the background; a result for a commit no longer selected is ignored.
  Switching branch keeps the selection if the commit is still shown.
- Details panel, collapsed: summary, author (avatar) and short hash, and the description cut to
  3 lines with "…" (blank lines skipped in this preview), then a "More info" link. Expanded: the full
  description, date, committer (only if different), full hash (selectable) and parent links
  (click to select). The choice is sticky across commits (in memory; R7 could save it). The changed
  files with coloured status letters and +/- counts always follow.
- The summary tooltip in the list is gone (the panel shows it all).
- Verified on Windows: merge and ordinary commits, divider drag, parent link, Down arrow.

R6 details:
- Git layer: `file_diff(repo, DiffRequest)` runs `git diff-tree -p` for one file against the first
  parent (or `--root`), with `--no-ext-diff --no-textconv --no-color` and fixed `a/` `b/` prefixes,
  so the user's config can neither change the output nor run programs. Renames pass both paths.
  Output is capped at 4 MiB (an over-limit diff shows "too large to show") and 20,000 lines
  (`truncated`). A pure parser builds hunks of numbered lines, handling binary files, "no newline"
  markers, CRLF, and over-long lines (cut at 2,000 characters). It never fails.
- All git commands now set `GIT_LITERAL_PATHSPECS=1`, so file names are never read as patterns.
  A test with `weird[1].txt` next to a `weird1.txt` decoy proves it.
- The right pane is split vertically (details above, diff below), and both dividers can be dragged.
  `RepoLayout` holds both ratios and the "More info" flag (in memory; R7 saves them).
- Selecting a commit loads its details and then automatically its first file's diff. Clicking a file
  in the list shows its diff. Late results for an earlier commit or file are ignored.
- Diff view: old and new line numbers, +/- markers, green and red tints, hunk headers, tabs as 4 spaces,
  horizontal scrolling for long lines. Only visible rows are laid out. Each commit and file starts
  scrolled to the top (scroll areas have their own ids).
- Verified on Windows: auto-selected first file, clicking another file, a new file shown all green.

R7 details:
- Settings gain `[layout]` (both divider ratios, rounded to 3 places, and "More info") and
  `[repositories]` (up to 20 recent repositories, newest first, each with its branch or none for
  "All branches"). Paths are compared as paths, so `/` and `\` match. Non-Unicode paths aren't saved.
- Startup reopens the newest repository in the background. If it has gone, the status bar says why
  and the welcome page shows. The saved branch is restored if it still exists, otherwise forgotten.
- The branch is saved once its history has loaded (so a failed load doesn't save it).
- Damaged ratios are clamped (0.1 to 0.9) or reset (not a number). The schema version stays 1:
  missing sections fall back to defaults.
- Verified on Windows with a hand-written config (forward slashes): the repository, branch and dividers
  were restored, and the file came back with a single entry.

## Milestone 3: Git actions (plan approved 2026-10-31)

Decisions: the Commit tab works like GitHub Desktop: every change starts ticked, and files, hunks or
single lines can be unticked; Commit stages exactly what is ticked (`git add` for whole files,
`git apply --cached` with a generated patch for partial ones) and then commits. Unticked changes stay
in the working tree. If something is already staged when the tab first reads it (`git add` in a
terminal), the ticks start out matching the index exactly. Pull follows the user's git config,
exactly like the command line. Submodules as designed (G7).

| Step | What | State |
|---|---|---|
| G1 | Commit tab: changed files (`git status`) and each file's diff from HEAD to the working tree | done |
| G2 | File checkboxes, summary/description, Commit (Ctrl+Enter), Amend; hook output on failure | next |
| G3 | Hunk and line checkboxes; pure patch builder for `git apply --cached` | |
| G4 | Discard changes (file, hunk, lines) after a confirmation naming what is lost | |
| G5 | Branches: new, checkout, rename, delete (with confirmations) | |
| G6 | Repository menu: fetch, pull, push on a second worker; cancel; `--force-with-lease` only | |
| G7 | Submodules: Repository → Submodules menu, list, open with breadcrumb, Commit tab entries | |

G1 details:
- Git layer: `status(repo)` runs `git status --porcelain=v2 -z --untracked-files=all --no-renames
  --ignore-submodules=none`; a pure parser reads ordinary, rename, conflict and untracked records,
  rejects paths that leave the repository (platform path rules) and stops at 50,000 entries.
  `StatusEntry::change()` combines staged and unstaged into one HEAD-to-working-tree change;
  `has_staged()` will seed the ticks in G2.
- `working_diff(repo, request)`: `git diff HEAD -- path`, or, for a file not in HEAD (untracked,
  newly added, or no commits yet), `git diff --no-index -- /dev/null path` (exit code 1 accepted
  via the new `RunOptions::also_ok`). Same safety flags and limits as commit diffs.
- Commit tab: the file list (status letter, path) and the selected file's diff, side by side with a
  draggable divider (saved as `changes_ratio`). The first file is selected automatically.
- Refreshes: when the repository opens, when the Commit tab is shown, when the window gets the focus
  back, and on F5. A refresh asked for while one is running is queued once, never piled up. The
  selection stays if the file is still changed; a refreshed diff replaces the old one without a
  spinner.
- The clickable file row is shared with the commit details panel (`ui/file_row.rs`).
- Verified on Windows with the demo repository: a modified file (two hunks), a deleted file (all
  red) and an untracked file (all green).

## Backlog

Ideas agreed for later, not scheduled into a step yet.

- Commit tab: replace the status letters (M, A, D, !) with drawn icons (generic shapes, no third-party marks).
- Commit tab: detect renames in the working tree and show them as one renamed file (as GitHub Desktop
  does), not as a deleted plus an added file. Needs rename detection that includes untracked files
  (e.g. intent-to-add or comparing contents) and patch generation that keeps both halves together.

## Open questions

- None.

## To verify on Fedora (deferred; batched for later)

- Window opens under Wayland; close works.
- Window size restore. Wayland compositors may ignore the requested size or maximized state.
- Reopening the last repository, branch and dividers after a restart (`~/.config/gitcraft/config.toml`).
- Commit tab: status and diffs on Linux (`/dev/null` with `--no-index`, file names with `:` or a backslash), and the focus refresh under Wayland.

## Known issues / notes

- The window and taskbar icon is eframe's default (the egui logo). GitCraft needs its own generic
  icon (Polish milestone; list it in ATTRIBUTION.md).
- No way to load history past the first 10,000 commits yet.

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
