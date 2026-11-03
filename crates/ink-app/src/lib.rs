//! The inkwell application: window, canvas and tool UI built with egui.
//!
//! All drawing logic lives in `ink-core`; this crate only turns pointer input into calls on it and draws the
//! result.

mod app;
mod brushes;
mod canvas;
mod color_panel;
mod commands;
mod config;
mod files;
mod flyouts;
mod gpu;
mod guides;
mod hud;
mod icons;
mod layers_panel;
mod library;
mod library_page;
mod paper;
mod pen_art;
mod precision;
mod rail;
mod render;
mod selection;
mod selection_bar;
mod settings;
mod shortcuts;
mod shortcuts_panel;
mod size_fields;
mod state;
mod tab_strip;
mod tabs;
mod textures;
mod theme;
mod tools;
mod touch_taps;
mod widgets;

use std::path::PathBuf;

pub use app::InkApp;

/// Command-line options.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent command-line switches; grouping them into enums would only obscure them"
)]
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// `.ink` files to open at start-up, each in its own tab.
    pub open: Vec<PathBuf>,
    /// Start with the color panel open (used to check it in screenshots).
    pub show_color_picker: bool,
    /// Start on the notes page (used to check it in screenshots).
    pub library: bool,
    /// Put the notes page in a state worth a screenshot once the first note has loaded: `menu`, `rename`,
    /// `undo` or `search`.
    pub library_demo: Option<String>,
    /// Keep drawings in this folder instead of the one in the settings.
    pub library_folder: Option<PathBuf>,
    /// Start with the case of every pen open (used to check it in screenshots).
    pub show_brushes: bool,
    /// Start with the size and opacity of the pen open (used to check it in screenshots).
    pub show_dial: bool,
    /// Start with the note menu open (used to check it in screenshots).
    pub show_menu: bool,
    /// Start with the precision panel open (used to check it in screenshots).
    pub show_precision: bool,
    /// Start with snapping on whatever the settings say (used to check it in screenshots).
    pub snap: bool,
    /// Put an artboard of this preset (`A4`, `HD`, ...) on the paper once it is on the screen (used to check
    /// artboards in screenshots).
    pub artboard: Option<String>,
    /// Turn the paper this many degrees counterclockwise to begin with (used to check turned paper in screenshots).
    pub turn: Option<f32>,
    /// Put these shape guides on the paper once it is on the screen (used to check them in screenshots): `line`,
    /// `arc`, `angle`, `circle` or `square`.
    pub guides: Vec<String>,
    /// Start with the settings window open on this section, named as in the window (used to check it in
    /// screenshots).
    pub settings: Option<String>,
    /// Use the left-handed layout whatever the settings say (used to check it in screenshots).
    pub left_handed: bool,
    /// Read and save the settings here instead of in the usual place. Without it, screenshot runs use the
    /// defaults and save nothing.
    pub config: Option<PathBuf>,
    /// A folder holding stamp textures for the pencils, marker, airbrush and watercolor. Without one, the
    /// `INKWELL_TEXTURES` variable and a `textures` folder next to the program are tried.
    pub textures: Option<PathBuf>,
    /// Start with this pen selected (used to check pens in screenshots).
    pub pen: Option<ink_core::Pen>,
    /// Start with this pen size for the selected pen (used to check pens in screenshots; it is saved with the
    /// settings the next time they change).
    pub size: Option<f32>,
    /// Draw strokes with egui shapes on the CPU instead of the OpenGL stroke renderer.
    pub cpu_render: bool,
    /// Start a stroke with synthetic pointer input and leave it unfinished (used to check live drawing in
    /// screenshots).
    pub demo_stroke: bool,
    /// Draw a curve, select it and open the numbers of the selection (used to check the selection bar in
    /// screenshots).
    pub demo_select: bool,
    /// Save a screenshot of the first fully drawn frames to this PNG path and exit (used for testing).
    pub screenshot: Option<PathBuf>,
    /// Open the window this big, in points, instead of 1280 by 800 (used to check the layout of small windows in
    /// screenshots). It is never smaller than the smallest the window can be.
    pub window: Option<[f32; 2]>,
    /// Rest the pointer here from the second frame on, as if hovering (used to see what hovering does in
    /// screenshots).
    pub hover: Option<[f32; 2]>,
}

/// Opens the window and runs until it is closed.
///
/// # Errors
/// Returns the windowing or graphics error if the app could not start.
pub fn run(options: Options) -> eframe::Result {
    let least = [640.0, 480.0];
    let size = options
        .window
        .map_or([1280.0, 800.0], |[w, h]| [w.max(least[0]), h.max(least[1])]);
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("inkwell")
        .with_inner_size(size)
        .with_min_inner_size(least);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let native = eframe::NativeOptions {
        viewport,
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "inkwell",
        native,
        Box::new(move |cc| Ok(Box::new(InkApp::new(cc, options.clone())))),
    )
}

/// The logo as the icon of the window (the title bar and the task bar). The Windows executable carries its own
/// copy for Explorer, see `build.rs`. `None` if the picture cannot be read, which leaves the system's icon.
fn window_icon() -> Option<std::sync::Arc<eframe::egui::IconData>> {
    let png = include_bytes!("../../../assets/logo/inkwell-256.png");
    let image = image::load_from_memory(png).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(std::sync::Arc::new(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }))
}
