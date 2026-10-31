# Status

Last updated: 2026-10-31

## Current milestone: 1, Skeleton

Plan (approved 2026-10-31): a Cargo workspace (`crates/app`, `crates/git`); licence MIT OR
Apache-2.0, matching craft-libs.

| Step | What | State |
|---|---|---|
| 1 | Workspace scaffold, lints, craft-libs pin, empty window | done |
| 4a | Settings: remember window size and maximized state (done early, on request) | done |
| 2 | `gitcraft-git`: locate git, run commands, validate repo, parse log | next |
| 3 | Worker thread, folder picker, commit list, status bar | |
| 4b | Settings: remember and reopen the last repository | |

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

## To verify on Fedora (deferred; batched for later)

- Window opens under Wayland; close works.
- Window size restore. Wayland compositors may ignore the requested size or maximized state.

## Known issues / notes

- The `[patch.crates-io]` lines in the root `Cargo.toml` must be updated whenever the craft-libs rev
  changes.
- The suite's egui fork panics if a frame's `TexturesDelta` is dropped unhandled; headless tests
  must call `output.textures_delta.clear()`.
- Window position is not saved (size only). It can't be set on Wayland anyway; it could be added for
  Windows if wanted.
- Set `GITCRAFT_CONFIG_DIR` to use a different settings folder (empty means don't save at all), handy for testing.
- Fonts: egui's default fonts for now; craft-fonts is not wired in yet.
