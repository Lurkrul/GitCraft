//! Author avatars: initials in a coloured circle, shown before each commit's summary. Generated
//! locally, so no author's email ever leaves the machine (no Gravatar look-ups).

use egui::{Align2, Color32, FontId, Sense, Vec2};

use super::paint;

/// Avatar diameter, in points.
pub const SIZE: f32 = 18.0;
const TEXT_COLOR: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1b);

/// One or two capital letters for an author: the first letters of their first and last names
/// ("Ada Lovelace" → "AL", "Lurkrul" → "L"), else the first letter of their email, else "?".
pub fn initials(name: &str, email: &str) -> String {
    // Words that start with a letter or digit ("(bot)" and "-" don't count).
    let words: Vec<&str> = name.split_whitespace().filter(|w| w.chars().next().is_some_and(char::is_alphanumeric)).collect();
    let first_letter = |word: &str| word.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    match (words.first(), words.last()) {
        (Some(first), Some(last)) if words.len() > 1 => first_letter(first) + &first_letter(last),
        (Some(only), _) => first_letter(only),
        _ => match email.chars().find(|c| c.is_alphanumeric()) {
            Some(c) => c.to_uppercase().collect(),
            None => "?".to_string(),
        },
    }
}

/// A palette index that is always the same for the same author, so each person keeps one colour
/// throughout the history. Keyed on the email (lower-cased), or the name when there is no email.
///
/// FNV-1a: a tiny, fixed hash. `std`'s default hasher is randomised per run, which would give
/// people different colours every time GitCraft starts.
pub fn color_index(name: &str, email: &str) -> usize {
    let key = if email.trim().is_empty() { name.trim().to_lowercase() } else { email.trim().to_lowercase() };
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3));
    // The top bits: FNV's low bits hardly mix (emails sharing "@example.com" would all get the
    // same colour). Six bits give 0..64, which always fits in a usize.
    usize::try_from(hash >> 58).unwrap_or(0)
}

/// Draws the avatar at the current position in `ui`, with the author's name and email on hover.
pub fn show(ui: &mut egui::Ui, name: &str, email: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::hover());
    let painter = ui.painter();
    let radius = SIZE / 2.0;
    painter.circle_filled(rect.center(), radius, paint::lane_color(color_index(name, email)));
    let text = initials(name, email);
    let font = FontId::proportional(radius * if text.chars().count() > 1 { 0.95 } else { 1.15 });
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, TEXT_COLOR);
    response.on_hover_text(format!("{name} <{email}>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_and_last_names() {
        assert_eq!(initials("Ada Lovelace", ""), "AL");
        assert_eq!(initials("Ada King Lovelace", ""), "AL");
        assert_eq!(initials("Lurkrul", ""), "L");
    }

    #[test]
    fn non_ascii_names() {
        assert_eq!(initials("Zoë Müller-Łukasz", ""), "ZM");
        assert_eq!(initials("łukasz ąb", ""), "ŁĄ");
        assert_eq!(initials("山田 太郎", ""), "山太");
    }

    #[test]
    fn falls_back_to_the_email_then_a_question_mark() {
        assert_eq!(initials("", "dev@example.com"), "D");
        assert_eq!(initials("  ", "  "), "?");
        assert_eq!(initials("(bot)", "<>"), "?");
    }

    #[test]
    fn an_author_keeps_one_colour() {
        assert_eq!(color_index("Ada", "ada@example.com"), color_index("Ada L.", "ADA@example.com "));
        assert_eq!(color_index("Ada", ""), color_index(" ada ", ""));
        // Fixed across runs and versions (a known FNV-1a value), not randomised.
        assert_eq!(color_index("", "a"), usize::try_from(0xaf63_dc4c_8601_ec8c_u64 >> 58).unwrap());
    }

    #[test]
    fn authors_at_one_domain_get_different_colours() {
        let emails = ["ada@example.com", "grace@example.com", "d@example.com", "linus@example.com", "margaret@example.com"];
        let mut colours: Vec<_> = emails.iter().map(|e| paint::lane_color(color_index("", e))).collect();
        colours.sort_by_key(|c| c.to_array());
        colours.dedup();
        assert!(colours.len() >= 4, "only {} distinct colours for 5 authors", colours.len());
    }

    #[test]
    fn draws_without_a_window() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = show(ui, "Ada Lovelace", "ada@example.com");
            assert_eq!(response.rect.size(), Vec2::splat(SIZE));
        });
        output.textures_delta.clear();
    }
}
