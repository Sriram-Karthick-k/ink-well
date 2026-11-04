//! What a drawing keeps besides its layers and strokes: the artboard, the shape guides, the locked strokes, the
//! groups and the words written on it.

use ink_core::artboard::Artboard;
use ink_core::guide::Guide;
use ink_core::marks::{Label, Marks};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The extras of one drawing, as saved in `extras.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extras {
    pub artboard: Option<Artboard>,
    pub guides: Vec<Guide>,
    pub groups: Vec<Vec<Uuid>>,
    pub locked: Vec<Uuid>,
    /// The words written on the drawing, to be changed later.
    pub labels: Vec<Label>,
}

impl Extras {
    /// The extras a drawing has now.
    #[must_use]
    pub fn of(artboard: Option<Artboard>, guides: &[Guide], marks: &Marks) -> Self {
        Self {
            artboard,
            guides: guides.to_vec(),
            groups: marks.groups().to_vec(),
            locked: marks.locked(),
            labels: marks.labels().to_vec(),
        }
    }

    /// `true` if there is nothing to keep.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artboard.is_none()
            && self.guides.is_empty()
            && self.groups.is_empty()
            && self.locked.is_empty()
            && self.labels.is_empty()
    }

    /// The locks and groups, put right.
    #[must_use]
    pub fn marks(&self) -> Marks {
        Marks::from_parts(self.locked.clone(), self.groups.clone()).with_labels(self.labels.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_written_on_a_drawing_are_kept_and_come_back() {
        let mut marks = Marks::default();
        marks.set_label(Label {
            text: "Hello".to_owned(),
            size: 30.0,
            height: 22.0,
            ids: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
        });
        let extras = Extras::of(None, &[], &marks);
        assert!(!extras.is_empty());
        let text = serde_json::to_string(&extras).expect("written");
        let back: Extras = serde_json::from_str(&text).expect("read");
        assert_eq!(back.marks().labels(), marks.labels());
    }

    #[test]
    fn extras_from_before_there_were_labels_still_load() {
        let back: Extras = serde_json::from_str(r#"{"groups":[],"locked":[]}"#).expect("read");
        assert_eq!(back.labels.len(), 0);
        assert!(back.is_empty());
    }
}
