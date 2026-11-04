//! Marks on strokes that are not part of what is drawn: which are locked, which are grouped, and which are letters.
//!
//! They are saved with the drawing, in its extras. A locked stroke can be neither selected, erased, nudged nor
//! sliced; the strokes of a group are selected together.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Text that was written on the drawing. The letters are strokes like any other, so they can be selected, moved,
/// resized and erased; this remembers what they say, so the words can be changed later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub text: String,
    /// How tall the letters were made, in canvas units, per em.
    pub size: f32,
    /// How tall all the letters together were when they were made, to tell how much they have been scaled since.
    pub height: f32,
    /// The strokes that make the letters.
    pub ids: Vec<Uuid>,
}

/// The locked strokes, the groups and the labels of a drawing, by stroke id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    locked: HashSet<Uuid>,
    /// Each group has at least two strokes, and no stroke is in two.
    groups: Vec<Vec<Uuid>>,
    /// Each stroke is in at most one label.
    labels: Vec<Label>,
}

impl Marks {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locked.is_empty() && self.groups.is_empty() && self.labels.is_empty()
    }

    // --- labels -------------------------------------------------------------------------------

    #[must_use]
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// The label the stroke `id` is a letter of, if it is one.
    #[must_use]
    pub fn label_of(&self, id: Uuid) -> Option<&Label> {
        self.labels.iter().find(|l| l.ids.contains(&id))
    }

    /// Keeps `label`, and forgets any other label that shares a stroke with it.
    pub fn set_label(&mut self, label: Label) {
        self.labels
            .retain(|other| !other.ids.iter().any(|id| label.ids.contains(id)));
        if !label.ids.is_empty() {
            self.labels.push(label);
        }
    }

    /// Forgets the label that the stroke `id` is a letter of, and returns it.
    pub fn remove_label_of(&mut self, id: Uuid) -> Option<Label> {
        let at = self.labels.iter().position(|l| l.ids.contains(&id))?;
        Some(self.labels.remove(at))
    }

    // --- locks --------------------------------------------------------------------------------

    pub fn lock(&mut self, ids: impl IntoIterator<Item = Uuid>) {
        self.locked.extend(ids);
    }

    /// Unlocks everything. Returns how many strokes were locked.
    pub fn unlock_all(&mut self) -> usize {
        std::mem::take(&mut self.locked).len()
    }

    /// Unlocks the strokes with these ids. Returns how many were locked.
    pub fn unlock(&mut self, ids: impl IntoIterator<Item = Uuid>) -> usize {
        ids.into_iter().filter(|id| self.locked.remove(id)).count()
    }

    #[must_use]
    pub fn is_locked(&self, id: Uuid) -> bool {
        self.locked.contains(&id)
    }

    /// The locked ids, in a fixed order.
    #[must_use]
    pub fn locked(&self) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self.locked.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    // --- groups -------------------------------------------------------------------------------

    /// Makes one group of `ids` and of every group any of them is in. Returns `false` (and changes nothing) if
    /// that would be fewer than two strokes.
    pub fn group(&mut self, ids: &[Uuid]) -> bool {
        let mut members: Vec<Uuid> = Vec::new();
        for id in ids {
            if !members.contains(id) {
                members.push(*id);
            }
        }
        let touched: Vec<usize> = self
            .groups
            .iter()
            .enumerate()
            .filter(|(_, g)| g.iter().any(|m| members.contains(m)))
            .map(|(i, _)| i)
            .collect();
        for &i in &touched {
            for m in &self.groups[i] {
                if !members.contains(m) {
                    members.push(*m);
                }
            }
        }
        if members.len() < 2 {
            return false;
        }
        for &i in touched.iter().rev() {
            self.groups.remove(i);
        }
        self.groups.push(members);
        true
    }

    /// Breaks up every group that any of `ids` is in. Returns how many groups there were.
    pub fn ungroup(&mut self, ids: &[Uuid]) -> usize {
        let before = self.groups.len();
        self.groups.retain(|g| !g.iter().any(|m| ids.contains(m)));
        before - self.groups.len()
    }

    /// The strokes in the same group as `id`, itself included, if it is in one.
    #[must_use]
    pub fn group_of(&self, id: Uuid) -> Option<&[Uuid]> {
        self.groups
            .iter()
            .find(|g| g.contains(&id))
            .map(Vec::as_slice)
    }

    /// Adds to `ids` every stroke that is grouped with one of them, so that a group is picked up whole.
    pub fn expand(&self, ids: &mut Vec<Uuid>) {
        let mut more = Vec::new();
        for id in ids.iter() {
            if let Some(group) = self.group_of(*id) {
                more.extend(group.iter().copied().filter(|m| !ids.contains(m)));
            }
        }
        for id in more {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    #[must_use]
    pub fn groups(&self) -> &[Vec<Uuid>] {
        &self.groups
    }

    // --- keeping them true --------------------------------------------------------------------

    /// Forgets strokes that `exists` says are gone, and groups left with fewer than two.
    pub fn prune(&mut self, exists: impl Fn(Uuid) -> bool) {
        self.locked.retain(|id| exists(*id));
        for g in &mut self.groups {
            g.retain(|id| exists(*id));
        }
        self.groups.retain(|g| g.len() >= 2);
        for label in &mut self.labels {
            label.ids.retain(|id| exists(*id));
        }
        self.labels.retain(|l| !l.ids.is_empty());
    }

    /// These labels, read from a file, as the drawing's: one that shares a stroke with another is dropped.
    #[must_use]
    pub fn with_labels(mut self, labels: Vec<Label>) -> Self {
        for label in labels {
            if !label.ids.iter().any(|id| self.label_of(*id).is_some()) {
                self.set_label(label);
            }
        }
        self
    }

    /// Marks read from a file: ids may repeat or be in two groups, and are put right. Groups of one are dropped.
    #[must_use]
    pub fn from_parts(locked: Vec<Uuid>, groups: Vec<Vec<Uuid>>) -> Self {
        let mut marks = Self {
            locked: locked.into_iter().collect(),
            groups: Vec::new(),
            labels: Vec::new(),
        };
        let mut taken: HashSet<Uuid> = HashSet::new();
        for g in groups {
            let mut members: Vec<Uuid> = Vec::new();
            for id in g {
                if !members.contains(&id) && taken.insert(id) {
                    members.push(id);
                }
            }
            if members.len() >= 2 {
                marks.groups.push(members);
            }
        }
        marks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn locking_and_unlocking_by_id() {
        let mut marks = Marks::default();
        assert!(marks.is_empty());
        marks.lock([id(3), id(1), id(2)]);
        assert!(marks.is_locked(id(1)) && !marks.is_locked(id(9)));
        assert_eq!(
            marks.locked(),
            vec![id(1), id(2), id(3)],
            "in a fixed order"
        );
        assert_eq!(marks.unlock([id(2), id(9)]), 1);
        assert_eq!(marks.unlock_all(), 2);
        assert!(marks.is_empty());
    }

    #[test]
    fn a_group_needs_two_strokes_and_keeps_its_order() {
        let mut marks = Marks::default();
        assert!(!marks.group(&[id(1)]));
        assert!(
            !marks.group(&[id(1), id(1)]),
            "one stroke twice is still one"
        );
        assert!(marks.group(&[id(3), id(1), id(2)]));
        assert_eq!(marks.groups(), [vec![id(3), id(1), id(2)]]);
        assert_eq!(marks.group_of(id(1)), Some(&[id(3), id(1), id(2)][..]));
        assert_eq!(marks.group_of(id(9)), None);
    }

    #[test]
    fn grouping_strokes_that_are_in_groups_joins_those_groups_into_one() {
        let mut marks = Marks::default();
        marks.group(&[id(1), id(2)]);
        marks.group(&[id(3), id(4)]);
        assert!(marks.group(&[id(2), id(5)]));
        assert_eq!(
            marks.groups().len(),
            2,
            "the group of 1 and 2 took 5 in; the other is alone"
        );
        assert_eq!(marks.group_of(id(5)).map(<[Uuid]>::len), Some(3));
        assert_eq!(marks.group_of(id(1)).map(<[Uuid]>::len), Some(3));
        // Joining two groups.
        assert!(marks.group(&[id(1), id(3)]));
        assert_eq!(marks.groups().len(), 1);
        assert_eq!(marks.groups()[0].len(), 5);
    }

    #[test]
    fn ungrouping_breaks_up_each_group_a_stroke_is_in() {
        let mut marks = Marks::default();
        marks.group(&[id(1), id(2)]);
        marks.group(&[id(3), id(4)]);
        assert_eq!(marks.ungroup(&[id(2)]), 1);
        assert_eq!(marks.groups().len(), 1);
        assert_eq!(marks.ungroup(&[id(9)]), 0);
        assert_eq!(marks.ungroup(&[id(3), id(4)]), 1);
        assert!(marks.is_empty());
    }

    #[test]
    fn a_group_is_picked_up_whole_and_nothing_is_picked_twice() {
        let mut marks = Marks::default();
        marks.group(&[id(1), id(2), id(3)]);
        let mut picked = vec![id(2), id(9)];
        marks.expand(&mut picked);
        assert_eq!(picked.len(), 4);
        for n in [1, 2, 3, 9] {
            assert_eq!(picked.iter().filter(|p| **p == id(n)).count(), 1, "{n}");
        }
        let mut none = vec![id(9)];
        marks.expand(&mut none);
        assert_eq!(none, vec![id(9)]);
    }

    #[test]
    fn pruning_forgets_what_is_gone_and_drops_groups_that_shrink_to_one() {
        let mut marks = Marks::default();
        marks.lock([id(1), id(2)]);
        marks.group(&[id(1), id(2), id(3)]);
        marks.group(&[id(4), id(5)]);
        marks.prune(|i| i != id(2) && i != id(5));
        assert_eq!(marks.locked(), vec![id(1)]);
        assert_eq!(marks.groups().len(), 1, "4 is alone, so its group is gone");
        assert_eq!(marks.groups()[0], vec![id(1), id(3)]);
    }

    #[test]
    fn marks_read_from_a_file_are_put_right() {
        let marks = Marks::from_parts(
            vec![id(1), id(1)],
            vec![
                vec![id(1), id(2), id(2)],
                vec![id(2), id(3)],
                vec![id(4)],
                vec![id(5), id(6)],
            ],
        );
        assert_eq!(marks.locked(), vec![id(1)]);
        // 2 is in the first group only; 3 is left alone in the second, which is dropped; 4 alone is dropped.
        assert_eq!(marks.groups(), [vec![id(1), id(2)], vec![id(5), id(6)]]);
    }

    fn label(text: &str, ids: &[u128]) -> Label {
        Label {
            text: text.to_owned(),
            size: 24.0,
            height: 20.0,
            ids: ids.iter().map(|n| id(*n)).collect(),
        }
    }

    #[test]
    fn a_label_is_found_from_any_of_its_letters() {
        let mut marks = Marks::default();
        marks.set_label(label("Hi", &[1, 2, 3]));
        assert_eq!(marks.label_of(id(2)).map(|l| l.text.as_str()), Some("Hi"));
        assert!(marks.label_of(id(9)).is_none());
        assert!(!marks.is_empty());
    }

    #[test]
    fn a_new_label_replaces_the_one_it_shares_letters_with() {
        let mut marks = Marks::default();
        marks.set_label(label("Old", &[1, 2]));
        marks.set_label(label("New", &[2, 3]));
        assert_eq!(marks.labels().len(), 1);
        assert_eq!(marks.labels()[0].text, "New");
    }

    #[test]
    fn a_label_with_no_letters_is_not_kept() {
        let mut marks = Marks::default();
        marks.set_label(label("Nothing", &[]));
        assert_eq!(marks.labels().len(), 0);
    }

    #[test]
    fn a_label_loses_letters_that_are_gone_and_is_gone_with_the_last() {
        let mut marks = Marks::default();
        marks.set_label(label("Hi", &[1, 2]));
        marks.prune(|i| i == id(1));
        assert_eq!(marks.labels()[0].ids, vec![id(1)]);
        marks.prune(|_| false);
        assert_eq!(marks.labels().len(), 0);
    }

    #[test]
    fn taking_a_label_away_gives_it_back() {
        let mut marks = Marks::default();
        marks.set_label(label("Hi", &[1, 2]));
        assert_eq!(
            marks.remove_label_of(id(1)).map(|l| l.text),
            Some("Hi".to_owned())
        );
        assert!(marks.remove_label_of(id(1)).is_none());
    }

    #[test]
    fn labels_read_from_a_file_that_share_letters_keep_only_the_first() {
        let marks = Marks::default().with_labels(vec![label("A", &[1, 2]), label("B", &[2, 3])]);
        assert_eq!(marks.labels().len(), 1);
        assert_eq!(marks.labels()[0].text, "A");
    }
}
