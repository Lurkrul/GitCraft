//! Tab strips: the main one (Repository, Issues, Projects) and, at the top of the Repository tab's
//! left pane, Changes and History (as in GitHub Desktop).

/// The app's main areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Repository,
    Issues,
    Projects,
}

impl MainTab {
    pub const ALL: [MainTab; 3] = [MainTab::Repository, MainTab::Issues, MainTab::Projects];
}

/// The Repository tab's two views, picked at the top of its left pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepoView {
    /// The working tree's changes: ticking, committing, discarding.
    #[default]
    Changes,
    /// The commit history, with each commit's details and diffs.
    History,
}

impl RepoView {
    pub const ALL: [RepoView; 2] = [RepoView::Changes, RepoView::History];
}

/// Something shown as a tab. A trait (like a C++ interface) so one strip draws both kinds of tab.
pub trait TabItem: Copy + PartialEq {
    fn label(self) -> &'static str;
    /// A stable id for egui, independent of the (one day translated) label.
    fn id_salt(self) -> &'static str;
}

impl TabItem for MainTab {
    fn label(self) -> &'static str {
        match self {
            MainTab::Repository => "Repository",
            MainTab::Issues => "Issues",
            MainTab::Projects => "Projects",
        }
    }

    fn id_salt(self) -> &'static str {
        match self {
            MainTab::Repository => "tab-repository",
            MainTab::Issues => "tab-issues",
            MainTab::Projects => "tab-projects",
        }
    }
}

impl TabItem for RepoView {
    fn label(self) -> &'static str {
        match self {
            RepoView::Changes => "Changes",
            RepoView::History => "History",
        }
    }

    fn id_salt(self) -> &'static str {
        match self {
            RepoView::Changes => "view-changes",
            RepoView::History => "view-history",
        }
    }
}

const TAB_PADDING: egui::Vec2 = egui::vec2(14.0, 6.0);
const UNDERLINE: f32 = 2.0;

/// Draws a strip of `tabs` and updates `selected` when one is clicked (or activated from the
/// keyboard). craft-ui's `Tab` handles the clicking, keyboard focus and accessibility; we do the
/// painting. `T: TabItem` makes this work for any kind of tab (a generic, like a C++ template).
pub fn strip<T: TabItem>(ui: &mut egui::Ui, tabs: &[T], selected: &mut T) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for &tab in tabs {
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

/// The main tab strip.
pub fn show(ui: &mut egui::Ui, selected: &mut MainTab) {
    strip(ui, &MainTab::ALL, selected);
}

/// The Changes / History strip at the top of the Repository tab's left pane.
pub fn view_tabs(ui: &mut egui::Ui, view: &mut RepoView) {
    strip(ui, &RepoView::ALL, view);
    ui.separator();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_distinct() {
        assert_eq!(MainTab::ALL.map(MainTab::label), ["Repository", "Issues", "Projects"]);
        assert_eq!(RepoView::ALL.map(RepoView::label), ["Changes", "History"]);
        assert_eq!(RepoView::default(), RepoView::Changes, "like GitHub Desktop");
    }

    #[test]
    fn draws_without_a_window() {
        let ctx = egui::Context::default();
        let mut selected = MainTab::Repository;
        let mut view = RepoView::History;
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(ui, &mut selected);
                view_tabs(ui, &mut view);
            });
            output.textures_delta.clear();
        }
        assert_eq!((selected, view), (MainTab::Repository, RepoView::History));
    }
}
