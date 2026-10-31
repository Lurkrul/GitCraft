//! The main tab strip: Repository, Issues, Projects.

/// The app's main areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    /// Status, staging and creating commits (Milestone 3).
    Commit,
    Repository,
    Issues,
    Projects,
}

impl MainTab {
    pub const ALL: [MainTab; 4] = [MainTab::Commit, MainTab::Repository, MainTab::Issues, MainTab::Projects];

    pub fn label(self) -> &'static str {
        match self {
            MainTab::Commit => "Commit",
            MainTab::Repository => "Repository",
            MainTab::Issues => "Issues",
            MainTab::Projects => "Projects",
        }
    }

    /// A stable id for egui, independent of the (one day translated) label.
    fn id_salt(self) -> &'static str {
        match self {
            MainTab::Commit => "tab-commit",
            MainTab::Repository => "tab-repository",
            MainTab::Issues => "tab-issues",
            MainTab::Projects => "tab-projects",
        }
    }
}

const TAB_PADDING: egui::Vec2 = egui::vec2(14.0, 6.0);
const UNDERLINE: f32 = 2.0;

/// Draws the strip and updates `selected` when a tab is clicked (or activated from the keyboard).
/// craft-ui's `Tab` handles the clicking, keyboard focus and accessibility; we do the painting.
pub fn show(ui: &mut egui::Ui, selected: &mut MainTab) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for tab in MainTab::ALL {
            let is_selected = *selected == tab;
            let font = egui::TextStyle::Button.resolve(ui.style());
            let color = if is_selected { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
            let galley = ui.painter().layout_no_wrap(tab.label().to_string(), font, color);
            let size = galley.size() + 2.0 * TAB_PADDING;
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

            let response = craft_ui::tabs::Tab::new(ui.scope_id().with(tab.id_salt()), tab.label(), is_selected).show_at(ui, rect, |ui, response| {
                let visuals = ui.visuals();
                if response.hovered() && !is_selected {
                    ui.painter().rect_filled(response.rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
                }
                if is_selected {
                    let line = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - UNDERLINE), rect.right_bottom());
                    ui.painter().rect_filled(line, 0.0, visuals.selection.bg_fill);
                }
                let text_pos = rect.center() - galley.size() / 2.0;
                ui.painter().galley(text_pos, galley.clone(), color);
            });
            if response.clicked() {
                *selected = tab;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_distinct() {
        assert_eq!(MainTab::ALL.map(MainTab::label), ["Commit", "Repository", "Issues", "Projects"]);
    }

    #[test]
    fn draws_without_a_window() {
        let ctx = egui::Context::default();
        let mut selected = MainTab::Repository;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show(ui, &mut selected));
            output.textures_delta.clear();
        }
        assert_eq!(selected, MainTab::Repository);
    }
}
