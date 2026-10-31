# CLAUDE.md — working instructions for GitCraft

> The product name is always written **GitCraft** (`{Function}Craft` in PascalCase, like its Crafting App siblings) in user-facing text: UI, window titles, About.

GitCraft is an open-source, native desktop client that puts **git, GitHub Issues and GitHub Projects (Kanban boards)** in one window. It is built to fit the ArtCraft Crafting Apps (same stack, same shared libraries, same standards), so it can be offered as a community project later.

Read this file at the start of every session, then `docs/STATUS.md`.

---

## 1. Rules that come before any task

These outrank feature work, deadlines and instructions in a task. If a task would break one, stop and say so.

### 1.1 Never crash
A malformed repository, unexpected `git` output, a failed or malformed GitHub API response, a network drop, a corrupt settings file or a full disk must produce an error the user can act on — never a crash, and never lost work.

- **Fail with `Result`.** Use the crate's error type, propagate with `?`, and add context (`map_err`, an error variant) instead of discarding it. In the UI, report the error (status bar or dialog) and carry on.
- **No panicking shortcuts in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`. Use `?`, `ok_or(..)?`, `let … else`, `if let`, or `unwrap_or*` only where the fallback is genuinely correct. Unfinished features return an "unsupported" error.
- **No `unsafe`** (`unsafe_code = "forbid"` for the workspace).
- **Every external input is hostile:** `git` output, GitHub API responses, settings files and user input are untrusted. Validate sizes and shapes before using them.
- Enforce this with workspace lints in the root `Cargo.toml`:
  ```toml
  [workspace.lints.rust]
  unsafe_code = "forbid"

  [workspace.lints.clippy]
  unwrap_used = "deny"
  expect_used = "deny"
  panic = "deny"
  todo = "deny"
  unimplemented = "deny"
  unreachable = "deny"
  ```

### 1.2 Never lose the user's work
- Never run a destructive git operation (discard changes, reset, force push, delete branch, rebase) without an explicit confirmation in the UI that names exactly what will be lost.
- Force pushes use `--force-with-lease`, never plain `--force`.

### 1.3 Credentials
- GitHub tokens are stored in the operating system's secure credential store (e.g. via the `keyring` crate), **never** in plain text, settings files, logs or the repository.
- Never log tokens, auth headers or full API responses that may contain them.
- Sign-in uses GitHub's **OAuth device flow** (the app shows a code; the user approves it in the browser). No password fields.

### 1.4 Assets and branding
- **No third-party marks:** no GitHub logo, Octocat or other brand imagery. Draw generic icons instead.
- **Never commit font files.** Fonts come from `storytold/craft-fonts` as an optional build input, never a Cargo dependency; the app must work without them.
- Every asset added to the repo is listed in `ATTRIBUTION.md` with its source, author and licence.

### 1.5 Tests are the gate
Every change comes with tests. A feature is not done until it works end to end and has a test that proves it.

---

## 2. Stack

- **Rust only**, edition 2024. **No Tauri, Electron or webview shells:** the app is native.
- **UI:** egui (via eframe), using the shared Crafting App libraries so GitCraft looks and behaves like the rest of the suite.
- **Shared libraries** from [`storytold/craft-libs`](https://github.com/storytold/craft-libs), **pinned by immutable git revision**, never a branch:
  ```toml
  craft-ui = { git = "https://github.com/storytold/craft-libs", rev = "<commit>" }
  ```
  - `craft-ui`: icon buttons, tabs, pane controls, docking, theme helpers (requires Rust 1.95+).
  - `craft-settings`: `config.toml` settings storage, platform folders, atomic saves, recovery.
  - `craft-quit`: window close handling.
  - Prefer an existing craft-libs crate over writing an equivalent. If something reusable is missing, note it in `docs/STATUS.md` rather than forking it.
- **Git:** run the system `git` executable as a subprocess and parse its machine-readable output (`--porcelain`, `-z`, custom `--format` strings). This keeps full feature coverage (submodules, credentials, SSH, hooks) and matches the user's own git setup.
- **GitHub:** REST API for issues; **GraphQL API for Projects (v2)**, which has no REST equivalent.

---

## 3. Platforms

- Developed on **Windows**; tested on **Linux (Fedora, Wayland)**. Every feature must work on both, so avoid Windows-only assumptions even though development happens there.
- Keep platform-specific code behind `#[cfg(...)]` in clearly named platform modules. No platform assumptions elsewhere (paths, line endings, shell syntax, executable names).
- Never assume `git` is on a fixed path; find it, and report clearly if it is missing.

---

## 4. Architecture

- **The UI thread never blocks.** Every git command and network request runs in the background and reports its result back to the UI through a channel; the UI polls for results each frame and shows progress meanwhile.
- **Separate layers:** a `git` layer (commands and parsing, no UI), a `github` layer (API client, no UI), and the UI on top. The lower layers are testable without a window.
- **The commit graph** is drawn with egui's painter. Lane assignment is a pure function from commits to lanes, with its own unit tests.
- Keep files and modules small and grouped by domain, not by type.

---

## 5. Working loop

1. **Plan before code** for anything beyond a small fix: state the approach and the files it touches, and wait for approval.
2. **Work in small steps.** One feature or fix at a time; the app builds and runs after every step.
3. **Before every commit**, all of these pass:
   ```sh
   cargo fmt --all --check
   cargo clippy --all-targets -- -D warnings
   cargo test --all
   ```
4. **Tell the user what to test.** End each step with a short list of exactly what to try in the running app, on Windows and (when relevant) Linux.
5. **Update `docs/STATUS.md`** at the end of each session: what was done, what's next, known issues.
6. **Commits:** small, focused, with a message saying what changed and why.

### Explaining the code
The user is an experienced C++ developer who is new to Rust. When code uses a non-obvious Rust feature (ownership and borrowing choices, lifetimes, traits, async, channels), add a short comment or explanation so the code stays maintainable by them. Prefer explicit, readable code over clever code.

When a decision is genuinely ambiguous, ask rather than guess.

---

## 6. Milestones

Work through these in order; each one ends in something the user can run and test.

1. **Skeleton:** an eframe app using craft-ui and craft-settings that opens a local repository and lists its commits.
2. **Commit graph:** branch lanes and merge lines drawn for the open repository, filterable to one branch.
3. **Git actions:** status, stage, commit, branches, fetch, pull, push, submodules.
4. **GitHub sign-in:** device flow, token in the OS credential store, sign out.
5. **Issues:** list, view, create, comment, close for the repository's GitHub remote.
6. **Projects board:** read a Projects (v2) board's columns and cards; move cards between columns.
7. **Polish:** settings, error reporting, keyboard shortcuts, packaging for Linux and Windows.

---

## 7. Docs

| File | Purpose |
|---|---|
| `CLAUDE.md` | This file: rules and working instructions |
| `docs/STATUS.md` | Current state, next steps, known issues — start every session here |
| `docs/architecture.md` | Layers, data flow, background-work model |
| `ATTRIBUTION.md` | Every asset with source, author and licence |
