GitCraft
========

A native desktop client that puts git, GitHub Issues and GitHub Projects (Kanban boards) in one
window. Built in Rust with egui and the shared [craft-libs](https://github.com/storytold/craft-libs),
so it looks and behaves like the other Crafting Apps.

Status: early development (Milestone 1, skeleton). See [docs/STATUS.md](docs/STATUS.md).

## Building

Needs Rust 1.95 or later and `git` on your `PATH`.

```sh
cargo run -p gitcraft
```

On Linux, the usual egui/winit build dependencies are needed (Wayland and X11 client libraries,
xkbcommon).

## License

GitCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Assets keep their own licences; each is listed in [ATTRIBUTION.md](ATTRIBUTION.md).
