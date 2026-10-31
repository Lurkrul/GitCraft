# Architecture

## Crates (layers)

| Crate | Path | Role | May depend on |
|---|---|---|---|
| `gitcraft` | `crates/app` | The egui app (binary): windows, panels, input | everything below |
| `gitcraft-git` | `crates/git` | Runs the system `git` and parses its output. No UI. | std, serde |
| `gitcraft-github` | `crates/github` | GitHub REST + GraphQL client. No UI. (Milestone 4) | |

The lower crates never depend on egui, so the compiler keeps UI code out of them and they can be
tested without a window.

## Background work

The UI thread never blocks. Git commands and network requests run on a worker thread:

```
UI thread                          worker thread
---------                          -------------
send Job ───────── mpsc channel ─▶ run git / HTTP
                                   send Event
try_recv each frame ◀─ channel ─── ctx.request_repaint()
```

Each frame the UI drains finished events with `try_recv` (never `recv`, which would block) and
shows progress while a job is running. The worker calls `request_repaint` so the UI wakes up even
when the mouse is idle.

## Shared libraries

From `storytold/craft-libs`, pinned by commit in the root `Cargo.toml`. craft-libs builds against
the suite's egui and winit forks; the root `Cargo.toml` repeats its `[patch.crates-io]` lines, and
they must be updated together with the craft-libs rev.

- `craft-quit`: window close handling (`on_close_request`, last in `App::logic`).
- `craft-settings`: `config.toml` storage (`crates/app/src/settings.rs`); saved on a debounce from
  `App::logic` and flushed in `App::on_exit`.
- `craft-dialogs`: non-blocking folder picker (Milestone 1, step 3).
- `craft-ui`: shared widgets and theme helpers.
