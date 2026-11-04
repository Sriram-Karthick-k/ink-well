//! What is on the tool bar of a touch device, and in what order.
//!
//! The person arranges it: tools can be taken off, put back, moved along, and any shape, the app's or their own, can be
//! pinned to it. The choice is kept in the settings as a list of keys; this is the one place that reads and changes
//! that list, and the interface draws whatever it says.

use crate::config::Config;

/// What the tool bar holds until the person arranges it.
pub const DEFAULT: [&str; 11] = [
    "undo", "redo", "eraser", "select", "slice", "nudge", "move", "shapes", "text", "pens", "color",
];

/// The key a pinned shape is kept under: this, then the id of the shape.
const SHAPE_PREFIX: &str = "shape:";

/// One place on the tool bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Undo,
    Redo,
    Eraser,
    Select,
    Slice,
    Nudge,
    /// Moving the page: the hand.
    Move,
    /// The menu of shapes and the person's own objects.
    Shapes,
    /// Words, in a shape or on their own.
    Text,
    /// The case with every pen in it.
    Pens,
    /// The ink.
    Color,
    /// A shape put on the paper with one tap, by its id.
    Shape(String),
}

impl Item {
    /// Every item that is not a shape, in the order the bar starts with.
    #[must_use]
    pub fn tools() -> [Self; 11] {
        [
            Self::Undo,
            Self::Redo,
            Self::Eraser,
            Self::Select,
            Self::Slice,
            Self::Nudge,
            Self::Move,
            Self::Shapes,
            Self::Text,
            Self::Pens,
            Self::Color,
        ]
    }

    /// What the item is kept under in the settings.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::Undo => "undo".to_owned(),
            Self::Redo => "redo".to_owned(),
            Self::Eraser => "eraser".to_owned(),
            Self::Select => "select".to_owned(),
            Self::Slice => "slice".to_owned(),
            Self::Nudge => "nudge".to_owned(),
            Self::Move => "move".to_owned(),
            Self::Shapes => "shapes".to_owned(),
            Self::Text => "text".to_owned(),
            Self::Pens => "pens".to_owned(),
            Self::Color => "color".to_owned(),
            Self::Shape(id) => format!("{SHAPE_PREFIX}{id}"),
        }
    }

    /// The item kept under `key`, or `None` for a key nobody knows (one from a newer version, say).
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        if let Some(id) = key.strip_prefix(SHAPE_PREFIX) {
            return (!id.is_empty()).then(|| Self::Shape(id.to_owned()));
        }
        Self::tools().into_iter().find(|item| item.key() == key)
    }

    /// What a tool is called on the bar. A shape is called what the shape is, which only the shapes know.
    #[must_use]
    pub fn label(&self) -> Option<&'static str> {
        match self {
            Self::Undo => Some("Undo"),
            Self::Redo => Some("Redo"),
            Self::Eraser => Some("Eraser"),
            Self::Select => Some("Select"),
            Self::Slice => Some("Slice"),
            Self::Nudge => Some("Nudge"),
            Self::Move => Some("Move"),
            Self::Shapes => Some("Shapes"),
            Self::Text => Some("Text"),
            Self::Pens => Some("All pens"),
            Self::Color => Some("Color"),
            Self::Shape(_) => None,
        }
    }
}

impl Config {
    /// The items on the tool bar, in order. A key nobody knows and a repeated one are left out.
    #[must_use]
    pub fn toolbar_items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = Vec::new();
        for item in self
            .interface
            .toolbar
            .iter()
            .filter_map(|key| Item::from_key(key))
        {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        items
    }

    fn store_toolbar(&mut self, items: &[Item]) {
        self.interface.toolbar = items.iter().map(Item::key).collect();
    }

    /// Puts `item` at the end of the bar, if it is not on it.
    pub fn toolbar_add(&mut self, item: Item) {
        let mut items = self.toolbar_items();
        if !items.contains(&item) {
            items.push(item);
            self.store_toolbar(&items);
        }
    }

    /// Takes `item` off the bar. The last item stays, so there is always something to touch.
    pub fn toolbar_remove(&mut self, item: &Item) {
        let mut items = self.toolbar_items();
        if items.len() > 1 {
            items.retain(|i| i != item);
            self.store_toolbar(&items);
        }
    }

    /// Moves `item` one place toward the start (`-1`) or the end (`1`) of the bar.
    pub fn toolbar_shift(&mut self, item: &Item, by: isize) {
        let mut items = self.toolbar_items();
        let Some(at) = items.iter().position(|i| i == item) else {
            return;
        };
        let Some(to) = at.checked_add_signed(by).filter(|&to| to < items.len()) else {
            return;
        };
        items.swap(at, to);
        self.store_toolbar(&items);
    }

    /// Takes a shape that is gone off the bar, wherever it was, even if it was the only thing there.
    pub fn toolbar_forget_shape(&mut self, id: &str) {
        let gone = Item::Shape(id.to_owned()).key();
        self.interface.toolbar.retain(|key| *key != gone);
        if self.toolbar_items().is_empty() {
            self.toolbar_reset();
        }
    }

    /// Puts the bar back as it started, shapes taken off.
    pub fn toolbar_reset(&mut self) {
        self.interface.toolbar = DEFAULT.iter().map(|k| (*k).to_owned()).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(config: &Config) -> Vec<String> {
        config.toolbar_items().iter().map(Item::key).collect()
    }

    #[test]
    fn the_bar_starts_with_the_tools_a_person_needs_and_the_shapes_among_them() {
        let config = Config::default();
        let items = config.toolbar_items();
        assert_eq!(items.len(), DEFAULT.len());
        assert!(items.contains(&Item::Shapes) && items.contains(&Item::Undo));
    }

    #[test]
    fn every_tool_is_found_again_by_its_key_and_every_key_is_different() {
        let mut seen = Vec::new();
        for item in Item::tools() {
            assert_eq!(Item::from_key(&item.key()), Some(item.clone()));
            assert!(!seen.contains(&item.key()), "{} twice", item.key());
            seen.push(item.key());
        }
        assert_eq!(
            Item::from_key("shape:star"),
            Some(Item::Shape("star".to_owned()))
        );
        assert_eq!(Item::from_key("shape:"), None);
        assert_eq!(Item::from_key("what"), None);
    }

    #[test]
    fn a_key_nobody_knows_and_a_repeat_are_left_out() {
        let mut config = Config::default();
        config.interface.toolbar = vec![
            "undo".to_owned(),
            "from_the_future".to_owned(),
            "undo".to_owned(),
            "color".to_owned(),
        ];
        assert_eq!(keys(&config), ["undo", "color"]);
    }

    #[test]
    fn a_shape_can_be_pinned_once_and_goes_to_the_end() {
        let mut config = Config::default();
        config.toolbar_add(Item::Shape("star".to_owned()));
        config.toolbar_add(Item::Shape("star".to_owned()));
        let k = keys(&config);
        assert_eq!(k.last().map(String::as_str), Some("shape:star"));
        assert_eq!(k.iter().filter(|k| *k == "shape:star").count(), 1);
    }

    #[test]
    fn a_tool_taken_off_comes_back_at_the_end() {
        let mut config = Config::default();
        config.toolbar_remove(&Item::Slice);
        assert!(!keys(&config).contains(&"slice".to_owned()));
        config.toolbar_add(Item::Slice);
        assert_eq!(keys(&config).last().map(String::as_str), Some("slice"));
    }

    #[test]
    fn the_last_item_cannot_be_taken_off() {
        let mut config = Config::default();
        config.interface.toolbar = vec!["pens".to_owned()];
        config.toolbar_remove(&Item::Pens);
        assert_eq!(keys(&config), ["pens"]);
    }

    #[test]
    fn an_item_moves_one_place_at_a_time_and_stops_at_the_ends() {
        let mut config = Config::default();
        config.interface.toolbar = vec!["undo".to_owned(), "redo".to_owned(), "color".to_owned()];
        config.toolbar_shift(&Item::Color, -1);
        assert_eq!(keys(&config), ["undo", "color", "redo"]);
        config.toolbar_shift(&Item::Color, -1);
        config.toolbar_shift(&Item::Color, -1);
        assert_eq!(
            keys(&config),
            ["color", "undo", "redo"],
            "it stops at the start"
        );
        config.toolbar_shift(&Item::Redo, 1);
        assert_eq!(keys(&config), ["color", "undo", "redo"], "and at the end");
    }

    #[test]
    fn a_shape_that_is_deleted_leaves_the_bar_even_when_it_was_all_there_was() {
        let mut config = Config::default();
        config.interface.toolbar = vec!["shape:mine-1".to_owned()];
        config.toolbar_forget_shape("mine-1");
        assert_eq!(keys(&config).len(), DEFAULT.len(), "the bar starts over");
        config.toolbar_add(Item::Shape("mine-2".to_owned()));
        config.toolbar_forget_shape("mine-2");
        assert_eq!(keys(&config).len(), DEFAULT.len());
    }

    #[test]
    fn the_arrangement_is_kept_in_the_settings_file() {
        let mut config = Config::default();
        config.toolbar_remove(&Item::Nudge);
        config.toolbar_add(Item::Shape("heart".to_owned()));
        let text = serde_json::to_string(&config).expect("written");
        let back: Config = serde_json::from_str(&text).expect("read");
        assert_eq!(back.toolbar_items(), config.toolbar_items());
    }

    #[test]
    fn settings_from_before_the_bar_could_be_arranged_still_load_with_the_usual_bar() {
        let back: Config = serde_json::from_str(r#"{"interface":{"scale":1.0}}"#).expect("read");
        assert_eq!(back.toolbar_items().len(), DEFAULT.len());
    }
}
