//! What a drawing keeps besides its layers and strokes: the artboard, the shape guides, the locked strokes and the
//! groups.

use ink_core::artboard::Artboard;
use ink_core::guide::Guide;
use ink_core::marks::Marks;
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
        }
    }

    /// `true` if there is nothing to keep.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artboard.is_none()
            && self.guides.is_empty()
            && self.groups.is_empty()
            && self.locked.is_empty()
    }

    /// The locks and groups, put right.
    #[must_use]
    pub fn marks(&self) -> Marks {
        Marks::from_parts(self.locked.clone(), self.groups.clone())
    }
}
