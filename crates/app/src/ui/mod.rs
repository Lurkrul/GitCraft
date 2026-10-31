//! Drawing code, one module per part of the window. These modules only draw what they are given;
//! the state and decisions stay in `app.rs`.

pub mod about;
pub mod commit_list;
pub mod details;
pub mod menu;
pub mod placeholder;
pub mod ref_labels;
pub mod repository;
pub mod split;
pub mod status_bar;
pub mod tabs;
pub mod time;
