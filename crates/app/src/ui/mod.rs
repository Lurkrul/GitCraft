//! Drawing code, one module per part of the window. These modules only draw what they are given;
//! the state and decisions stay in `app.rs`.

pub mod about;
pub mod branches;
pub mod commit_list;
pub mod commit_tab;
pub mod confirm;
pub mod details;
pub mod diff_view;
pub mod file_row;
pub mod menu;
pub mod placeholder;
pub mod recent;
pub mod ref_labels;
pub mod repository;
pub mod split;
pub mod status_bar;
pub mod sync;
pub mod tabs;
pub mod time;
pub mod toolbar_box;
