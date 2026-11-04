//! Running commands: the keyboard looks a press up in the keymap (see `shortcuts.rs`), and everything a command
//! does is here, so a button, a menu and a key all do the same thing.

use eframe::egui::{self, Modifiers};

use crate::app::InkApp;
use crate::config::{BrushConfig, EraserKind, TapAction, ToolsConfig};
use crate::settings::Section;
use crate::shortcuts::{Binding, Command, Keymap, Scope};
use crate::state::{Mode, ViewState};
use crate::{canvas, library_page, selection};

/// How much one press of the bigger or smaller brush key changes the size.
const SIZE_STEP: f32 = 1.15;
/// How much one zoom key press zooms.
const ZOOM_STEP: f32 = 1.25;

impl InkApp {
    /// Reads the keys from the settings again, after they changed.
    pub(crate) fn rebuild_keymap(&mut self) {
        self.keymap = Keymap::new(&self.config.shortcuts);
    }

    /// The key presses of this frame: finds the commands they run and runs them. Nothing happens if the keyboard
    /// has been turned off in the settings.
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.config.workspace.keyboard_shortcuts {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let settings = self.ui.settings.is_some();
        let library = self.ui.library_open;
        let renaming = self.library.renaming.is_some();
        let capturing = self.ui.capture.is_some();
        let renaming_layer = self.ui.renaming.is_some();
        let allowed = move |command: Command, binding: Binding| {
            if capturing {
                return false;
            }
            match command.scope() {
                Scope::App => !(typing && binding.is_typed()),
                Scope::Canvas => !typing && !settings && !library,
                Scope::Cancel => {
                    if library {
                        !renaming
                    } else {
                        renaming_layer || (!typing && !settings)
                    }
                }
            }
        };
        let keymap = &self.keymap;
        let commands = ctx.input_mut(|input| keymap.take(input, allowed));
        for (command, modifiers) in commands {
            self.run_command(ctx, command, modifiers);
        }
    }

    /// Opens the library of drawings, or goes back to the drawing if it is open.
    fn toggle_library(&mut self, ctx: &egui::Context) {
        if self.ui.library_open {
            self.close_library();
        } else {
            self.open_library(ctx);
        }
    }

    /// Opens the settings window, or puts it away if it is open.
    fn toggle_settings(&mut self) {
        if self.ui.settings.is_some() {
            self.ui.settings = None;
        } else {
            self.open_settings(Section::Brushes);
        }
    }

    /// Does what `command` says. `modifiers` are the keys that were held, which change how far a nudge goes.
    pub(crate) fn run_command(
        &mut self,
        ctx: &egui::Context,
        command: Command,
        modifiers: Modifiers,
    ) {
        use Command as C;
        if self.run_indexed_command(ctx, command) {
            return;
        }
        match command {
            C::NewTab => self.new_tab(ctx),
            C::Open => self.request_open(ctx),
            C::Save => self.save(ctx),
            C::SaveAs => self.save_as(ctx),
            C::CloseTab => {
                if let Some(active) = self.active {
                    self.close_tab(ctx, active);
                }
            }
            C::NextTab => self.cycle_tab(ctx, 1),
            C::PreviousTab => self.cycle_tab(ctx, -1),
            C::Undo => {
                self.undo();
            }
            C::Redo => {
                self.redo();
            }
            C::Cancel => self.cancel(),
            C::ToolDraw => self.set_mode(Mode::Draw),
            C::ToolSelect => self.toggle_mode(Mode::Select),
            C::ToolSlice => self.toggle_mode(Mode::Slice),
            C::ToolNudge => self.toggle_mode(Mode::Nudge),
            C::PreviousTool => self.set_mode(self.previous_mode),
            C::ToolEraser => self.toggle_mode(Mode::Erase),
            C::ToolSoftEraser => self.choose_eraser(EraserKind::Soft),
            C::ToolHardEraser => self.choose_eraser(EraserKind::Hard),
            C::ToolPan => self.toggle_mode(Mode::Pan),
            C::ZoomIn => self.zoom_by(ctx, ZOOM_STEP),
            C::ZoomOut => self.zoom_by(ctx, 1.0 / ZOOM_STEP),
            C::ZoomFit => self.fit_pending = true,
            C::TurnLeft | C::TurnRight | C::TurnReset | C::TurnTo => {
                self.run_turn_command(ctx, command);
            }
            C::NewLayer => self.new_layer(),
            C::DuplicateLayer => self.duplicate_active_layer(),
            C::LayerUp => self.step_active_layer(true),
            C::LayerDown => self.step_active_layer(false),
            C::BrushBigger => self.scale_brush(SIZE_STEP),
            C::BrushSmaller => self.scale_brush(1.0 / SIZE_STEP),
            C::ToggleSnap => self.toggle_snap(ctx),
            C::ToggleRecognition => self.toggle_recognition(ctx),
            C::OpenPrecision => self.ui.toggle_precision(),
            C::HidePanels => self.ui.panels_hidden = !self.ui.panels_hidden,
            C::OpenBrushes => self.ui.toggle_case(),
            C::OpenShapes => self.ui.toggle_shapes(),
            C::WriteText => self.toggle_text(ctx),
            C::OpenLayers => self.ui.toggle_layers(),
            C::OpenLibrary => self.toggle_library(ctx),
            C::OpenSettings => self.toggle_settings(),
            C::SelectAll
            | C::Delete
            | C::Duplicate
            | C::Copy
            | C::Cut
            | C::Paste
            | C::FlipHorizontal
            | C::FlipVertical
            | C::MoveLeft
            | C::MoveRight
            | C::MoveUp
            | C::MoveDown
            | C::GroupSelection
            | C::UngroupSelection
            | C::LockSelection
            | C::UnlockAll
            | C::SelectionLayerUp
            | C::SelectionLayerDown => self.run_selection_command(command, modifiers),
            // The numbered commands were handled above, and holding a key to move the canvas is not something
            // that is pressed: see `hold_pan_active`.
            C::HoldPan
            | C::Tab1
            | C::Tab2
            | C::Tab3
            | C::Tab4
            | C::Tab5
            | C::Tab6
            | C::Tab7
            | C::Tab8
            | C::Tab9
            | C::RailPen1
            | C::RailPen2
            | C::RailPen3
            | C::RailPen4
            | C::RailPen5
            | C::RailPen6
            | C::RailPen7
            | C::RailPen8
            | C::GuideLine
            | C::GuideArc
            | C::GuideAngle
            | C::GuideCircle
            | C::GuideSquare => {}
        }
    }

    /// The commands that work on the selected strokes.
    pub(crate) fn run_selection_command(&mut self, command: Command, modifiers: Modifiers) {
        use Command as C;
        let tools_config = self.config.tools.clone();
        let nudge = |step: f32| {
            if modifiers.shift {
                tools_config.arrow_large
            } else if modifiers.alt {
                tools_config.arrow_small
            } else {
                step
            }
        };
        let distance = nudge(tools_config.arrow_step);
        match command {
            C::SelectAll => {
                self.tool.mode = Mode::Select;
                self.gesture = None;
                selection::select_all(&self.editor, &mut self.selection);
            }
            C::Delete => {
                selection::delete(&mut self.editor, &mut self.selection);
            }
            C::Duplicate => {
                selection::duplicate(
                    &mut self.editor,
                    &mut self.selection,
                    tools_config.duplicate_offset,
                );
            }
            C::Copy => self.copy_selection(),
            C::Cut => {
                self.copy_selection();
                selection::delete(&mut self.editor, &mut self.selection);
            }
            C::Paste => self.paste_clipboard(),
            C::FlipHorizontal => {
                selection::flip(&mut self.editor, &self.selection, true);
            }
            C::FlipVertical => {
                selection::flip(&mut self.editor, &self.selection, false);
            }
            C::MoveLeft => {
                selection::move_by(&mut self.editor, &self.selection, -distance, 0.0);
            }
            C::MoveRight => {
                selection::move_by(&mut self.editor, &self.selection, distance, 0.0);
            }
            C::MoveUp => {
                selection::move_by(&mut self.editor, &self.selection, 0.0, distance);
            }
            C::MoveDown => {
                selection::move_by(&mut self.editor, &self.selection, 0.0, -distance);
            }
            C::GroupSelection => {
                selection::group(&mut self.editor, &self.selection);
            }
            C::UngroupSelection => {
                selection::ungroup(&mut self.editor, &self.selection);
            }
            C::LockSelection => {
                selection::lock(&mut self.editor, &mut self.selection);
            }
            C::UnlockAll => {
                selection::unlock_all(&mut self.editor);
            }
            C::SelectionLayerUp => {
                selection::move_to_layer(&mut self.editor, &self.selection, 1);
            }
            C::SelectionLayerDown => {
                selection::move_to_layer(&mut self.editor, &self.selection, -1);
            }
            _ => {}
        }
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.abandon_gesture();
        let done = self.editor.undo();
        self.fix_active_layer();
        done
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.abandon_gesture();
        let done = self.editor.redo();
        self.fix_active_layer();
        done
    }

    /// `true` while the key for moving the canvas is held and nothing else is going on that it would interrupt:
    /// a stroke, an erase or a drag of the select, slice or nudge tool already under way, text being typed, or a
    /// window covering the paper.
    pub(crate) fn hold_pan_active(&self, ctx: &egui::Context) -> bool {
        if !self.config.workspace.keyboard_shortcuts
            || self.ui.settings.is_some()
            || self.ui.library_open
            || self.ui.capture.is_some()
            || self.builder.is_some()
            || self.eraser.is_some()
            || self.gesture.is_some()
            || ctx.egui_wants_keyboard_input()
        {
            return false;
        }
        ctx.input(|i| {
            self.keymap.bindings(Command::HoldPan).any(|b| {
                i.key_down(b.key)
                    && i.modifiers.command == b.command
                    && i.modifiers.alt == b.alt
                    && i.modifiers.shift == b.shift
            })
        })
    }

    /// Taps with several fingers at once: does what the settings say for that many fingers. Ignored while a
    /// stroke, an erase or a drag is going on or has just ended (a hand resting on the screen while the pen draws
    /// is not a tap), and while a window covers the paper.
    pub(crate) fn handle_taps(&mut self, ctx: &egui::Context) {
        let (events, now) = ctx.input(|i| (i.events.clone(), i.time));
        let busy = self.builder.is_some() || self.eraser.is_some() || self.gesture.is_some();
        let revision = self.editor.revision();
        if busy || revision != self.taps_revision {
            self.taps_revision = revision;
            self.no_taps_until = now + 0.6;
        }
        let fingers = self.taps.update(&events, now);
        if let Some(at) = self.taps.take_double() {
            self.double_tap_at(at);
        }
        let Some(fingers) = fingers else {
            return;
        };
        if now < self.no_taps_until || self.ui.library_open || self.ui.settings.is_some() {
            return;
        }
        let action = self.config.input.tap_action(fingers);
        self.run_tap_action(action);
        // What the tap itself changed (an undo, say) is not a stroke, so it does not hold the next tap back.
        self.taps_revision = self.editor.revision();
    }

    /// A finger tapped twice at `at` on the screen: if that is on words or on a figure of the drawing they are opened for
    /// writing, as a double click does. Fingers that draw have their double touch noticed with the pen's, on the paper.
    fn double_tap_at(&mut self, at: egui::Pos2) {
        let on_paper = self.canvas_rect.is_some_and(|r| r.contains(at))
            && !self.chrome.iter().any(|area| area.contains(at));
        let free = !self.ui.library_open
            && self.ui.settings.is_none()
            && !self.ui.any_popover()
            && !self.ui.layers_open
            && self.ui.text.is_none()
            && matches!(self.tool.mode, Mode::Draw | Mode::Select);
        if !on_paper || !free || self.builder.is_some() {
            return;
        }
        let radius = 10.0 / self.view.zoom;
        if let Some(ids) =
            crate::text_ops::words_hit(&self.editor, self.view.to_canvas(at), radius, None)
        {
            self.selection = ids;
            self.set_mode(Mode::Select);
            self.begin_text();
        }
    }

    pub(crate) fn run_tap_action(&mut self, action: TapAction) {
        match action {
            TapAction::None => {}
            TapAction::Undo => {
                self.undo();
            }
            TapAction::Redo => {
                self.redo();
            }
            TapAction::HidePanels => self.ui.panels_hidden = !self.ui.panels_hidden,
            TapAction::ZoomToFit => self.fit_pending = true,
            TapAction::PreviousTool => self.set_mode(self.previous_mode),
        }
    }

    /// Runs a command that is one of a numbered set (a tab, a pen of the rail, a kind of guide). Returns `false`
    /// for any other command.
    fn run_indexed_command(&mut self, ctx: &egui::Context, command: Command) -> bool {
        if let Some(n) = command.tab_number() {
            self.goto_tab(ctx, n);
        } else if let Some(slot) = command.rail_slot() {
            self.pick_rail_pen(slot);
        } else if let Some(kind) = command.guide_kind() {
            self.toggle_guide(kind);
        } else {
            return false;
        }
        true
    }

    /// Puts a shape guide of `kind` in the middle of the paper, or takes it away if there is one. The pen is
    /// the tool guides work with, so it is chosen.
    pub(crate) fn toggle_guide(&mut self, kind: ink_core::guide::Kind) {
        let Some(rect) = self.canvas_rect else { return };
        let size = rect.width().min(rect.height()) * 0.5 / self.view.zoom;
        // Each guide is put a little way along from the one before, so several can be told apart.
        let nudge = self.guides.list.len() as f32 * size * 0.12;
        let middle = self.view.to_canvas(rect.center());
        let center = ink_core::Point::new(middle.x + nudge, middle.y - nudge);
        if self.guides.toggle(kind, center, size) {
            self.set_mode(Mode::Draw);
        }
    }

    /// Picks the eraser of this kind. If it is the one already in use, it is put away, as the eraser key does.
    fn choose_eraser(&mut self, kind: EraserKind) {
        if self.tool.mode == Mode::Erase && self.config.tools.eraser_kind == kind {
            self.toggle_mode(Mode::Erase);
            return;
        }
        self.config.tools.eraser_kind = kind;
        self.mark_config_changed();
        self.set_mode(Mode::Erase);
    }

    /// Turns shape recognition on or off, and says which.
    fn toggle_recognition(&mut self, ctx: &egui::Context) {
        let on = !self.config.precision.recognize_shapes;
        self.config.precision.recognize_shapes = on;
        self.mark_config_changed();
        self.notify(
            ctx,
            if on {
                "Shape recognition on"
            } else {
                "Shape recognition off"
            },
        );
    }

    /// Turns snapping on or off, and says which.
    fn toggle_snap(&mut self, ctx: &egui::Context) {
        let on = !self.config.precision.snap;
        self.config.precision.snap = on;
        self.mark_config_changed();
        self.notify(ctx, if on { "Snap on" } else { "Snap off" });
    }

    /// Stops a drag of the select, slice or nudge tool. The drawing is only changed when the pen is lifted, so
    /// there is nothing to put back.
    pub(crate) fn abandon_gesture(&mut self) {
        self.gesture = None;
    }

    /// The cancel key: undoes a drag in progress, else closes the open flyout, else drops the selection, else
    /// leaves the select and slice tools.
    pub(crate) fn cancel(&mut self) {
        if self.ui.library_open {
            library_page::escape(self);
        } else if self.ui.renaming.take().is_some() {
            // A layer name was being typed; leave it as it was.
        } else if self.gesture.is_some() {
            self.abandon_gesture();
        } else if self.ui.any_popover() {
            self.ui.close_popovers();
        } else if !self.selection.is_empty() {
            self.selection.clear();
        } else if matches!(self.tool.mode, Mode::Select | Mode::Slice | Mode::Nudge) {
            self.set_mode(Mode::Draw);
        }
    }

    pub(crate) fn set_mode(&mut self, mode: Mode) {
        self.abandon_gesture();
        if mode != self.tool.mode {
            self.previous_mode = self.tool.mode;
        }
        self.tool.mode = mode;
    }

    /// Switches to `mode`, or back to drawing if that is the tool already in use.
    pub(crate) fn toggle_mode(&mut self, mode: Mode) {
        let next = if self.tool.mode == mode {
            Mode::Draw
        } else {
            mode
        };
        self.set_mode(next);
    }

    /// A pen was tapped in a panel of tools: the one in use opens its size and opacity, any other becomes the one in
    /// use.
    pub(crate) fn choose_pen(&mut self, pen: ink_core::Pen) {
        if self.tool.mode == Mode::Draw && self.tool.pen == pen {
            self.ui.toggle_dial();
        } else {
            self.select_pen(pen);
            self.set_mode(Mode::Draw);
            self.ui.close_popovers();
        }
    }

    /// A tool was tapped in a panel of tools. The hand switches on and off; any other tool opens its options when it
    /// is the one in use, and becomes the one in use when it is not.
    pub(crate) fn choose_tool(&mut self, mode: Mode) {
        if mode == Mode::Pan {
            self.toggle_mode(Mode::Pan);
        } else if self.tool.mode == mode {
            self.ui.toggle_dial();
        } else {
            self.set_mode(mode);
            self.ui.close_popovers();
        }
    }

    fn pick_rail_pen(&mut self, slot: usize) {
        if let Some(pen) = self.config.rail_pens().get(slot).copied() {
            self.select_pen(pen);
            self.set_mode(Mode::Draw);
        }
    }

    /// Turns the paper a step to one side or back upright, about the middle of the window, if turning is on; or
    /// opens the panel where the angle is typed.
    fn run_turn_command(&mut self, ctx: &egui::Context, command: Command) {
        if !self.config.input.rotation_enabled {
            return;
        }
        if command == Command::TurnTo {
            self.ui.toggle_turn();
            return;
        }
        let by = match command {
            Command::TurnLeft => ViewState::ANGLE_STEP.to_radians(),
            Command::TurnRight => -ViewState::ANGLE_STEP.to_radians(),
            _ => -self.view.angle,
        };
        let center = self
            .canvas_rect
            .map_or_else(|| ctx.content_rect().center(), |r| r.center());
        self.view.rotate_at(center, by);
        canvas::note_turning(ctx);
    }

    fn zoom_by(&mut self, ctx: &egui::Context, factor: f32) {
        if !self.config.input.zoom_enabled {
            return;
        }
        let center = self
            .canvas_rect
            .map_or_else(|| ctx.content_rect().center(), |r| r.center());
        self.view.zoom_at(center, factor);
    }

    /// Makes the brush in use bigger or smaller, or the eraser while it is the tool.
    fn scale_brush(&mut self, factor: f32) {
        let grow = factor > 1.0;
        let step = |size: f32| {
            if grow {
                (size * factor).max(size + 0.5)
            } else {
                (size * factor).min(size - 0.5)
            }
        };
        if self.tool.mode == Mode::Erase {
            let (low, high) = ToolsConfig::ERASER_RANGE;
            let radius = &mut self.config.tools.eraser_radius;
            *radius = step(*radius).clamp(low, high);
        } else {
            let pen = self.tool.pen;
            let brush = self.config.brush_mut(pen);
            brush.size = step(brush.size).clamp(BrushConfig::MIN_SIZE, BrushConfig::MAX_SIZE);
        }
        self.mark_config_changed();
    }

    fn copy_selection(&mut self) {
        let copied = selection::copy(&self.editor, &self.selection);
        if !copied.is_empty() {
            self.clipboard = copied;
            self.pastes = 0;
        }
    }

    /// Puts the copied strokes on the active layer, a little further down and right each time.
    pub(crate) fn paste_clipboard(&mut self) {
        self.pastes += 1;
        let offset = self.config.tools.duplicate_offset * self.pastes as f32;
        let pasted = selection::paste(
            &mut self.editor,
            self.active_layer,
            &self.clipboard,
            offset,
            &mut self.selection,
        );
        if pasted > 0 {
            self.set_mode(Mode::Select);
        } else {
            self.pastes -= 1;
        }
    }
}
