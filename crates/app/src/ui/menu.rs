//! The menu bar, drawn by craft-menu so it behaves like the rest of the suite (Alt and F10 to
//! reach it, mnemonics, overflow on narrow windows, and Exit/About placed per platform).

use craft_menu::{CommandRef, Item, ItemRole, LayoutOptions, Menu, MenuBar, MenuBarUi, MenuRole, Node, Target};

use crate::app::APP_NAME;
use crate::commands::Command;
use crate::ui::recent::RecentItem;

/// The id of a recent repository's item is this prefix and its place in the list (`.0` newest).
const OPEN_RECENT_PREFIX: &str = "file.open_recent.";

/// Something chosen from the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Command(Command),
    /// The recent repository at this place in the list the menu was built from.
    OpenRecent(usize),
}

/// The menus before platform layout. craft-menu adds Exit (Windows) or Quit (Linux) to File by
/// itself, wired to the normal window close.
fn menu_bar(recent: &[RecentItem]) -> MenuBar {
    let item = |command: Command| {
        let item = Item::new(command.command_ref(), command.label());
        match command.shortcut() {
            Some(chord) => item.shortcut(chord),
            None => item,
        }
    };
    let mut bar = MenuBar {
        menus: vec![
            Menu::new("File", vec![item(Command::OpenRepository).into(), Node::submenu("Open Recent", recent_items(recent))]).role(MenuRole::File),
            Menu::new("Branch", vec![item(Command::NewBranch).into(), item(Command::RenameBranch).into()]),
            Menu::new("Help", vec![item(Command::About).into()]).role(MenuRole::Help),
        ],
        documents: Vec::new(),
    };
    bar.tag_item(&Command::About.command_ref(), ItemRole::About);
    bar
}

/// The Open Recent submenu: each repository (the open one checked, missing ones greyed out),
/// then "Clear recent list"; or a greyed-out placeholder when there are none.
fn recent_items(recent: &[RecentItem]) -> Vec<Node> {
    if recent.is_empty() {
        return vec![Item::new(CommandRef::new("file.no_recent"), "No recent repositories").enabled(false).into()];
    }
    let mut nodes: Vec<Node> = recent
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let label = if entry.missing { format!("{} (not found)", entry.menu_label()) } else { entry.menu_label() };
            Item::new(CommandRef::new(format!("{OPEN_RECENT_PREFIX}{index}")), label).enabled(!entry.missing).check(entry.current).into()
        })
        .collect();
    nodes.push(Node::Separator);
    nodes.push(Item::new(Command::ClearRecent.command_ref(), Command::ClearRecent.label()).into());
    nodes
}

/// The menus laid out for this platform, with `recent` in File → Open Recent. Built again when
/// the recent list changes.
pub fn laid_out(recent: &[RecentItem]) -> MenuBar {
    craft_menu::layout(&menu_bar(recent), Target::current(), &LayoutOptions::new(APP_NAME)).bar
}

/// What a menu click means for the app.
fn choice_of(command: &CommandRef) -> Option<Choice> {
    if let Some(index) = command.id.strip_prefix(OPEN_RECENT_PREFIX) {
        return index.parse().ok().map(Choice::OpenRecent);
    }
    Command::from_ref(command).map(Choice::Command)
}

/// Draws the bar and returns what was chosen this frame. Exit/Quit is handled inside craft-menu
/// (it asks the window to close), so it never appears here.
pub fn show(ui: &mut egui::Ui, bar: &MenuBar) -> Vec<Choice> {
    MenuBarUi::new(bar).show(ui).iter().filter_map(choice_of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recent(names: &[&str]) -> Vec<RecentItem> {
        names.iter().map(|name| RecentItem { path: std::env::temp_dir().join(name), missing: *name == "gone", current: *name == "open" }).collect()
    }

    /// The Open Recent submenu's items: (command id, label, enabled, checked).
    fn open_recent(bar: &MenuBar) -> Vec<(String, String, bool, Option<bool>)> {
        let file = bar.menu(MenuRole::File).expect("File menu");
        let children = file
            .children
            .iter()
            .find_map(|node| match node {
                Node::Submenu { label, children, .. } if label == "Open Recent" => Some(children.clone()),
                _ => None,
            })
            .expect("Open Recent submenu");
        children
            .iter()
            .filter_map(|node| match node {
                Node::Item(item) => Some((item.command.id.clone(), item.label.clone(), item.enabled, item.kind.checked())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn has_open_repository_with_its_shortcut_and_about() {
        let bar = laid_out(&[]);
        let open = bar.find(&Command::OpenRepository.command_ref()).expect("Open Repository item");
        assert_eq!(open.shortcut, Command::OpenRepository.shortcut());
        assert!(bar.find(&Command::About.command_ref()).is_some());
    }

    #[test]
    fn open_recent_lists_repositories_newest_first() {
        let bar = laid_out(&recent(&["open", "gone", "other"]));
        let items = open_recent(&bar);
        assert_eq!(items.len(), 4, "three repositories and Clear");
        assert_eq!(items[0].0, "file.open_recent.0");
        assert!(items[0].1.starts_with("open — "), "{}", items[0].1);
        assert_eq!(items[0].3, Some(true), "the open one is checked");
        assert!(!items[1].2 && items[1].1.ends_with("(not found)"), "a missing one is greyed out: {:?}", items[1]);
        assert!(items[2].2 && items[2].3 == Some(false));
        assert_eq!(items[3].0, Command::ClearRecent.id());
    }

    #[test]
    fn the_branch_menu_has_new_and_rename() {
        let bar = laid_out(&[]);
        let new = bar.find(&Command::NewBranch.command_ref()).expect("New Branch item");
        assert_eq!(new.shortcut, Command::NewBranch.shortcut());
        assert!(new.shortcut.is_some(), "Ctrl+Shift+N");
        assert!(bar.find(&Command::RenameBranch.command_ref()).is_some());
    }

    #[test]
    fn an_empty_list_shows_a_placeholder() {
        let items = open_recent(&laid_out(&[]));
        assert_eq!(items.len(), 1);
        assert!(!items[0].2, "greyed out");
    }

    #[test]
    fn clicks_map_to_choices() {
        assert_eq!(choice_of(&CommandRef::new("file.open_recent.3")), Some(Choice::OpenRecent(3)));
        assert_eq!(choice_of(&CommandRef::new("file.open_recent.x")), None);
        assert_eq!(choice_of(&Command::About.command_ref()), Some(Choice::Command(Command::About)));
        assert_eq!(choice_of(&CommandRef::new("file.no_recent")), None);
    }

    #[test]
    fn draws_without_a_window() {
        let bar = laid_out(&recent(&["open"]));
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(show(ui, &bar).is_empty(), "nothing chosen without input");
        });
        output.textures_delta.clear();
    }
}
