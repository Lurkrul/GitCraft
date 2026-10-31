//! Stand-in content for tabs whose features come in later milestones.

pub fn coming_later(ui: &mut egui::Ui, feature: &str, milestone: u32) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading(feature);
        ui.label(format!("{feature} arrive in a later version of GitCraft (milestone {milestone})."));
    });
}
