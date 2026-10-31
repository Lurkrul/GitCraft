//! The menu bar, drawn by craft-menu so it behaves like the rest of the suite (Alt and F10 to
//! reach it, mnemonics, overflow on narrow windows, and Exit/About placed per platform).

use craft_menu::{Item, ItemRole, LayoutOptions, Menu, MenuBar, MenuBarUi, MenuRole, Target};

use crate::app::APP_NAME;
use crate::commands::Command;

/// The menus before platform layout. craft-menu adds Exit (Windows) or Quit (Linux) to File by
/// itself, wired to the normal window close.
fn menu_bar() -> MenuBar {
    let item = |command: Command| {
        let item = Item::new(command.command_ref(), command.label());
        match command.shortcut() {
            Some(chord) => item.shortcut(chord),
            None => item,
        }
    };
    let mut bar = MenuBar {
        menus: vec![
            Menu::new("File", vec![item(Command::OpenRepository).into()]).role(MenuRole::File),
            Menu::new("Help", vec![item(Command::About).into()]).role(MenuRole::Help),
        ],
        documents: Vec::new(),
    };
    bar.tag_item(&Command::About.command_ref(), ItemRole::About);
    bar
}

/// The menus laid out for this platform. Built once at startup and kept.
pub fn laid_out() -> MenuBar {
    craft_menu::layout(&menu_bar(), Target::current(), &LayoutOptions::new(APP_NAME)).bar
}

/// Draws the bar and returns the commands chosen this frame. Exit/Quit is handled inside
/// craft-menu (it asks the window to close), so it never appears here.
pub fn show(ui: &mut egui::Ui, bar: &MenuBar) -> Vec<Command> {
    MenuBarUi::new(bar).show(ui).iter().filter_map(Command::from_ref).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_open_repository_with_its_shortcut_and_about() {
        let bar = laid_out();
        let open = bar.find(&Command::OpenRepository.command_ref()).expect("Open Repository item");
        assert_eq!(open.shortcut, Command::OpenRepository.shortcut());
        assert!(bar.find(&Command::About.command_ref()).is_some());
    }

    #[test]
    fn draws_without_a_window() {
        let bar = laid_out();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(show(ui, &bar).is_empty(), "nothing chosen without input");
        });
        output.textures_delta.clear();
    }
}
