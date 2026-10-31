//! Branch and tag labels shown beside the commits they point at.

use std::collections::HashMap;

use gitcraft_git::{Head, RefKind, Refs};

/// What a label stands for; decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// The checked-out branch, or "HEAD" when detached.
    Head,
    Branch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub text: String,
    pub kind: LabelKind,
}

/// Commit id → its labels, HEAD first, then branches, remote branches and tags. Built once
/// whenever the refs change, so drawing a row is just a lookup.
pub type LabelMap = HashMap<String, Vec<RefLabel>>;

pub fn label_map(refs: &Refs) -> LabelMap {
    let mut map: LabelMap = HashMap::new();
    if let Head::Detached { commit } = &refs.head {
        map.entry(commit.clone()).or_default().push(RefLabel { text: "HEAD".into(), kind: LabelKind::Head });
    }
    let head_branch = refs.head_branch();
    // `refs.refs` is sorted branches, remote branches, tags; the checked-out branch moves to the front.
    for r in &refs.refs {
        let is_head = Some(r.name.as_str()) == head_branch;
        let kind = match r.kind {
            RefKind::Branch if is_head => LabelKind::Head,
            RefKind::Branch => LabelKind::Branch,
            RefKind::RemoteBranch => LabelKind::RemoteBranch,
            RefKind::Tag => LabelKind::Tag,
        };
        let labels = map.entry(r.target.clone()).or_default();
        let label = RefLabel { text: r.short_name().to_string(), kind };
        if kind == LabelKind::Head { labels.insert(0, label) } else { labels.push(label) }
    }
    map
}

/// Draws `labels` as small pills, left to right. `lane_color` is the colour of the commit's lane,
/// so a branch label matches its line in the graph.
pub fn show(ui: &mut egui::Ui, labels: &[RefLabel], lane_color: egui::Color32) {
    for label in labels {
        let visuals = ui.visuals();
        let (fill, stroke, text_color) = match label.kind {
            LabelKind::Head | LabelKind::Branch => (lane_color, egui::Stroke::NONE, egui::Color32::from_rgb(0x1b, 0x1b, 0x1b)),
            LabelKind::RemoteBranch => (egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, lane_color), lane_color),
            LabelKind::Tag => (visuals.widgets.inactive.bg_fill, egui::Stroke::NONE, visuals.text_color()),
        };
        let mut text = egui::RichText::new(&label.text).small().color(text_color);
        if label.kind == LabelKind::Head {
            text = text.strong();
        }
        let tooltip = match label.kind {
            LabelKind::Head => "Checked out",
            LabelKind::Branch => "Local branch",
            LabelKind::RemoteBranch => "Remote branch",
            LabelKind::Tag => "Tag",
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(3.0)
            .inner_margin(egui::Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.add(egui::Label::new(text).selectable(false));
            })
            .response
            .on_hover_text(tooltip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcraft_git::Ref;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn r(name: &str, kind: RefKind, target: &str) -> Ref {
        Ref { name: name.into(), kind, target: target.into() }
    }

    #[test]
    fn groups_labels_by_commit_with_the_head_branch_first() {
        let refs = Refs {
            refs: vec![
                r("refs/heads/feature", RefKind::Branch, A),
                r("refs/heads/main", RefKind::Branch, A),
                r("refs/remotes/origin/main", RefKind::RemoteBranch, A),
                r("refs/tags/v1", RefKind::Tag, B),
            ],
            head: Head::Branch { name: "refs/heads/main".into(), commit: A.into() },
        };
        let map = label_map(&refs);
        let texts = |id: &str| map.get(id).map(|l| l.iter().map(|l| (l.text.as_str(), l.kind)).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(texts(A), [("main", LabelKind::Head), ("feature", LabelKind::Branch), ("origin/main", LabelKind::RemoteBranch)]);
        assert_eq!(texts(B), [("v1", LabelKind::Tag)]);
    }

    #[test]
    fn a_detached_head_gets_its_own_label() {
        let refs = Refs { refs: vec![r("refs/heads/main", RefKind::Branch, A)], head: Head::Detached { commit: B.into() } };
        let map = label_map(&refs);
        assert_eq!(map.get(B).map(Vec::len), Some(1));
        assert_eq!(map.get(B).and_then(|l| l.first()).map(|l| l.kind), Some(LabelKind::Head));
        assert_eq!(map.get(A).and_then(|l| l.first()).map(|l| l.kind), Some(LabelKind::Branch));
    }
}
