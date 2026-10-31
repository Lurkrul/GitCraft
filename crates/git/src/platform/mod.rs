//! The only place in this crate with operating-system-specific code. Each platform module offers
//! the same functions; `#[cfg]` picks one at compile time (like `#ifdef _WIN32` in C++).

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{GIT_EXE_NAME, hide_console, path_from_git_output};
#[cfg(windows)]
pub use windows::{GIT_EXE_NAME, hide_console, path_from_git_output};
