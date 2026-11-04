//! The eframe application: owns the document and tool state and lays out the screen.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Modifiers, ViewportCommand, ecolor::Hsva,
};
use ink_core::document::Stroke;
use ink_core::{Editor, IdGen, Pen, StrokeBuilder};
use ink_format::FormatError;
use uuid::Uuid;

use crate::Options;
use crate::brushes;
use crate::canvas::{self, CanvasEnv};
use crate::color_panel;
use crate::config::{self, Config, Renderer};
use crate::gpu::GpuScene;
use crate::guides::GuideState;
use crate::library::Library;
use crate::objects::{self, Objects};
use crate::platform;
use crate::render::StrokeCache;
use crate::settings::Section;
use crate::shapes_drawer;
use crate::shell;
use crate::shortcuts::Keymap;
use crate::state::{Mode, ToolState, UiState, ViewState};
use crate::tabs::Tab;
use crate::text_editor;
use crate::textures;
use crate::tools::Gesture;
use crate::touch_taps::TapTracker;
use crate::{flyouts, hud, paper, selection_bar, theme};

/// Frames to wait before taking a requested screenshot, so layout and fonts have settled.
const SCREENSHOT_FRAME: u32 = 12;

type Shot = Arc<Mutex<Option<Arc<ColorImage>>>>;

/// No strokes: what is left out of the drawing when no words are being changed.
static NO_STROKES: std::sync::LazyLock<std::collections::HashSet<Uuid>> =
    std::sync::LazyLock::new(std::collections::HashSet::new);

/// The application state.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags of the frame in progress"
)]
pub struct InkApp {
    pub(crate) editor: Editor,
    pub(crate) tool: ToolState,
    pub(crate) view: ViewState,
    pub(crate) ui: UiState,
    /// The user's settings.
    pub(crate) config: Config,
    /// Where the settings are saved; `None` when they are not (tests and screenshots).
    pub(crate) config_path: Option<PathBuf>,
    config_dirty: bool,
    /// The shapes the app provides and the objects the person made.
    pub(crate) objects: Objects,
    /// The notes page: what is in the notes folder.
    pub(crate) library: Library,
    /// The editor revision last seen, and when it first appeared, for saving a moment after the last change.
    pub(crate) autosave_seen: u64,
    pub(crate) autosave_at: f64,
    /// The screen areas of everything floating over the paper this frame. A press outside all of them
    /// closes the open flyout.
    pub(crate) chrome: Vec<egui::Rect>,
    /// How visible the floating panels are, `0.0..=1.0`; they fade while a stroke is being drawn.
    pub(crate) chrome_alpha: f32,
    /// While the eraser is down, what it is doing.
    pub(crate) eraser: Option<canvas::Erasing>,
    /// The strokes picked with the select tool, and what the select or slice tool is doing with the pen down.
    pub(crate) selection: Vec<Uuid>,
    pub(crate) gesture: Option<Gesture>,
    /// The tool in use before this one, for the key that goes back to it.
    pub(crate) previous_mode: Mode,
    /// The key for moving the canvas is being held.
    pub(crate) pan_held: bool,
    /// Follows the fingers on the screen for taps with several of them.
    pub(crate) taps: TapTracker,
    /// Taps are not acted on until this time (the frame time, in seconds), and the editor revision seen last: a
    /// hand resting on the screen while the pen draws must not count as a tap.
    pub(crate) no_taps_until: f64,
    pub(crate) taps_revision: u64,
    /// The strokes last copied or cut, and how many times they have been pasted since.
    pub(crate) clipboard: Vec<Stroke>,
    pub(crate) pastes: u32,
    /// The keys of every command, read from the settings.
    pub(crate) keymap: Keymap,
    /// Where the paper was on screen last frame.
    pub(crate) canvas_rect: Option<egui::Rect>,
    /// The open drawings other than the live one are parked in these tabs (see `tabs.rs`).
    pub(crate) tabs: Vec<Tab>,
    /// The tab whose drawing is the live one; `None` when no drawing is open.
    pub(crate) active: Option<usize>,
    /// Identifies the live drawing, so render caches never mix two of them up.
    pub(crate) doc_id: u64,
    pub(crate) next_tab: u64,
    /// The tab and revision for which the "unsaved changes" warning on closing was already shown.
    pub(crate) armed_close: Option<(u64, u64)>,
    /// Index into `document.layers` of the layer strokes are drawn on.
    pub(crate) active_layer: usize,
    /// When set, the view is fitted to the drawing on the next frame.
    pub(crate) fit_pending: bool,
    /// The file the drawing was opened from or last saved to.
    pub(crate) path: Option<PathBuf>,
    /// The editor revision that matches what is on disk (or the blank start).
    pub(crate) saved_revision: u64,
    /// A status message and the time it appeared.
    pub(crate) message: Option<(String, f64)>,
    last_title: String,
    pub(crate) builder: Option<StrokeBuilder>,
    /// The shape guides of the drawing on the screen.
    pub(crate) guides: GuideState,
    /// The page the drawing is on, if it has one. Kept with the drawing while it is open, and not saved in the
    /// file.
    pub(crate) artboard: Option<ink_core::artboard::Artboard>,
    /// The artboard, guides, locks and groups as they were when the drawing was last saved or opened.
    pub(crate) saved_extras: ink_format::Extras,
    /// Decides what each touch of a stylus, finger or palm does (Android).
    pub(crate) touch: crate::touch_input::TouchFilter,
    /// The same as seen by the autosave, to tell when they have changed since it last looked.
    pub(crate) autosave_extras: ink_format::Extras,
    /// How long the pen has been still, for shape recognition.
    hold: canvas::Hold,
    pub(crate) cache: StrokeCache,
    /// The OpenGL stroke renderer; `None` means strokes are drawn with egui shapes.
    gpu: Option<GpuScene>,
    force: Option<f32>,
    themed: bool,
    pub(crate) options: Options,
    frames: u32,
    demo_frame: u32,
    shot: Shot,
}

pub(crate) fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

impl InkApp {
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Self {
        let mut app = Self::blank(options);
        app.load_config(&cc.egui_ctx);
        let wants_gpu = !app.options.cpu_render && app.config.rendering.renderer == Renderer::Gpu;
        if wants_gpu && let Some(gl) = cc.gl.as_deref() {
            let configured = app
                .config
                .rendering
                .textures_folder
                .as_deref()
                .map(PathBuf::from);
            let folder =
                textures::find_folder(app.options.textures.as_deref().or(configured.as_deref()));
            let masks = folder
                .as_deref()
                .map(textures::load_masks)
                .unwrap_or_default();
            match &folder {
                Some(dir) if !masks.is_empty() => {
                    println!(
                        "loaded {} stamp textures from {}",
                        masks.len(),
                        dir.display()
                    );
                }
                _ => eprintln!(
                    "no stamp textures found; pencils and airbrush use plain soft stamps (a textures folder next to the program, or the texture folder setting, adds them)"
                ),
            }
            match GpuScene::new(gl, &masks) {
                Ok(scene) => app.gpu = Some(scene),
                Err(e) => {
                    eprintln!("the GPU renderer is not available, using the CPU renderer: {e}");
                }
            }
        }
        if let Some(pen) = app.options.pen {
            app.select_pen(pen);
        }
        if let Some(size) = app.options.size {
            let pen = app.tool.pen;
            app.config.brush_mut(pen).size = size;
        }
        if !app.options.open.is_empty() {
            app.clear_tabs();
            for path in app.options.open.clone() {
                app.open_in_tab(&cc.egui_ctx, &path);
            }
        }
        app
    }

    /// Reads the settings file, if there is one, and applies it. A file that cannot be read is moved aside so
    /// the next save does not overwrite it, and the app starts with the defaults.
    fn load_config(&mut self, ctx: &egui::Context) {
        let persist = self.options.config.is_some() || self.options.screenshot.is_none();
        self.config_path = persist
            .then(|| self.options.config.clone().or_else(config::default_path))
            .flatten();
        if let Some(path) = self.config_path.clone() {
            match Config::load(&path) {
                Ok(config) => self.config = config,
                Err(e) => {
                    let mut aside = path.as_os_str().to_owned();
                    aside.push(".broken");
                    let moved = std::fs::rename(&path, &aside).is_ok();
                    eprintln!("could not read the settings: {e}");
                    self.notify(
                        ctx,
                        if moved {
                            "The settings file was damaged. It was kept as config.json.broken and the defaults are in use."
                        } else {
                            "The settings file could not be read, so the defaults are in use."
                        },
                    );
                }
            }
        }
        if let Some(path) = self.config_path.as_deref().map(objects::file_beside)
            && let Err(e) = self.objects.load(&path)
        {
            eprintln!("could not read the objects: {e}");
        }
        self.apply_options_to_config();
        self.apply_config_to_tool();
        self.ui.layers_open = self.config.interface.show_layers;
        self.start_note();
        let starts_on_library = self.config.files.start_in_library
            && self.options.open.is_empty()
            && self.options.screenshot.is_none();
        if starts_on_library || self.options.library {
            // The Library is all there is to show: no drawing is open yet.
            self.clear_tabs();
            self.open_library(ctx);
        }
    }

    /// Command-line switches that stand in for settings, used by screenshots.
    fn apply_options_to_config(&mut self) {
        if self.options.left_handed {
            self.config.interface.left_handed = true;
        }
        if self.options.snap {
            self.config.precision.snap = true;
        }
        if self.options.show_drawer {
            self.config.interface.shapes_drawer = config::DrawerSide::Right;
        }
        if let Some(folder) = &self.options.library_folder {
            self.config.files.library_folder = Some(folder.display().to_string());
        }
    }

    /// The pen and color the settings say were in use last.
    fn apply_config_to_tool(&mut self) {
        self.tool.pen = brushes::from_key(&self.config.colors.last_brush).unwrap_or(Pen::Pen);
        self.tool.color = Hsva::from_srgb(self.config.colors.current);
        self.rebuild_keymap();
    }

    /// An empty drawing on the paper color from the settings.
    fn blank_editor(&self) -> Editor {
        Editor::blank_on(
            IdGen::new(seed()),
            &color_panel::to_hex(self.config.canvas.paper),
        )
    }

    pub(crate) fn blank(options: Options) -> Self {
        Self {
            editor: Editor::blank(IdGen::new(seed())),
            tool: ToolState::default(),
            view: ViewState::default(),
            ui: UiState {
                library_open: options.library,
                color_open: options.show_color_picker,
                case_open: options.show_brushes,
                dial_open: options.show_dial,
                sheet_open: options.show_menu,
                precision_open: options.show_precision,
                shapes_open: options.show_shapes,
                toolbar_open: options.show_toolbar,
                text: options
                    .show_text
                    .then(|| crate::text_ops::TextEntry::typing("Hello")),
                settings: options.settings.as_deref().map(Section::named),
                settings_home: options.settings.as_deref() == Some("home"),
                ..UiState::default()
            },
            config: Config::default(),
            config_path: None,
            config_dirty: false,
            objects: Objects::default(),
            library: Library::default(),
            autosave_seen: 0,
            autosave_at: 0.0,
            chrome: Vec::new(),
            chrome_alpha: 1.0,
            eraser: None,
            selection: Vec::new(),
            gesture: None,
            previous_mode: Mode::Draw,
            pan_held: false,
            taps: TapTracker::default(),
            no_taps_until: 0.0,
            taps_revision: 0,
            clipboard: Vec::new(),
            pastes: 0,
            keymap: Keymap::new(&BTreeMap::new()),
            canvas_rect: None,
            tabs: vec![Tab { id: 1, doc: None }],
            active: Some(0),
            doc_id: 1,
            next_tab: 2,
            armed_close: None,
            active_layer: 0,
            fit_pending: true,
            path: None,
            saved_revision: 0,
            message: None,
            last_title: String::new(),
            builder: None,
            guides: GuideState::default(),
            artboard: None,
            saved_extras: ink_format::Extras::default(),
            touch: crate::touch_input::TouchFilter::default(),
            autosave_extras: ink_format::Extras::default(),
            hold: canvas::Hold::default(),
            cache: StrokeCache::default(),
            gpu: None,
            force: None,
            themed: false,
            options,
            frames: 0,
            demo_frame: 0,
            shot: Arc::default(),
        }
    }

    /// Starts a new blank drawing, without asking about unsaved changes.
    pub fn new_document(&mut self) {
        self.editor = self.blank_editor();
        self.path = None;
        self.reset_after_load();
    }

    /// Replaces the document with one read from `.ink` bytes. The drawing stays as it was on failure.
    pub fn open_bytes(&mut self, bytes: &[u8]) -> Result<(), FormatError> {
        self.open_drawing(ink_format::read(bytes)?);
        Ok(())
    }

    /// Replaces the document with `drawing`, with its artboard, guides, locks and groups.
    pub(crate) fn open_drawing(&mut self, drawing: ink_format::Drawing) {
        self.editor = Editor::new(drawing.document, IdGen::new(seed()));
        self.path = None;
        self.reset_after_load();
        self.take_extras(&drawing.extras);
        self.saved_extras = self.extras();
        self.fit_pending = self.config.files.fit_on_open;
    }

    fn reset_after_load(&mut self) {
        self.active_layer = self
            .editor
            .content_layers()
            .find(|(_, l)| l.visible)
            .map_or(0, |(i, _)| i);
        self.cache.clear();
        self.builder = None;
        self.selection.clear();
        self.gesture = None;
        self.ui.renaming = None;
        self.fit_pending = true;
        self.saved_revision = self.editor.revision();
    }

    /// Opens files dropped on the window, each in a tab of its own.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path().to_owned();
            if path.as_os_str().is_empty() {
                match file.bytes() {
                    Ok(bytes) => {
                        if let Err(e) = self.open_bytes_in_tab(ctx, &bytes) {
                            self.notify(ctx, format!("Could not open the file: {e}"));
                        }
                    }
                    Err(e) => self.notify(ctx, format!("Could not read the file: {e}")),
                }
            } else {
                self.open_in_tab(ctx, &path);
            }
        }
    }

    /// Saves a requested screenshot once it arrives, then closes the window.
    fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.options.screenshot.clone() else {
            return;
        };
        self.frames += 1;
        if self.frames == SCREENSHOT_FRAME {
            let slot = Arc::clone(&self.shot);
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::ScreenshotCallback::new(
                move |image| {
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(image);
                    }
                },
            )));
        }
        let image = self.shot.lock().ok().and_then(|mut s| s.take());
        if let Some(image) = image {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(Color32::to_array).collect();
            let [w, h] = image.size;
            match image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
                Ok(()) => println!("screenshot saved to {}", path.display()),
                Err(e) => eprintln!("could not save the screenshot: {e}"),
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    /// Puts on the paper what the command line asked for to be there once it is on the screen: an artboard, a
    /// turn and shape guides (used to check them in screenshots). Each is done once.
    fn apply_view_options(&mut self, canvas_rect: egui::Rect) {
        if let Some(name) = self.options.artboard.take() {
            let ppi = self.config.measurement.pixels_per_inch;
            if let Some(preset) = ink_core::artboard::PRESETS
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
            {
                let (w, h) = preset.size(ppi);
                self.artboard = Some(ink_core::artboard::Artboard::centered(
                    ink_core::Point::new(0.0, 0.0),
                    w,
                    h,
                ));
                self.fit_pending = true;
            }
        }
        if let Some(degrees) = self.options.turn.take() {
            self.view
                .rotate_at(canvas_rect.center(), degrees.to_radians());
        }
        for name in std::mem::take(&mut self.options.guides) {
            if let Some(kind) = ink_core::guide::Kind::ALL
                .into_iter()
                .find(|k| k.label().eq_ignore_ascii_case(&name))
            {
                self.toggle_guide(kind);
            }
        }
    }

    /// Fills the strips of the window the system covers (its status and navigation bars) with the color of the top
    /// bar, so the icons the system draws there can be read.
    fn paint_under_system_bars(ctx: &egui::Context) {
        let (whole, safe) = (ctx.viewport_rect(), ctx.content_rect());
        if whole == safe {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::background());
        let strips = [
            egui::Rect::from_min_max(whole.min, egui::pos2(whole.max.x, safe.min.y)),
            egui::Rect::from_min_max(egui::pos2(whole.min.x, safe.max.y), whole.max),
            egui::Rect::from_min_max(
                egui::pos2(whole.min.x, safe.min.y),
                egui::pos2(safe.min.x, safe.max.y),
            ),
            egui::Rect::from_min_max(
                egui::pos2(safe.max.x, safe.min.y),
                egui::pos2(whole.max.x, safe.max.y),
            ),
        ];
        for strip in strips {
            painter.rect_filled(strip, 0.0, platform::SYSTEM_BAR);
        }
    }

    /// Lays out one frame. Separate from [`eframe::App::ui`] so tests can run it without a window.
    #[allow(
        clippy::too_many_lines,
        reason = "the panels of a frame, one after the other"
    )]
    pub fn ui_root(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        // Everything is laid out inside the part of the window the system leaves free (not under Android's status
        // and navigation bars); the strips it covers are painted by `paint_under_system_bars`.
        let mut inside = root.new_child(egui::UiBuilder::new().max_rect(ctx.content_rect()));
        let ui = &mut inside;
        if !self.themed {
            theme::apply(&ctx);
            self.themed = true;
        }
        let scale = self.config.interface.scale;
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        shell::begin_frame(&ctx);
        self.chrome.clear();
        Self::paint_under_system_bars(&ctx);
        if self.tabs.is_empty() {
            self.ui.library_open = true;
        }
        self.handle_dropped_files(&ctx);
        self.handle_system_files(&ctx);
        if platform::take_back() {
            self.go_back(&ctx);
        }
        self.handle_shortcuts(&ctx);
        self.pan_held = self.hold_pan_active(&ctx);
        // The selection only means something while the select tool is in use.
        if self.tool.mode != Mode::Select {
            self.selection.clear();
        }
        if !matches!(self.tool.mode, Mode::Select | Mode::Slice | Mode::Nudge) {
            self.abandon_gesture();
        }

        let title = self.title();
        if title != self.last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        shell::top_bar(ui, self);

        let mut edit_words = false;
        let canvas_rect = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let mut env = CanvasEnv {
                    editor: &mut self.editor,
                    view: &mut self.view,
                    tool: &self.tool,
                    config: &self.config,
                    active_layer: self.active_layer,
                    builder: &mut self.builder,
                    eraser: &mut self.eraser,
                    cache: &mut self.cache,
                    epoch: self.doc_id,
                    gpu: self.gpu.as_mut(),
                    force: &mut self.force,
                    fit: &mut self.fit_pending,
                    selection: &mut self.selection,
                    gesture: &mut self.gesture,
                    pan_held: self.pan_held,
                    guides: &mut self.guides,
                    artboard: self.artboard,
                    hold: &mut self.hold,
                    hidden: self.ui.text.as_ref().map_or(&NO_STROKES, |t| &t.hide),
                    edit_words: &mut edit_words,
                };
                canvas::show(ui, &mut env)
            })
            .inner;
        self.canvas_rect = Some(canvas_rect);
        if edit_words {
            self.set_mode(Mode::Select);
            self.begin_text();
        }
        self.apply_view_options(canvas_rect);
        if std::mem::take(&mut self.options.demo_shapes) {
            self.demo_shapes();
        }
        self.handle_taps(&ctx);

        if self.ui.text.is_none() {
            self.paint_empty_hint(ui, canvas_rect);
        }

        // The panels fade out while a stroke is being drawn, when asked to, and come back when the pen lifts.
        let drawing = self.builder.is_some() || self.eraser.is_some();
        let put_away = (drawing && self.config.interface.hide_while_drawing)
            || self.ui.panels_hidden
            || self.ui.text.is_some();
        let visible = !put_away;
        self.chrome_alpha =
            ctx.animate_bool_with_time(egui::Id::unique("chrome_alpha"), visible, 0.2);
        shell::library(&ctx, self);
        if self.chrome_alpha > 0.01 {
            hud::show(&ctx, self);
            if !self.ui.library_open {
                shell::precision(&ctx, self);
                shell::tools(&ctx, self);
                shell::layers(&ctx, self);
                shell::shapes(&ctx, self);
                shapes_drawer::show(&ctx, self);
                shell::toolbar_editor(&ctx, self);
                selection_bar::show(&ctx, self);
                flyouts::show_dial(&ctx, self);
                flyouts::show_case(&ctx, self);
                color_panel::show(&ctx, self);
            }
        }
        if !self.ui.library_open {
            text_editor::show(&ctx, self);
        }
        shell::overlays(&ctx, self);
        self.close_flyouts_on_outside_press(&ctx);
        shell::settings(&ctx, self);
        self.autosave_tick(&ctx);
        self.tell_platform(&ctx);

        self.flush_config(&ctx);
        self.drive_screenshot(&ctx);
    }
}

impl InkApp {
    /// A faint invitation in the middle of a drawing that has nothing in it yet.
    fn paint_empty_hint(&self, ui: &egui::Ui, canvas_rect: egui::Rect) {
        let is_empty = self
            .editor
            .document()
            .layers
            .iter()
            .all(|l| l.strokes.is_empty());
        if !is_empty || self.builder.is_some() {
            return;
        }
        let page = canvas::parse_hex(&self.editor.document().metadata.background_solid_color);
        let ink = paper::ink_for(page);
        let faint = |alpha: u8| Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), alpha);
        let center = canvas_rect.center();
        ui.painter().text(
            center - egui::vec2(0.0, 64.0),
            Align2::CENTER_CENTER,
            "Start writing",
            FontId::proportional(32.0),
            faint(56),
        );
        ui.painter().text(
            center - egui::vec2(0.0, 28.0),
            Align2::CENTER_CENTER,
            "or drop a .ink file here",
            FontId::proportional(15.0),
            faint(48),
        );
    }

    /// A press anywhere but on the floating panels puts away the open flyout.
    fn close_flyouts_on_outside_press(&mut self, ctx: &egui::Context) {
        if !self.ui.any_popover() || !ctx.input(|i| i.pointer.any_pressed()) {
            return;
        }
        if let Some(at) = ctx.input(|i| i.pointer.interact_pos())
            && !self.chrome.iter().any(|area| area.contains(at))
        {
            self.ui.close_popovers();
        }
    }

    /// Tells the system what it asks about: whether a keyboard is wanted, and whether Back would do something here.
    fn tell_platform(&self, ctx: &egui::Context) {
        platform::soft_keyboard(
            ctx.egui_wants_keyboard_input(),
            ctx.input(|i| i.pointer.any_pressed()),
        );
        platform::set_can_go_back(self.can_go_back());
    }

    /// Notes that the settings changed, so they are saved soon.
    pub(crate) fn mark_config_changed(&mut self) {
        self.config_dirty = true;
    }

    /// Writes changed settings to disk, but not while a button is held, so dragging a slider is not a stream
    /// of writes.
    fn flush_config(&mut self, ctx: &egui::Context) {
        self.flush_objects();
        if !self.config_dirty || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.config_dirty = false;
        if let Some(path) = &self.config_path
            && let Err(e) = self.config.save(path)
        {
            eprintln!("could not save the settings: {e}");
        }
    }

    /// Writes the person's objects if they changed. A change is a thing done once, so it is written at once.
    fn flush_objects(&mut self) {
        if !self.objects.take_changed() {
            return;
        }
        if let Some(path) = self.config_path.as_deref().map(objects::file_beside)
            && let Err(e) = self.objects.save(&path)
        {
            eprintln!("could not save the objects: {e}");
        }
    }

    /// Makes `pen` the brush in use.
    pub(crate) fn select_pen(&mut self, pen: Pen) {
        self.tool.pen = pen;
        self.config.use_in_rail(pen);
        brushes::key(pen).clone_into(&mut self.config.colors.last_brush);
        self.mark_config_changed();
    }

    /// Makes `rgb` the drawing color.
    pub(crate) fn set_color_rgb(&mut self, rgb: [u8; 3]) {
        self.tool.color = Hsva::from_srgb(rgb);
        self.sync_color();
    }

    /// Stores the color the picker currently holds in the settings.
    pub(crate) fn sync_color(&mut self) {
        self.config.colors.current = self.tool.color.to_srgb();
        self.mark_config_changed();
    }

    /// Adds the current color to the recent colors.
    pub(crate) fn remember_color(&mut self) {
        let color = self.tool.color.to_srgb();
        self.config.remember_color(color);
        self.mark_config_changed();
    }

    /// Opens the settings window on `section`.
    pub(crate) fn open_settings(&mut self, section: Section) {
        self.ui.close_popovers();
        self.ui.settings = Some(section);
        self.ui.settings_home = false;
    }

    /// Opens the settings on their landing page: the first section where the sections sit beside their content, the
    /// list of sections where there is room for only one thing.
    pub(crate) fn open_settings_home(&mut self) {
        self.open_settings(Section::Brushes);
        self.ui.settings_home = true;
    }

    /// Puts every setting back to its starting value.
    pub(crate) fn reset_config(&mut self) {
        self.config = Config::default();
        self.apply_config_to_tool();
        self.mark_config_changed();
    }

    /// Which renderer is drawing the strokes right now.
    pub(crate) fn renderer_name(&self) -> &'static str {
        if self.gpu.is_some() {
            "GPU (OpenGL)"
        } else {
            "CPU (plain shapes)"
        }
    }
}

impl InkApp {
    /// Feeds a wavy half-finished stroke to the canvas, one pointer event per frame.
    fn inject_demo_stroke(&mut self, input: &mut egui::RawInput) {
        use egui::{Event, PointerButton, pos2};
        self.demo_frame += 1;
        let at = |i: u32| {
            let t = i as f32 / 8.0;
            pos2(300.0 + t * 500.0, 380.0 + (t * 6.0).sin() * 70.0)
        };
        match self.demo_frame {
            2 => input.events.push(Event::PointerMoved(at(0))),
            3 => input.events.push(Event::PointerButton {
                pos: at(0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }),
            4..=11 => input
                .events
                .push(Event::PointerMoved(at(self.demo_frame - 3))),
            _ => {}
        }
    }
}

impl InkApp {
    /// Input from the platform's own touch and key queues (Android): each stylus, finger and palm is told apart and
    /// turned into the events the rest of the app reads. Nothing is queued on other systems, where the window
    /// library delivers input.
    fn android_input(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        crate::touch_input::wake_on_input(ctx);
        let samples = crate::touch_input::take_pointers();
        let keys = crate::touch_input::take_keys();
        let policy = crate::touch_input::Policy {
            fingers: self.config.input.finger_drawing,
            palm_rejection: self.config.input.palm_rejection,
            stylus_device: self.config.input.stylus_seen,
            pen_hold: self.config.input.pen_hold(),
        };
        let now = raw_input.time.unwrap_or(0.0);
        // A pen held still turns into a drag of the page, with or without new input, so this is asked every frame.
        let (held, came) = self.touch.tick(now);
        if came {
            // What the pen drew while it was held (a dot) is not kept: it was the start of a drag.
            self.builder = None;
            self.abandon_gesture();
        }
        if samples.is_empty() && keys.is_empty() && held.is_empty() {
            return;
        }
        raw_input.events.extend(held);
        let canvas = self.canvas_rect;
        let place = |pos: egui::Pos2| {
            let over_paper = canvas.is_some_and(|r| r.contains(pos))
                && ctx
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer.order == egui::Order::Background);
            if over_paper {
                crate::touch_input::Place::Canvas
            } else {
                crate::touch_input::Place::Panel
            }
        };
        let events = self
            .touch
            .process(now, &samples, ctx.pixels_per_point(), policy, place);
        raw_input.events.extend(events);
        // The first time a stylus touches the screen is remembered: from then on a finger moves the page.
        if self.touch.stylus_seen() && !self.config.input.stylus_seen {
            self.config.input.stylus_seen = true;
            self.mark_config_changed();
        }
        for key in &keys {
            raw_input.events.extend(crate::touch_input::key_events(key));
        }
    }

    /// Draws a short curve, switches to the select tool, selects it and opens the numbers of the selection (used to
    /// check the selection bar in screenshots).
    fn inject_demo_select(&mut self, input: &mut egui::RawInput) {
        use egui::{Event, Key, PointerButton, pos2};
        self.demo_frame += 1;
        let at = |i: u32| {
            let t = i as f32 / 4.0;
            pos2(420.0 + t * 380.0, 400.0 + (t * 5.0).sin() * 60.0)
        };
        let key = |key: Key, command: bool| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                command,
                ctrl: command,
                ..Modifiers::NONE
            },
        };
        let button = |pressed: bool| Event::PointerButton {
            pos: at(4),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        match self.demo_frame {
            2 => input.events.push(Event::PointerMoved(at(0))),
            3 => input.events.push(Event::PointerButton {
                pos: at(0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }),
            4..=7 => input
                .events
                .push(Event::PointerMoved(at(self.demo_frame - 3))),
            8 => input.events.push(button(false)),
            9 => input.events.push(key(Key::V, false)),
            10 => {
                input.events.push(key(Key::A, true));
                self.ui.size_fields.open = true;
            }
            _ => {}
        }
    }
}

impl eframe::App for InkApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // The last change may still be waiting for its moment, and the settings for the next quiet frame.
        if self.is_dirty()
            && self.autosaves()
            && let Some(path) = self.path.clone()
        {
            let _ = self.write_file(&path);
        }
        if self.config_dirty
            && let Some(path) = &self.config_path
        {
            let _ = self.config.save(path);
        }
        self.flush_objects();
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.android_input(ctx, raw_input);
        // Where the system covers the window (Android's bars and keyboard), so panels keep clear of it.
        let scale = raw_input
            .viewports
            .get(&raw_input.viewport_id)
            .and_then(|v| v.native_pixels_per_point)
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        platform::set_keyboard(ctx, platform::keyboard_pixels() / scale);
        if let Some([left, top, right, bottom]) = platform::covered_pixels() {
            raw_input.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
                left: left / scale,
                top: top / scale,
                right: right / scale,
                bottom: bottom / scale,
            }));
        }
        if self.options.demo_stroke {
            self.inject_demo_stroke(raw_input);
        }
        if self.options.demo_select {
            self.inject_demo_select(raw_input);
        }
        if let Some([x, y]) = self.options.hover {
            self.demo_frame += 1;
            if self.demo_frame >= 2 {
                raw_input
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(x, y)));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_root(ui);
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Key, PointerButton, RawInput, Rect, pos2, vec2};

    use super::*;

    struct Harness {
        ctx: Context,
        app: InkApp,
        time: f64,
        /// The size of the window, in points.
        size: egui::Vec2,
    }

    impl Harness {
        fn new() -> Self {
            Self::sized(1280.0, 800.0)
        }

        fn sized(width: f32, height: f32) -> Self {
            let mut h = Self {
                ctx: Context::default(),
                app: InkApp::blank(Options::default()),
                time: 0.0,
                size: vec2(width, height),
            };
            h.frame(Vec::new()); // lay everything out once so hit testing knows the layers
            h
        }

        /// Runs one frame of the app and returns what it drew.
        fn run(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
                events,
                time: Some(self.time),
                ..RawInput::default()
            };
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| app.ui_root(ui));
            output.textures_delta.clear(); // nothing is rendered in tests
            output
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.run(events);
        }

        /// Runs a frame and returns a number that is the same for two frames only if they drew the same picture.
        fn frame_hash(&mut self, events: Vec<Event>) -> u64 {
            use std::hash::{Hash, Hasher};
            let output = self.run(events);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", output.shapes).hash(&mut hasher);
            hasher.finish()
        }

        /// Runs a frame and returns the place of everything it drew, to a quarter of a point.
        fn drawn(&mut self, events: Vec<Event>) -> std::collections::HashSet<[i32; 4]> {
            fn flatten(shape: &egui::Shape, out: &mut std::collections::HashSet<[i32; 4]>) {
                if let egui::Shape::Vec(shapes) = shape {
                    for shape in shapes {
                        flatten(shape, out);
                    }
                    return;
                }
                let r = shape.visual_bounding_rect();
                if r.is_finite() && r.width() > 0.0 {
                    out.insert(
                        [r.min.x, r.min.y, r.max.x, r.max.y].map(|v| (v * 4.0).round() as i32),
                    );
                }
            }
            let output = self.run(events);
            let mut all = std::collections::HashSet::new();
            for clipped in &output.shapes {
                flatten(&clipped.shape, &mut all);
            }
            all
        }

        fn drag(&mut self, points: &[(f32, f32)]) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let first = pos2(points[0].0, points[0].1);
            self.frame(vec![Event::PointerMoved(first)]);
            self.frame(vec![button(first, true)]);
            for &(x, y) in &points[1..] {
                self.frame(vec![Event::PointerMoved(pos2(x, y))]);
            }
            let last = points[points.len() - 1];
            self.frame(vec![button(pos2(last.0, last.1), false)]);
        }

        fn strokes(&self) -> usize {
            self.app
                .editor
                .document()
                .layers
                .iter()
                .map(|l| l.strokes.len())
                .sum()
        }
    }

    #[test]
    fn dragging_on_the_canvas_draws_one_stroke_that_can_be_undone() {
        let mut h = Harness::new();
        h.drag(&[
            (400.0, 300.0),
            (450.0, 340.0),
            (520.0, 330.0),
            (600.0, 380.0),
        ]);
        assert_eq!(h.strokes(), 1);
        assert!(h.app.editor.undo());
        assert_eq!(h.strokes(), 0);
    }

    #[test]
    fn a_hidden_layer_is_not_drawn_on() {
        let mut h = Harness::new();
        h.app.editor.set_layer_visible(0, false);
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        assert_eq!(h.strokes(), 0);
    }

    #[test]
    fn panning_mode_moves_the_view_instead_of_drawing() {
        let mut h = Harness::new();
        h.app.tool.mode = crate::state::Mode::Pan;
        let before = h.app.view.origin;
        h.drag(&[(400.0, 300.0), (450.0, 340.0), (500.0, 380.0)]);
        assert_eq!(h.strokes(), 0);
        assert_ne!(h.app.view.origin, before);
    }

    fn first_stroke(app: &InkApp) -> &ink_core::Stroke {
        app.editor
            .document()
            .layers
            .iter()
            .flat_map(|l| &l.strokes)
            .next()
            .expect("a stroke was drawn")
    }

    #[test]
    fn strokes_take_their_size_and_opacity_from_the_settings() {
        let mut h = Harness::new();
        let brush = h.app.config.brush_mut(Pen::Pen);
        brush.size = 12.0;
        brush.opacity = 0.5;
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        let stroke = first_stroke(&h.app);
        assert_eq!(stroke.width, Some(12.0));
        assert!((stroke.color.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn each_brush_keeps_its_own_settings() {
        let mut h = Harness::new();
        h.app.config.brush_mut(Pen::Marker).size = 40.0;
        h.app.select_pen(Pen::Marker);
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        assert_eq!(first_stroke(&h.app).pen, Pen::Marker);
        assert_eq!(first_stroke(&h.app).width, Some(40.0));
        assert!(
            (h.app.config.brush(Pen::Pen).size - 3.0).abs() < 1e-6,
            "the pen is untouched"
        );
    }

    #[test]
    fn the_pen_and_colors_chosen_are_kept_in_the_settings() {
        let mut app = InkApp::blank(Options::default());
        app.select_pen(Pen::Marker);
        app.set_color_rgb([10, 20, 30]);
        app.remember_color();
        assert_eq!(app.config.colors.last_brush, "marker");
        assert_eq!(app.config.colors.current, [10, 20, 30]);
        assert_eq!(app.config.colors.recent.first(), Some(&[10, 20, 30]));
        assert!(app.config_dirty, "the change is waiting to be saved");
        assert_eq!(
            app.tool.color.to_srgb(),
            [10, 20, 30],
            "the picker keeps the exact color"
        );
    }

    #[test]
    fn every_panel_and_settings_section_can_be_drawn() {
        let mut h = Harness::new();
        for left_handed in [false, true] {
            h.app.config.interface.left_handed = left_handed;
            h.app.ui.toggle_case();
            h.frame(Vec::new());
            h.app.ui.toggle_color();
            h.frame(Vec::new());
            h.app.ui.toggle_dial();
            h.frame(Vec::new());
            h.app.ui.toggle_sheet();
            h.frame(Vec::new());
            h.app.ui.close_popovers();
            for section in Section::ALL {
                h.app.open_settings(section);
                h.frame(Vec::new());
                h.frame(Vec::new());
                assert_eq!(h.app.ui.settings, Some(section));
            }
            h.app.ui.settings = None;
            h.frame(Vec::new());
        }
    }

    #[test]
    fn the_keyboard_opens_the_settings_and_the_brushes_panel() {
        let mut h = Harness::new();
        let press = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        h.frame(vec![press(Key::Comma, Modifiers::COMMAND)]);
        assert!(h.app.ui.settings.is_some());
        h.frame(vec![press(Key::Comma, Modifiers::COMMAND)]);
        assert!(h.app.ui.settings.is_none());
        h.frame(vec![press(Key::B, Modifiers::NONE)]);
        assert!(h.app.ui.case_open);
        h.frame(vec![press(Key::Escape, Modifiers::NONE)]);
        assert!(!h.app.ui.case_open);
        h.frame(vec![press(Key::E, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Erase);
        h.frame(vec![press(Key::E, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Draw);
    }

    /// The places the panels over the paper took this frame, in a fixed order so frames can be compared.
    fn panels(h: &Harness) -> Vec<Rect> {
        let mut all = h.app.chrome.clone();
        all.sort_by(|a, b| {
            (a.min.x, a.min.y, a.max.x, a.max.y)
                .partial_cmp(&(b.min.x, b.min.y, b.max.x, b.max.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all
    }

    /// The largest amount any edge of any panel differs by between two frames, or `None` if the panels are not the
    /// same set.
    fn drift(a: &[Rect], b: &[Rect]) -> Option<f32> {
        (a.len() == b.len()).then(|| {
            a.iter()
                .zip(b)
                .flat_map(|(x, y)| {
                    [
                        (x.min.x - y.min.x).abs(),
                        (x.min.y - y.min.y).abs(),
                        (x.max.x - y.max.x).abs(),
                        (x.max.y - y.max.y).abs(),
                    ]
                })
                .fold(0.0, f32::max)
        })
    }

    /// Runs `frames` frames with nothing happening.
    fn settle(h: &mut Harness, frames: usize) {
        for _ in 0..frames {
            h.frame(Vec::new());
        }
    }

    /// Everything that can be put on the screen over the paper, one at a time.
    fn open_one(h: &mut Harness, which: usize) {
        h.app.ui.close_popovers();
        h.app.ui.layers_open = which == 0;
        match which {
            1 => h.app.ui.case_open = true,
            2 => h.app.ui.dial_open = true,
            3 => h.app.ui.color_open = true,
            4 => h.app.ui.sheet_open = true,
            5 => h.app.ui.precision_open = true,
            _ => {}
        }
    }

    #[test]
    fn every_panel_stays_inside_the_window_and_holds_perfectly_still_at_every_size() {
        let mut trouble = Vec::new();
        for (width, height) in [
            (1920.0, 1080.0),
            (1280.0, 800.0),
            (1024.0, 600.0),
            (800.0, 1280.0),
            (600.0, 480.0),
        ] {
            for which in 0..6 {
                for left_handed in [false, true] {
                    let mut h = Harness::sized(width, height);
                    h.app.config.interface.left_handed = left_handed;
                    draw_line(&mut h);
                    open_one(&mut h, which);
                    settle(&mut h, 45);
                    let window =
                        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height)).expand(1.0);
                    let before = panels(&h);
                    for rect in &before {
                        // The rail's reach is allowed past the edge: a pen is drawn lying partly off screen.
                        let rail = rect.width() < 120.0 && rect.height() > 200.0;
                        if !window.contains_rect(*rect) && !rail {
                            trouble.push(format!(
                                "{width}x{height} left_handed={left_handed} panel {which}: {rect:?} is out of the window"
                            ));
                        }
                    }
                    settle(&mut h, 6);
                    let moved = drift(&before, &panels(&h));
                    if moved.is_none_or(|d| d > 0.01) {
                        trouble.push(format!(
                            "{width}x{height} left_handed={left_handed} panel {which} moves with nothing happening: {moved:?}"
                        ));
                    }
                }
            }
        }
        assert!(
            trouble.is_empty(),
            "{} problems:\n{}",
            trouble.len(),
            trouble.join("\n")
        );
    }

    #[test]
    fn a_panel_in_a_tall_window_shows_all_of_what_is_in_it() {
        // Content that is cut off to egui's default area size (350 tall) leaves the swatches of the color picker and
        // the paper of the menu out of sight, and the panel looks complete.
        for (which, least) in [(3, 470.0), (4, 560.0), (5, 330.0)] {
            let mut h = Harness::sized(1280.0, 900.0);
            open_one(&mut h, which);
            settle(&mut h, 30);
            let tallest = panels(&h).iter().map(Rect::height).fold(0.0, f32::max);
            assert!(
                tallest >= least,
                "panel {which} is only {tallest} tall, it is cut off"
            );
        }
    }

    #[test]
    fn a_panel_opening_does_not_jump_up_or_down_or_change_size_on_the_way() {
        // The slide in is sideways for the flyouts; what they must not do is lurch up or down, or grow, as they
        // open (as when their height is guessed first and corrected a frame later).
        let mut trouble = Vec::new();
        for (width, height) in [(1280.0, 800.0), (1024.0, 600.0), (1920.0, 1080.0)] {
            for which in [1, 2, 3] {
                for (anchor_y, tool) in [(60.0, false), (420.0, false), (700.0, true)] {
                    let mut h = Harness::sized(width, height);
                    h.app.ui.anchors.pen = anchor_y;
                    h.app.ui.anchors.case = anchor_y;
                    h.app.ui.anchors.color = anchor_y;
                    if tool {
                        h.app.tool.mode = Mode::Nudge;
                    }
                    settle(&mut h, 5);
                    open_one(&mut h, which);
                    let mut frames = Vec::new();
                    for _ in 0..40 {
                        h.frame(Vec::new());
                        // The tall panel that is neither the rail nor the tab bar: the one that is opening.
                        let opening = panels(&h)
                            .into_iter()
                            .filter(|r| (200.0..700.0).contains(&r.width()) && r.height() > 150.0)
                            .max_by(|a, b| a.area().total_cmp(&b.area()));
                        frames.push(opening);
                    }
                    let first = frames.iter().position(Option::is_some);
                    if first.is_none_or(|i| i > 6) {
                        trouble.push(format!(
                            "{width}x{height} panel {which} y={anchor_y}: appeared at frame {first:?}"
                        ));
                    }
                    let Some(last) = frames.last().copied().flatten() else {
                        trouble.push(format!(
                            "{width}x{height} panel {which} y={anchor_y}: nothing opened"
                        ));
                        continue;
                    };
                    // A flyout is not shown (and not counted as a panel) while it finds out how tall it is.
                    for (i, rect) in frames.iter().enumerate() {
                        let Some(rect) = rect else { continue };
                        let (dy, dh) = (
                            (rect.min.y - last.min.y).abs(),
                            (rect.height() - last.height()).abs(),
                        );
                        if dy > 1.0 || dh > 1.0 {
                            trouble.push(format!(
                                "{width}x{height} panel {which} y={anchor_y}: frame {i} is {dy:.1} off in y and {dh:.1} in height"
                            ));
                            break;
                        }
                    }
                }
            }
        }
        assert!(
            trouble.is_empty(),
            "{} problems:\n{}",
            trouble.len(),
            trouble[..trouble.len().min(12)].join("\n")
        );
    }

    #[test]
    fn a_pointer_resting_anywhere_draws_the_same_picture_frame_after_frame() {
        // A picture that keeps changing under a still pointer is a flicker: two things fighting over the hover, or
        // an animation that never comes to rest.
        let mut flicker = Vec::new();
        for which in 0..6 {
            let mut h = Harness::new();
            // The paper has thousands of lines and is not what is being looked at; without it a frame is quick to hash.
            h.app.config.canvas.pattern = crate::config::Pattern::Blank;
            draw_line(&mut h);
            open_one(&mut h, which);
            for y in (40..800).step_by(190) {
                for x in (40..1280).step_by(210) {
                    h.frame(vec![Event::PointerMoved(pos2(x as f32, y as f32))]);
                    settle(&mut h, 50);
                    let hashes: Vec<u64> = (0..3).map(|_| h.frame_hash(Vec::new())).collect();
                    if hashes.windows(2).any(|w| w[0] != w[1]) {
                        flicker.push(format!(
                            "panel {which}: still pointer at ({x},{y}) keeps changing the picture"
                        ));
                    }
                }
            }
        }
        assert!(
            flicker.is_empty(),
            "{} places:\n{}",
            flicker.len(),
            flicker[..flicker.len().min(12)].join("\n")
        );
    }

    /// What is on the screen in each scene the pointer is tried in: the rail alone, then with something open.
    fn open_scene(h: &mut Harness, scene: usize) {
        use crate::settings::Section;
        h.app.ui.close_popovers();
        h.app.ui.settings = None;
        match scene {
            1 => h.app.ui.sheet_open = true,
            2 => h.app.ui.layers_open = true,
            3 => h.app.ui.color_open = true,
            4 => h.app.ui.case_open = true,
            5 => h.app.open_settings(Section::Brushes),
            6 => h.app.open_settings(Section::Tools),
            7 => h.app.open_settings(Section::Workspace),
            8 => h.app.open_settings(Section::Input),
            9 => h.app.open_settings(Section::Canvas),
            10 => h.app.open_settings(Section::Interface),
            11 => h.app.ui.precision_open = true,
            12 => h.app.open_settings(Section::Precision),
            13 => {
                h.app
                    .view
                    .rotate_at(pos2(600.0, 400.0), 30.0_f32.to_radians());
                h.app.ui.turn_open = true;
            }
            14 => {
                // A drawn stroke, selected, with the numbers of the selection open under the bar.
                draw_through(h, &[(0.0, 0.0), (50.0, 30.0), (100.0, 0.0)]);
                h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
                h.frame(vec![key_event(Key::A, Modifiers::COMMAND)]);
                h.app.ui.size_fields.open = true;
            }
            _ => {}
        }
    }

    /// Rests the pointer at each place of the grid in turn, in `scene`, and says where something that was drawn
    /// without the pointer was not drawn (or drawn somewhere else, or another size) with it.
    fn hover_moved(scene: usize, xs: &[i32], ys: &[i32]) -> Vec<String> {
        let mut moved = Vec::new();
        let mut h = Harness::new();
        h.app.config.canvas.pattern = crate::config::Pattern::Blank;
        open_scene(&mut h, scene);
        h.frame(vec![Event::PointerMoved(pos2(-10.0, -10.0))]);
        settle(&mut h, 40);
        let rest = h.drawn(Vec::new());
        for &y in ys {
            for &x in xs {
                h.frame(vec![Event::PointerMoved(pos2(x as f32, y as f32))]);
                settle(&mut h, 14);
                let now = h.drawn(Vec::new());
                let gone: Vec<_> = rest.difference(&now).collect();
                if !gone.is_empty() {
                    let first = gone[0].map(|v| v as f32 / 4.0);
                    moved.push(format!(
                        "scene {scene} pointer at ({x},{y}): {} things moved or resized, e.g. {first:?}",
                        gone.len()
                    ));
                }
            }
        }
        moved
    }

    fn assert_nothing_moved(moved: &[String]) {
        assert!(
            moved.is_empty(),
            "{} places, the first few:\n{}",
            moved.len(),
            moved[..moved.len().min(25)].join("\n")
        );
    }

    #[test]
    fn hovering_the_rail_the_case_and_the_settings_never_moves_or_resizes_anything() {
        // What a pointer resting on a button may do is change its color or add to it, but not shift it, grow it or
        // slide a pen: things that move under a pointer make the hover look like a shake.
        let rail = [30, 57];
        let down: Vec<i32> = (160..700).step_by(29).collect();
        let mut moved = hover_moved(0, &rail, &down);
        moved.extend(hover_moved(
            4,
            &[150, 250, 350],
            &(170..650).step_by(60).collect::<Vec<_>>(),
        ));
        moved.extend(hover_moved(
            5,
            &(330..960).step_by(130).collect::<Vec<_>>(),
            &(90..700).step_by(100).collect::<Vec<_>>(),
        ));
        assert_nothing_moved(&moved);
    }

    #[test]
    #[ignore = "slow (two minutes): every scene on a fine grid; run with --ignored"]
    fn hovering_anywhere_in_any_scene_never_moves_or_resizes_anything() {
        let xs: Vec<i32> = (30..1280).step_by(83).collect();
        let ys: Vec<i32> = (26..800).step_by(67).collect();
        let moved: Vec<String> = (0..15)
            .flat_map(|scene| hover_moved(scene, &xs, &ys))
            .collect();
        assert_nothing_moved(&moved);
    }

    #[test]
    fn a_pointer_resting_on_the_rail_never_makes_a_pen_shake() {
        // A pen that slides out from under a pointer to meet it can slide back, as it is no longer under it: the
        // edges of the rail are where that happens (a fine step of 6 by 23 points over it found nothing).
        for left_handed in [false, true] {
            let mut h = Harness::new();
            h.app.config.interface.left_handed = left_handed;
            h.app.config.canvas.pattern = crate::config::Pattern::Blank;
            settle(&mut h, 30);
            let (from, to) = if left_handed {
                (1160.0, 1280.0)
            } else {
                (0.0, 120.0)
            };
            let mut shaking = Vec::new();
            for y in (150..720).step_by(57) {
                for x in (from as i32..to as i32).step_by(11) {
                    h.frame(vec![Event::PointerMoved(pos2(x as f32, y as f32))]);
                    settle(&mut h, 30);
                    let hashes: Vec<u64> = (0..3).map(|_| h.frame_hash(Vec::new())).collect();
                    if hashes.windows(2).any(|w| w[0] != w[1]) {
                        shaking.push((x, y));
                    }
                }
            }
            assert!(
                shaking.is_empty(),
                "left_handed={left_handed}: {} places keep changing the picture, the first few: {:?}",
                shaking.len(),
                &shaking[..shaking.len().min(8)]
            );
        }
    }

    #[test]
    fn hovering_anywhere_never_moves_or_resizes_a_panel() {
        for which in 0..6 {
            let mut h = Harness::new();
            h.app.config.canvas.pattern = crate::config::Pattern::Blank;
            draw_line(&mut h);
            open_one(&mut h, which);
            h.frame(vec![Event::PointerMoved(pos2(-50.0, -50.0))]);
            settle(&mut h, 45);
            let rest = panels(&h);
            let mut trouble = Vec::new();
            for y in (8..800).step_by(48) {
                for x in (8..1280).step_by(48) {
                    h.frame(vec![Event::PointerMoved(pos2(x as f32, y as f32))]);
                    h.frame(Vec::new());
                    let now = panels(&h);
                    // Hovering the rail pulls a pen out of it, which widens its reach, but never the panels.
                    if drift(&rest, &now).is_none_or(|d| d > 0.5) {
                        trouble.push((x, y, drift(&rest, &now)));
                    }
                }
            }
            h.frame(vec![Event::PointerGone]);
            assert!(
                trouble.is_empty(),
                "panel {which}: hovering moved the panels at {} places, the first few: {:?}",
                trouble.len(),
                &trouble[..trouble.len().min(6)]
            );
        }
    }

    #[test]
    fn clicking_anywhere_leaves_the_panels_inside_the_window_and_still() {
        let (width, height) = (1280.0, 800.0);
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height)).expand(1.0);
        let mut trouble = Vec::new();
        for which in 0..6 {
            for y in (30..800).step_by(250) {
                for x in (30..1280).step_by(250) {
                    let mut h = Harness::new();
                    h.app.config.canvas.pattern = crate::config::Pattern::Blank;
                    open_one(&mut h, which);
                    settle(&mut h, 20);
                    let at = pos2(x as f32, y as f32);
                    let button = |pressed| Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    };
                    h.frame(vec![Event::PointerMoved(at)]);
                    h.frame(vec![button(true)]);
                    h.frame(vec![button(false)]);
                    settle(&mut h, 40);
                    let before = panels(&h);
                    settle(&mut h, 5);
                    let after = panels(&h);
                    for rect in &before {
                        let rail = rect.width() < 120.0 && rect.height() > 200.0;
                        if !window.contains_rect(*rect) && !rail {
                            trouble.push(format!(
                                "panel {which} click ({x},{y}): {rect:?} is out of the window"
                            ));
                        }
                    }
                    if drift(&before, &after).is_none_or(|d| d > 0.01) {
                        trouble.push(format!("panel {which} click ({x},{y}): panels keep moving"));
                    }
                }
            }
        }
        assert!(
            trouble.is_empty(),
            "{} problems, the first few:\n{}",
            trouble.len(),
            trouble[..trouble.len().min(8)].join("\n")
        );
    }

    fn key_event(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// A horizontal stroke on screen from x = 400 to x = 600 at y = 300, which is also its place on the canvas
    /// (the view starts at zoom 1 with the canvas origin at the middle of the paper).
    fn draw_line(h: &mut Harness) {
        h.drag(&[
            (400.0, 300.0),
            (450.0, 300.0),
            (500.0, 300.0),
            (550.0, 300.0),
            (600.0, 300.0),
        ]);
    }

    /// The canvas height of the sample of `stroke` nearest to canvas x position `x`.
    fn y_near(stroke: &ink_core::Stroke, x: f32) -> f32 {
        stroke
            .samples
            .iter()
            .min_by(|a, b| (a.point.x - x).abs().total_cmp(&(b.point.x - x).abs()))
            .expect("samples")
            .point
            .y
    }

    /// The canvas x position of the middle of the screen, which is where screen x = 500 is.
    fn canvas_x(app: &InkApp, screen_x: f32) -> f32 {
        app.view.to_canvas(pos2(screen_x, 300.0)).x
    }

    #[test]
    fn looping_a_stroke_selects_it_and_dragging_the_box_moves_it_in_one_undoable_step() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Select);
        h.drag(&[
            (350.0, 250.0),
            (650.0, 250.0),
            (650.0, 350.0),
            (350.0, 350.0),
            (352.0, 252.0),
        ]);
        assert_eq!(h.app.selection, vec![before.id]);
        h.drag(&[(500.0, 300.0), (500.0, 340.0), (500.0, 380.0)]);
        let moved = first_stroke(&h.app);
        assert_eq!(
            moved.id, before.id,
            "a moved stroke is still the same stroke"
        );
        let dy = moved.samples[0].point.y - before.samples[0].point.y;
        assert!(
            (dy + 80.0).abs() < 0.5,
            "80 down on the screen is 80 less on the canvas: {dy}"
        );
        assert!(h.app.editor.undo());
        assert_eq!(first_stroke(&h.app), &before, "one undo puts it all back");
    }

    #[test]
    fn a_tap_selects_and_a_tap_on_bare_paper_deselects() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        h.drag(&[(500.0, 302.0), (500.0, 302.0)]);
        assert_eq!(h.app.selection.len(), 1);
        h.drag(&[(500.0, 600.0), (500.0, 600.0)]);
        assert_eq!(h.app.selection, Vec::<Uuid>::new());
    }

    #[test]
    fn the_slice_tool_cuts_the_stroke_it_crosses_and_undo_joins_it_again() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::K, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Slice);
        h.drag(&[(475.0, 250.0), (475.0, 300.0), (475.0, 350.0)]);
        assert_eq!(h.strokes(), 2);
        assert!(h.app.editor.undo());
        assert_eq!(h.strokes(), 1);
        assert_eq!(first_stroke(&h.app), &before);
    }

    #[test]
    fn a_blade_that_misses_leaves_the_drawing_alone() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let revision = h.app.editor.revision();
        h.frame(vec![key_event(Key::K, Modifiers::NONE)]);
        h.drag(&[(700.0, 250.0), (700.0, 300.0), (700.0, 350.0)]);
        assert_eq!(h.app.editor.revision(), revision);
    }

    #[test]
    fn pulling_with_the_nudge_tool_bends_the_stroke_where_it_is_held_and_undo_restores_it() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Nudge);
        h.app.config.tools.nudge_reach = 60.0;
        let x = canvas_x(&h.app, 500.0);
        h.drag(&[(500.0, 300.0), (500.0, 320.0), (500.0, 340.0)]);
        let bent = first_stroke(&h.app);
        assert_eq!(bent.id, before.id);
        let moved = y_near(bent, x) - y_near(&before, x);
        assert!(
            (moved + 40.0).abs() < 1.5,
            "the held point followed the pen down: {moved}"
        );
        assert!(
            (bent.samples[0].point.y - before.samples[0].point.y).abs() < 1e-3,
            "the far end stays where it was"
        );
        assert!(
            bent.samples.len() >= before.samples.len(),
            "extra points make the bend smooth"
        );
        assert!(h.app.editor.undo());
        assert_eq!(first_stroke(&h.app), &before);
    }

    #[test]
    fn pushing_with_the_nudge_tool_carries_the_stroke_along() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.app.config.tools.nudge_variant = crate::config::NudgeVariant::Push;
        h.app.config.tools.nudge_reach = 80.0;
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        let x = canvas_x(&h.app, 500.0);
        h.drag(&[(500.0, 302.0), (500.0, 330.0), (500.0, 360.0)]);
        let pushed = y_near(first_stroke(&h.app), x);
        let original = y_near(&before, x);
        assert!(
            pushed < original - 20.0,
            "it moved down: {original} to {pushed}"
        );
        assert!(h.app.editor.undo());
        assert_eq!(first_stroke(&h.app), &before);
    }

    #[test]
    fn escape_gives_up_a_drag_in_progress() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        let button = |pressed| Event::PointerButton {
            pos: pos2(500.0, 300.0),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        h.frame(vec![button(true)]);
        h.frame(vec![Event::PointerMoved(pos2(500.0, 360.0))]);
        let Some(crate::tools::Gesture::Nudge { shown, .. }) = &h.app.gesture else {
            panic!("a nudge is under way");
        };
        assert_ne!(&**shown, &before, "the stroke being shaped follows the pen");
        h.frame(vec![key_event(Key::Escape, Modifiers::NONE)]);
        assert_eq!(
            first_stroke(&h.app),
            &before,
            "and is dropped when cancelled"
        );
        assert!(h.app.gesture.is_none());
        assert!(h.app.editor.undo(), "the only step left is the line itself");
        assert_eq!(h.strokes(), 0);
    }

    #[test]
    fn nudging_leaves_the_drawing_alone_until_the_pen_is_lifted() {
        // Changing the drawing on every move makes the renderer redraw all of it on every move, which is what
        // made nudging slow.
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        let revision = h.app.editor.revision();
        let steps = h.app.editor.history_mark();
        let button = |y, pressed| Event::PointerButton {
            pos: pos2(500.0, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        h.frame(vec![button(300.0, true)]);
        for y in [310.0, 320.0, 335.0, 350.0] {
            h.frame(vec![Event::PointerMoved(pos2(500.0, y))]);
            assert_eq!(
                h.app.editor.revision(),
                revision,
                "nothing is redrawn at {y}"
            );
        }
        h.frame(vec![button(350.0, false)]);
        assert_ne!(
            h.app.editor.revision(),
            revision,
            "it is put in when let go"
        );
        assert_eq!(h.app.editor.history_mark(), steps + 1, "as one step");
    }

    /// Draws a line, selects it by looping it, and returns the stroke as it was.
    fn line_selected(h: &mut Harness) -> ink_core::Stroke {
        draw_line(h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        h.drag(&[
            (350.0, 250.0),
            (650.0, 250.0),
            (650.0, 350.0),
            (350.0, 350.0),
            (352.0, 252.0),
        ]);
        assert_eq!(h.app.selection, vec![before.id]);
        before
    }

    fn primary(y: f32, pressed: bool) -> Event {
        Event::PointerButton {
            pos: pos2(500.0, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    /// A harness with snapping on, over a square grid of 32 canvas units.
    fn snapping() -> Harness {
        let mut h = Harness::new();
        h.app.config.precision.snap = true;
        h.app.config.canvas.pattern = crate::config::Pattern::Grid;
        h.app.config.canvas.grid_size = 32.0;
        h
    }

    /// Where a canvas point is on the screen with the view as it is.
    fn at(h: &Harness, x: f32, y: f32) -> (f32, f32) {
        let p = h.app.view.to_screen(ink_core::Point::new(x, y));
        (p.x, p.y)
    }

    fn all_strokes(h: &Harness) -> Vec<ink_core::Stroke> {
        h.app
            .editor
            .document()
            .layers
            .iter()
            .flat_map(|l| l.strokes.clone())
            .collect()
    }

    #[test]
    fn with_snap_on_a_stroke_starts_and_ends_on_the_grid_and_the_rest_is_as_drawn() {
        let mut h = snapping();
        // Starts 3 and 2 off the crossing at (64, 96) and ends 4 and 3 off the one at (160, 192).
        let (start, mid, end) = (
            at(&h, 67.0, 94.0),
            at(&h, 100.0, 150.0),
            at(&h, 164.0, 189.0),
        );
        h.drag(&[start, mid, end]);
        let stroke = &all_strokes(&h)[0];
        let (first, last) = (
            stroke.samples.first().expect("samples").point,
            stroke.samples.last().expect("samples").point,
        );
        assert_eq!(first, ink_core::Point::new(64.0, 96.0));
        assert_eq!(last, ink_core::Point::new(160.0, 192.0));
        assert!(
            stroke
                .samples
                .iter()
                .skip(1)
                .take(stroke.samples.len() - 2)
                .all(|s| s.point != ink_core::Point::new(64.0, 96.0)),
            "only the two ends are moved"
        );
    }

    /// Snapping on, to nothing but what the test switches on, on blank paper.
    fn snapping_to_strokes() -> Harness {
        let mut h = guided();
        h.app.config.precision.snap = true;
        h.app.config.precision.snap_to_grid = false;
        h.app.config.precision.snap_to_key_points = false;
        h
    }

    /// The first and last point of the last stroke drawn.
    fn ends_of_last(h: &Harness) -> (ink_core::Point, ink_core::Point) {
        let stroke = all_strokes(h).pop().expect("a stroke");
        (
            stroke.samples.first().expect("samples").point,
            stroke.samples.last().expect("samples").point,
        )
    }

    #[test]
    fn a_stroke_ends_on_the_middle_of_another_when_snapping_to_middles_is_on() {
        let mut h = snapping_to_strokes();
        draw_through(&mut h, &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)]);
        // From 4 above the middle (100, 0) up and away.
        draw_through(&mut h, &[(103.0, 5.0), (110.0, 60.0), (120.0, 120.0)]);
        let (first, _) = ends_of_last(&h);
        assert!(
            (first.x - 103.0).abs() < 0.01 && (first.y - 5.0).abs() < 0.01,
            "off: {first:?}"
        );
        h.app.config.precision.snap_to_middles = true;
        draw_through(&mut h, &[(103.0, 5.0), (110.0, 60.0), (120.0, 120.0)]);
        let (first, _) = ends_of_last(&h);
        assert!(
            first.distance(ink_core::Point::new(100.0, 0.0)) < 0.5,
            "on: {first:?}"
        );
    }

    #[test]
    fn a_stroke_ends_on_the_crossing_of_two_others_when_snapping_to_crossings_is_on() {
        let mut h = snapping_to_strokes();
        draw_through(&mut h, &[(0.0, 0.0), (100.0, 100.0), (200.0, 200.0)]);
        draw_through(&mut h, &[(0.0, 200.0), (100.0, 100.0), (200.0, 0.0)]);
        // The two cross at (100, 100). A new stroke ends 5 from it.
        let to = [(300.0, 0.0), (200.0, 60.0), (104.0, 103.0)];
        draw_through(&mut h, &to);
        let (_, last) = ends_of_last(&h);
        assert!((last.x - 104.0).abs() < 0.01, "off: {last:?}");
        h.app.config.precision.snap_to_crossings = true;
        draw_through(&mut h, &to);
        let (_, last) = ends_of_last(&h);
        assert!(
            last.distance(ink_core::Point::new(100.0, 100.0)) < 0.5,
            "on: {last:?}"
        );
    }

    #[test]
    fn the_triangle_and_isometric_papers_have_nodes_to_snap_to() {
        use crate::config::Pattern;
        for (pattern, near, node) in [
            // Lines 32 apart at 0, 60 and 120 degrees: a node at the origin and one 36.95 along.
            (Pattern::Triangle, (2.0, -3.0), (0.0, 0.0)),
            (Pattern::Triangle, (39.0, 2.0), (36.95, 0.0)),
            // Upright lines 32 apart, nodes 36.95 up the first one.
            (Pattern::Isometric, (-2.0, 38.0), (0.0, 36.95)),
        ] {
            let mut h = Harness::new();
            h.app.config.precision.snap = true;
            h.app.config.precision.snap_to_key_points = false;
            h.app.config.canvas.pattern = pattern;
            h.app.config.canvas.grid_size = 32.0;
            draw_through(
                &mut h,
                &[
                    near,
                    (near.0 + 80.0, near.1 + 60.0),
                    (near.0 + 150.0, near.1 + 90.0),
                ],
            );
            let (first, _) = ends_of_last(&h);
            assert!(
                first.distance(ink_core::Point::new(node.0, node.1)) < 0.1,
                "{pattern:?}: {first:?} should be {node:?}"
            );
            // Away from every node it stays where it was drawn.
            draw_through(&mut h, &[(18.0, 9.0), (60.0, 60.0), (90.0, 80.0)]);
            let (first, _) = ends_of_last(&h);
            assert!(
                (first.x - 18.0).abs() < 0.01 && (first.y - 9.0).abs() < 0.01,
                "{pattern:?}: {first:?}"
            );
        }
    }

    #[test]
    fn the_new_snap_switches_are_saved_and_off_to_begin_with() {
        let defaults = crate::config::PrecisionConfig::default();
        assert!(!defaults.snap_to_middles && !defaults.snap_to_crossings);
        let mut changed = defaults.clone();
        changed.snap_to_middles = true;
        changed.snap_to_crossings = true;
        let text = serde_json::to_string(&changed).expect("serializes");
        let back: crate::config::PrecisionConfig = serde_json::from_str(&text).expect("reads back");
        assert_eq!(back, changed);
        let old: crate::config::PrecisionConfig =
            serde_json::from_str(r#"{"snap": true}"#).expect("an old file");
        assert!(
            old.snap && !old.snap_to_middles,
            "settings saved before have the new ones off"
        );
    }

    #[test]
    fn snapping_is_off_until_it_is_switched_on_and_far_points_do_not_snap() {
        let mut h = snapping();
        h.app.config.precision.snap = false;
        let (start, end) = (at(&h, 67.0, 94.0), at(&h, 164.0, 189.0));
        h.drag(&[start, end]);
        let stroke = &all_strokes(&h)[0];
        let first = stroke.samples.first().expect("samples").point;
        assert!(
            (first.x - 67.0).abs() < 0.01 && (first.y - 94.0).abs() < 0.01,
            "{first:?}"
        );
        h.app.config.precision.snap = true;
        // Right in the middle of a square: 16 from every crossing, more than the reach of 14.
        let (start, end) = (at(&h, 208.0, 80.0), at(&h, 300.0, 80.0));
        h.drag(&[start, end]);
        let second = &all_strokes(&h)[1];
        let first = second.samples.first().expect("samples").point;
        assert!(
            (first.x - 208.0).abs() < 0.01,
            "too far from the grid to snap: {first:?}"
        );
    }

    #[test]
    fn a_stroke_can_start_exactly_on_the_end_of_another() {
        let mut h = snapping();
        h.app.config.canvas.pattern = crate::config::Pattern::Blank;
        h.drag(&[at(&h, 10.0, 10.0), at(&h, 110.0, 53.0)]);
        let end = all_strokes(&h)[0].samples.last().expect("samples").point;
        h.drag(&[at(&h, 114.0, 56.0), at(&h, 200.0, 200.0)]);
        let second = &all_strokes(&h)[1];
        assert_eq!(
            second.samples.first().expect("samples").point,
            end,
            "it starts where the first one ended"
        );
    }

    #[test]
    fn what_is_shown_while_the_pen_is_down_is_exactly_what_is_kept() {
        let mut h = snapping();
        let (start, end) = (at(&h, 67.0, 94.0), at(&h, 164.0, 189.0));
        let button = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(start.0, start.1))]);
        h.frame(vec![button(start, true)]);
        h.frame(vec![Event::PointerMoved(pos2(
            f32::midpoint(start.0, end.0),
            f32::midpoint(start.1, end.1),
        ))]);
        h.frame(vec![Event::PointerMoved(pos2(end.0, end.1))]);
        let shown = h.app.builder.as_ref().expect("the pen is down").snapshot();
        h.frame(vec![button(end, false)]);
        assert_eq!(
            &all_strokes(&h)[0],
            &shown,
            "the stroke does not change when the pen lifts"
        );
    }

    #[test]
    fn the_end_goes_back_to_the_pen_when_it_moves_away_from_what_it_snapped_to() {
        let mut h = snapping();
        let button = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let start = at(&h, 10.0, 10.0);
        h.frame(vec![Event::PointerMoved(pos2(start.0, start.1))]);
        h.frame(vec![button(start, true)]);
        let near = at(&h, 164.0, 189.0);
        h.frame(vec![Event::PointerMoved(pos2(near.0, near.1))]);
        assert!(
            h.app
                .builder
                .as_ref()
                .and_then(ink_core::StrokeBuilder::end)
                .is_some(),
            "snapped"
        );
        let away = at(&h, 208.0, 80.0);
        h.frame(vec![Event::PointerMoved(pos2(away.0, away.1))]);
        assert!(
            h.app
                .builder
                .as_ref()
                .and_then(ink_core::StrokeBuilder::end)
                .is_none(),
            "free again"
        );
        h.frame(vec![button(away, false)]);
    }

    fn guided() -> Harness {
        let mut h = Harness::new();
        h.app.config.canvas.pattern = crate::config::Pattern::Blank;
        h
    }

    /// Draws through canvas points as a drag.
    fn draw_through(h: &mut Harness, points: &[(f32, f32)]) {
        let screen: Vec<(f32, f32)> = points.iter().map(|&(x, y)| at(h, x, y)).collect();
        h.drag(&screen);
    }

    fn pt(x: f32, y: f32) -> ink_core::Point {
        ink_core::Point::new(x, y)
    }

    #[test]
    fn a_guide_goes_on_the_paper_and_off_again_from_the_key_and_the_pen_is_chosen() {
        use ink_core::guide::Kind;
        let mut h = guided();
        h.app.tool.mode = Mode::Erase;
        h.frame(vec![Event::Key {
            key: Key::Num1,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::ALT,
        }]);
        assert!(
            h.app.guides.has(Kind::Line),
            "Alt+1 puts a line guide on the paper"
        );
        assert_eq!(h.app.tool.mode, Mode::Draw, "guides are for the pen");
        h.app.toggle_guide(Kind::Circle);
        assert_eq!(h.app.guides.list.len(), 2, "one of each kind at most");
        h.app.toggle_guide(Kind::Circle);
        h.app.toggle_guide(Kind::Line);
        assert_eq!(h.app.guides.list.len(), 0);
    }

    #[test]
    fn a_stroke_drawn_along_a_line_guide_follows_it_exactly_and_stops_at_its_end() {
        let mut h = guided();
        h.app.guides.list = vec![ink_core::guide::Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(200.0, 0.0),
        }];
        // Starts 3 above the line and wanders 8 either side of it, past the end.
        draw_through(
            &mut h,
            &[
                (20.0, 3.0),
                (60.0, -8.0),
                (100.0, 6.0),
                (150.0, -4.0),
                (240.0, 5.0),
            ],
        );
        let stroke = &all_strokes(&h)[0];
        assert!(
            stroke.samples.iter().all(|s| s.point.y.abs() < 1e-3),
            "every point is on the line"
        );
        assert!((stroke.samples[0].point.x - 20.0).abs() < 0.5);
        let last = stroke.samples.last().expect("samples").point;
        assert!(
            (last.x - 200.0).abs() < 1e-3,
            "held at the end of the guide: {last:?}"
        );
        assert_eq!(h.app.guides.lock, None, "and let go afterwards");
    }

    #[test]
    fn a_stroke_that_starts_away_from_every_guide_is_free() {
        let mut h = guided();
        h.app.guides.list = vec![ink_core::guide::Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(200.0, 0.0),
        }];
        draw_through(&mut h, &[(20.0, 90.0), (80.0, 70.0), (140.0, 95.0)]);
        let stroke = &all_strokes(&h)[0];
        assert!(
            stroke.samples.iter().any(|s| s.point.y > 60.0),
            "it was not pulled to the guide"
        );
    }

    #[test]
    fn a_circle_guide_makes_a_circle_of_whatever_is_drawn_around_it() {
        let mut h = guided();
        let guide = ink_core::guide::Guide::Circle {
            center: pt(50.0, 20.0),
            rx: 80.0,
            ry: 80.0,
        };
        h.app.guides.list = vec![guide];
        // A lopsided loop that starts on the ring and wobbles in and out.
        // It starts on the ring away from the handles (which are at the right, the top and the middle).
        let around: Vec<(f32, f32)> = (0..=24)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_6 + std::f32::consts::TAU * i as f32 / 24.0;
                let r = if i == 0 {
                    80.0
                } else if i % 2 == 0 {
                    70.0
                } else {
                    105.0
                };
                (50.0 + r * a.cos(), 20.0 + r * a.sin())
            })
            .collect();
        draw_through(&mut h, &around);
        let stroke = &all_strokes(&h)[0];
        assert!(stroke.samples.len() > 20);
        assert!(
            stroke
                .samples
                .iter()
                .all(|s| guide.distance(s.point) < 0.05),
            "every point is on the ring"
        );
    }

    #[test]
    fn a_square_guide_keeps_its_corners_sharp_because_a_guided_stroke_is_not_smoothed() {
        let mut h = guided();
        let guide = ink_core::guide::Guide::Square {
            min: pt(0.0, 0.0),
            max: pt(100.0, 100.0),
        };
        h.app.guides.list = vec![guide];
        draw_through(
            &mut h,
            // From the middle of the bottom side, clear of the handles, once round.
            &[
                (50.0, 3.0),
                (98.0, 1.0),
                (97.0, 50.0),
                (99.0, 99.0),
                (50.0, 98.0),
                (2.0, 97.0),
                (1.0, 50.0),
                (2.0, 2.0),
                (30.0, 3.0),
            ],
        );
        let stroke = &all_strokes(&h)[0];
        assert!(
            stroke
                .samples
                .iter()
                .all(|s| guide.distance(s.point) < 0.05)
        );
        let near_corner =
            |c: ink_core::Point| stroke.samples.iter().any(|s| s.point.distance(c) < 0.5);
        for corner in [
            pt(100.0, 0.0),
            pt(100.0, 100.0),
            pt(0.0, 100.0),
            pt(0.0, 0.0),
        ] {
            assert!(
                near_corner(corner),
                "the pen cut {corner:?}, but the stroke does not"
            );
        }
    }

    #[test]
    fn what_is_shown_along_a_guide_is_what_is_kept() {
        let mut h = guided();
        h.app.guides.list = vec![ink_core::guide::Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(200.0, 0.0),
        }];
        let button = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let (start, end) = (at(&h, 40.0, 4.0), at(&h, 120.0, -9.0));
        h.frame(vec![Event::PointerMoved(pos2(start.0, start.1))]);
        h.frame(vec![button(start, true)]);
        h.frame(vec![Event::PointerMoved(pos2(end.0, end.1))]);
        let shown = h.app.builder.as_ref().expect("the pen is down").snapshot();
        h.frame(vec![button(end, false)]);
        assert_eq!(&all_strokes(&h)[0], &shown);
    }

    #[test]
    fn dragging_a_handle_reshapes_the_guide_and_draws_nothing() {
        let mut h = guided();
        h.app.guides.list = vec![ink_core::guide::Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(200.0, 0.0),
        }];
        // The end handle is at (200, 0).
        draw_through(&mut h, &[(200.0, 0.0), (200.0, 50.0), (200.0, 100.0)]);
        assert_eq!(h.strokes(), 0, "grabbing a handle is not drawing");
        let ink_core::guide::Guide::Line { a, b } = h.app.guides.list[0] else {
            panic!("still a line");
        };
        assert_eq!(a, pt(0.0, 0.0), "the other end stays");
        assert!(b.distance(pt(200.0, 100.0)) < 0.5, "{b:?}");
        assert!(h.app.guides.drag.is_none());
    }

    #[test]
    fn a_handle_that_is_dragged_lands_on_the_grid_when_snapping_is_on() {
        let mut h = snapping();
        h.app.guides.list = vec![ink_core::guide::Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(192.0, 0.0),
        }];
        // Dragged to (203, 66): within reach of the crossing at (192, 64).
        draw_through(&mut h, &[(192.0, 0.0), (198.0, 30.0), (203.0, 66.0)]);
        let ink_core::guide::Guide::Line { b, .. } = h.app.guides.list[0] else {
            panic!("still a line");
        };
        assert_eq!(b, pt(192.0, 64.0));
    }

    #[test]
    fn a_guide_stays_with_its_drawing_when_the_tab_changes() {
        let mut h = guided();
        h.app.toggle_guide(ink_core::guide::Kind::Square);
        assert_eq!(h.app.guides.list.len(), 1);
        let ctx = h.ctx.clone();
        h.app.new_tab(&ctx);
        h.frame(Vec::new());
        assert_eq!(h.app.guides.list.len(), 0, "a new drawing has none");
        h.app.switch_tab(&ctx, 0);
        h.frame(Vec::new());
        assert_eq!(
            h.app.guides.list.len(),
            1,
            "and the first one still has its own"
        );
    }

    #[test]
    fn guides_are_put_away_for_the_tools_that_do_not_use_them() {
        let mut h = guided();
        h.app.toggle_guide(ink_core::guide::Kind::Line);
        let before = h.app.editor.revision();
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        // With the select tool a press on the guide's handle starts a selection loop, not a guide drag.
        let handle = h.app.guides.list[0].handles()[0].point;
        let screen = at(&h, handle.x, handle.y);
        h.drag(&[screen, (screen.0 + 30.0, screen.1 + 30.0)]);
        assert_eq!(h.app.editor.revision(), before);
        assert!(h.app.guides.drag.is_none());
    }

    /// Holds the pen still for `seconds` with it down where it is.
    fn hold_still(h: &mut Harness, seconds: f32) {
        for _ in 0..(seconds * 60.0).ceil() as usize {
            h.frame(Vec::new());
        }
    }

    /// A wobbly circle of radius 80 about (0, 0) on the paper, the pen going round and left down at its end.
    fn wobbly_circle(h: &mut Harness) -> (f32, f32) {
        let around: Vec<(f32, f32)> = (0..=40)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 40.0 + 0.4;
                let r = 80.0 + if i % 3 == 0 { 2.5 } else { -2.0 };
                at(h, r * a.cos(), r * a.sin())
            })
            .collect();
        let button = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(around[0].0, around[0].1))]);
        h.frame(vec![button(around[0], true)]);
        for &(x, y) in &around[1..] {
            h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        }
        *around.last().expect("points")
    }

    fn press_with_control(key: Key, shift: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Modifiers::NONE
            },
        }
    }

    fn press_with(key: Key, ctrl: bool, alt: bool, shift: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl,
                command: ctrl,
                alt,
                shift,
                ..Modifiers::NONE
            },
        }
    }

    /// Three lines on layer 0, each 100 long, at canvas heights 0, 60 and 120, with the select tool on.
    fn three_lines() -> Harness {
        let mut h = guided();
        for y in [0.0, 60.0, 120.0] {
            draw_through(&mut h, &[(0.0, y), (50.0, y), (100.0, y)]);
        }
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        h
    }

    /// Taps the line at canvas height `y`, with Shift held if `add`.
    fn tap_line(h: &mut Harness, y: f32, add: bool) {
        let (x, y) = at(h, 50.0, y);
        let modifiers = Modifiers {
            shift: add,
            ..Modifiers::NONE
        };
        let button = |pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        h.frame(vec![button(true)]);
        h.frame(vec![button(false)]);
    }

    #[test]
    fn a_group_is_picked_up_whole_by_a_tap_and_goes_out_whole_with_a_shift_tap() {
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        assert_eq!(h.app.selection.len(), 3);
        h.frame(vec![press_with(Key::G, true, false, false)]);
        assert_eq!(h.app.editor.marks().groups().len(), 1);
        h.app.selection.clear();
        tap_line(&mut h, 60.0, false);
        assert_eq!(h.app.selection.len(), 3, "one tap picked up the group");
        tap_line(&mut h, 0.0, true);
        assert_eq!(
            h.app.selection.len(),
            0,
            "and a shift tap took the whole group out again"
        );
        tap_line(&mut h, 120.0, true);
        assert_eq!(h.app.selection.len(), 3, "or put it in");
        h.frame(vec![press_with(Key::G, true, false, true)]);
        assert!(
            h.app.editor.marks().groups().is_empty(),
            "Ctrl+Shift+G breaks it up"
        );
        h.app.selection.clear();
        tap_line(&mut h, 60.0, false);
        assert_eq!(h.app.selection.len(), 1, "now they are separate");
    }

    #[test]
    fn a_marquee_that_takes_one_of_a_group_takes_the_group() {
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        h.frame(vec![press_with(Key::G, true, false, false)]);
        h.app.selection.clear();
        // A loop around the top line only.
        let corners = [
            (-20.0, 140.0),
            (120.0, 140.0),
            (120.0, 105.0),
            (-20.0, 105.0),
            (-18.0, 138.0),
        ];
        let screen: Vec<(f32, f32)> = corners.iter().map(|&(x, y)| at(&h, x, y)).collect();
        h.drag(&screen);
        assert_eq!(h.app.selection.len(), 3, "the other two come with it");
    }

    #[test]
    fn a_locked_stroke_cannot_be_selected_erased_nudged_or_sliced_until_it_is_unlocked() {
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        h.frame(vec![press_with(Key::L, true, false, true)]);
        assert_eq!(h.app.editor.marks().locked().len(), 3);
        assert_eq!(h.app.selection.len(), 0);
        tap_line(&mut h, 60.0, false);
        assert_eq!(h.app.selection.len(), 0, "a tap cannot pick it up");
        h.frame(vec![press_with(Key::A, true, false, false)]);
        assert!(h.app.selection.is_empty(), "nor select all");
        // Erasing, either kind, goes over it and leaves it.
        for kind in [
            crate::config::EraserKind::Hard,
            crate::config::EraserKind::Soft,
        ] {
            h.app.config.tools.eraser_kind = kind;
            h.app.tool.mode = Mode::Erase;
            let (a, b) = (at(&h, 50.0, 60.0), at(&h, 52.0, 60.0));
            h.drag(&[a, b]);
            assert_eq!(h.strokes(), 3, "{kind:?} erased a locked stroke");
        }
        // Slicing across all three.
        h.frame(vec![key_event(Key::K, Modifiers::NONE)]);
        let (a, b) = (at(&h, 30.0, 150.0), at(&h, 30.0, -30.0));
        h.drag(&[a, b]);
        assert_eq!(h.strokes(), 3, "the blade went through locked strokes");
        // Nudging has nothing to take hold of.
        let place = ink_core::Point::new(50.0, 60.0);
        assert_eq!(
            crate::tools::stroke_to_nudge(&h.app.editor, place, 9.0),
            None
        );
        // Unlocked, they are ordinary strokes again.
        h.frame(vec![press_with(Key::L, true, true, false)]);
        assert_eq!(h.app.editor.marks().locked().len(), 0);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        tap_line(&mut h, 60.0, false);
        assert_eq!(h.app.selection.len(), 1);
        assert!(crate::tools::stroke_to_nudge(&h.app.editor, place, 9.0).is_some());
    }

    #[test]
    fn the_soft_eraser_takes_only_what_it_touches_in_one_step_and_the_hard_one_the_whole_stroke() {
        let mut h = guided();
        draw_through(&mut h, &[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0)]);
        let steps = h.app.editor.history_mark();
        h.app.config.tools.eraser_radius = 12.0;
        h.app.config.tools.eraser_kind = crate::config::EraserKind::Soft;
        h.app.tool.mode = Mode::Erase;
        // Over the middle, in three moves.
        h.drag(&[at(&h, 90.0, 0.0), at(&h, 100.0, 0.0), at(&h, 110.0, 0.0)]);
        assert_eq!(
            h.strokes(),
            2,
            "the stroke was cut in two with a gap where the eraser went"
        );
        let strokes = all_strokes(&h);
        let gap_from = strokes[0].samples.last().expect("samples").point.x;
        let gap_to = strokes[1].samples.first().expect("samples").point.x;
        assert!(
            gap_to - gap_from > 24.0 && gap_from < 90.0 && gap_to > 110.0,
            "{gap_from} {gap_to}"
        );
        assert_eq!(
            h.app.editor.history_mark(),
            steps + 1,
            "one sweep is one step"
        );
        assert!(h.app.editor.undo());
        assert_eq!(h.strokes(), 1, "undone whole");
        // The hard one takes the whole stroke for the same touch.
        h.app.config.tools.eraser_kind = crate::config::EraserKind::Hard;
        h.drag(&[at(&h, 90.0, 0.0), at(&h, 100.0, 0.0)]);
        assert_eq!(h.strokes(), 0);
    }

    #[test]
    fn the_eraser_keys_choose_the_kind_and_the_same_key_again_puts_it_away() {
        let mut h = guided();
        h.frame(vec![press_with(Key::E, false, false, true)]);
        assert_eq!(h.app.tool.mode, Mode::Erase);
        assert_eq!(
            h.app.config.tools.eraser_kind,
            crate::config::EraserKind::Soft
        );
        h.frame(vec![press_with(Key::E, false, true, false)]);
        assert_eq!(
            h.app.tool.mode,
            Mode::Erase,
            "switched to the hard one, not put away"
        );
        assert_eq!(
            h.app.config.tools.eraser_kind,
            crate::config::EraserKind::Hard
        );
        h.frame(vec![press_with(Key::E, false, true, false)]);
        assert_ne!(
            h.app.tool.mode,
            Mode::Erase,
            "the same one again puts it away"
        );
    }

    #[test]
    fn the_layer_keys_move_the_selection_to_the_layer_above_or_below() {
        let mut h = guided();
        h.app.new_layer();
        h.app.active_layer = 0;
        draw_through(&mut h, &[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)]);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        h.frame(vec![press_with(Key::A, true, false, false)]);
        let id = h.app.selection[0];
        let layer = |h: &Harness| h.app.editor.find_stroke(id).map(|p| p.0);
        assert_eq!(layer(&h), Some(0));
        h.frame(vec![press_with(Key::ArrowUp, true, true, false)]);
        assert_eq!(layer(&h), Some(1), "up");
        h.frame(vec![press_with(Key::ArrowDown, true, true, false)]);
        assert_eq!(layer(&h), Some(0), "down");
        assert_eq!(h.app.selection, vec![id], "and it stays selected");
    }

    #[test]
    fn the_numbers_of_a_selection_move_stretch_and_turn_it_and_a_whole_drag_is_one_undo() {
        use crate::selection_bar::{Change, change};
        use crate::size_fields::Field;
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        let steps = h.app.editor.history_mark();
        let bounds = |h: &Harness| {
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("bounds")
        };
        let before = bounds(&h);
        // A drag of the width box: many frames, each a little wider.
        for width in [110.0, 130.0, 160.0, 200.0] {
            assert!(change(&mut h.app, Change::Field(Field::Width, width)));
        }
        let wide = bounds(&h);
        assert!((wide[2] - wide[0] - 200.0).abs() < 1e-2, "{wide:?}");
        assert!(
            (wide[0] - before[0]).abs() < 1e-3 && (wide[3] - before[3]).abs() < 1e-3,
            "the top left stays"
        );
        // The box is the strokes with half their width around them, which stretches along with them.
        let (across, down) = (
            (wide[2] - wide[0]) / (before[2] - before[0]),
            (wide[3] - wide[1]) / (before[3] - before[1]),
        );
        assert!(
            (across - down).abs() < 1e-3,
            "kept its shape: {across} wider, {down} higher"
        );
        assert_eq!(
            h.app.editor.history_mark(),
            steps + 1,
            "the drag is one step"
        );
        // Typing a position afterwards is another.
        h.app.ui.size_fields.finish();
        assert!(change(&mut h.app, Change::Field(Field::X, 30.0)));
        assert!((bounds(&h)[0] - 30.0).abs() < 1e-3);
        assert_eq!(h.app.editor.history_mark(), steps + 2);
        assert!(h.app.editor.undo());
        assert!(
            (bounds(&h)[0] - before[0]).abs() < 1e-3,
            "undone to where it was after the stretch"
        );
        assert!(h.app.editor.undo());
        assert_eq!(bounds(&h), before, "the whole stretch undone at once");
    }

    #[test]
    fn turning_by_a_number_goes_around_one_point_however_many_frames_it_takes() {
        use crate::selection_bar::{Change, change};
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        let middle = |h: &Harness| {
            let b =
                crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("bounds");
            (f32::midpoint(b[0], b[2]), f32::midpoint(b[1], b[3]))
        };
        let start = middle(&h);
        let pivot = (start.0, start.1);
        let steps = h.app.editor.history_mark();
        for _ in 0..9 {
            assert!(change(&mut h.app, Change::Turn(10.0)));
        }
        // Ninety degrees about the same point: the lines (wide and flat) are now tall and thin about it.
        let b = crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("bounds");
        assert!(b[3] - b[1] > 100.0 && b[2] - b[0] < 130.0, "{b:?}");
        let end = middle(&h);
        assert!(
            (end.0 - pivot.0).abs() < 1e-2 && (end.1 - pivot.1).abs() < 1e-2,
            "{end:?} {pivot:?}"
        );
        assert_eq!(h.app.editor.history_mark(), steps + 1);
    }

    #[test]
    fn something_else_ending_the_edit_starts_a_new_step_for_the_next_change() {
        use crate::selection_bar::{Change, change};
        use crate::size_fields::Field;
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        let steps = h.app.editor.history_mark();
        assert!(change(&mut h.app, Change::Field(Field::X, 20.0)));
        // A flip in between is not part of the edit.
        h.frame(vec![press_with(Key::H, false, false, true)]);
        assert!(change(&mut h.app, Change::Field(Field::X, 40.0)));
        assert!(
            h.app.editor.history_mark() >= steps + 3,
            "three steps: move, flip, move"
        );
    }

    #[test]
    fn the_selection_bar_with_the_numbers_open_stays_inside_any_window_and_holds_still() {
        for (width, height) in [(1280.0, 800.0), (640.0, 480.0), (600.0, 480.0)] {
            let mut h = Harness::sized(width, height);
            h.app.config.canvas.pattern = crate::config::Pattern::Blank;
            draw_through(&mut h, &[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)]);
            h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
            h.frame(vec![press_with(Key::A, true, false, false)]);
            let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height));
            for open in [false, true] {
                h.app.ui.size_fields.open = open;
                settle(&mut h, 4);
                let first = panels(&h);
                settle(&mut h, 2);
                assert_eq!(
                    drift(&first, &panels(&h)),
                    Some(0.0),
                    "{width}x{height} open={open}"
                );
                for rect in panels(&h) {
                    assert!(
                        window.contains_rect(rect),
                        "{width}x{height} open={open}: {rect:?}"
                    );
                }
            }
            let bar = panels(&h)
                .into_iter()
                .find(|r| r.height() > 60.0 && r.min.y < 100.0);
            assert!(
                bar.is_some(),
                "the bar is up with its second row and the numbers"
            );
        }
    }

    #[test]
    fn with_everything_locked_a_small_bar_offers_to_unlock_and_the_numbers_close_with_the_selection()
     {
        let mut h = three_lines();
        h.frame(vec![press_with(Key::A, true, false, false)]);
        h.app.ui.size_fields.open = true;
        settle(&mut h, 3);
        h.frame(vec![press_with(Key::L, true, false, true)]);
        settle(&mut h, 3);
        assert_eq!(h.app.selection.len(), 0);
        assert!(!h.app.ui.size_fields.open, "no selection, no numbers");
        // The bar is wide; the round buttons of the top bar are panels too, and are not it.
        let bars = panels(&h);
        assert!(
            bars.iter()
                .any(|r| r.min.y < 100.0 && r.height() < 60.0 && r.width() > 100.0),
            "the locked bar: {bars:?}"
        );
        h.frame(vec![press_with(Key::L, true, true, false)]);
        settle(&mut h, 3);
        assert_eq!(h.app.editor.marks().locked().len(), 0);
        assert!(
            !panels(&h)
                .iter()
                .any(|r| r.min.y < 100.0 && r.height() < 60.0 && r.width() > 100.0),
            "and it goes"
        );
    }

    #[test]
    fn the_turn_panel_opens_from_its_key_stays_in_the_window_and_leaves_the_paper_alone() {
        for (width, height) in [(1280.0, 800.0), (640.0, 480.0)] {
            let mut h = Harness::sized(width, height);
            let closed = panels(&h).len();
            h.frame(vec![press_with(Key::R, true, false, true)]);
            settle(&mut h, 6);
            assert!(h.app.ui.turn_open, "{width}x{height}");
            assert_eq!(h.app.view.angle, 0.0, "opening it does not turn the paper");
            let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height));
            let first = panels(&h);
            settle(&mut h, 2);
            assert_eq!(drift(&first, &panels(&h)), Some(0.0), "{width}x{height}");
            assert!(
                first.len() >= closed + 2,
                "the chip and the panel are up: {first:?}"
            );
            for rect in &first {
                assert!(window.contains_rect(*rect), "{width}x{height}: {rect:?}");
            }
            h.frame(vec![press_with(Key::Escape, false, false, false)]);
            settle(&mut h, 20);
            assert!(!h.app.ui.turn_open);
            assert_eq!(panels(&h).len(), closed, "and both go again");
        }
        let mut h = Harness::new();
        h.app.config.input.rotation_enabled = false;
        h.frame(vec![press_with(Key::R, true, false, true)]);
        assert!(!h.app.ui.turn_open, "no turning, no panel");
    }

    #[test]
    fn the_turn_panel_shows_the_angle_the_chip_says_and_the_chip_opens_it() {
        let mut h = Harness::new();
        h.app
            .view
            .rotate_at(pos2(600.0, 400.0), 30.0_f32.to_radians());
        settle(&mut h, 3);
        let chip = panels(&h)
            .into_iter()
            .find(|r| r.width() < 90.0 && r.max.y > 700.0 && r.min.x < 1150.0)
            .expect("the turn chip beside the zoom chip");
        let click = |pressed| Event::PointerButton {
            pos: chip.center(),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(chip.center())]);
        h.frame(vec![click(true)]);
        h.frame(vec![click(false)]);
        assert!(h.app.ui.turn_open, "a tap on the chip opens the panel");
        settle(&mut h, 6);
        assert!(
            (h.app.view.degrees() - 30.0).abs() < 1e-3,
            "and does not turn the paper"
        );
    }

    #[test]
    fn the_turn_keys_turn_the_paper_a_step_either_way_and_back_upright() {
        let mut h = guided();
        h.frame(vec![press_with_control(Key::ArrowLeft, false)]);
        assert!(
            (h.app.view.degrees() - 15.0).abs() < 1e-3,
            "counterclockwise"
        );
        h.frame(vec![press_with_control(Key::ArrowLeft, false)]);
        assert!((h.app.view.degrees() - 30.0).abs() < 1e-3);
        h.frame(vec![press_with_control(Key::ArrowRight, false)]);
        assert!((h.app.view.degrees() - 15.0).abs() < 1e-3);
        h.frame(vec![press_with_control(Key::Num0, true)]);
        assert!(h.app.view.degrees().abs() < 1e-3, "upright again");
    }

    #[test]
    fn the_turn_keys_keep_the_middle_of_the_window_still_and_do_nothing_when_turning_is_off() {
        let mut h = guided();
        let middle = h.app.canvas_rect.expect("a canvas").center();
        let before = h.app.view.to_canvas(middle);
        h.frame(vec![press_with_control(Key::ArrowLeft, false)]);
        let after = h.app.view.to_canvas(middle);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        h.app.view.rotate_at(middle, -h.app.view.angle);
        h.app.config.input.rotation_enabled = false;
        h.frame(vec![press_with_control(Key::ArrowLeft, false)]);
        assert!(h.app.view.degrees().abs() < 1e-3, "turning is switched off");
    }

    #[test]
    fn what_is_drawn_on_turned_paper_is_where_the_pen_was_on_the_screen() {
        let mut h = guided();
        let middle = h.app.canvas_rect.expect("a canvas").center();
        h.app.view.rotate_at(middle, 40.0_f32.to_radians());
        let screen = [
            (400.0, 300.0),
            (480.0, 340.0),
            (560.0, 330.0),
            (640.0, 400.0),
        ];
        h.drag(&screen);
        let stroke = &all_strokes(&h)[0];
        for (sample, (x, y)) in stroke.samples.iter().zip(screen) {
            let there = h.app.view.to_screen(sample.point);
            assert!(
                there.distance(pos2(x, y)) < 2.5,
                "a sample is under the pen: {there:?} vs {x},{y}"
            );
        }
        // And it is not where it would be if the paper were upright.
        let first = stroke.samples[0].point;
        let upright = ink_core::Point::new(400.0 - middle.x, middle.y - 300.0);
        assert!(first.distance(upright) > 50.0, "{first:?} {upright:?}");
    }

    #[test]
    fn a_turned_paper_shows_its_angle_beside_the_zoom_and_tapping_it_opens_the_panel_that_turns_it_upright()
     {
        let mut h = guided();
        h.frame(Vec::new());
        let panels_before = panels(&h).len();
        let middle = h.app.canvas_rect.expect("a canvas").center();
        h.app.view.rotate_at(middle, 25.0_f32.to_radians());
        // From the very first frame it is on the screen, it is where it stays.
        h.frame(Vec::new());
        let first: Vec<Rect> = panels(&h)
            .into_iter()
            .filter(|r| r.max.y > h.size.y - 80.0 && r.min.x > 800.0)
            .collect();
        assert_eq!(first.len(), 2);
        h.frame(Vec::new());
        let panels_now = panels(&h);
        let settled: Vec<Rect> = panels_now
            .iter()
            .copied()
            .filter(|r| r.max.y > h.size.y - 80.0 && r.min.x > 800.0)
            .collect();
        assert_eq!(first, settled, "the chips did not move after they appeared");
        assert_eq!(
            panels_now.len(),
            panels_before + 1,
            "the angle chip is one more panel"
        );
        let window = Rect::from_min_size(pos2(0.0, 0.0), h.size).expand(1.0);
        let outside: Vec<&Rect> = panels_now
            .iter()
            .filter(|r| !(window.contains_rect(**r) || (r.width() < 120.0 && r.height() > 200.0)))
            .collect();
        assert!(outside.is_empty(), "outside the window: {outside:?}");
        // The new chip is the one to the left of the zoom chip, and the two do not overlap.
        let mut chips: Vec<Rect> = panels_now
            .iter()
            .copied()
            .filter(|r| r.max.y > h.size.y - 60.0 && r.min.x > 800.0)
            .collect();
        chips.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
        assert_eq!(chips.len(), 2);
        assert!(chips[0].max.x < chips[1].min.x, "{chips:?}");
        let at = chips[0].center();
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(at)]);
        h.frame(vec![button(true)]);
        h.frame(vec![button(false)]);
        assert!(
            h.app.ui.turn_open,
            "tapping it opens the panel for the angle"
        );
        assert!(
            (h.app.view.degrees() - 25.0).abs() < 1e-3,
            "and does not turn the paper"
        );
        settle(&mut h, 8);
        // The panel is the new rectangle above the chips; its button for upright is the second in the first
        // row under the title, after the field for the angle (the switch for turning with two fingers is below it).
        let panel = panels(&h)
            .into_iter()
            .find(|r| r.max.y < chips[0].min.y && r.width() > 150.0 && r.min.x > 600.0)
            .expect("the turn panel above the chips");
        let upright = pos2(
            panel.min.x + 12.0 + 88.0 + 8.0 + 44.0,
            panel.min.y + 12.0 + 16.0 + 8.0 + 15.0,
        );
        let press = |pressed| Event::PointerButton {
            pos: upright,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(upright)]);
        h.frame(vec![press(true)]);
        h.frame(vec![press(false)]);
        assert!(
            h.app.view.degrees().abs() < 1e-3,
            "its Upright button turns the paper upright: {}",
            h.app.view.degrees()
        );
    }

    fn artboard_of(h: &mut Harness, confine: bool) -> ink_core::artboard::Artboard {
        let mut board = ink_core::artboard::Artboard::centered(pt(0.0, 0.0), 300.0, 200.0);
        board.confine = confine;
        h.app.artboard = Some(board);
        board
    }

    fn saved_in(name: &str) -> (Harness, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("inkwell-keep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        let path = dir.join("Page.ink");
        let mut h = guided();
        draw_through(&mut h, &[(0.0, 0.0), (50.0, 20.0), (100.0, 0.0)]);
        draw_through(&mut h, &[(0.0, 60.0), (50.0, 80.0), (100.0, 60.0)]);
        (h, path)
    }

    #[test]
    fn the_artboard_guides_locks_and_groups_are_saved_in_the_drawing_and_come_back_with_it() {
        let (mut h, path) = saved_in("roundtrip");
        h.app.artboard = Some(ink_core::artboard::Artboard::centered(
            pt(0.0, 0.0),
            400.0,
            300.0,
        ));
        h.app.toggle_guide(ink_core::guide::Kind::Circle);
        let ids: Vec<uuid::Uuid> = all_strokes(&h).iter().map(|s| s.id).collect();
        h.app.editor.edit_marks(|m| {
            m.lock([ids[0]]);
            m.group(&ids);
        });
        assert!(h.app.is_dirty(), "marking strokes is a change to be saved");
        h.app.write_file(&path).expect("saved");
        assert!(!h.app.is_dirty(), "saved");
        // Open it afresh in another app.
        let mut other = Harness::new();
        let ctx = other.ctx.clone();
        assert!(other.app.open_in_tab(&ctx, &path));
        other.frame(Vec::new());
        assert_eq!(other.app.artboard.map(|b| b.width()), Some(400.0));
        assert_eq!(other.app.guides.list.len(), 1);
        assert!(other.app.editor.is_locked(ids[0]) && !other.app.editor.is_locked(ids[1]));
        assert_eq!(other.app.editor.marks().groups().len(), 1);
        assert!(!other.app.is_dirty(), "opening it is not a change");
        let _ = std::fs::remove_dir_all(path.parent().expect("folder"));
    }

    #[test]
    fn a_change_to_what_the_drawing_keeps_besides_its_strokes_makes_it_unsaved_and_autosave_notices()
     {
        let (mut h, path) = saved_in("dirty");
        h.app.write_file(&path).expect("saved");
        assert!(!h.app.is_dirty());
        let at = h.app.editor.revision();
        h.app.toggle_guide(ink_core::guide::Kind::Line);
        assert_eq!(h.app.editor.revision(), at, "the strokes did not change");
        assert!(h.app.is_dirty(), "but the drawing is not as it was saved");
        h.app.write_file(&path).expect("saved");
        assert!(!h.app.is_dirty());
        let _ = std::fs::remove_dir_all(path.parent().expect("folder"));
    }

    #[test]
    fn a_confined_stroke_cannot_begin_outside_the_artboard() {
        let mut h = guided();
        artboard_of(&mut h, true);
        draw_through(&mut h, &[(200.0, 0.0), (210.0, 20.0), (230.0, 40.0)]);
        assert_eq!(h.strokes(), 0, "begun outside: nothing is drawn");
        draw_through(&mut h, &[(0.0, 0.0), (40.0, 20.0), (60.0, 40.0)]);
        assert_eq!(h.strokes(), 1, "begun inside: it is");
    }

    #[test]
    fn a_confined_stroke_stays_on_the_edge_where_the_pen_goes_outside_and_is_kept_as_shown() {
        let mut h = guided();
        let board = artboard_of(&mut h, true);
        draw_through(
            &mut h,
            &[
                (100.0, 0.0),
                (140.0, 10.0),
                (200.0, 30.0),
                (260.0, 60.0),
                (120.0, 40.0),
            ],
        );
        let stroke = &all_strokes(&h)[0];
        assert!(
            stroke.samples.iter().all(|s| board.contains(s.point)),
            "every point is on the page"
        );
        let right = board.max.x;
        // The pen's smoothing rounds the path off a little before the edge, so it comes close rather than exact.
        assert!(
            stroke
                .samples
                .iter()
                .any(|s| (s.point.x - right).abs() < 6.0),
            "it rode the edge"
        );
        // The same without being confined goes out past it.
        let mut free = guided();
        artboard_of(&mut free, false);
        draw_through(
            &mut free,
            &[(100.0, 0.0), (140.0, 10.0), (200.0, 30.0), (260.0, 60.0)],
        );
        assert!(
            all_strokes(&free)[0]
                .samples
                .iter()
                .any(|s| s.point.x > right + 10.0)
        );
    }

    #[test]
    fn showing_the_whole_page_fits_the_artboard_and_stands_it_upright() {
        let mut h = guided();
        let board = artboard_of(&mut h, false);
        let middle = h.app.canvas_rect.expect("a canvas").center();
        h.app.view.rotate_at(middle, 0.7);
        h.app.fit_pending = true;
        h.frame(Vec::new());
        assert!(h.app.view.angle.abs() < 1e-6, "upright");
        let (lo, hi) = (
            h.app.view.to_screen(board.min),
            h.app.view.to_screen(board.max),
        );
        let canvas = h.app.canvas_rect.expect("a canvas");
        assert!(
            canvas.contains(lo) && canvas.contains(hi),
            "the whole page is in view: {lo:?} {hi:?}"
        );
        assert!(
            (f32::midpoint(lo.x, hi.x) - canvas.center().x).abs() < 1.0,
            "and in the middle"
        );
    }

    #[test]
    fn an_artboard_stays_with_its_drawing_when_the_tab_changes() {
        let mut h = guided();
        artboard_of(&mut h, true);
        let ctx = h.ctx.clone();
        h.app.new_tab(&ctx);
        h.frame(Vec::new());
        assert!(h.app.artboard.is_none(), "a new drawing has none");
        h.app.switch_tab(&ctx, 0);
        h.frame(Vec::new());
        assert!(
            h.app.artboard.is_some_and(|b| b.confine),
            "and the first keeps its own"
        );
    }

    #[test]
    fn the_artboard_and_its_settings_can_be_drawn_turned_and_with_the_pattern_kept_inside() {
        let mut h = Harness::new();
        for grid_inside in [false, true] {
            let mut board = artboard_of(&mut h, false);
            board.grid_inside = grid_inside;
            h.app.artboard = Some(board);
            for angle in [0.0, 0.6, -2.0] {
                let middle = h.app.canvas_rect.expect("a canvas").center();
                h.app.view.rotate_at(middle, angle);
                h.frame(Vec::new());
            }
        }
        h.app.open_settings(crate::settings::Section::Precision);
        settle(&mut h, 3);
        h.app.artboard = None;
        settle(&mut h, 3);
    }

    #[test]
    fn a_stroke_held_still_at_its_end_becomes_the_clean_shape_and_is_kept_that_way() {
        let mut h = guided();
        h.app.config.precision.recognize_shapes = true;
        let end = wobbly_circle(&mut h);
        let wobbly = h.app.builder.as_ref().expect("the pen is down").snapshot();
        hold_still(&mut h, 0.9);
        let shown = h.app.builder.as_ref().expect("the pen is down").snapshot();
        assert_ne!(
            shown, wobbly,
            "the stroke became the shape while the pen was held"
        );
        let xs = shown.samples.iter().map(|s| s.point.x);
        let ys = shown.samples.iter().map(|s| s.point.y);
        let (x0, x1) = (
            xs.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.clone().fold(f32::MAX, f32::min),
            ys.fold(f32::MIN, f32::max),
        );
        let center = pt(f32::midpoint(x0, x1), f32::midpoint(y0, y1));
        let radius = (x1 - x0) / 2.0;
        assert!(
            center.distance(pt(0.0, 0.0)) < 4.0,
            "about where it was drawn: {center:?}"
        );
        assert!(
            (radius - 80.0).abs() < 5.0,
            "about the size it was drawn: {radius}"
        );
        assert!(
            shown
                .samples
                .iter()
                .all(|s| (s.point.distance(center) - radius).abs() < 0.5),
            "and a true circle"
        );
        h.frame(vec![Event::PointerButton {
            pos: pos2(end.0, end.1),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        assert_eq!(&all_strokes(&h)[0], &shown, "what is shown is what is kept");
    }

    #[test]
    fn two_arms_with_a_corner_held_still_become_a_clean_angle_that_is_kept_that_way() {
        let mut h = guided();
        h.app.config.precision.recognize_shapes = true;
        // A corner a little off square, drawn with a wobble.
        let path = [(0.0, 0.0), (180.0, 4.0), (176.0, 130.0)];
        let mut points = Vec::new();
        for pair in path.windows(2) {
            for i in 0..30 {
                let t = i as f32 / 30.0;
                let wobble = if i % 2 == 0 { 1.2 } else { -1.2 };
                points.push((
                    pair[0].0 + (pair[1].0 - pair[0].0) * t + wobble,
                    pair[0].1 + (pair[1].1 - pair[0].1) * t - wobble,
                ));
            }
        }
        points.push(path[2]);
        let screen: Vec<(f32, f32)> = points.iter().map(|&(x, y)| at(&h, x, y)).collect();
        let first = pos2(screen[0].0, screen[0].1);
        let end = screen[screen.len() - 1];
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(first)]);
        h.frame(vec![button(first, true)]);
        for &(x, y) in &screen[1..] {
            h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        }
        let drawn = h.app.builder.as_ref().expect("the pen is down").snapshot();
        hold_still(&mut h, 0.9);
        let shown = h.app.builder.as_ref().expect("the pen is down").snapshot();
        assert_ne!(shown, drawn, "it became the angle while the pen was held");
        let corner = shown.samples.len();
        assert!(corner >= 3, "{corner}");
        // Every point is on one of two straight arms that meet at a right angle.
        let (a, b) = (shown.samples[0].point, shown.samples[corner - 1].point);
        let vertex = shown
            .samples
            .iter()
            .map(|s| s.point)
            .max_by(|p, q| {
                (p.distance(a) + p.distance(b)).total_cmp(&(q.distance(a) + q.distance(b)))
            })
            .expect("a corner");
        let (u, v) = (
            (a.x - vertex.x, a.y - vertex.y),
            (b.x - vertex.x, b.y - vertex.y),
        );
        let cosine = (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1));
        assert!(cosine.abs() < 0.01, "a right angle: {cosine}");
        h.frame(vec![button(pos2(end.0, end.1), false)]);
        assert_eq!(&all_strokes(&h)[0], &shown, "what is shown is what is kept");
    }

    #[test]
    fn a_stroke_is_left_alone_when_recognition_is_off_or_the_pen_is_not_held_long_enough() {
        let mut h = guided();
        let end = wobbly_circle(&mut h);
        let wobbly = h.app.builder.as_ref().expect("the pen is down").snapshot();
        hold_still(&mut h, 1.5);
        assert_eq!(
            h.app.builder.as_ref().expect("down").snapshot(),
            wobbly,
            "recognition is off to begin with"
        );
        h.app.config.precision.recognize_shapes = true;
        h.app.config.precision.hold_delay = 2.0;
        // The hold above is already 1.5 seconds; with a delay of 2 seconds a little more is not enough.
        hold_still(&mut h, 0.3);
        assert_eq!(
            h.app.builder.as_ref().expect("down").snapshot(),
            wobbly,
            "not yet"
        );
        hold_still(&mut h, 1.0);
        assert_ne!(
            h.app.builder.as_ref().expect("down").snapshot(),
            wobbly,
            "now"
        );
        let _ = end;
    }

    #[test]
    fn a_scribble_held_still_is_not_turned_into_anything() {
        let mut h = guided();
        h.app.config.precision.recognize_shapes = true;
        let scribble: Vec<(f32, f32)> = (0..30)
            .map(|i| at(&h, i as f32 * 7.0, if i % 2 == 0 { 0.0 } else { 45.0 }))
            .collect();
        let button = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(
            scribble[0].0,
            scribble[0].1,
        ))]);
        h.frame(vec![button(scribble[0], true)]);
        for &(x, y) in &scribble[1..] {
            h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        }
        let before = h.app.builder.as_ref().expect("down").snapshot();
        hold_still(&mut h, 1.5);
        assert_eq!(h.app.builder.as_ref().expect("down").snapshot(), before);
    }

    #[test]
    fn moving_the_pen_again_restarts_the_wait() {
        let mut h = guided();
        h.app.config.precision.recognize_shapes = true;
        let end = wobbly_circle(&mut h);
        hold_still(&mut h, 0.4);
        // A move of more than the slop starts the wait again: the circle is not made yet after 0.4 more.
        h.frame(vec![Event::PointerMoved(pos2(end.0 + 30.0, end.1 + 30.0))]);
        let before = h.app.builder.as_ref().expect("down").snapshot();
        hold_still(&mut h, 0.4);
        assert_eq!(h.app.builder.as_ref().expect("down").snapshot(), before);
        hold_still(&mut h, 0.5);
        assert!(h.app.builder.as_ref().expect("down").is_replaced());
    }

    #[test]
    fn dragging_a_selection_lands_it_on_the_grid_and_what_is_shown_is_what_is_kept() {
        let mut h = snapping();
        h.app.config.precision.snap = false;
        // A line from (64, 96) to (160, 96): both ends on the grid.
        h.drag(&[at(&h, 64.0, 96.0), at(&h, 112.0, 96.0), at(&h, 160.0, 96.0)]);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        let (a, b, c, d) = (
            at(&h, 40.0, 130.0),
            at(&h, 190.0, 130.0),
            at(&h, 190.0, 60.0),
            at(&h, 40.0, 60.0),
        );
        h.drag(&[a, b, c, d, (a.0 + 2.0, a.1 + 2.0)]);
        assert_eq!(h.app.selection.len(), 1);
        h.app.config.precision.snap = true;
        // Grab the middle of the line, and drag it 37 right and 9 down on the canvas: 5 short of landing its
        // ends on the grid in x (37 is 5 more than 32) and 9 more than 0 in y, which is within reach of 14.
        let from = at(&h, 112.0, 96.0);
        let to = at(&h, 112.0 + 37.0, 96.0 - 9.0);
        let press = |(x, y): (f32, f32), pressed| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(from.0, from.1))]);
        h.frame(vec![press(from, true)]);
        h.frame(vec![Event::PointerMoved(pos2(to.0, to.1))]);
        let gesture = h.app.gesture.as_ref().expect("a drag");
        let shown: Vec<ink_core::Point> = crate::tools::moving_strokes(gesture)
            .iter()
            .flat_map(|(_, _, s)| s.points().collect::<Vec<_>>())
            .collect();
        let (first, last) = (shown[0], *shown.last().expect("points"));
        assert!(
            first.distance(ink_core::Point::new(96.0, 96.0)) < 0.01,
            "its start lands on the crossing at (96, 96): {first:?}"
        );
        assert!(
            last.distance(ink_core::Point::new(192.0, 96.0)) < 0.01,
            "and its end on the one at (192, 96): {last:?}"
        );
        assert!(
            crate::tools::landing(gesture).is_some(),
            "the landing is marked"
        );
        h.frame(vec![press(to, false)]);
        let kept = &all_strokes(&h)[0];
        assert!(
            kept.samples[0].point.distance(first) < 0.01,
            "the same is kept"
        );
        assert!(kept.samples.last().expect("samples").point.distance(last) < 0.01);
        assert!(h.app.gesture.is_none());
    }

    /// Draws a horizontal line, selects it, drags the turning handle through `degrees` and returns the angle the
    /// line is at afterwards, in degrees.
    fn turn_the_line_by(h: &mut Harness, degrees: f32) -> f32 {
        h.drag(&[at(h, 64.0, 96.0), at(h, 112.0, 96.0), at(h, 160.0, 96.0)]);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        let (a, b, c, d) = (
            at(h, 40.0, 130.0),
            at(h, 190.0, 130.0),
            at(h, 190.0, 60.0),
            at(h, 40.0, 60.0),
        );
        h.drag(&[a, b, c, d, (a.0 + 2.0, a.1 + 2.0)]);
        let bounds =
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("a selection");
        let frame = crate::tools::frame_of(h.app.view, bounds);
        let center = ink_core::Point::new(
            f32::midpoint(bounds[0], bounds[2]),
            f32::midpoint(bounds[1], bounds[3]),
        );
        let handle = frame.rotate;
        let from = h.app.view.to_canvas(handle);
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (dx, dy) = (from.x - center.x, from.y - center.y);
        let now = h.app.view.to_screen(ink_core::Point::new(
            center.x + dx * cos - dy * sin,
            center.y + dx * sin + dy * cos,
        ));
        h.drag(&[
            (handle.x, handle.y),
            (
                f32::midpoint(handle.x, now.x),
                f32::midpoint(handle.y, now.y),
            ),
            (now.x, now.y),
        ]);
        let stroke = &all_strokes(h)[0];
        let (first, last) = (
            stroke.samples.first().expect("samples").point,
            stroke.samples.last().expect("samples").point,
        );
        (last.y - first.y).atan2(last.x - first.x).to_degrees()
    }

    #[test]
    fn turning_a_selection_lands_on_a_multiple_of_fifteen_degrees_when_snapping_is_on() {
        let mut h = snapping();
        h.app.config.canvas.pattern = crate::config::Pattern::Blank;
        h.app.config.precision.snap = false;
        // Without it the line is turned exactly as far as the pen went (the handle is above the box, so turning
        // the pen counterclockwise round the center is a turn of the same size).
        let mut free = snapping();
        free.app.config.canvas.pattern = crate::config::Pattern::Blank;
        free.app.config.precision.snap = false;
        let raw = turn_the_line_by(&mut free, 13.0);
        assert!((raw.abs() - 13.0).abs() < 0.5, "turned by the pen: {raw}");
        h.app.config.precision.snap = true;
        let snapped = turn_the_line_by(&mut h, 13.0);
        assert!(
            (snapped.abs() - 15.0).abs() < 0.05,
            "turned to a multiple of 15: {snapped}"
        );
    }

    #[test]
    fn a_selection_does_not_snap_while_editing_is_switched_off() {
        let mut h = snapping();
        h.app.config.precision.snap = false;
        h.drag(&[at(&h, 64.0, 96.0), at(&h, 112.0, 96.0), at(&h, 160.0, 96.0)]);
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        let (a, b, c, d) = (
            at(&h, 40.0, 130.0),
            at(&h, 190.0, 130.0),
            at(&h, 190.0, 60.0),
            at(&h, 40.0, 60.0),
        );
        h.drag(&[a, b, c, d, (a.0 + 2.0, a.1 + 2.0)]);
        h.app.config.precision.snap = true;
        h.app.config.precision.snap_while_editing = false;
        let (from, to) = (at(&h, 112.0, 96.0), at(&h, 112.0 + 37.0, 96.0 - 9.0));
        h.drag(&[from, to]);
        let first = all_strokes(&h)[0].samples[0].point;
        assert!(
            (first.x - 101.0).abs() < 0.5 && (first.y - 87.0).abs() < 0.5,
            "moved exactly as dragged: {first:?}"
        );
    }

    #[test]
    fn dragging_the_selection_leaves_the_drawing_alone_until_the_pen_is_lifted() {
        // Rewriting every selected stroke on every move makes the renderer redraw the whole drawing on every move,
        // which is what made nudging slow and made dragging a big selection stutter.
        let mut h = Harness::new();
        let before = line_selected(&mut h);
        let (revision, steps) = (h.app.editor.revision(), h.app.editor.history_mark());
        h.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        h.frame(vec![primary(300.0, true)]);
        for y in [310.0, 330.0, 360.0, 380.0] {
            h.frame(vec![Event::PointerMoved(pos2(500.0, y))]);
            assert_eq!(
                h.app.editor.revision(),
                revision,
                "nothing is redrawn at {y}"
            );
            assert_eq!(first_stroke(&h.app), &before, "the drawing is as it was");
        }
        let gesture = h.app.gesture.as_ref().expect("a drag is under way");
        let moving = crate::tools::moving_strokes(gesture);
        assert_eq!(
            moving.len(),
            1,
            "the selection is shown where the pen has taken it"
        );
        let dy = moving[0].2.samples[0].point.y - before.samples[0].point.y;
        assert!((dy + 80.0).abs() < 0.5, "80 down on the screen: {dy}");
        h.frame(vec![primary(380.0, false)]);
        assert_ne!(
            h.app.editor.revision(),
            revision,
            "it is put in when let go"
        );
        assert_eq!(h.app.editor.history_mark(), steps + 1, "as one step");
        assert!(h.app.gesture.is_none());
        let moved = first_stroke(&h.app);
        assert_eq!(moved.id, before.id);
        assert!((moved.samples[0].point.y - before.samples[0].point.y + 80.0).abs() < 0.5);
        assert!(h.app.editor.undo());
        assert_eq!(first_stroke(&h.app), &before, "one undo puts it all back");
    }

    #[test]
    fn the_selection_box_follows_the_drag_before_the_drawing_does() {
        let mut h = Harness::new();
        line_selected(&mut h);
        let tool_gesture = |h: &Harness| {
            crate::tools::current_bounds(&h.app.editor, &h.app.selection, h.app.gesture.as_ref())
                .expect("a selection")
        };
        let rest = tool_gesture(&h);
        h.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        h.frame(vec![primary(300.0, true)]);
        h.frame(vec![Event::PointerMoved(pos2(500.0, 360.0))]);
        let dragged = tool_gesture(&h);
        assert!(
            (rest[1] - dragged[1] - 60.0).abs() < 0.5,
            "the box moved 60 down: {rest:?} to {dragged:?}"
        );
        h.frame(vec![primary(360.0, false)]);
        let after = tool_gesture(&h);
        assert!(
            (after[1] - dragged[1]).abs() < 0.5,
            "and is where it was shown when the pen is lifted: {dragged:?} then {after:?}"
        );
    }

    #[test]
    fn escape_gives_up_a_drag_of_the_selection_and_the_drawing_was_never_touched() {
        let mut h = Harness::new();
        let before = line_selected(&mut h);
        let (revision, steps) = (h.app.editor.revision(), h.app.editor.history_mark());
        h.frame(vec![Event::PointerMoved(pos2(500.0, 300.0))]);
        h.frame(vec![primary(300.0, true)]);
        h.frame(vec![Event::PointerMoved(pos2(500.0, 380.0))]);
        assert!(h.app.gesture.is_some());
        h.frame(vec![key_event(Key::Escape, Modifiers::NONE)]);
        assert!(h.app.gesture.is_none());
        assert_eq!(first_stroke(&h.app), &before);
        assert_eq!(h.app.editor.revision(), revision, "not even rewritten");
        assert_eq!(h.app.editor.history_mark(), steps, "and no step to undo");
        assert_eq!(h.app.selection, vec![before.id], "the selection is kept");
    }

    #[test]
    fn grabbing_a_handle_and_letting_go_without_moving_changes_nothing() {
        let mut h = Harness::new();
        line_selected(&mut h);
        let (revision, steps) = (h.app.editor.revision(), h.app.editor.history_mark());
        // The top left handle of the box around the line, a little outside its corner.
        let corner = h
            .app
            .selection
            .first()
            .and_then(|_| {
                crate::tools::selection_bounds(&h.app.editor, &h.app.selection)
                    .map(|b| crate::tools::frame_of(h.app.view, b).corners[0])
            })
            .expect("the selection has a box");
        let button = |pressed| Event::PointerButton {
            pos: corner,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(corner)]);
        h.frame(vec![button(true)]);
        h.frame(vec![button(false)]);
        assert_eq!(h.app.editor.revision(), revision);
        assert_eq!(h.app.editor.history_mark(), steps);
    }

    #[test]
    fn a_click_with_the_nudge_tool_changes_nothing() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        let (revision, before) = (h.app.editor.revision(), first_stroke(&h.app).clone());
        h.drag(&[(500.0, 300.0), (500.0, 300.0)]);
        assert_eq!(h.app.editor.revision(), revision);
        assert_eq!(first_stroke(&h.app), &before, "not even extra points");
    }

    #[test]
    fn several_pulls_along_the_same_stroke_each_take_and_each_undo() {
        let mut h = Harness::new();
        draw_line(&mut h);
        let before = first_stroke(&h.app).clone();
        h.frame(vec![key_event(Key::N, Modifiers::NONE)]);
        h.app.config.tools.nudge_reach = 40.0;
        let (x1, x2) = (canvas_x(&h.app, 440.0), canvas_x(&h.app, 560.0));
        h.drag(&[(440.0, 300.0), (440.0, 320.0), (440.0, 330.0)]);
        let after_first = first_stroke(&h.app).clone();
        h.drag(&[(560.0, 300.0), (560.0, 320.0), (560.0, 330.0)]);
        let after_second = first_stroke(&h.app).clone();
        for (x, name) in [(x1, "first"), (x2, "second")] {
            let moved = y_near(&after_second, x) - y_near(&before, x);
            assert!(
                (moved + 30.0).abs() < 2.0,
                "the {name} pull is kept: {moved}"
            );
        }
        assert!(
            (y_near(&after_first, x2) - y_near(&before, x2)).abs() < 1.0,
            "the second place had not moved yet after the first pull"
        );
        assert!(h.app.editor.undo());
        assert_eq!(
            first_stroke(&h.app),
            &after_first,
            "one undo takes back the second pull only"
        );
        assert!(h.app.editor.undo());
        assert_eq!(first_stroke(&h.app), &before);
    }

    #[test]
    fn select_all_delete_and_duplicate_work_from_the_keyboard_and_each_is_one_undo() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.drag(&[(400.0, 360.0), (500.0, 360.0), (600.0, 360.0)]);
        assert_eq!(h.strokes(), 2);
        h.frame(vec![key_event(Key::A, Modifiers::COMMAND)]);
        assert_eq!(h.app.tool.mode, Mode::Select);
        assert_eq!(h.app.selection.len(), 2);
        h.frame(vec![key_event(Key::D, Modifiers::COMMAND)]);
        assert_eq!(h.strokes(), 4);
        assert_eq!(
            h.app.selection.len(),
            2,
            "the copies are what is selected now"
        );
        h.frame(vec![key_event(Key::Delete, Modifiers::NONE)]);
        assert_eq!(h.strokes(), 2);
        h.frame(vec![key_event(Key::Z, Modifiers::COMMAND)]);
        assert_eq!(h.strokes(), 4, "deleting two strokes is one step back");
        h.frame(vec![key_event(Key::Z, Modifiers::COMMAND)]);
        assert_eq!(h.strokes(), 2, "and so is duplicating them");
    }

    #[test]
    fn arrow_keys_move_the_selection_further_with_shift_and_finer_with_alt() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(Key::A, Modifiers::COMMAND)]);
        let x0 = first_stroke(&h.app).samples[0].point.x;
        h.frame(vec![key_event(Key::ArrowRight, Modifiers::NONE)]);
        let x1 = first_stroke(&h.app).samples[0].point.x;
        assert!((x1 - x0 - h.app.config.tools.arrow_step).abs() < 1e-3);
        h.frame(vec![key_event(Key::ArrowRight, Modifiers::SHIFT)]);
        let x2 = first_stroke(&h.app).samples[0].point.x;
        assert!((x2 - x1 - h.app.config.tools.arrow_large).abs() < 1e-3);
        h.frame(vec![key_event(Key::ArrowRight, Modifiers::ALT)]);
        let x3 = first_stroke(&h.app).samples[0].point.x;
        assert!((x3 - x2 - h.app.config.tools.arrow_small).abs() < 1e-3);
    }

    #[test]
    fn copy_and_paste_put_the_strokes_back_offset_and_selected() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(Key::A, Modifiers::COMMAND)]);
        h.frame(vec![Event::Copy]);
        assert_eq!(h.app.clipboard.len(), 1);
        h.frame(vec![Event::Paste(String::new())]);
        assert_eq!(h.strokes(), 2);
        h.frame(vec![Event::Paste(String::new())]);
        assert_eq!(h.strokes(), 3, "pasting again adds another");
        assert_eq!(h.app.selection.len(), 1);
    }

    #[test]
    fn a_changed_key_runs_the_command_and_the_old_key_stops() {
        let mut h = Harness::new();
        h.app
            .config
            .shortcuts
            .insert("tool_eraser".to_owned(), vec!["X".to_owned()]);
        h.app.rebuild_keymap();
        h.frame(vec![key_event(Key::E, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Draw, "E is no longer the eraser");
        h.frame(vec![key_event(Key::X, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Erase);
    }

    #[test]
    fn choosing_a_key_in_the_settings_rebinds_the_command_and_escape_only_cancels_the_choice() {
        use crate::shortcuts::Command;
        let mut h = Harness::new();
        h.app.open_settings(Section::Shortcuts);
        h.frame(Vec::new());
        h.app.ui.capture = Some((Command::ToolPan, Some(0)));
        h.frame(vec![key_event(Key::Escape, Modifiers::NONE)]);
        assert!(h.app.ui.capture.is_none(), "the choice was cancelled");
        assert_eq!(
            h.app.ui.settings,
            Some(Section::Shortcuts),
            "Esc did not also close the settings"
        );
        assert!(h.app.config.shortcuts.is_empty());

        h.app.ui.capture = Some((Command::ToolPan, Some(0)));
        h.frame(vec![key_event(Key::J, Modifiers::NONE)]);
        assert!(h.app.ui.capture.is_none());
        assert_eq!(
            h.app.keymap.resolve(Key::J, Modifiers::NONE),
            Some(Command::ToolPan)
        );
        assert_eq!(h.app.keymap.resolve(Key::H, Modifiers::NONE), None);
        assert!(h.app.config.shortcuts.contains_key("tool_pan"));

        // A key that belongs to another command moves over to this one.
        h.app.ui.capture = Some((Command::ToolPan, None));
        h.frame(vec![key_event(Key::K, Modifiers::NONE)]);
        assert_eq!(
            h.app.keymap.resolve(Key::K, Modifiers::NONE),
            Some(Command::ToolPan)
        );
        assert_eq!(h.app.keymap.bindings(Command::ToolSlice).count(), 0);
    }

    #[test]
    fn the_key_chosen_in_the_settings_is_not_also_run_as_a_command() {
        use crate::shortcuts::Command;
        let mut h = Harness::new();
        h.app.open_settings(Section::Shortcuts);
        h.frame(Vec::new());
        h.app.ui.capture = Some((Command::Undo, Some(0)));
        h.frame(vec![key_event(Key::E, Modifiers::NONE)]);
        assert_eq!(
            h.app.tool.mode,
            Mode::Draw,
            "E was an answer, not the eraser"
        );
        assert_eq!(
            h.app.keymap.resolve(Key::E, Modifiers::NONE),
            Some(Command::Undo)
        );
    }

    fn space(pressed: bool) -> Event {
        Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn holding_space_moves_the_canvas_with_any_tool_and_letting_go_draws_again() {
        let mut h = Harness::new();
        let origin = h.app.view.origin;
        h.frame(vec![space(true)]);
        assert!(h.app.pan_held);
        h.drag(&[(400.0, 300.0), (450.0, 340.0), (500.0, 380.0)]);
        assert_eq!(h.strokes(), 0, "nothing was drawn while space was held");
        assert_ne!(h.app.view.origin, origin, "the canvas moved with the pen");
        assert_eq!(h.app.tool.mode, Mode::Draw, "the tool did not change");
        h.frame(vec![space(false)]);
        assert!(!h.app.pan_held);
        h.drag(&[(400.0, 300.0), (500.0, 300.0), (600.0, 300.0)]);
        assert_eq!(h.strokes(), 1);
    }

    #[test]
    fn holding_space_also_moves_the_canvas_in_the_erase_and_select_tools() {
        for mode in [Mode::Erase, Mode::Select, Mode::Slice, Mode::Nudge] {
            let mut h = Harness::new();
            draw_line(&mut h);
            h.app.tool.mode = mode;
            let (origin, before) = (h.app.view.origin, first_stroke(&h.app).clone());
            h.frame(vec![space(true)]);
            h.drag(&[(450.0, 250.0), (450.0, 300.0), (450.0, 350.0)]);
            assert_ne!(h.app.view.origin, origin, "{mode:?}");
            assert_eq!(first_stroke(&h.app), &before, "{mode:?} touched nothing");
        }
    }

    #[test]
    fn space_pressed_in_the_middle_of_a_stroke_does_not_cut_it_short() {
        let mut h = Harness::new();
        let origin = h.app.view.origin;
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(pos2(400.0, 300.0))]);
        h.frame(vec![button(pos2(400.0, 300.0), true)]);
        h.frame(vec![Event::PointerMoved(pos2(450.0, 300.0))]);
        h.frame(vec![space(true)]);
        h.frame(vec![Event::PointerMoved(pos2(520.0, 300.0))]);
        h.frame(vec![button(pos2(520.0, 300.0), false)]);
        assert_eq!(h.strokes(), 1, "the stroke was finished, not lost");
        assert_eq!(h.app.view.origin, origin);
    }

    #[test]
    fn the_key_for_moving_the_canvas_can_be_changed() {
        let mut h = Harness::new();
        h.app
            .config
            .shortcuts
            .insert("hold_pan".to_owned(), vec!["J".to_owned()]);
        h.app.rebuild_keymap();
        h.frame(vec![space(true)]);
        assert!(!h.app.pan_held, "space no longer moves it");
        h.frame(vec![space(false), key_event(Key::J, Modifiers::NONE)]);
        assert!(h.app.pan_held, "the new key does");
    }

    #[test]
    fn layers_are_added_copied_and_moved_from_the_keyboard_and_undone() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.frame(vec![key_event(
            Key::N,
            Modifiers::COMMAND | Modifiers::SHIFT,
        )]);
        assert_eq!(h.app.editor.document().layers.len(), 2);
        assert_eq!(h.app.active_layer, 1, "the new layer is the one drawn on");
        h.frame(vec![key_event(
            Key::D,
            Modifiers::COMMAND | Modifiers::SHIFT,
        )]);
        assert_eq!(h.app.editor.document().layers.len(), 3);
        h.frame(vec![key_event(Key::OpenBracket, Modifiers::COMMAND)]);
        h.frame(vec![key_event(Key::Z, Modifiers::COMMAND)]);
        h.frame(vec![key_event(Key::Z, Modifiers::COMMAND)]);
        h.frame(vec![key_event(Key::Z, Modifiers::COMMAND)]);
        assert_eq!(
            h.app.editor.document().layers.len(),
            1,
            "back to the one layer"
        );
        assert_eq!(h.app.active_layer, 0, "and drawing on one that exists");
        assert_eq!(h.strokes(), 1);
    }

    #[test]
    fn the_layers_panel_can_be_drawn_while_renaming_and_with_a_hidden_layer() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.app.new_layer();
        h.app.editor.set_layer_visible(0, false);
        h.app.ui.layers_open = true;
        h.frame(Vec::new());
        h.app.ui.renaming = Some((1, "Ink".to_owned()));
        h.frame(Vec::new());
        h.frame(vec![key_event(Key::Escape, Modifiers::NONE)]);
        assert!(h.app.ui.renaming.is_none(), "Esc gives up the rename");
    }

    fn touch(id: u64, phase: egui::TouchPhase, x: f32, y: f32, force: Option<f32>) -> Event {
        Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos: pos2(x, y),
            force,
        }
    }

    /// Lets enough frames go by that a tap right after drawing would no longer be taken for the hand resting.
    fn wait(h: &mut Harness, seconds: f64) {
        for _ in 0..=(seconds * 60.0) as usize {
            h.frame(Vec::new());
        }
    }

    /// Puts `fingers` fingers on the paper and lifts them a moment later.
    fn tap(h: &mut Harness, fingers: u64) {
        let down = (1..=fingers)
            .map(|i| {
                touch(
                    i,
                    egui::TouchPhase::Start,
                    300.0 + i as f32 * 60.0,
                    500.0,
                    None,
                )
            })
            .collect();
        h.frame(down);
        let up = (1..=fingers)
            .map(|i| {
                touch(
                    i,
                    egui::TouchPhase::End,
                    300.0 + i as f32 * 60.0,
                    500.0,
                    None,
                )
            })
            .collect();
        h.frame(up);
    }

    #[test]
    fn a_two_finger_tap_undoes_and_a_three_finger_tap_redoes() {
        let mut h = Harness::new();
        draw_line(&mut h);
        wait(&mut h, 0.8);
        tap(&mut h, 2);
        assert_eq!(h.strokes(), 0, "two fingers undid the line");
        wait(&mut h, 0.1);
        tap(&mut h, 3);
        assert_eq!(h.strokes(), 1, "three fingers brought it back");
        wait(&mut h, 0.1);
        tap(&mut h, 4);
        assert_eq!(h.strokes(), 1, "four fingers do nothing by default");
    }

    #[test]
    fn the_tap_actions_come_from_the_settings() {
        let mut h = Harness::new();
        draw_line(&mut h);
        h.app.config.input.two_finger_tap = crate::config::TapAction::HidePanels;
        h.app.config.input.four_finger_tap = crate::config::TapAction::Undo;
        wait(&mut h, 0.8);
        tap(&mut h, 2);
        assert!(h.app.ui.panels_hidden);
        assert_eq!(h.strokes(), 1);
        wait(&mut h, 0.1);
        tap(&mut h, 2);
        assert!(!h.app.ui.panels_hidden, "the same tap brings them back");
        wait(&mut h, 0.1);
        tap(&mut h, 4);
        assert_eq!(h.strokes(), 0);
    }

    #[test]
    fn a_hand_resting_on_the_screen_while_the_pen_draws_is_not_a_tap() {
        let mut h = Harness::new();
        wait(&mut h, 0.8);
        draw_line(&mut h);
        tap(&mut h, 2);
        assert_eq!(
            h.strokes(),
            1,
            "the tap came right after a stroke, so it was the hand"
        );
    }

    #[test]
    fn taps_do_nothing_over_the_library() {
        let mut h = Harness::new();
        draw_line(&mut h);
        wait(&mut h, 0.8);
        h.app.ui.library_open = true;
        tap(&mut h, 2);
        assert_eq!(h.strokes(), 1);
    }

    #[test]
    fn the_key_for_hiding_the_panels_toggles_them() {
        let mut h = Harness::new();
        h.frame(vec![key_event(Key::Period, Modifiers::COMMAND)]);
        assert!(h.app.ui.panels_hidden);
        h.frame(vec![key_event(Key::Period, Modifiers::COMMAND)]);
        assert!(!h.app.ui.panels_hidden);
    }

    #[test]
    fn zoom_can_be_locked_and_the_keys_then_leave_it_alone() {
        let mut h = Harness::new();
        let zoom = h.app.view.zoom;
        h.frame(vec![key_event(Key::Equals, Modifiers::COMMAND)]);
        assert!(h.app.view.zoom > zoom, "unlocked: the key zooms in");
        h.app.config.input.zoom_enabled = false;
        let locked = h.app.view.zoom;
        h.frame(vec![key_event(Key::Equals, Modifiers::COMMAND)]);
        h.frame(vec![key_event(Key::Minus, Modifiers::COMMAND)]);
        assert!(
            (h.app.view.zoom - locked).abs() < 1e-6,
            "locked: it does not move"
        );
    }

    #[test]
    fn the_wheel_does_not_zoom_a_locked_canvas_and_it_pans_instead() {
        let mut h = Harness::new();
        h.app.config.input.wheel_zooms = true;
        h.app.config.input.zoom_enabled = false;
        let (zoom, origin) = (h.app.view.zoom, h.app.view.origin);
        h.frame(vec![Event::PointerMoved(pos2(640.0, 400.0))]);
        h.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        }]);
        wait(&mut h, 0.3);
        assert!((h.app.view.zoom - zoom).abs() < 1e-6);
        assert_ne!(
            h.app.view.origin, origin,
            "the wheel moved the paper instead"
        );
    }

    #[test]
    fn a_zoom_that_stops_near_a_round_level_settles_on_it_after_a_moment() {
        let mut h = Harness::new();
        h.app.config.input.wheel_zooms = true;
        h.frame(vec![Event::PointerMoved(pos2(640.0, 400.0))]);
        h.app.view.zoom = 0.98;
        // A little wheel scroll lands the zoom close to 100% but not on it.
        h.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 4.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        }]);
        wait(&mut h, 0.6);
        assert!((h.app.view.zoom - 1.0).abs() < 1e-5, "{}", h.app.view.zoom);
        // With snapping off it stays where it was left.
        h.app.config.input.zoom_snap = false;
        h.app.view.zoom = 0.98;
        h.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 4.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        }]);
        wait(&mut h, 0.6);
        assert!((h.app.view.zoom - 1.0).abs() > 1e-4, "{}", h.app.view.zoom);
    }

    #[test]
    fn the_pressure_response_reshapes_what_the_pen_reports() {
        let mut h = Harness::new();
        h.app.config.input.pressure_low = 0.2;
        h.app.config.input.pressure_high = 0.8;
        h.frame(vec![touch(
            1,
            egui::TouchPhase::Start,
            400.0,
            300.0,
            Some(0.35),
        )]);
        h.drag(&[
            (400.0, 300.0),
            (450.0, 300.0),
            (500.0, 300.0),
            (550.0, 300.0),
        ]);
        let pressure = &first_stroke(&h.app).pressure;
        assert_ne!(pressure.len(), 0);
        assert!(
            pressure.iter().all(|p| (p - 0.25).abs() < 0.02),
            "0.35 between 0.2 and 0.8 is a quarter of the way: {pressure:?}"
        );
        h.frame(vec![touch(1, egui::TouchPhase::End, 550.0, 300.0, None)]);
    }

    #[test]
    fn turning_pressure_off_ignores_what_the_pen_reports() {
        let mut h = Harness::new();
        h.app.config.input.use_pressure = false;
        h.frame(vec![touch(
            1,
            egui::TouchPhase::Start,
            400.0,
            300.0,
            Some(0.9),
        )]);
        h.drag(&[(400.0, 300.0), (450.0, 300.0), (500.0, 300.0)]);
        assert_eq!(first_stroke(&h.app).pressure, Vec::<f32>::new());
    }

    #[test]
    fn turning_the_keyboard_off_stops_every_key_including_the_hold_to_move() {
        let mut h = Harness::new();
        h.app.config.workspace.keyboard_shortcuts = false;
        h.frame(vec![key_event(Key::E, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Draw, "E did nothing");
        h.frame(vec![space(true)]);
        assert!(!h.app.pan_held, "nor does holding space");
        h.frame(vec![space(false)]);
        h.app.config.workspace.keyboard_shortcuts = true;
        h.frame(vec![key_event(Key::E, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Erase);
    }

    #[test]
    fn a_rail_of_eight_pens_can_be_drawn_and_each_has_its_key() {
        let mut h = Harness::new();
        h.app.config.resize_rail(8);
        h.frame(Vec::new());
        let eighth = h.app.config.rail_pens()[7];
        h.frame(vec![key_event(Key::Num8, Modifiers::NONE)]);
        assert_eq!(h.app.tool.pen, eighth);
        let third = h.app.config.rail_pens()[2];
        h.frame(vec![key_event(Key::Num3, Modifiers::NONE)]);
        assert_eq!(h.app.tool.pen, third);
    }

    #[test]
    fn the_workspace_and_interaction_pages_can_be_drawn_with_everything_switched_either_way() {
        let mut h = Harness::new();
        for on in [false, true] {
            h.app.config.input.zoom_enabled = on;
            h.app.config.input.use_pressure = on;
            h.app.config.workspace.hover_brush_preview = on;
            h.app.config.workspace.show_stroke_length = on;
            h.app.config.canvas.grid_color = on.then_some([10, 20, 30]);
            h.app.config.canvas.pattern = crate::config::Pattern::Grid;
            for section in [Section::Workspace, Section::Input, Section::Canvas] {
                h.app.open_settings(section);
                h.frame(Vec::new());
                h.frame(Vec::new());
            }
        }
    }

    #[test]
    fn previous_tool_goes_back_and_forth() {
        let mut h = Harness::new();
        h.frame(vec![key_event(Key::V, Modifiers::NONE)]);
        h.frame(vec![key_event(Key::K, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Slice);
        h.frame(vec![key_event(Key::Q, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Select);
        h.frame(vec![key_event(Key::Q, Modifiers::NONE)]);
        assert_eq!(h.app.tool.mode, Mode::Slice);
    }

    #[test]
    fn every_tool_and_the_selection_bar_can_be_drawn() {
        let mut h = Harness::new();
        draw_line(&mut h);
        for mode in [
            Mode::Select,
            Mode::Slice,
            Mode::Nudge,
            Mode::Erase,
            Mode::Pan,
            Mode::Draw,
        ] {
            h.app.tool.mode = mode;
            h.app.ui.dial_open = true;
            h.frame(Vec::new());
            h.frame(Vec::new());
        }
        h.frame(vec![key_event(Key::A, Modifiers::COMMAND)]);
        h.frame(Vec::new());
        assert_ne!(h.app.selection, Vec::<Uuid>::new());
    }

    #[test]
    fn panels_fade_while_a_stroke_is_drawn_and_return_afterwards() {
        let mut h = Harness::new();
        h.app.config.interface.hide_while_drawing = true;
        let first = pos2(400.0, 300.0);
        let button = |pressed| Event::PointerButton {
            pos: first,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(first)]);
        h.frame(vec![button(true)]);
        for i in 0..20 {
            h.frame(vec![Event::PointerMoved(pos2(
                400.0 + 5.0 * i as f32,
                300.0,
            ))]);
        }
        assert!(h.app.builder.is_some());
        assert!(h.app.chrome_alpha < 0.05, "the panels have faded out");
        h.frame(vec![button(false)]);
        for _ in 0..30 {
            h.frame(Vec::new());
        }
        assert!(h.app.chrome_alpha > 0.99, "and are back once the pen lifts");
    }

    #[test]
    fn a_press_on_the_paper_puts_away_the_open_flyout() {
        let mut h = Harness::new();
        h.app.ui.toggle_case();
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(h.app.ui.case_open);
        let at = pos2(900.0, 600.0);
        h.frame(vec![Event::PointerMoved(at)]);
        h.frame(vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        assert!(!h.app.ui.case_open);
    }

    #[test]
    fn a_new_drawing_uses_the_paper_color_from_the_settings() {
        let mut app = InkApp::blank(Options::default());
        app.config.canvas.paper = [0x10, 0x20, 0x30];
        app.new_document();
        assert_eq!(
            app.editor.document().metadata.background_solid_color,
            "#102030"
        );
    }

    #[test]
    fn a_damaged_settings_file_is_set_aside_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("inkwell-app-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        let path = dir.join("config.json");
        std::fs::write(&path, "{ not json").expect("written");
        let mut app = InkApp::blank(Options {
            config: Some(path.clone()),
            ..Options::default()
        });
        app.load_config(&Context::default());
        assert_eq!(app.config, Config::default());
        assert!(!path.exists());
        assert!(dir.join("config.json.broken").exists());
        assert!(app.message.is_some(), "the user is told");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_settings_are_loaded_at_start() {
        let dir = std::env::temp_dir().join(format!("inkwell-app-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        let path = dir.join("config.json");
        let mut config = Config::default();
        config.colors.current = [1, 2, 3];
        config.colors.last_brush = "watercolor".to_owned();
        config.interface.show_layers = false;
        config.canvas.paper = [0, 0, 0];
        config.save(&path).expect("saved");
        let mut app = InkApp::blank(Options {
            config: Some(path),
            ..Options::default()
        });
        app.load_config(&Context::default());
        assert_eq!(app.tool.pen, Pen::Watercolor);
        assert_eq!(app.tool.color.to_srgb(), [1, 2, 3]);
        assert!(!app.ui.layers_open);
        assert_eq!(
            app.editor.document().metadata.background_solid_color,
            "#000000"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_eraser_removes_what_it_touches_and_a_sweep_is_one_undo() {
        let mut h = Harness::new();
        h.drag(&[(400.0, 300.0), (500.0, 300.0), (600.0, 300.0)]);
        h.drag(&[(400.0, 340.0), (500.0, 340.0), (600.0, 340.0)]);
        h.drag(&[(400.0, 500.0), (500.0, 500.0), (600.0, 500.0)]);
        assert_eq!(h.strokes(), 3);
        h.app.tool.mode = Mode::Erase;
        h.drag(&[(500.0, 280.0), (500.0, 320.0), (500.0, 360.0)]);
        assert_eq!(h.strokes(), 1, "only the two strokes it crossed are gone");
        assert!(h.app.editor.undo());
        assert_eq!(h.strokes(), 3, "one undo brings both back");
    }

    #[test]
    fn the_eraser_leaves_untouched_strokes_alone() {
        let mut h = Harness::new();
        h.drag(&[(400.0, 300.0), (500.0, 300.0), (600.0, 300.0)]);
        h.app.tool.mode = Mode::Erase;
        h.drag(&[(400.0, 500.0), (500.0, 520.0), (600.0, 540.0)]);
        assert_eq!(h.strokes(), 1);
    }

    #[test]
    fn choosing_a_pen_puts_it_on_the_rail() {
        let mut app = InkApp::blank(Options::default());
        app.select_pen(Pen::Airbrush);
        assert!(app.config.rail_pens().contains(&Pen::Airbrush));
        assert_eq!(app.config.colors.last_brush, "airbrush");
    }

    /// Draws a wobbly line with `pen` through the real canvas input, and returns what the canvas was showing
    /// the moment before the pen lifted, with the stroke that ended up in the document.
    fn draw_and_compare(pen: Pen) -> (ink_core::Stroke, ink_core::Stroke) {
        let mut h = Harness::new();
        h.app.select_pen(pen);
        let at = |i: usize| {
            let t = i as f32;
            (
                320.0 + t * 7.0,
                380.0 + (t / 4.0).sin() * 70.0 + (t * 1.3).cos() * 9.0,
            )
        };
        let (x, y) = at(0);
        h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        h.frame(vec![Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        for i in 1..90 {
            let (x, y) = at(i);
            h.frame(vec![Event::PointerMoved(pos2(x, y))]);
        }
        let shown = h
            .app
            .builder
            .as_ref()
            .expect("the stroke is still being drawn")
            .snapshot();
        let (x, y) = at(89);
        h.frame(vec![Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        (shown, first_stroke(&h.app).clone())
    }

    #[test]
    fn a_stroke_does_not_change_when_the_pen_is_lifted() {
        for pen in [
            Pen::Pen,
            Pen::Fountain,
            Pen::DynamicPen,
            Pen::Wire,
            Pen::Marker,
            Pen::Watercolor,
            Pen::Dotted,
        ] {
            let (shown, kept) = draw_and_compare(pen);
            assert_eq!(shown.id, kept.id, "{pen:?} kept its identity");
            assert_eq!(
                shown.samples, kept.samples,
                "{pen:?} changed shape on pen-up"
            );
            assert_eq!(shown.pressure, kept.pressure, "{pen:?}");
        }
    }

    #[test]
    fn textured_pens_look_exactly_the_same_before_and_after_the_pen_is_lifted() {
        for pen in [
            Pen::SoftPencil,
            Pen::HardPencil,
            Pen::Airbrush,
            Pen::Marker,
            Pen::Watercolor,
        ] {
            let (shown, kept) = draw_and_compare(pen);
            let before = ink_brush::batch_for(&shown, &crate::render::smoothed(&shown));
            let after = ink_brush::batch_for(&kept, &crate::render::smoothed(&kept));
            assert!(!before.stamps.is_empty(), "{pen:?} drew something");
            assert_eq!(
                before, after,
                "{pen:?}: the stamps (their scatter, turn and texture tile) changed on pen-up"
            );
        }
    }

    fn notes_in(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("inkwell-app-notes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    fn idle(h: &mut Harness, frames: u32) {
        for _ in 0..frames {
            h.frame(Vec::new());
        }
    }

    #[test]
    fn a_new_note_has_a_place_in_the_library_folder_but_no_file_until_it_is_drawn() {
        let folder = notes_in("new");
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        h.app.start_note();
        let path = h.app.path.clone().expect("a place was chosen");
        assert_eq!(path.parent(), Some(folder.as_path()));
        idle(&mut h, 120);
        assert!(!path.exists(), "an empty note leaves no file behind");
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        assert!(h.app.is_dirty());
        assert!(!path.exists(), "it waits a moment after the last change");
        idle(&mut h, 120);
        assert!(path.exists(), "and then saves by itself");
        assert!(!h.app.is_dirty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_from_outside_the_library_folder_is_never_saved_by_itself() {
        let folder = notes_in("outside");
        let elsewhere = notes_in("elsewhere");
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        h.app.path = Some(elsewhere.join("Theirs.ink"));
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        idle(&mut h, 200);
        assert!(h.app.is_dirty());
        assert!(!elsewhere.join("Theirs.ink").exists());
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn autosave_can_be_turned_off() {
        let folder = notes_in("off");
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        h.app.config.files.autosave = false;
        h.app.start_note();
        assert_eq!(h.app.path, None);
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        idle(&mut h, 200);
        assert!(h.app.is_dirty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn opening_another_note_first_saves_the_one_on_the_paper() {
        let folder = notes_in("switch");
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        h.app.start_note();
        let first = h.app.path.clone().expect("a place");
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        // Another note is waiting in the folder.
        let mut other = InkApp::blank(Options::default());
        let other_path = folder.join("Other.ink");
        other.write_file(&other_path).expect("written");
        let ctx = h.ctx.clone();
        h.app.open_note(&ctx, &other_path);
        assert!(
            first.exists(),
            "the first note was saved before it was left"
        );
        assert_eq!(h.app.path, Some(other_path));
        assert!(!h.app.ui.library_open);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_notes_page_covers_the_paper_and_the_keyboard_toggles_it() {
        let folder = notes_in("page");
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        let press = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        h.frame(vec![press(Key::L, Modifiers::COMMAND)]);
        assert!(h.app.ui.library_open);
        idle(&mut h, 20);
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        assert_eq!(h.strokes(), 0, "nothing is drawn through the page");
        h.frame(vec![press(Key::Escape, Modifiers::NONE)]);
        assert!(!h.app.ui.library_open);
        h.frame(vec![press(Key::L, Modifiers::COMMAND)]);
        h.frame(vec![press(Key::L, Modifiers::COMMAND)]);
        assert!(!h.app.ui.library_open);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_notes_page_lists_what_is_in_the_folder() {
        let folder = notes_in("list");
        let mut writer = InkApp::blank(Options::default());
        for name in ["One", "Two"] {
            writer
                .write_file(&folder.join(format!("{name}.ink")))
                .expect("written");
        }
        let mut h = Harness::new();
        h.app.config.files.library_folder = Some(folder.display().to_string());
        let ctx = h.ctx.clone();
        h.app.open_library(&ctx);
        for _ in 0..200 {
            h.frame(Vec::new());
            if h.app.library.count() == 2 && !h.app.library.is_scanning() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(h.app.library.count(), 2);
        h.app.library.query = "tw".to_owned();
        h.frame(Vec::new());
        assert_eq!(h.app.library.search_results().1.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_new_document_is_blank() {
        let mut h = Harness::new();
        h.drag(&[(400.0, 300.0), (500.0, 340.0), (600.0, 380.0)]);
        h.app.new_document();
        assert_eq!(h.strokes(), 0);
        assert!(!h.app.editor.can_undo());
    }

    /// A press and a release at `at`, in two frames.
    fn click(h: &mut Harness, at: egui::Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(at)]);
        h.frame(vec![button(true)]);
        h.frame(vec![button(false)]);
    }

    /// Where the middle of the selection is on the screen, and the selection put away, the tool being the pen.
    fn leave_it_alone(h: &mut Harness) -> egui::Pos2 {
        let [x0, y0, x1, y1] =
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("something");
        let at = h.app.view.to_screen(ink_core::Point::new(
            f32::midpoint(x0, x1),
            f32::midpoint(y0, y1),
        ));
        h.app.selection.clear();
        h.app.tool.mode = Mode::Draw;
        h.frame(Vec::new());
        at
    }

    #[test]
    fn a_double_touch_on_a_filled_shape_opens_it_for_words_even_with_the_pen_in_use() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        h.app.config.shapes.fill = Some([240, 200, 60]);
        assert!(h.app.insert_shape(&ctx, "circle"));
        let at = leave_it_alone(&mut h);
        let before = h.strokes();
        click(&mut h, at);
        click(&mut h, at);
        let entry = h.app.ui.text.as_ref().expect("the words are open");
        assert!(!entry.placing && !entry.editing, "written inside the shape");
        assert_eq!(h.app.selection.len(), before, "the whole shape is picked");
        assert_eq!(h.strokes(), before, "no dot was left by the first touch");
    }

    #[test]
    fn a_double_touch_inside_an_outline_with_nothing_in_it_opens_it_too() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        assert!(h.app.insert_shape(&ctx, "square"));
        let at = leave_it_alone(&mut h);
        click(&mut h, at);
        click(&mut h, at);
        assert!(h.app.ui.text.is_some(), "the middle of an empty square");
    }

    #[test]
    fn a_double_touch_on_words_opens_them_to_be_changed() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        assert!(h.app.write_text(&ctx, "Hello"));
        let at = leave_it_alone(&mut h);
        click(&mut h, at);
        click(&mut h, at);
        let entry = h.app.ui.text.as_ref().expect("the words are open");
        assert!(entry.editing);
        assert_eq!(entry.text, "Hello");
    }

    #[test]
    fn a_double_touch_on_words_inside_a_shape_changes_the_words_not_the_shape() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        h.app.config.shapes.fill = Some([240, 200, 60]);
        assert!(h.app.insert_shape(&ctx, "rectangle"));
        assert!(h.app.write_text(&ctx, "Hi"));
        let at = leave_it_alone(&mut h);
        click(&mut h, at);
        click(&mut h, at);
        let entry = h.app.ui.text.as_ref().expect("the words are open");
        assert!(entry.editing, "the words that were there");
        assert_eq!(entry.text, "Hi");
    }

    #[test]
    fn two_touches_on_empty_paper_are_two_dots_and_open_nothing() {
        let mut h = Harness::new();
        let at = pos2(300.0, 300.0);
        click(&mut h, at);
        click(&mut h, at);
        assert!(h.app.ui.text.is_none());
        assert_eq!(h.strokes(), 2, "both dots stay");
    }

    #[test]
    fn two_slow_touches_on_a_shape_are_not_a_double_touch() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        h.app.config.shapes.fill = Some([240, 200, 60]);
        assert!(h.app.insert_shape(&ctx, "circle"));
        let at = leave_it_alone(&mut h);
        click(&mut h, at);
        for _ in 0..60 {
            h.frame(Vec::new()); // a whole second
        }
        click(&mut h, at);
        assert!(h.app.ui.text.is_none());
    }

    #[test]
    fn the_eraser_is_left_to_erase_even_on_a_double_touch() {
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        h.app.config.shapes.fill = Some([240, 200, 60]);
        assert!(h.app.insert_shape(&ctx, "circle"));
        let at = leave_it_alone(&mut h);
        h.app.tool.mode = Mode::Erase;
        click(&mut h, at);
        click(&mut h, at);
        assert!(h.app.ui.text.is_none());
    }

    #[test]
    fn a_double_touch_on_a_plain_line_is_left_to_the_pen() {
        let mut h = Harness::new();
        h.drag(&[(300.0, 300.0), (400.0, 300.0), (500.0, 300.0)]);
        let before = h.strokes();
        let on_it = pos2(400.0, 300.0);
        click(&mut h, on_it);
        click(&mut h, on_it);
        assert!(h.app.ui.text.is_none(), "a line is not a figure");
        assert_eq!(h.strokes(), before + 2, "the two touches are two dots");
    }

    /// A harness with the drawer of shapes open on the right, and the middle of its first tile.
    fn with_the_drawer() -> (Harness, egui::Pos2) {
        let mut h = Harness::new();
        h.app.config.interface.shapes_drawer = crate::config::DrawerSide::Right;
        h.frame(Vec::new());
        h.frame(Vec::new());
        let drawer = h.app.ui.anchors.drawer.expect("the drawer is open");
        // The margin, the header, and then the first tile.
        let tile = pos2(
            drawer.left() + 12.0 + 30.0,
            drawer.top() + 12.0 + 40.0 + 30.0,
        );
        (h, tile)
    }

    #[test]
    fn the_drawer_of_shapes_stays_where_it_was_put_and_keeps_the_paper_to_the_rest() {
        let (h, _) = with_the_drawer();
        let drawer = h.app.ui.anchors.drawer.expect("open");
        let paper = h.app.canvas_rect.expect("paper");
        assert!(paper.contains_rect(drawer), "inside the paper");
        assert!(
            h.app.chrome.iter().any(|r| r.intersects(drawer)),
            "it counts as a panel"
        );
    }

    #[test]
    fn tapping_a_shape_in_the_drawer_puts_it_on_the_paper_away_from_the_drawer() {
        let (mut h, tile) = with_the_drawer();
        click(&mut h, tile);
        assert!(h.strokes() >= 1, "a shape landed");
        let drawer = h.app.ui.anchors.drawer.expect("open");
        let [_, _, x1, _] =
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("box");
        let right_edge = h.app.view.to_screen(ink_core::Point::new(x1, 0.0)).x;
        assert!(
            right_edge <= drawer.left() + 1.0,
            "clear of the drawer: {right_edge}"
        );
    }

    #[test]
    fn dragging_a_shape_out_of_the_drawer_lets_it_go_where_the_pointer_is() {
        let (mut h, tile) = with_the_drawer();
        let target = pos2(400.0, 400.0);
        h.frame(vec![Event::PointerMoved(tile)]);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![button(tile, true)]);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let at = pos2(
                tile.x + (target.x - tile.x) * t,
                tile.y + (target.y - tile.y) * t,
            );
            h.frame(vec![Event::PointerMoved(at)]);
        }
        assert!(h.app.ui.objects.dragging.is_some(), "it is being carried");
        h.frame(vec![button(target, false)]);
        assert!(h.app.ui.objects.dragging.is_none());
        assert!(h.strokes() >= 1, "it was let go");
        let [x0, y0, x1, y1] =
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("box");
        let middle = h.app.view.to_screen(ink_core::Point::new(
            f32::midpoint(x0, x1),
            f32::midpoint(y0, y1),
        ));
        assert!(
            middle.distance(target) < 3.0,
            "{middle:?} against {target:?}"
        );
    }

    #[test]
    fn a_shape_let_go_on_the_drawer_again_is_not_put_down() {
        let (mut h, tile) = with_the_drawer();
        let back = pos2(tile.x, tile.y + 120.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(tile)]);
        h.frame(vec![button(tile, true)]);
        h.frame(vec![Event::PointerMoved(pos2(
            tile.x - 30.0,
            tile.y + 40.0,
        ))]);
        h.frame(vec![Event::PointerMoved(back)]);
        h.frame(vec![button(back, false)]);
        assert_eq!(h.strokes(), 0, "nothing was put down");
        assert!(h.app.ui.objects.dragging.is_none());
    }

    #[test]
    fn the_drawer_is_closed_by_its_button_and_remembered_in_the_settings() {
        let (mut h, _) = with_the_drawer();
        h.app.config.interface.shapes_drawer = crate::config::DrawerSide::Off;
        h.frame(Vec::new());
        assert!(h.app.ui.anchors.drawer.is_none());
        let text = serde_json::to_string(&h.app.config).expect("written");
        let back: crate::config::Config = serde_json::from_str(&text).expect("read");
        assert_eq!(back.interface.shapes_drawer, crate::config::DrawerSide::Off);
    }

    #[test]
    fn docking_the_shapes_opens_the_drawer_and_puts_the_menu_away() {
        let mut h = Harness::new();
        h.app.ui.toggle_shapes();
        h.app.dock_shapes();
        assert_eq!(
            h.app.config.interface.shapes_drawer,
            crate::config::DrawerSide::Right
        );
        assert!(!h.app.ui.shapes_open);
    }

    #[test]
    fn a_shape_is_let_go_where_the_finger_was_even_though_the_pointer_is_gone_as_it_lifts() {
        let (mut h, tile) = with_the_drawer();
        let target = pos2(420.0, 380.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.frame(vec![Event::PointerMoved(tile)]);
        h.frame(vec![button(tile, true)]);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let at = pos2(
                tile.x + (target.x - tile.x) * t,
                tile.y + (target.y - tile.y) * t,
            );
            h.frame(vec![Event::PointerMoved(at)]);
        }
        h.frame(vec![button(target, false), Event::PointerGone]);
        h.frame(Vec::new());
        assert!(h.app.ui.objects.dragging.is_none(), "not left carrying");
        assert!(h.strokes() >= 1, "it was put down");
        let [x0, y0, x1, y1] =
            crate::tools::selection_bounds(&h.app.editor, &h.app.selection).expect("box");
        let middle = h.app.view.to_screen(ink_core::Point::new(
            f32::midpoint(x0, x1),
            f32::midpoint(y0, y1),
        ));
        assert!(
            middle.distance(target) < 3.0,
            "{middle:?} against {target:?}"
        );
    }
}
