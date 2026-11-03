//! Every command the keyboard can run, the keys each starts with, and the keys the user has chosen instead.
//!
//! The keys are stored in the settings as text such as `Ctrl+Shift+Z`, by command id, and only for commands the
//! user changed. [`Keymap`] is those settings read once into something quick to look keys up in.

use std::collections::BTreeMap;

use eframe::egui::{Event, InputState, Key, Modifiers};

/// Where a command can be used from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Works from anywhere in the app, even over the Library or the settings.
    App,
    /// Works on the paper: not while typing, and not while the Library or the settings cover it.
    Canvas,
    /// The cancel key: backs out of whatever is open, nearest first.
    Cancel,
}

/// How the settings window groups commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Files,
    Edit,
    Selection,
    Tools,
    View,
    Brushes,
    Layers,
    Precision,
    Panels,
}

impl Group {
    pub const ALL: [Self; 9] = [
        Self::Files,
        Self::Edit,
        Self::Selection,
        Self::Tools,
        Self::View,
        Self::Brushes,
        Self::Layers,
        Self::Precision,
        Self::Panels,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "Notes and tabs",
            Self::Edit => "Edit",
            Self::Selection => "Selection",
            Self::Tools => "Tools",
            Self::View => "View",
            Self::Brushes => "Brushes",
            Self::Layers => "Layers",
            Self::Precision => "Precision",
            Self::Panels => "Panels",
        }
    }
}

macro_rules! commands {
    ($($variant:ident => ($id:literal, $label:literal, $group:ident, $scope:ident, [$($default:literal),*])),* $(,)?) => {
        /// Something the keyboard can do.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Command {
            $($variant),*
        }

        impl Command {
            /// Every command, in the order the settings list them.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// The name the settings file knows the command by. It never changes.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id),*
                }
            }

            /// What the settings window calls it.
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),*
                }
            }

            #[must_use]
            pub const fn group(self) -> Group {
                match self {
                    $(Self::$variant => Group::$group),*
                }
            }

            #[must_use]
            pub const fn scope(self) -> Scope {
                match self {
                    $(Self::$variant => Scope::$scope),*
                }
            }

            /// The keys the command has until the user changes them.
            #[must_use]
            pub const fn defaults(self) -> &'static [&'static str] {
                match self {
                    $(Self::$variant => &[$($default),*]),*
                }
            }
        }
    };
}

commands! {
    NewTab => ("new_tab", "New note", Files, App, ["Ctrl+T", "Ctrl+N"]),
    Open => ("open", "Open a file", Files, App, ["Ctrl+O"]),
    Save => ("save", "Save", Files, App, ["Ctrl+S"]),
    SaveAs => ("save_as", "Save as", Files, App, ["Ctrl+Shift+S"]),
    CloseTab => ("close_tab", "Close the note", Files, App, ["Ctrl+W"]),
    NextTab => ("next_tab", "Next tab", Files, App, ["Ctrl+Tab"]),
    PreviousTab => ("previous_tab", "Previous tab", Files, App, ["Ctrl+Shift+Tab"]),
    Tab1 => ("tab_1", "Go to tab 1", Files, App, ["Ctrl+1"]),
    Tab2 => ("tab_2", "Go to tab 2", Files, App, ["Ctrl+2"]),
    Tab3 => ("tab_3", "Go to tab 3", Files, App, ["Ctrl+3"]),
    Tab4 => ("tab_4", "Go to tab 4", Files, App, ["Ctrl+4"]),
    Tab5 => ("tab_5", "Go to tab 5", Files, App, ["Ctrl+5"]),
    Tab6 => ("tab_6", "Go to tab 6", Files, App, ["Ctrl+6"]),
    Tab7 => ("tab_7", "Go to tab 7", Files, App, ["Ctrl+7"]),
    Tab8 => ("tab_8", "Go to tab 8", Files, App, ["Ctrl+8"]),
    Tab9 => ("tab_9", "Go to tab 9", Files, App, ["Ctrl+9"]),

    Undo => ("undo", "Undo", Edit, Canvas, ["Ctrl+Z"]),
    Redo => ("redo", "Redo", Edit, Canvas, ["Ctrl+Shift+Z", "Ctrl+Y"]),
    Cancel => ("cancel", "Cancel, close, deselect", Edit, Cancel, ["Escape"]),

    SelectAll => ("select_all", "Select everything", Selection, Canvas, ["Ctrl+A"]),
    Delete => ("delete", "Delete the selection", Selection, Canvas, ["Delete", "Backspace"]),
    Duplicate => ("duplicate", "Duplicate the selection", Selection, Canvas, ["Ctrl+D"]),
    Copy => ("copy", "Copy", Selection, Canvas, ["Ctrl+C"]),
    Cut => ("cut", "Cut", Selection, Canvas, ["Ctrl+X"]),
    Paste => ("paste", "Paste", Selection, Canvas, ["Ctrl+V"]),
    FlipHorizontal => ("flip_horizontal", "Flip left to right", Selection, Canvas, ["Shift+H"]),
    FlipVertical => ("flip_vertical", "Flip top to bottom", Selection, Canvas, ["Shift+V"]),
    GroupSelection => ("group", "Group the selection", Selection, Canvas, ["Ctrl+G"]),
    UngroupSelection => ("ungroup", "Break up the group", Selection, Canvas, ["Ctrl+Shift+G"]),
    LockSelection => ("lock", "Lock the selection", Selection, Canvas, ["Ctrl+Shift+L"]),
    UnlockAll => ("unlock_all", "Unlock every stroke", Selection, Canvas, ["Ctrl+Alt+L"]),
    SelectionLayerUp => ("selection_layer_up", "Move the selection to the layer above", Selection, Canvas, ["Ctrl+Alt+Up"]),
    SelectionLayerDown => ("selection_layer_down", "Move the selection to the layer below", Selection, Canvas, ["Ctrl+Alt+Down"]),
    MoveLeft => ("move_left", "Move the selection left", Selection, Canvas, ["Left"]),
    MoveRight => ("move_right", "Move the selection right", Selection, Canvas, ["Right"]),
    MoveUp => ("move_up", "Move the selection up", Selection, Canvas, ["Up"]),
    MoveDown => ("move_down", "Move the selection down", Selection, Canvas, ["Down"]),

    ToolDraw => ("tool_draw", "Draw", Tools, Canvas, ["P"]),
    ToolSelect => ("tool_select", "Select", Tools, Canvas, ["V"]),
    ToolSlice => ("tool_slice", "Slice", Tools, Canvas, ["K"]),
    ToolNudge => ("tool_nudge", "Nudge", Tools, Canvas, ["N"]),
    PreviousTool => ("previous_tool", "Back to the tool before", Tools, Canvas, ["Q"]),
    ToolEraser => ("tool_eraser", "Eraser", Tools, Canvas, ["E"]),
    ToolSoftEraser => ("tool_soft_eraser", "Soft eraser: only what it touches", Tools, Canvas, ["Shift+E"]),
    ToolHardEraser => ("tool_hard_eraser", "Hard eraser: the whole stroke", Tools, Canvas, ["Alt+E"]),
    ToolPan => ("tool_pan", "Move the canvas", Tools, Canvas, ["H"]),
    HoldPan => ("hold_pan", "Hold to move the canvas", Tools, Canvas, ["Space"]),

    ZoomIn => ("zoom_in", "Zoom in", View, Canvas, ["Ctrl+Equals", "Ctrl+Plus"]),
    ZoomOut => ("zoom_out", "Zoom out", View, Canvas, ["Ctrl+Minus"]),
    ZoomFit => ("zoom_fit", "Fit the drawing in the window", View, Canvas, ["Ctrl+0"]),
    TurnLeft => ("turn_left", "Turn the paper counterclockwise", View, Canvas, ["Ctrl+Left"]),
    TurnRight => ("turn_right", "Turn the paper clockwise", View, Canvas, ["Ctrl+Right"]),
    TurnReset => ("turn_reset", "Turn the paper upright", View, Canvas, ["Ctrl+Shift+0"]),
    TurnTo => ("turn_to", "Turn the paper to an angle", View, Canvas, ["Ctrl+Shift+R"]),

    RailPen1 => ("rail_pen_1", "Pen 1 of the rail", Brushes, Canvas, ["1"]),
    RailPen2 => ("rail_pen_2", "Pen 2 of the rail", Brushes, Canvas, ["2"]),
    RailPen3 => ("rail_pen_3", "Pen 3 of the rail", Brushes, Canvas, ["3"]),
    RailPen4 => ("rail_pen_4", "Pen 4 of the rail", Brushes, Canvas, ["4"]),
    RailPen5 => ("rail_pen_5", "Pen 5 of the rail", Brushes, Canvas, ["5"]),
    RailPen6 => ("rail_pen_6", "Pen 6 of the rail", Brushes, Canvas, ["6"]),
    RailPen7 => ("rail_pen_7", "Pen 7 of the rail", Brushes, Canvas, ["7"]),
    RailPen8 => ("rail_pen_8", "Pen 8 of the rail", Brushes, Canvas, ["8"]),
    BrushBigger => ("brush_bigger", "Bigger brush", Brushes, Canvas, ["CloseBracket"]),
    BrushSmaller => ("brush_smaller", "Smaller brush", Brushes, Canvas, ["OpenBracket"]),

    NewLayer => ("new_layer", "New layer", Layers, Canvas, ["Ctrl+Shift+N"]),
    DuplicateLayer => ("duplicate_layer", "Duplicate the layer", Layers, Canvas, ["Ctrl+Shift+D"]),
    LayerUp => ("layer_up", "Move the layer up", Layers, Canvas, ["Ctrl+CloseBracket"]),
    LayerDown => ("layer_down", "Move the layer down", Layers, Canvas, ["Ctrl+OpenBracket"]),

    ToggleSnap => ("toggle_snap", "Snap on or off", Precision, Canvas, ["G"]),
    OpenPrecision => ("open_precision", "Precision", Precision, Canvas, ["Shift+G"]),
    ToggleRecognition => ("toggle_shape_recognition", "Shape recognition on or off", Precision, Canvas, ["Shift+R"]),
    GuideLine => ("guide_line", "Line guide on or off", Precision, Canvas, ["Alt+1"]),
    GuideArc => ("guide_arc", "Arc guide on or off", Precision, Canvas, ["Alt+2"]),
    GuideAngle => ("guide_angle", "Angle guide on or off", Precision, Canvas, ["Alt+3"]),
    GuideCircle => ("guide_circle", "Circle guide on or off", Precision, Canvas, ["Alt+4"]),
    GuideSquare => ("guide_square", "Square guide on or off", Precision, Canvas, ["Alt+5"]),

    HidePanels => ("hide_panels", "Hide or show the panels", Panels, Canvas, ["Ctrl+Period"]),
    OpenBrushes => ("open_brushes", "All brushes", Panels, Canvas, ["B"]),
    OpenLayers => ("open_layers", "Layers", Panels, Canvas, ["L"]),
    OpenLibrary => ("open_library", "Library", Panels, App, ["Ctrl+L"]),
    OpenSettings => ("open_settings", "Settings", Panels, App, ["Ctrl+Comma"]),
}

impl Command {
    /// The command with this settings id.
    #[cfg(test)]
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// `true` for commands whose keys also work with Shift or Alt held, which change how much they do.
    #[must_use]
    pub const fn scaled(self) -> bool {
        matches!(
            self,
            Self::MoveLeft | Self::MoveRight | Self::MoveUp | Self::MoveDown
        )
    }

    /// The kind of shape guide a "guide on or off" command is for.
    #[must_use]
    pub const fn guide_kind(self) -> Option<ink_core::guide::Kind> {
        use ink_core::guide::Kind;
        Some(match self {
            Self::GuideLine => Kind::Line,
            Self::GuideArc => Kind::Arc,
            Self::GuideAngle => Kind::Angle,
            Self::GuideCircle => Kind::Circle,
            Self::GuideSquare => Kind::Square,
            _ => return None,
        })
    }

    /// The command that turns the guide of `kind` on or off.
    #[must_use]
    pub const fn for_guide(kind: ink_core::guide::Kind) -> Self {
        use ink_core::guide::Kind;
        match kind {
            Kind::Line => Self::GuideLine,
            Kind::Arc => Self::GuideArc,
            Kind::Angle => Self::GuideAngle,
            Kind::Circle => Self::GuideCircle,
            Kind::Square => Self::GuideSquare,
        }
    }

    /// The tab (counting from 1) a "go to tab" command goes to.
    #[must_use]
    pub const fn tab_number(self) -> Option<usize> {
        Some(match self {
            Self::Tab1 => 1,
            Self::Tab2 => 2,
            Self::Tab3 => 3,
            Self::Tab4 => 4,
            Self::Tab5 => 5,
            Self::Tab6 => 6,
            Self::Tab7 => 7,
            Self::Tab8 => 8,
            Self::Tab9 => 9,
            _ => return None,
        })
    }

    /// The place on the rail (counting from 0) a "pen N" command picks.
    #[must_use]
    pub const fn rail_slot(self) -> Option<usize> {
        Some(match self {
            Self::RailPen1 => 0,
            Self::RailPen2 => 1,
            Self::RailPen3 => 2,
            Self::RailPen4 => 3,
            Self::RailPen5 => 4,
            Self::RailPen6 => 5,
            Self::RailPen7 => 6,
            Self::RailPen8 => 7,
            _ => return None,
        })
    }
}

/// One key with the modifiers that must be held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    pub key: Key,
    /// Ctrl, or Cmd on a Mac.
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Keys that are themselves typed with Shift held on most keyboards, so Shift is no part of the shortcut.
fn is_shifted_symbol(key: Key) -> bool {
    matches!(
        key,
        Key::Plus
            | Key::Colon
            | Key::Pipe
            | Key::Questionmark
            | Key::Exclamationmark
            | Key::OpenCurlyBracket
            | Key::CloseCurlyBracket
    )
}

impl Binding {
    /// Reads `Ctrl+Shift+Z`. The parts may be in any order and any case; `Cmd`, `Command` and `Control` mean
    /// Ctrl, and `Option` means Alt. `None` if the text has no key, an unknown key, or a repeated modifier.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = Key::from_name(parts.pop().filter(|p| !p.is_empty())?)?;
        let mut binding = Self {
            key,
            command: false,
            shift: false,
            alt: false,
        };
        for part in parts {
            let flag = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "meta" => &mut binding.command,
                "shift" => &mut binding.shift,
                "alt" | "option" => &mut binding.alt,
                _ => return None,
            };
            if std::mem::replace(flag, true) {
                return None;
            }
        }
        Some(binding)
    }

    /// The binding for a key press.
    #[must_use]
    pub fn of(key: Key, modifiers: Modifiers) -> Self {
        Self {
            key,
            command: modifiers.command,
            shift: modifiers.shift && !is_shifted_symbol(key),
            alt: modifiers.alt,
        }
    }

    /// The text kept in the settings file; [`Binding::parse`] reads it back.
    #[must_use]
    pub fn text(self) -> String {
        self.join(["Ctrl", "Shift", "Alt"], self.key.name())
    }

    /// The text shown to people: the same, with the key spelled the way it is on the keycap.
    #[must_use]
    pub fn label(self) -> String {
        let command = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        let key = match self.key {
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => self.key.name(),
            key => key.symbol_or_name(),
        };
        self.join([command, "Shift", "Alt"], key)
    }

    fn join(self, names: [&str; 3], key: &str) -> String {
        let mut text = String::new();
        for (held, name) in [self.command, self.shift, self.alt].into_iter().zip(names) {
            if held {
                text.push_str(name);
                text.push('+');
            }
        }
        text.push_str(key);
        text
    }

    /// `true` when this key press, with exactly these modifiers held, is the binding.
    #[must_use]
    pub fn matches(self, key: Key, modifiers: Modifiers) -> bool {
        self.key == key && self == Self::of(key, modifiers)
    }

    /// Like [`Binding::matches`], but also when Shift or Alt are held that the binding does not ask for.
    fn matches_with_extra(self, key: Key, modifiers: Modifiers) -> bool {
        self.key == key
            && !self.shift
            && !self.alt
            && self.command == modifiers.command
            && (modifiers.shift || modifiers.alt)
    }

    /// `true` for a key typed on its own (or with Shift): it must not fire while text is being typed.
    #[must_use]
    pub fn is_typed(self) -> bool {
        !self.command && !self.alt
    }
}

/// The keys of every command, ready to look presses up in.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    entries: Vec<(Binding, Command)>,
}

impl Keymap {
    /// The keys for these settings: the changed commands from `overrides`, the starting keys for the rest.
    #[must_use]
    pub fn new(overrides: &BTreeMap<String, Vec<String>>) -> Self {
        let entries = Command::ALL
            .iter()
            .flat_map(|cmd| {
                bindings_of(overrides, *cmd)
                    .into_iter()
                    .map(move |b| (b, *cmd))
            })
            .collect();
        Self { entries }
    }

    /// The command a key press runs, if any. Exact matches win over the Shift and Alt variants of nudging.
    #[must_use]
    pub fn resolve(&self, key: Key, modifiers: Modifiers) -> Option<Command> {
        self.entries
            .iter()
            .find(|(b, _)| b.matches(key, modifiers))
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(b, c)| c.scaled() && b.matches_with_extra(key, modifiers))
            })
            .map(|(_, c)| *c)
    }

    /// The keys of `command`, in order.
    pub fn bindings(&self, command: Command) -> impl Iterator<Item = Binding> + '_ {
        self.entries
            .iter()
            .filter(move |(_, c)| *c == command)
            .map(|(b, _)| *b)
    }

    /// The first key of `command` as people read it, if it has any.
    #[must_use]
    pub fn first_label(&self, command: Command) -> Option<String> {
        self.bindings(command).next().map(Binding::label)
    }

    /// `text` with the command's first key after it, for a tooltip: `Eraser (E)`.
    #[must_use]
    pub fn hint(&self, command: Command, text: &str) -> String {
        match self.first_label(command) {
            Some(keys) => format!("{text} ({keys})"),
            None => text.to_owned(),
        }
    }

    /// Takes the key presses of this frame that run a command out of `input`, so nothing else sees them, and
    /// returns what they run with the modifiers that were held. `allowed` can refuse a command for now (the
    /// press is then left alone).
    ///
    /// Ctrl+C, Ctrl+X and Ctrl+V reach egui as events of their own rather than as key presses; they are taken
    /// here as the keys they come from.
    pub fn take(
        &self,
        input: &mut InputState,
        allowed: impl Fn(Command, Binding) -> bool,
    ) -> Vec<(Command, Modifiers)> {
        let mut found = Vec::new();
        input.events.retain(|event| {
            let (key, modifiers) = match event {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => (*key, *modifiers),
                Event::Copy => (Key::C, Modifiers::COMMAND),
                Event::Cut => (Key::X, Modifiers::COMMAND),
                Event::Paste(_) => (Key::V, Modifiers::COMMAND),
                _ => return true,
            };
            let Some(command) = self.resolve(key, modifiers) else {
                return true;
            };
            if !allowed(command, Binding::of(key, modifiers)) {
                return true;
            }
            found.push((command, modifiers));
            false
        });
        found
    }
}

/// The keys of `command` as the settings have them: what the user chose, or the starting keys.
#[must_use]
pub fn bindings_of(overrides: &BTreeMap<String, Vec<String>>, command: Command) -> Vec<Binding> {
    let parse = |texts: &[String]| -> Vec<Binding> {
        let mut keys: Vec<Binding> = Vec::new();
        for binding in texts.iter().filter_map(|t| Binding::parse(t)) {
            if !keys.contains(&binding) {
                keys.push(binding);
            }
        }
        keys
    };
    match overrides.get(command.id()) {
        Some(texts) => parse(texts),
        None => parse(
            &command
                .defaults()
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>(),
        ),
    }
}

/// Stores `keys` as the keys of `command`, or forgets the override if they are its starting keys.
pub fn set_bindings(
    overrides: &mut BTreeMap<String, Vec<String>>,
    command: Command,
    keys: &[Binding],
) {
    let starting: Vec<Binding> = command
        .defaults()
        .iter()
        .filter_map(|t| Binding::parse(t))
        .collect();
    if keys == starting {
        overrides.remove(command.id());
    } else {
        overrides.insert(
            command.id().to_owned(),
            keys.iter().map(|b| b.text()).collect(),
        );
    }
}

/// Gives `command` the key `binding`, in place of its key number `slot` (or as one more when `slot` is `None`
/// or past the end). If another command had the key it loses it, and that command is returned.
pub fn assign(
    overrides: &mut BTreeMap<String, Vec<String>>,
    command: Command,
    slot: Option<usize>,
    binding: Binding,
) -> Option<Command> {
    let taken_from = Command::ALL
        .iter()
        .copied()
        .find(|c| *c != command && bindings_of(overrides, *c).contains(&binding));
    if let Some(other) = taken_from {
        let mut keys = bindings_of(overrides, other);
        keys.retain(|b| *b != binding);
        set_bindings(overrides, other, &keys);
    }
    let mut keys = bindings_of(overrides, command);
    match slot.filter(|i| *i < keys.len()) {
        Some(i) => keys[i] = binding,
        None => keys.push(binding),
    }
    let mut seen = Vec::new();
    keys.retain(|b| {
        let first = !seen.contains(b);
        seen.push(*b);
        first
    });
    set_bindings(overrides, command, &keys);
    taken_from
}

/// Takes away the key number `slot` of `command`.
pub fn unbind(overrides: &mut BTreeMap<String, Vec<String>>, command: Command, slot: usize) {
    let mut keys = bindings_of(overrides, command);
    if slot < keys.len() {
        keys.remove(slot);
        set_bindings(overrides, command, &keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Binding {
        Binding::parse(text).expect(text)
    }

    #[test]
    fn keys_are_read_in_any_order_and_case_and_written_back_the_same() {
        let b = key("shift + CTRL + z");
        assert_eq!(
            b,
            Binding {
                key: Key::Z,
                command: true,
                shift: true,
                alt: false
            }
        );
        assert_eq!(b.text(), "Ctrl+Shift+Z");
        assert_eq!(key("cmd+alt+Left").text(), "Ctrl+Alt+Left");
        assert_eq!(key("Ctrl+Plus").text(), "Ctrl+Plus");
        assert_eq!(key("Ctrl+1").key, Key::Num1);
        for bad in [
            "",
            "Ctrl+",
            "Ctrl+Shift",
            "Hyper+A",
            "Ctrl+Ctrl+A",
            "Ctrl+Nonsense",
        ] {
            assert_eq!(Binding::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_key_on_the_keyboard_survives_being_written_and_read() {
        for k in Key::ALL {
            let b = Binding {
                key: *k,
                command: true,
                shift: false,
                alt: true,
            };
            assert_eq!(Binding::parse(&b.text()), Some(b), "{k:?}");
        }
    }

    #[test]
    fn the_starting_keys_all_read_and_no_two_commands_share_one() {
        let ids: Vec<&str> = Command::ALL.iter().map(|c| c.id()).collect();
        let mut seen: Vec<Binding> = Vec::new();
        for (i, command) in Command::ALL.iter().enumerate() {
            assert!(!ids[..i].contains(&command.id()), "{}", command.id());
            assert_eq!(Command::from_id(command.id()), Some(*command));
            assert_ne!(command.label(), "");
            for text in command.defaults() {
                let binding = Binding::parse(text).unwrap_or_else(|| panic!("{text}"));
                assert!(!seen.contains(&binding), "{text} is used twice");
                seen.push(binding);
            }
        }
    }

    #[test]
    fn a_press_runs_the_command_with_exactly_those_modifiers() {
        let map = Keymap::new(&BTreeMap::new());
        assert_eq!(map.resolve(Key::Z, Modifiers::COMMAND), Some(Command::Undo));
        assert_eq!(
            map.resolve(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
            Some(Command::Redo)
        );
        assert_eq!(map.resolve(Key::Y, Modifiers::COMMAND), Some(Command::Redo));
        assert_eq!(map.resolve(Key::Z, Modifiers::NONE), None);
        assert_eq!(
            map.resolve(Key::E, Modifiers::NONE),
            Some(Command::ToolEraser)
        );
        assert_eq!(
            map.resolve(Key::E, Modifiers::COMMAND),
            None,
            "Ctrl+E is not E"
        );
        assert_eq!(
            map.resolve(Key::H, Modifiers::SHIFT),
            Some(Command::FlipHorizontal)
        );
        assert_eq!(map.resolve(Key::H, Modifiers::NONE), Some(Command::ToolPan));
    }

    #[test]
    fn shift_and_alt_still_nudge_but_only_when_no_command_wants_them() {
        let mut keys = BTreeMap::new();
        let map = Keymap::new(&keys);
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::NONE),
            Some(Command::MoveLeft)
        );
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::SHIFT),
            Some(Command::MoveLeft)
        );
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::ALT),
            Some(Command::MoveLeft)
        );
        // Ctrl is not a nudge: it is its own command, which turns the paper.
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::COMMAND),
            Some(Command::TurnLeft)
        );
        assign(&mut keys, Command::ZoomFit, None, key("Shift+Left"));
        let map = Keymap::new(&keys);
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::SHIFT),
            Some(Command::ZoomFit)
        );
        assert_eq!(
            map.resolve(Key::ArrowLeft, Modifiers::NONE),
            Some(Command::MoveLeft)
        );
    }

    #[test]
    fn a_key_typed_with_shift_does_not_need_shift_in_its_binding() {
        let map = Keymap::new(&BTreeMap::new());
        assert_eq!(
            map.resolve(Key::Plus, Modifiers::COMMAND | Modifiers::SHIFT),
            Some(Command::ZoomIn)
        );
        assert_eq!(
            map.resolve(Key::Plus, Modifiers::COMMAND),
            Some(Command::ZoomIn)
        );
    }

    #[test]
    fn changing_a_command_stores_only_that_command_and_putting_it_back_forgets_it() {
        let mut keys = BTreeMap::new();
        assert_eq!(
            assign(&mut keys, Command::Undo, Some(0), key("Ctrl+U")),
            None
        );
        assert_eq!(keys.len(), 1);
        let map = Keymap::new(&keys);
        assert_eq!(map.resolve(Key::U, Modifiers::COMMAND), Some(Command::Undo));
        assert_eq!(
            map.resolve(Key::Z, Modifiers::COMMAND),
            None,
            "the old key is gone"
        );
        assert_eq!(
            assign(&mut keys, Command::Undo, Some(0), key("Ctrl+Z")),
            None
        );
        assert!(keys.is_empty(), "back to the starting keys, nothing stored");
    }

    #[test]
    fn taking_a_key_from_another_command_leaves_that_command_without_it() {
        let mut keys = BTreeMap::new();
        let lost = assign(&mut keys, Command::ToolSlice, Some(0), key("E"));
        assert_eq!(lost, Some(Command::ToolEraser));
        let map = Keymap::new(&keys);
        assert_eq!(
            map.resolve(Key::E, Modifiers::NONE),
            Some(Command::ToolSlice)
        );
        assert_eq!(map.bindings(Command::ToolEraser).count(), 0);
        assert_eq!(
            keys.get("tool_eraser"),
            Some(&Vec::new()),
            "unbound is remembered as unbound"
        );
    }

    #[test]
    fn a_second_key_can_be_added_and_one_removed() {
        // The label names the command key the way the system does: Cmd on a Mac, Ctrl elsewhere.
        let ctrl = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        let mut keys = BTreeMap::new();
        assign(&mut keys, Command::Save, None, key("Ctrl+Alt+S"));
        let map = Keymap::new(&keys);
        assert_eq!(map.bindings(Command::Save).count(), 2);
        assert_eq!(map.first_label(Command::Save), Some(format!("{ctrl}+S")));
        unbind(&mut keys, Command::Save, 0);
        let map = Keymap::new(&keys);
        assert_eq!(
            map.first_label(Command::Save),
            Some(format!("{ctrl}+Alt+S"))
        );
        assert_eq!(
            map.hint(Command::Save, "Save"),
            format!("Save ({ctrl}+Alt+S)")
        );
        assert_eq!(map.hint(Command::Cut, "Cut"), format!("Cut ({ctrl}+X)"));
    }

    #[test]
    fn damaged_settings_cannot_break_the_keys() {
        let mut keys = BTreeMap::new();
        keys.insert(
            "undo".to_owned(),
            vec![
                "nonsense+".to_owned(),
                "Ctrl+U".to_owned(),
                "Ctrl+U".to_owned(),
            ],
        );
        keys.insert("from_the_future".to_owned(), vec!["F1".to_owned()]);
        let map = Keymap::new(&keys);
        assert_eq!(
            map.bindings(Command::Undo).count(),
            1,
            "the bad one is skipped, the repeat is dropped"
        );
        assert_eq!(map.resolve(Key::U, Modifiers::COMMAND), Some(Command::Undo));
    }

    #[test]
    fn the_numbered_commands_know_their_numbers() {
        assert_eq!(Command::Tab3.tab_number(), Some(3));
        assert_eq!(Command::Undo.tab_number(), None);
        assert_eq!(Command::RailPen5.rail_slot(), Some(4));
        assert_eq!(Command::RailPen8.rail_slot(), Some(7));
        let slots = Command::ALL
            .iter()
            .filter(|c| c.rail_slot().is_some())
            .count();
        assert_eq!(
            slots,
            crate::config::RAIL_SLOTS.1,
            "a command for every place the rail can have"
        );
        let tabs = Command::ALL
            .iter()
            .filter(|c| c.tab_number().is_some())
            .count();
        assert_eq!(tabs, 9);
    }
}
