//! The sizes that differ between a mouse and a finger.
//!
//! A shell (see `shell`) sets them once a frame; the widgets read them and know nothing of which shell they are in,
//! so a switch or a slider is drawn the same way by both and only its size changes.

use eframe::egui::{Context, Id, Vec2, vec2};

/// How big the controls are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density {
    /// The height of the bar along the top.
    pub bar_height: f32,
    /// The least height of a row of settings: that of the biggest control in it.
    pub row_height: f32,
    /// The size of an on/off switch.
    pub switch: Vec2,
    /// The height a slider is given, which its handle follows.
    pub slider_height: f32,
    /// The least size of a button in a row of choices.
    pub choice: Vec2,
    /// Whether keys are named next to the commands they run.
    pub key_hints: bool,
    /// The height of the tools along the bottom of a window taller than it is wide, which things in the bottom corners
    /// are lifted above.
    pub dock_height: f32,
    /// The tools are along the bottom in a wide window too, not on its edge.
    pub always_docked: bool,
    /// What is under a pointer that hovers is explained in a tooltip. A finger does not hover, and a stylus that does
    /// would only get in the way of what it is about to touch.
    pub tooltips: bool,
}

impl Density {
    /// For a mouse and a keyboard.
    pub const DESKTOP: Self = Self {
        bar_height: 46.0,
        row_height: 36.0,
        switch: vec2(48.0, 28.0),
        slider_height: 32.0,
        choice: vec2(0.0, 34.0),
        key_hints: true,
        dock_height: 108.0,
        always_docked: false,
        tooltips: true,
    };

    /// For a finger and a stylus: nothing smaller than a fingertip, and no keys to name.
    pub const TOUCH: Self = Self {
        bar_height: 56.0,
        row_height: 48.0,
        switch: vec2(56.0, 32.0),
        slider_height: 40.0,
        choice: vec2(56.0, 44.0),
        key_hints: false,
        dock_height: 156.0,
        always_docked: true,
        tooltips: false,
    };
}

impl Default for Density {
    fn default() -> Self {
        Self::DESKTOP
    }
}

/// The densities are the only `Density` kept in the context, so the type is the key.
const KEY: Id = Id::NULL;

/// Sets the sizes the widgets use from now on.
pub fn set(ctx: &Context, density: Density) {
    ctx.data_mut(|d| d.insert_temp(KEY, density));
}

/// The sizes the widgets use now.
#[must_use]
pub fn get(ctx: &Context) -> Density {
    ctx.data(|d| d.get_temp(KEY)).unwrap_or_default()
}

/// The height of the bar along the top.
#[must_use]
pub fn bar_height(ctx: &Context) -> f32 {
    get(ctx).bar_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finger_gets_bigger_controls_than_a_mouse() {
        let (mouse, finger) = (Density::DESKTOP, Density::TOUCH);
        assert!(finger.bar_height > mouse.bar_height);
        assert!(finger.row_height > mouse.row_height);
        assert!(finger.switch.x > mouse.switch.x && finger.switch.y > mouse.switch.y);
        assert!(finger.slider_height > mouse.slider_height);
        assert!(finger.choice.y > mouse.choice.y);
        assert!(mouse.key_hints && !finger.key_hints);
        assert!(finger.dock_height > mouse.dock_height);
        assert!(finger.always_docked && !mouse.always_docked);
        assert!(mouse.tooltips && !finger.tooltips);
    }

    #[test]
    fn the_desktop_dock_is_as_tall_as_the_rail_draws_it() {
        assert!((Density::DESKTOP.dock_height - crate::rail::DOCK_HEIGHT).abs() < 1e-4);
    }

    #[test]
    fn it_is_the_desktop_until_a_shell_says_otherwise() {
        let ctx = Context::default();
        assert_eq!(get(&ctx), Density::DESKTOP);
        set(&ctx, Density::TOUCH);
        assert_eq!(get(&ctx), Density::TOUCH);
    }
}
