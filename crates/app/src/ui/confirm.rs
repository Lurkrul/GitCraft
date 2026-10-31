//! A modal confirmation for actions that can't be taken back. The body says exactly what will
//! happen; the confirm button names the action, and Cancel (or Esc, or a click outside) is the
//! safe way out.

/// What to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub body: String,
    /// The confirm button's label, naming the action ("Amend anyway"), never just "OK".
    pub action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Confirmed,
    Cancelled,
}

/// Shows the question; returns the answer on the frame it is given.
pub fn show(ctx: &egui::Context, confirm: &Confirm) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::unique("gitcraft-confirm")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(&confirm.title);
        ui.add_space(6.0);
        ui.add(egui::Label::new(&confirm.body).wrap());
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                choice = Some(Choice::Cancelled);
            }
            let danger = egui::RichText::new(&confirm.action).color(ui.visuals().error_fg_color);
            if ui.button(danger).clicked() {
                choice = Some(Choice::Confirmed);
            }
        });
    });
    // Esc or a click outside counts as Cancel.
    if choice.is_none() && modal.should_close() {
        choice = Some(Choice::Cancelled);
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_without_an_answer() {
        let ctx = egui::Context::default();
        let confirm = Confirm { title: "Amend?".into(), body: "This rewrites history.".into(), action: "Amend anyway".into() };
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| assert_eq!(show(ui.ctx(), &confirm), None));
            output.textures_delta.clear();
        }
    }
}
