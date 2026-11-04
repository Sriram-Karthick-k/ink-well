//! The interface of a device used by touch: a finger and a stylus rather than a mouse and a keyboard.
//!
//! Each part is its own module and uses the app only through the same state and commands the desktop interface does;
//! none of them reaches into another, and the desktop modules know nothing of this one (see `shell`).

pub mod bar_items;
pub mod hold_ring;
pub mod layers_sheet;
pub mod library;
pub mod menu_sheet;
pub mod precision_sheet;
pub mod settings;
pub mod shapes_sheet;
pub mod sheet;
pub mod tool_bar;
pub mod toolbar_sheet;
pub mod top_bar;
pub mod view_readout;
