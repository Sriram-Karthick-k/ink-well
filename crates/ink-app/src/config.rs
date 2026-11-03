//! The user's settings, saved as JSON so they survive restarts.
//!
//! Everything configurable lives here, grouped the way the settings window groups it. Every field has a default
//! and unknown or missing fields are tolerated, so a file written by an older or newer version still loads.
//! Values are clamped after loading, so a hand-edited file cannot put the app in an unusable state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ink_core::Pen;
use serde::{Deserialize, Serialize};

use crate::brushes;

/// The most recent colors kept for quick access.
pub const RECENT_COLORS: usize = 10;

/// What one brush remembers between uses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushConfig {
    /// Size in canvas units.
    pub size: f32,
    /// `0.0..=1.0`.
    pub opacity: f32,
    /// `0.0..=1.0`; how strongly a stroke is smoothed while drawing.
    pub smoothness: f32,
}

impl Default for BrushConfig {
    fn default() -> Self {
        Self {
            size: 3.0,
            opacity: 1.0,
            smoothness: 0.1,
        }
    }
}

impl BrushConfig {
    pub const MIN_SIZE: f32 = 0.5;
    pub const MAX_SIZE: f32 = 300.0;
    pub const MIN_OPACITY: f32 = 0.05;

    fn clamped(self) -> Self {
        Self {
            size: self.size.clamp(Self::MIN_SIZE, Self::MAX_SIZE),
            opacity: self.opacity.clamp(Self::MIN_OPACITY, 1.0),
            smoothness: self.smoothness.clamp(0.0, 1.0),
        }
    }
}

/// What is printed on the paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pattern {
    Blank,
    Dots,
    Grid,
    Lines,
    /// Lines at 30 degrees either way and straight up and down: graph paper for drawing in three dimensions.
    Isometric,
    /// Lines at 60 degrees either way and across: a lattice of equal triangles.
    Triangle,
    /// Lines running to one vanishing point in the middle of the paper.
    Perspective1,
    /// Lines running to two vanishing points on the horizon.
    Perspective2,
    /// Two vanishing points on the horizon and one below.
    Perspective3,
}

impl Pattern {
    pub const ALL: [Self; 9] = [
        Self::Blank,
        Self::Dots,
        Self::Grid,
        Self::Lines,
        Self::Isometric,
        Self::Triangle,
        Self::Perspective1,
        Self::Perspective2,
        Self::Perspective3,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::Dots => "Dots",
            Self::Grid => "Grid",
            Self::Lines => "Lines",
            Self::Isometric => "Isometric",
            Self::Triangle => "Triangle",
            Self::Perspective1 => "1 point",
            Self::Perspective2 => "2 point",
            Self::Perspective3 => "3 point",
        }
    }

    /// `true` for patterns where every few lines is a main line (the setting for how many).
    #[must_use]
    pub fn has_main_lines(self) -> bool {
        !matches!(self, Self::Blank | Self::Dots | Self::Lines)
    }

    /// `true` for the patterns made of lines running to vanishing points.
    #[must_use]
    pub fn is_perspective(self) -> bool {
        matches!(
            self,
            Self::Perspective1 | Self::Perspective2 | Self::Perspective3
        )
    }
}

/// The drawing surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasConfig {
    pub pattern: Pattern,
    /// Distance between dots or lines, in canvas units.
    pub grid_size: f32,
    /// How visible the pattern is, `0.0..=1.0`.
    pub grid_strength: f32,
    /// The paper color of new drawings.
    pub paper: [u8; 3],
    /// Every this many lines of a grid is a main line, a little stronger than the rest; `1` makes every line a
    /// main line.
    pub grid_divisions: u32,
    /// The color the pattern is printed in; `None` picks one that shows on the paper.
    pub grid_color: Option<[u8; 3]>,
    /// How thick the printed lines are, in points.
    pub line_weight: f32,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            pattern: Pattern::Dots,
            grid_size: 32.0,
            grid_strength: 0.3,
            paper: [0xF9, 0xF9, 0xF9],
            grid_divisions: 4,
            grid_color: None,
            line_weight: 1.0,
        }
    }
}

/// What a tap with several fingers at once does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TapAction {
    None,
    Undo,
    Redo,
    /// Hides or brings back the panels, for drawing with nothing in the way.
    HidePanels,
    ZoomToFit,
    /// Goes back to the tool used before this one.
    PreviousTool,
}

impl TapAction {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Undo,
        Self::Redo,
        Self::HidePanels,
        Self::ZoomToFit,
        Self::PreviousTool,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Do nothing",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::HidePanels => "Hide the panels",
            Self::ZoomToFit => "Zoom to fit",
            Self::PreviousTool => "Previous tool",
        }
    }
}

/// How pens, fingers and the mouse wheel behave.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches that are saved and shown one by one"
)]
pub struct InputConfig {
    /// Use the pressure a stylus reports. Off, every stroke is drawn as if by a mouse.
    pub use_pressure: bool,
    /// Two fingers pan and pinch to zoom.
    pub two_finger_gestures: bool,
    /// The mouse wheel zooms; otherwise it pans, and Ctrl with the wheel zooms.
    pub wheel_zooms: bool,
    /// The canvas can be zoomed (pinch, wheel and the zoom keys). Off, the zoom is locked where it is.
    pub zoom_enabled: bool,
    /// Zooming close to a round level (25%, 50%, 100%, 200%, 400%, 800%) lands on it.
    pub zoom_snap: bool,
    /// The canvas can be turned (two fingers twisting, and the turn keys). Off, it stays as it is.
    pub rotation_enabled: bool,
    /// Turning the canvas to within a few degrees of a multiple of 15 lands on it.
    pub rotation_snap: bool,
    /// The pressure that counts as the lightest touch and the hardest, `0.0..=1.0`. Pressure outside them is
    /// cut off, and what is between is spread over the whole range: raising the first makes light touches draw
    /// nothing special, lowering the second makes a firm touch reach full width sooner.
    pub pressure_low: f32,
    pub pressure_high: f32,
    pub two_finger_tap: TapAction,
    pub three_finger_tap: TapAction,
    pub four_finger_tap: TapAction,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            use_pressure: true,
            two_finger_gestures: true,
            wheel_zooms: false,
            zoom_enabled: true,
            zoom_snap: true,
            rotation_enabled: true,
            rotation_snap: true,
            pressure_low: 0.0,
            pressure_high: 1.0,
            two_finger_tap: TapAction::Undo,
            three_finger_tap: TapAction::Redo,
            four_finger_tap: TapAction::None,
        }
    }
}

impl InputConfig {
    /// The pressure to draw with for a pen that reports `force`, after the pressure response is applied.
    #[must_use]
    pub fn pressure(&self, force: f32) -> f32 {
        let span = (self.pressure_high - self.pressure_low).max(0.05);
        ((force - self.pressure_low) / span).clamp(0.0, 1.0)
    }

    /// What a tap with `fingers` fingers at once does.
    #[must_use]
    pub fn tap_action(&self, fingers: usize) -> TapAction {
        match fingers {
            2 => self.two_finger_tap,
            3 => self.three_finger_tap,
            n if n >= 4 => self.four_finger_tap,
            _ => TapAction::None,
        }
    }

    fn clamped(mut self) -> Self {
        self.pressure_low = self.pressure_low.clamp(0.0, 0.9);
        self.pressure_high = self.pressure_high.clamp(self.pressure_low + 0.05, 1.0);
        self
    }
}

/// How the workspace helps while drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches that are saved and shown one by one"
)]
pub struct WorkspaceConfig {
    /// A ring the size of the brush follows the pen while it hovers over the paper.
    pub hover_brush_preview: bool,
    /// How long the stroke being drawn is, shown beside the pen.
    pub show_stroke_length: bool,
    /// How big the selection is, shown on the bar over the paper.
    pub show_selection_size: bool,
    /// The keyboard runs commands. Off, no key does anything (the settings can still be opened with the button).
    pub keyboard_shortcuts: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            hover_brush_preview: true,
            show_stroke_length: false,
            show_selection_size: true,
            keyboard_shortcuts: true,
        }
    }
}

/// Snapping, which is under Precision: a point that is drawn or dragged close to the grid or to a key point goes
/// to it, while drawing and while editing separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches that are saved and shown one by one"
)]
pub struct PrecisionConfig {
    /// Snapping is on at all.
    pub snap: bool,
    /// To the lines of the grid the paper shows (dots, squares and ruled lines), or the nodes of the isometric
    /// and triangle papers.
    pub snap_to_grid: bool,
    /// To the ends of the strokes already drawn.
    pub snap_to_key_points: bool,
    /// To the middle of the strokes already drawn (halfway along them).
    pub snap_to_middles: bool,
    /// To the places strokes already drawn cross each other. While drawing, and for the handles of a guide.
    pub snap_to_crossings: bool,
    /// The first and last point of a stroke snap as it is drawn.
    pub snap_while_drawing: bool,
    /// A selection snaps as it is dragged.
    pub snap_while_editing: bool,
    /// How near, in screen points, a point has to be to something for it to snap.
    pub snap_reach: f32,
    /// A stroke that is held still at its end is made into the clean shape it is, if it is clearly one. It
    /// can be turned off.
    pub recognize_shapes: bool,
    /// How long, in seconds, the pen is held still for that.
    pub hold_delay: f32,
}

impl PrecisionConfig {
    /// The nearest and farthest a snap may reach, in screen points.
    pub const REACH: (f32, f32) = (4.0, 40.0);
    /// The shortest and longest a pen is held still for shape recognition, in seconds.
    pub const HOLD: (f32, f32) = (0.3, 2.0);
}

impl Default for PrecisionConfig {
    fn default() -> Self {
        Self {
            snap: false,
            snap_to_grid: true,
            snap_to_key_points: true,
            snap_to_middles: false,
            snap_to_crossings: false,
            snap_while_drawing: true,
            snap_while_editing: true,
            snap_reach: 14.0,
            recognize_shapes: false,
            hold_delay: 0.6,
        }
    }
}

/// The look and layout of the interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceConfig {
    /// Scale of everything in the interface.
    pub scale: f32,
    /// Mirrors the panels for left-handed use.
    pub left_handed: bool,
    /// Fade the panels out while a stroke is being drawn.
    pub hide_while_drawing: bool,
    /// Show the layers panel when the app starts.
    pub show_layers: bool,
    /// The pens on the rail, by brush key. Choosing another pen swaps it in for the last one.
    pub rail: Vec<String>,
}

/// The fewest and the most places the pen rail can have.
pub const RAIL_SLOTS: (usize, usize) = (3, 8);

/// The pens on the rail until the user picks others.
pub const DEFAULT_RAIL: [&str; 5] = ["pen", "fountain", "soft_pencil", "marker", "watercolor"];

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            left_handed: false,
            hide_while_drawing: true,
            show_layers: false,
            rail: DEFAULT_RAIL.iter().map(|k| (*k).to_owned()).collect(),
        }
    }
}

/// Opening and saving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches that are saved and shown one by one"
)]
pub struct FilesConfig {
    /// Ask before a new drawing, an open or a drop would discard unsaved changes.
    pub confirm_discard: bool,
    /// Fit the drawing to the window when a file is opened.
    pub fit_on_open: bool,
    /// Where notes are kept; empty means a folder called inkwell in Documents.
    pub library_folder: Option<String>,
    /// Save notes in the notes folder by themselves a moment after every change.
    pub autosave: bool,
    /// Start on the notes page instead of a blank note.
    pub start_in_library: bool,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            confirm_discard: true,
            fit_on_open: true,
            library_folder: None,
            autosave: true,
            start_in_library: true,
        }
    }
}

/// What the select tool draws to pick strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectShape {
    /// A free loop around the strokes.
    Lasso,
    /// A box between two corners.
    Rectangle,
}

impl SelectShape {
    pub const ALL: [Self; 2] = [Self::Lasso, Self::Rectangle];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Lasso => "Lasso",
            Self::Rectangle => "Box",
        }
    }
}

/// What the slice tool cuts along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SliceMode {
    /// A straight blade from where you press to where you are.
    Line,
    /// The blade follows the pen.
    Freehand,
}

impl SliceMode {
    pub const ALL: [Self; 2] = [Self::Line, Self::Freehand];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Straight",
            Self::Freehand => "Freehand",
        }
    }
}

/// How the nudge tool reshapes a stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NudgeVariant {
    /// Take hold of a point and drag it; the stroke follows along its length.
    Pull,
    /// A soft round tip that carries along whatever it touches.
    Push,
}

impl NudgeVariant {
    pub const ALL: [Self; 2] = [Self::Pull, Self::Push];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pull => "Pull",
            Self::Push => "Push",
        }
    }
}

/// What the eraser takes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EraserKind {
    /// A stroke it touches goes whole.
    Hard,
    /// Only the part of a stroke it touches goes.
    Soft,
}

impl EraserKind {
    pub const ALL: [Self; 2] = [Self::Hard, Self::Soft];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Hard => "Whole stroke",
            Self::Soft => "Only what it touches",
        }
    }
}

/// The tools that work on strokes already drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub nudge_variant: NudgeVariant,
    /// What the eraser takes away.
    pub eraser_kind: EraserKind,
    /// How far the nudge tool reaches, in points.
    pub nudge_reach: f32,
    pub select_shape: SelectShape,
    /// How much of a stroke must be inside the loop for it to be selected, `0.0..=1.0`; `0` takes anything the
    /// loop touches.
    pub select_coverage: f32,
    pub slice_mode: SliceMode,
    /// How far the eraser reaches, in points.
    pub eraser_radius: f32,
    /// How far an arrow key nudges the selection while Alt is held, in canvas units.
    pub arrow_small: f32,
    /// How far an arrow key nudges the selection.
    pub arrow_step: f32,
    /// How far an arrow key nudges the selection while Shift is held.
    pub arrow_large: f32,
    /// How far from the original a duplicate is put, in canvas units.
    pub duplicate_offset: f32,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            nudge_variant: NudgeVariant::Pull,
            eraser_kind: EraserKind::Hard,
            nudge_reach: 64.0,
            select_shape: SelectShape::Lasso,
            select_coverage: 0.5,
            slice_mode: SliceMode::Line,
            eraser_radius: 12.0,
            arrow_small: 1.0,
            arrow_step: 8.0,
            arrow_large: 64.0,
            duplicate_offset: 24.0,
        }
    }
}

impl ToolsConfig {
    pub const ERASER_RANGE: (f32, f32) = (3.0, 80.0);
    pub const NUDGE_REACH_RANGE: (f32, f32) = (12.0, 240.0);
    pub const ARROW_RANGE: (f32, f32) = (0.25, 512.0);

    fn clamped(mut self) -> Self {
        self.select_coverage = self.select_coverage.clamp(0.0, 1.0);
        self.nudge_reach = self
            .nudge_reach
            .clamp(Self::NUDGE_REACH_RANGE.0, Self::NUDGE_REACH_RANGE.1);
        self.eraser_radius = self
            .eraser_radius
            .clamp(Self::ERASER_RANGE.0, Self::ERASER_RANGE.1);
        for nudge in [
            &mut self.arrow_small,
            &mut self.arrow_step,
            &mut self.arrow_large,
        ] {
            *nudge = nudge.clamp(Self::ARROW_RANGE.0, Self::ARROW_RANGE.1);
        }
        self.duplicate_offset = self.duplicate_offset.clamp(0.0, 512.0);
        self
    }
}

/// Which renderer draws the strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Renderer {
    /// The OpenGL stroke renderer.
    Gpu,
    /// Strokes drawn with plain shapes; slower and plainer, but works everywhere.
    Cpu,
}

/// Rendering choices. They take effect the next time the app starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderingConfig {
    pub renderer: Renderer,
    /// Where the stamp textures are loaded from; empty means a `textures` folder next to the program is used
    /// if there is one.
    pub textures_folder: Option<String>,
}

impl Default for RenderingConfig {
    fn default() -> Self {
        Self {
            renderer: Renderer::Gpu,
            textures_folder: None,
        }
    }
}

/// The color the pen draws with, and the colors chosen lately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorsConfig {
    /// The current color.
    pub current: [u8; 3],
    /// The palette shown in the color panel.
    pub palette: String,
    /// Recently used colors, newest first.
    pub recent: Vec<[u8; 3]>,
    /// The brush that was selected last.
    pub last_brush: String,
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            current: [0x18, 0x17, 0x12],
            palette: "Basic".to_owned(),
            recent: Vec::new(),
            last_brush: brushes::key(Pen::Pen).to_owned(),
        }
    }
}

/// All settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Brush settings by brush key (see [`brushes::key`]); brushes never touched are not stored.
    pub brushes: BTreeMap<String, BrushConfig>,
    pub canvas: CanvasConfig,
    pub input: InputConfig,
    pub workspace: WorkspaceConfig,
    pub precision: PrecisionConfig,
    /// How lengths, sizes and angles are written.
    pub measurement: ink_core::units::Measurement,
    pub tools: ToolsConfig,
    pub interface: InterfaceConfig,
    pub files: FilesConfig,
    pub rendering: RenderingConfig,
    pub colors: ColorsConfig,
    /// Keys the user has changed, by command id (see [`crate::shortcuts`]): each command's keys written like
    /// `Ctrl+Shift+Z`. A command that is not here keeps its usual keys; one with an empty list has none.
    pub shortcuts: BTreeMap<String, Vec<String>>,
}

impl Config {
    pub const MIN_SCALE: f32 = 0.75;
    pub const MAX_SCALE: f32 = 2.0;

    /// What `pen` remembers, or its starting values if it has never been changed.
    #[must_use]
    pub fn brush(&self, pen: Pen) -> BrushConfig {
        self.brushes
            .get(brushes::key(pen))
            .copied()
            .unwrap_or_else(|| brushes::defaults(pen))
    }

    /// The settings of `pen` to change, starting from its defaults.
    pub fn brush_mut(&mut self, pen: Pen) -> &mut BrushConfig {
        self.brushes
            .entry(brushes::key(pen).to_owned())
            .or_insert_with(|| brushes::defaults(pen))
    }

    /// Forgets what `pen` remembers, so it goes back to its defaults.
    pub fn reset_brush(&mut self, pen: Pen) {
        self.brushes.remove(brushes::key(pen));
    }

    /// The folder notes are kept in.
    #[must_use]
    pub fn library_folder(&self) -> PathBuf {
        self.files
            .library_folder
            .as_deref()
            .map(PathBuf::from)
            .or_else(default_library_folder)
            .unwrap_or_else(|| PathBuf::from("inkwell-notes"))
    }

    /// The pens on the rail, in order. Unknown or repeated entries are skipped.
    #[must_use]
    pub fn rail_pens(&self) -> Vec<Pen> {
        let mut pens: Vec<Pen> = Vec::new();
        for pen in self
            .interface
            .rail
            .iter()
            .filter_map(|key| brushes::from_key(key))
        {
            if !pens.contains(&pen) {
                pens.push(pen);
            }
        }
        if pens.is_empty() {
            pens = DEFAULT_RAIL
                .iter()
                .filter_map(|key| brushes::from_key(key))
                .collect();
        }
        pens.truncate(RAIL_SLOTS.1);
        pens
    }

    /// Makes sure `pen` is on the rail; if it is not, it takes the place of the last pen.
    pub fn use_in_rail(&mut self, pen: Pen) {
        let mut pens = self.rail_pens();
        if !pens.contains(&pen) {
            pens.pop();
            pens.push(pen);
        }
        self.store_rail(&pens);
    }

    fn store_rail(&mut self, pens: &[Pen]) {
        self.interface.rail = pens.iter().map(|p| brushes::key(*p).to_owned()).collect();
    }

    /// Puts `pen` in place number `slot` of the rail. A pen that is already elsewhere on the rail swaps places
    /// with the one it replaces, so no pen is on the rail twice.
    pub fn set_rail_slot(&mut self, slot: usize, pen: Pen) {
        let mut pens = self.rail_pens();
        if slot >= pens.len() {
            return;
        }
        match pens.iter().position(|p| *p == pen) {
            Some(other) => pens.swap(slot, other),
            None => pens[slot] = pen,
        }
        self.store_rail(&pens);
    }

    /// Gives the rail `slots` places (between [`RAIL_SLOTS`]). More places are filled with brushes that are not
    /// on the rail yet, in the order the case lists them; fewer drop the last ones.
    pub fn resize_rail(&mut self, slots: usize) {
        let slots = slots.clamp(RAIL_SLOTS.0, RAIL_SLOTS.1);
        let mut pens = self.rail_pens();
        pens.truncate(slots);
        for info in &brushes::BRUSHES {
            if pens.len() >= slots {
                break;
            }
            if !pens.contains(&info.pen) {
                pens.push(info.pen);
            }
        }
        self.store_rail(&pens);
    }

    /// Puts `color` first among the recent colors, without repeats.
    pub fn remember_color(&mut self, color: [u8; 3]) {
        self.colors.recent.retain(|c| *c != color);
        self.colors.recent.insert(0, color);
        self.colors.recent.truncate(RECENT_COLORS);
    }

    /// Brings every value into its valid range.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        for brush in self.brushes.values_mut() {
            *brush = brush.clamped();
        }
        self.canvas.grid_size = self.canvas.grid_size.clamp(4.0, 512.0);
        self.canvas.grid_strength = self.canvas.grid_strength.clamp(0.0, 1.0);
        self.interface.scale = self.interface.scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        self.colors.recent.truncate(RECENT_COLORS);
        self.tools = self.tools.clamped();
        self.input = self.input.clamped();
        self.canvas.grid_divisions = self.canvas.grid_divisions.clamp(1, 16);
        self.canvas.line_weight = self.canvas.line_weight.clamp(0.5, 3.0);
        self.measurement = self.measurement.clamped();
        self.precision.hold_delay = self
            .precision
            .hold_delay
            .clamp(PrecisionConfig::HOLD.0, PrecisionConfig::HOLD.1);
        self.precision.snap_reach = self
            .precision
            .snap_reach
            .clamp(PrecisionConfig::REACH.0, PrecisionConfig::REACH.1);
        if self
            .rendering
            .textures_folder
            .as_deref()
            .is_some_and(|s| s.trim().is_empty())
        {
            self.rendering.textures_folder = None;
        }
        if self
            .files
            .library_folder
            .as_deref()
            .is_some_and(|s| s.trim().is_empty())
        {
            self.files.library_folder = None;
        }
        self
    }

    /// Reads settings from `path`. A file that does not exist gives the defaults; one that cannot be read or
    /// parsed is an error.
    ///
    /// # Errors
    /// The message says what went wrong.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<Self>(&text)
                .map(Self::clamped)
                .map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Writes the settings to `path` through a temporary file, creating the folder if needed.
    ///
    /// # Errors
    /// The message says what went wrong.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, path).map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            format!("{}: {e}", path.display())
        })
    }
}

/// The notes folder when none is chosen: `inkwell` inside the user's Documents.
#[must_use]
pub fn default_library_folder() -> Option<PathBuf> {
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    }?;
    Some(PathBuf::from(home).join("Documents").join("inkwell"))
}

/// Where the settings file lives on this machine, if a suitable folder can be found.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|dir| dir.join("inkwell").join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkwell-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir.join(name)
    }

    #[test]
    fn untouched_brushes_report_their_defaults_and_are_not_stored() {
        let config = Config::default();
        assert!(config.brushes.is_empty());
        let marker = config.brush(Pen::Marker);
        assert!((marker.smoothness - 0.25).abs() < 1e-6);
        assert!((marker.size - 24.0).abs() < 1e-6);
    }

    #[test]
    fn changing_a_brush_stores_only_that_brush() {
        let mut config = Config::default();
        config.brush_mut(Pen::Fountain).size = 12.0;
        assert_eq!(config.brushes.len(), 1);
        assert!((config.brush(Pen::Fountain).size - 12.0).abs() < 1e-6);
        assert!(
            (config.brush(Pen::Fountain).smoothness - brushes::defaults(Pen::Fountain).smoothness)
                .abs()
                < 1e-6,
            "the other values start from the defaults"
        );
        config.reset_brush(Pen::Fountain);
        assert!(config.brushes.is_empty());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let mut config = Config::default();
        config.brush_mut(Pen::SoftPencil).smoothness = 0.4;
        config.canvas.pattern = Pattern::Grid;
        config.interface.left_handed = true;
        config.rendering.renderer = Renderer::Cpu;
        config.remember_color([1, 2, 3]);
        let path = temp_path("round.json");
        config.save(&path).expect("saved");
        assert_eq!(Config::load(&path).expect("loaded"), config);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let loaded = Config::load(&temp_path("never-written.json")).expect("not an error");
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn missing_and_unknown_fields_are_tolerated() {
        let path = temp_path("partial.json");
        std::fs::write(
            &path,
            r#"{ "canvas": { "pattern": "grid" }, "from_the_future": 1, "interface": { "left_handed": true } }"#,
        )
        .expect("written");
        let loaded = Config::load(&path).expect("loaded");
        assert_eq!(loaded.canvas.pattern, Pattern::Grid);
        assert!(
            (loaded.canvas.grid_size - 32.0).abs() < 1e-6,
            "an absent field keeps its default"
        );
        assert!(loaded.interface.left_handed);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_broken_file_is_an_error_not_a_crash() {
        let path = temp_path("broken.json");
        std::fs::write(&path, "{ this is not json").expect("written");
        assert!(Config::load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn values_are_clamped_after_loading() {
        let path = temp_path("wild.json");
        std::fs::write(
            &path,
            r#"{ "interface": { "scale": 40.0 }, "canvas": { "grid_size": 0.0 },
                 "brushes": { "pen": { "size": 100000.0, "opacity": -3.0, "smoothness": 9.0 } },
                 "rendering": { "textures_folder": "   " } }"#,
        )
        .expect("written");
        let loaded = Config::load(&path).expect("loaded");
        assert!((loaded.interface.scale - Config::MAX_SCALE).abs() < 1e-6);
        assert!((loaded.canvas.grid_size - 4.0).abs() < 1e-6);
        let pen = loaded.brush(Pen::Pen);
        assert!((pen.size - BrushConfig::MAX_SIZE).abs() < 1e-6);
        assert!((pen.opacity - BrushConfig::MIN_OPACITY).abs() < 1e-6);
        assert!((pen.smoothness - 1.0).abs() < 1e-6);
        assert_eq!(loaded.rendering.textures_folder, None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pressure_response_cuts_off_the_ends_and_spreads_what_is_between() {
        let mut input = InputConfig::default();
        assert!(
            (input.pressure(0.3) - 0.3).abs() < 1e-6,
            "untouched by default"
        );
        input.pressure_low = 0.2;
        input.pressure_high = 0.8;
        assert_eq!(
            input.pressure(0.1),
            0.0,
            "lighter than the lightest counts as nothing"
        );
        assert_eq!(input.pressure(0.95), 1.0, "firmer than the firmest is full");
        assert!(
            (input.pressure(0.5) - 0.5).abs() < 1e-6,
            "the middle stays the middle"
        );
        assert!((input.pressure(0.35) - 0.25).abs() < 1e-6);
        input.pressure_high = 0.2;
        assert!(
            input.pressure(0.2).is_finite(),
            "a closed range cannot divide by zero"
        );
    }

    #[test]
    fn a_wrecked_pressure_range_is_put_right_when_loaded() {
        let path = temp_path("pressure.json");
        std::fs::write(
            &path,
            r#"{ "input": { "pressure_low": 3.0, "pressure_high": -1.0 }, "canvas": { "grid_divisions": 0 } }"#,
        )
        .expect("written");
        let loaded = Config::load(&path).expect("loaded");
        assert!(loaded.input.pressure_low <= 0.9);
        assert!(loaded.input.pressure_high >= loaded.input.pressure_low + 0.05 - 1e-6);
        assert_eq!(loaded.canvas.grid_divisions, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn taps_with_more_fingers_find_their_action() {
        let input = InputConfig::default();
        assert_eq!(input.tap_action(2), TapAction::Undo);
        assert_eq!(input.tap_action(3), TapAction::Redo);
        assert_eq!(input.tap_action(4), TapAction::None);
        assert_eq!(
            input.tap_action(7),
            TapAction::None,
            "five fingers count as four"
        );
        assert_eq!(
            input.tap_action(1),
            TapAction::None,
            "one finger is drawing, not tapping"
        );
    }

    #[test]
    fn the_rail_can_be_resized_and_filled_without_repeating_a_pen() {
        let mut config = Config::default();
        config.resize_rail(8);
        let pens = config.rail_pens();
        assert_eq!(pens.len(), 8);
        for (i, pen) in pens.iter().enumerate() {
            assert!(!pens[..i].contains(pen), "{pen:?} is there twice");
        }
        assert_eq!(
            pens[..5],
            Config::default().rail_pens()[..],
            "the first ones stay"
        );
        config.resize_rail(1);
        assert_eq!(
            config.rail_pens().len(),
            RAIL_SLOTS.0,
            "never fewer than the least"
        );
        config.resize_rail(99);
        assert_eq!(
            config.rail_pens().len(),
            RAIL_SLOTS.1,
            "never more than the most"
        );
    }

    #[test]
    fn choosing_a_pen_for_a_slot_swaps_it_with_the_one_already_on_the_rail() {
        let mut config = Config::default();
        let before = config.rail_pens();
        config.set_rail_slot(0, before[2]);
        let after = config.rail_pens();
        assert_eq!((after[0], after[2]), (before[2], before[0]));
        config.set_rail_slot(1, Pen::Airbrush);
        assert_eq!(config.rail_pens()[1], Pen::Airbrush);
        let len = config.rail_pens().len();
        config.set_rail_slot(99, Pen::Pen);
        assert_eq!(
            config.rail_pens().len(),
            len,
            "a slot that is not there changes nothing"
        );
    }

    #[test]
    fn tool_settings_are_clamped_and_keep_their_defaults_when_absent() {
        let path = temp_path("tools.json");
        std::fs::write(
            &path,
            r#"{ "tools": { "select_shape": "rectangle", "eraser_radius": 9000.0, "arrow_step": 0.0,
                            "select_coverage": 4.0 } }"#,
        )
        .expect("written");
        let loaded = Config::load(&path).expect("loaded");
        assert_eq!(loaded.tools.select_shape, SelectShape::Rectangle);
        assert_eq!(
            loaded.tools.slice_mode,
            SliceMode::Line,
            "absent: the default"
        );
        assert!((loaded.tools.eraser_radius - ToolsConfig::ERASER_RANGE.1).abs() < 1e-6);
        assert!((loaded.tools.arrow_step - ToolsConfig::ARROW_RANGE.0).abs() < 1e-6);
        assert!((loaded.tools.select_coverage - 1.0).abs() < 1e-6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn changed_shortcuts_are_saved_and_the_others_are_not() {
        let mut config = Config::default();
        assert!(config.shortcuts.is_empty());
        config
            .shortcuts
            .insert("undo".to_owned(), vec!["Ctrl+Y".to_owned()]);
        let path = temp_path("keys.json");
        config.save(&path).expect("saved");
        assert_eq!(Config::load(&path).expect("loaded"), config);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_library_folder_is_chosen_or_defaults_under_documents() {
        let mut config = Config::default();
        assert!(config.library_folder().ends_with("inkwell"));
        config.files.library_folder = Some("/somewhere/else".to_owned());
        assert_eq!(config.library_folder(), PathBuf::from("/somewhere/else"));
        config.files.library_folder = Some("  ".to_owned());
        assert_eq!(config.clamped().files.library_folder, None);
    }

    #[test]
    fn a_pen_outside_the_rail_takes_the_last_slot() {
        let mut config = Config::default();
        assert_eq!(config.rail_pens().len(), DEFAULT_RAIL.len());
        config.use_in_rail(Pen::Airbrush);
        let pens = config.rail_pens();
        assert_eq!(pens.len(), DEFAULT_RAIL.len());
        assert_eq!(pens.last(), Some(&Pen::Airbrush));
        assert!(!pens.contains(&Pen::Watercolor));
        config.use_in_rail(Pen::Pen);
        assert_eq!(
            config.rail_pens(),
            pens,
            "a pen already there changes nothing"
        );
    }

    #[test]
    fn a_damaged_rail_falls_back_to_the_default() {
        let mut config = Config::default();
        config.interface.rail = vec!["nonsense".to_owned(), "pen".to_owned(), "pen".to_owned()];
        assert_eq!(config.rail_pens(), vec![Pen::Pen]);
        config.interface.rail = vec!["nonsense".to_owned()];
        assert_eq!(config.rail_pens().len(), DEFAULT_RAIL.len());
    }

    #[test]
    fn recent_colors_are_newest_first_unique_and_limited() {
        let mut config = Config::default();
        for i in 0..20_u8 {
            config.remember_color([i, 0, 0]);
        }
        config.remember_color([15, 0, 0]);
        assert_eq!(config.colors.recent.len(), RECENT_COLORS);
        assert_eq!(config.colors.recent[0], [15, 0, 0]);
        assert_eq!(
            config
                .colors
                .recent
                .iter()
                .filter(|c| **c == [15, 0, 0])
                .count(),
            1
        );
    }
}
