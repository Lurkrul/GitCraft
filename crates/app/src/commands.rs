//! The app's commands: what menu items and keyboard shortcuts run. One place for their ids and
//! shortcuts, so the menu, the shortcut handling and the tests agree.

use craft_menu::{Chord, CommandRef};

/// A command the app can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    OpenRepository,
    About,
}

impl Command {
    /// Every command, for building the menu and checking shortcuts.
    pub const ALL: [Command; 2] = [Command::OpenRepository, Command::About];

    /// The stable id craft-menu reports when the item is chosen.
    pub fn id(self) -> &'static str {
        match self {
            Command::OpenRepository => "file.open_repository",
            Command::About => "help.about",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::OpenRepository => "Open Repository…",
            Command::About => "About GitCraft",
        }
    }

    /// The keyboard shortcut, if any. "Cmd" is Ctrl on Windows and Linux, ⌘ on macOS.
    pub fn shortcut(self) -> Option<Chord> {
        let text = match self {
            Command::OpenRepository => "Cmd+O",
            Command::About => return None,
        };
        // The text is fixed above, so parsing can't fail in practice; `ok()` keeps it panic-free.
        Chord::parse(text).ok()
    }

    pub fn command_ref(self) -> CommandRef {
        CommandRef::new(self.id())
    }

    /// The command a menu click reported, if it is one of ours.
    pub fn from_ref(command: &CommandRef) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.id() == command.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_are_unique() {
        for command in Command::ALL {
            assert_eq!(Command::from_ref(&command.command_ref()), Some(command));
        }
        let mut ids: Vec<_> = Command::ALL.iter().map(|c| c.id()).collect();
        ids.dedup();
        assert_eq!(ids.len(), Command::ALL.len());
        assert_eq!(Command::from_ref(&CommandRef::new("unknown")), None);
    }

    #[test]
    fn open_repository_is_ctrl_o() {
        let shortcut = Command::OpenRepository.shortcut().unwrap().to_egui_shortcut();
        assert_eq!(shortcut.logical_key, egui::Key::O);
        assert!(shortcut.modifiers.command);
    }
}
