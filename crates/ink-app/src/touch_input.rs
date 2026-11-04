//! Touch, stylus and key input as a platform reports it, turned into what the app should do with it.
//!
//! On Android the activity hands over every sample of every pointer with the tool that made it (a stylus, its eraser
//! end, a finger, a mouse) and the size of the contact. This module keeps them in a queue ([`push_pointer`],
//! [`push_key`]) and a [`TouchFilter`] decides, for each pointer, whether it draws, moves the page or is ignored:
//!
//! - the stylus draws, and so does a mouse;
//! - fingers move and zoom the page and press buttons, and draw only when the settings say so (by default, only on
//!   a device that has no stylus);
//! - a palm is ignored: a contact that is too large, any finger that lands while a stylus is touching or hovering
//!   near the screen (or has just been), and a finger that was already down when the stylus came.
//!
//! It is plain logic on plain values, so it is tested without a device.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2,
    TouchDeviceId, TouchId, TouchPhase, Vec2,
};
use serde::{Deserialize, Serialize};

/// What touched the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Finger,
    Stylus,
    /// The back end of a stylus.
    Eraser,
    Mouse,
}

impl Tool {
    /// From Android's `MotionEvent.TOOL_TYPE_*`.
    #[must_use]
    pub const fn from_android(code: i32) -> Self {
        match code {
            2 => Self::Stylus,
            4 => Self::Eraser,
            3 => Self::Mouse,
            _ => Self::Finger,
        }
    }

    const fn is_pen(self) -> bool {
        matches!(self, Self::Stylus | Self::Eraser)
    }
}

/// What a sample says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Move,
    Cancel,
    /// Above the screen, not touching it.
    Hover,
    /// A stylus or mouse that was above the screen has gone out of range.
    Exit,
    /// The scroll wheel: `x` and `y` hold the amounts.
    Scroll,
}

impl Action {
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Down,
            1 => Self::Up,
            2 => Self::Move,
            3 => Self::Cancel,
            4 => Self::Hover,
            5 => Self::Scroll,
            6 => Self::Exit,
            _ => return None,
        })
    }
}

/// One sample of one pointer, in the platform's units: pixels of the window, milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub action: Action,
    /// The input device: a stylus digitizer and a touchscreen are different devices, and each numbers its own
    /// pointers from zero.
    pub device: i32,
    pub pointer: i32,
    pub tool: Tool,
    pub x: f32,
    pub y: f32,
    /// `0.0..=1.0`.
    pub pressure: f32,
    /// The size of the contact (its long side), in pixels.
    pub contact: f32,
    /// The buttons held, as Android's `MotionEvent.BUTTON_*` bits.
    pub buttons: u32,
    pub time_ms: i64,
}

/// A key going down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySample {
    pub down: bool,
    /// Android's `KeyEvent.KEYCODE_*`.
    pub code: i32,
    /// Android's `KeyEvent.META_*` bits.
    pub meta: u32,
    /// The character the key makes, or `0`.
    pub unicode: u32,
    pub repeat: bool,
}

/// How many samples wait at most: a stalled app drops the oldest rather than growing without end.
const MOST_WAITING: usize = 8192;

static POINTERS: Mutex<Vec<Sample>> = Mutex::new(Vec::new());
static KEYS: Mutex<Vec<KeySample>> = Mutex::new(Vec::new());
static STYLUS_DEVICE: AtomicBool = AtomicBool::new(false);
static WAKE: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

/// Remembers the context to wake when input arrives while the app is waiting.
pub(crate) fn wake_on_input(ctx: &egui::Context) {
    let _ = WAKE.set(ctx.clone());
}

/// Wakes the app, if it is waiting for something to happen, so it reads what was just queued.
pub(crate) fn wake() {
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

fn push_limited<T>(queue: &Mutex<Vec<T>>, item: T) {
    if let Ok(mut waiting) = queue.lock() {
        if waiting.len() >= MOST_WAITING {
            waiting.remove(0);
        }
        waiting.push(item);
    }
}

/// Queues a sample of a pointer, as the activity reports it. Unknown actions are dropped.
#[allow(
    clippy::too_many_arguments,
    reason = "the fields of one sample, as the platform gives them"
)]
pub fn push_pointer(
    action: i32,
    device: i32,
    pointer: i32,
    tool: i32,
    x: f32,
    y: f32,
    pressure: f32,
    contact: f32,
    buttons: i32,
    time_ms: i64,
) {
    let Some(action) = Action::from_code(action) else {
        return;
    };
    push_limited(
        &POINTERS,
        Sample {
            action,
            device,
            pointer,
            tool: Tool::from_android(tool),
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            contact: contact.max(0.0),
            buttons: u32::try_from(buttons).unwrap_or(0),
            time_ms,
        },
    );
    wake();
}

/// Queues a key going down or up, as the activity reports it.
pub fn push_key(down: bool, code: i32, meta: i32, unicode: i32, repeat: bool) {
    push_limited(
        &KEYS,
        KeySample {
            down,
            code,
            meta: u32::try_from(meta).unwrap_or(0),
            unicode: u32::try_from(unicode).unwrap_or(0),
            repeat,
        },
    );
    wake();
}

/// Says whether the device has a stylus digitizer.
pub fn set_stylus_device(has: bool) {
    STYLUS_DEVICE.store(has, Ordering::Relaxed);
}

/// The samples waiting, oldest first.
#[must_use]
pub(crate) fn take_pointers() -> Vec<Sample> {
    POINTERS
        .lock()
        .map(|mut waiting| std::mem::take(&mut *waiting))
        .unwrap_or_default()
}

/// The keys waiting, oldest first.
#[must_use]
pub(crate) fn take_keys() -> Vec<KeySample> {
    KEYS.lock()
        .map(|mut waiting| std::mem::take(&mut *waiting))
        .unwrap_or_default()
}

// --- the settings ---------------------------------------------------------------------------------------------

/// Whether a finger draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FingerDrawing {
    /// Fingers draw on a device with no stylus, and move the page on one that has one.
    Automatic,
    /// Fingers draw, as the stylus does, and everything on the paper can be done with them: a hand resting by the stylus
    /// is ignored (see the palm settings), so nothing is lost.
    #[default]
    Always,
    /// Fingers never draw: they move and zoom the page.
    Never,
}

impl FingerDrawing {
    pub const ALL: [Self; 3] = [Self::Always, Self::Automatic, Self::Never];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Always => "Always",
            Self::Never => "Never",
        }
    }
}

/// What the filter is told about the settings and the device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Policy {
    pub fingers: FingerDrawing,
    pub palm_rejection: bool,
    /// A stylus has been used on this device before (and the settings remembered it), so a finger moves the page from
    /// the first touch, not only after the stylus has touched in this session.
    pub stylus_device: bool,
    /// How long a pen (or a finger that draws) is held still before it drags the page instead of drawing, or `None`
    /// for never.
    pub pen_hold: Option<f64>,
}

/// How long after a stylus was last above or on the screen a finger landing is taken for a palm, in seconds.
const PALM_HOLD: f64 = 0.35;
/// How near the pen, in points, a finger is taken for the hand that holds it. A finger further off is the other hand,
/// which is meant.
const PALM_RADIUS: f32 = 230.0;
/// A contact larger than this (in points, which are about the size of the device-independent pixels of Android) is
/// a palm: a fingertip is about 45 to 65 of them across.
const PALM_CONTACT: f32 = 95.0;

fn key_of_sample(s: &Sample) -> Key2 {
    (s.device, s.pointer)
}

/// Where on the screen a pointer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// On the paper.
    Canvas,
    /// On a button or a panel.
    Panel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Draws, and presses: the stylus, a mouse, and a finger when fingers draw.
    Pen,
    /// A finger that moves the page, or presses a button.
    Finger { place: Place },
    /// A palm: nothing it does counts.
    Ignored,
}

#[derive(Debug, Clone, Copy)]
struct Pointer {
    role: Role,
    tool: Tool,
    last: Pos2,
    /// The buttons that have been reported down (a mouse); the pen button is `1`.
    pressed: u32,
    /// A press this pointer made on the screen, which must be let go of.
    holding: Option<PointerButton>,
    /// Where and when it went down, for a hold that turns a stroke into a drag of the page.
    down: Pos2,
    down_at: f64,
    /// How long it has to be held still for that, or `None` where holding does nothing.
    hold_after: Option<f64>,
    /// It has left the small place it went down in, so it is not being held still.
    moved: bool,
    /// The hold came, and the pointer now drags the page.
    panning: bool,
}

impl Pointer {
    /// A pointer that has just gone down at `pos`, with nothing pressed.
    fn at(pos: Pos2, now: f64, hold_after: Option<f64>) -> Self {
        Self {
            role: Role::Ignored,
            tool: Tool::Finger,
            last: pos,
            pressed: 0,
            holding: None,
            down: pos,
            down_at: now,
            hold_after,
            moved: false,
            panning: false,
        }
    }

    /// The pointer moved to `pos`.
    fn note_move(&mut self, pos: Pos2) {
        self.last = pos;
        if pos.distance(self.down) > HOLD_SLOP {
            self.moved = true;
        }
    }
}

/// How far, in points, a pointer may wander from where it went down and still count as held still: a hand is never
/// quite steady.
const HOLD_SLOP: f32 = 8.0;

/// A pointer being held still, as far as it has got.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    /// Where it is, in points.
    pub at: Pos2,
    /// How much of the wait is over, `0.0` to `1.0`.
    pub progress: f32,
    /// The wait is over: the pointer drags the page.
    pub panning: bool,
}

/// Turns samples into egui events, and keeps what it needs to know between frames.
#[derive(Debug, Default)]
pub struct TouchFilter {
    pointers: HashMap<Key2, Pointer>,
    /// The time until which a stylus counts as being around.
    pen_around_until: f64,
    /// Where the stylus last was, in points.
    pen_at: Pos2,
    /// A stylus has been used.
    stylus_seen: bool,
    /// The finger whose position stands for the pointer.
    primary: Option<Key2>,
}

/// A pointer of a device.
type Key2 = (i32, i32);

/// The buttons on the barrel of a stylus, as Android's `MotionEvent.BUTTON_STYLUS_PRIMARY` and `_SECONDARY` bits.
const STYLUS_BUTTONS: u32 = 0x20 | 0x40;

fn touch(key: Key2, phase: TouchPhase, pos: Pos2, force: Option<f32>) -> Event {
    Event::Touch {
        device_id: TouchDeviceId(u64::from(key.0.unsigned_abs())),
        id: TouchId(u64::from(key.1.unsigned_abs())),
        phase,
        pos,
        force,
    }
}

fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

impl TouchFilter {
    /// `true` while fingers draw under `policy`.
    fn fingers_draw(&self, policy: Policy) -> bool {
        match policy.fingers {
            FingerDrawing::Always => true,
            FingerDrawing::Never => false,
            FingerDrawing::Automatic => !(policy.stylus_device || self.stylus_seen),
        }
    }

    /// Called every frame, with or without input. A pen that has been held still for as long as the policy says
    /// stops drawing and drags the page: its press is let go of and a secondary press, which pans, takes its place.
    /// Returns the events, and whether a hold has just come, so the stroke in progress can be dropped.
    pub fn tick(&mut self, now: f64) -> (Vec<Event>, bool) {
        let mut out = Vec::new();
        let mut came = false;
        for pointer in self.pointers.values_mut() {
            let Some(after) = pointer.hold_after else {
                continue;
            };
            if pointer.role != Role::Pen
                || pointer.moved
                || pointer.panning
                || now - pointer.down_at < after
            {
                continue;
            }
            let Some(button) = pointer.holding else {
                continue;
            };
            out.push(Event::PointerMoved(pointer.last));
            out.push(press(pointer.last, button, false));
            out.push(press(pointer.last, PointerButton::Secondary, true));
            pointer.holding = Some(PointerButton::Secondary);
            pointer.panning = true;
            came = true;
        }
        (out, came)
    }

    /// The pen being held still, if there is one: for showing how long is left.
    #[must_use]
    pub fn pen_hold(&self, now: f64) -> Option<Hold> {
        self.pointers.values().find_map(|p| {
            let after = p.hold_after?;
            if p.role != Role::Pen {
                return None;
            }
            if p.panning {
                return Some(Hold {
                    at: p.last,
                    progress: 1.0,
                    panning: true,
                });
            }
            (!p.moved).then(|| Hold {
                at: p.last,
                progress: ((now - p.down_at) / after).clamp(0.0, 1.0) as f32,
                panning: false,
            })
        })
    }

    /// A stylus has touched or hovered over the screen since this was made.
    #[must_use]
    pub fn stylus_seen(&self) -> bool {
        self.stylus_seen
    }

    /// `true` when a finger landing at `pos` is the hand that holds the stylus: the stylus is touching the screen (the
    /// whole hand is down), or is just above it or has just left it, and the finger is near it.
    fn pen_is_near(&self, now: f64, pos: Pos2) -> bool {
        if now >= self.pen_around_until {
            return false;
        }
        self.pen_around_until.is_infinite() || pos.distance(self.pen_at) < PALM_RADIUS
    }

    /// The events for `samples`, in order. `scale` is the pixels to a point; `place` says what is under a point.
    pub fn process(
        &mut self,
        now: f64,
        samples: &[Sample],
        scale: f32,
        policy: Policy,
        place: impl Fn(Pos2) -> Place,
    ) -> Vec<Event> {
        let mut out = Vec::new();
        let scale = if scale > 0.0 { scale } else { 1.0 };
        for sample in samples {
            if sample.action == Action::Scroll {
                out.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta: Vec2::new(sample.x, sample.y),
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                });
                continue;
            }
            let pos = Pos2::new(sample.x / scale, sample.y / scale);
            if sample.tool.is_pen() {
                self.pen_sample(now, sample, pos, policy, &mut out);
            } else {
                self.other_sample(now, sample, pos, scale, policy, &place, &mut out);
            }
        }
        out
    }

    /// A stylus: hovering, or touching.
    fn pen_sample(
        &mut self,
        now: f64,
        s: &Sample,
        pos: Pos2,
        policy: Policy,
        out: &mut Vec<Event>,
    ) {
        self.stylus_seen = true;
        self.pen_at = pos;
        let touching = matches!(s.action, Action::Down | Action::Move);
        self.pen_around_until = if touching {
            f64::INFINITY
        } else if s.action == Action::Exit {
            // Out of range: nothing is held, so the next finger is meant.
            now
        } else {
            now + PALM_HOLD
        };
        match s.action {
            Action::Hover => out.push(Event::PointerMoved(pos)),
            // The pointer is gone, so a ring that follows the pen (the brush, the eraser) goes with it.
            Action::Exit => out.push(Event::PointerGone),
            Action::Down => {
                // The hand that holds the stylus is resting on the screen: what it touches is not input.
                if policy.palm_rejection {
                    self.cancel_fingers(out);
                }
                // With the button on its barrel held, the stylus moves the page (a secondary drag) instead of
                // drawing, so a pen can move the canvas without changing tool.
                let button = if s.buttons & STYLUS_BUTTONS == 0 {
                    PointerButton::Primary
                } else {
                    PointerButton::Secondary
                };
                self.pointers.insert(
                    key_of_sample(s),
                    Pointer {
                        role: Role::Pen,
                        tool: s.tool,
                        holding: Some(button),
                        ..Pointer::at(pos, now, policy.pen_hold)
                    },
                );
                out.push(touch(
                    key_of_sample(s),
                    TouchPhase::Start,
                    pos,
                    Some(s.pressure),
                ));
                out.push(Event::PointerMoved(pos));
                out.push(press(pos, button, true));
            }
            Action::Move => {
                if let Some(p) = self.pointers.get_mut(&key_of_sample(s)) {
                    p.note_move(pos);
                    out.push(touch(
                        key_of_sample(s),
                        TouchPhase::Move,
                        pos,
                        Some(s.pressure),
                    ));
                    out.push(Event::PointerMoved(pos));
                }
            }
            Action::Up | Action::Cancel => {
                if let Some(pointer) = self.pointers.remove(&key_of_sample(s)) {
                    let phase = if s.action == Action::Up {
                        TouchPhase::End
                    } else {
                        TouchPhase::Cancel
                    };
                    out.push(Event::PointerMoved(pos));
                    // The button that went down is the one that comes up.
                    out.push(press(
                        pos,
                        pointer.holding.unwrap_or(PointerButton::Primary),
                        false,
                    ));
                    out.push(touch(key_of_sample(s), phase, pos, None));
                }
                self.pen_around_until = now + PALM_HOLD;
            }
            Action::Scroll => {}
        }
    }

    /// Fingers, and a mouse.
    #[allow(
        clippy::too_many_arguments,
        reason = "one sample and what it is judged by"
    )]
    fn other_sample(
        &mut self,
        now: f64,
        s: &Sample,
        pos: Pos2,
        scale: f32,
        policy: Policy,
        place: &impl Fn(Pos2) -> Place,
        out: &mut Vec<Event>,
    ) {
        if s.tool == Tool::Mouse {
            self.mouse_sample(s, pos, out);
            return;
        }
        match s.action {
            Action::Down => self.finger_down(now, s, pos, scale, policy, place, out),
            Action::Move => self.finger_move(s, pos, out),
            Action::Up | Action::Cancel => self.finger_up(s, pos, out),
            Action::Hover | Action::Exit | Action::Scroll => {}
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one sample and what it is judged by"
    )]
    fn finger_down(
        &mut self,
        now: f64,
        s: &Sample,
        pos: Pos2,
        scale: f32,
        policy: Policy,
        place: &impl Fn(Pos2) -> Place,
        out: &mut Vec<Event>,
    ) {
        let palm = policy.palm_rejection
            && (s.contact / scale > PALM_CONTACT || self.pen_is_near(now, pos));
        let role = if palm {
            Role::Ignored
        } else if self.fingers_draw(policy) {
            Role::Pen
        } else {
            Role::Finger { place: place(pos) }
        };
        let mut pointer = Pointer {
            role,
            tool: Tool::Finger,
            ..Pointer::at(
                pos,
                now,
                if role == Role::Pen {
                    policy.pen_hold
                } else {
                    None
                },
            )
        };
        match role {
            Role::Ignored => {}
            Role::Pen | Role::Finger { .. } => {
                out.push(touch(key_of_sample(s), TouchPhase::Start, pos, None));
                self.pointer_down(&mut pointer, key_of_sample(s), pos, out);
            }
        }
        self.pointers.insert(key_of_sample(s), pointer);
    }

    /// The pointer events of a finger that has just landed.
    fn pointer_down(&mut self, pointer: &mut Pointer, id: Key2, pos: Pos2, out: &mut Vec<Event>) {
        // Only the first finger stands for the pointer; a second one makes a gesture of the two.
        if self.primary.is_some_and(|p| self.pointers.contains_key(&p)) {
            // A second finger: the first one's press ends, so the two make a gesture and not a drag.
            self.release_primary(out);
            return;
        }
        self.primary = Some(id);
        let button = match pointer.role {
            // On the paper a finger drags the page (a secondary drag pans); on a button it presses.
            Role::Finger {
                place: Place::Canvas,
            } => Some(PointerButton::Secondary),
            Role::Pen
            | Role::Finger {
                place: Place::Panel,
            } => Some(PointerButton::Primary),
            Role::Ignored => None,
        };
        if let Some(button) = button {
            out.push(Event::PointerMoved(pos));
            out.push(press(pos, button, true));
            pointer.holding = Some(button);
        }
    }

    /// Lets go of the press of the finger that stands for the pointer.
    fn release_primary(&mut self, out: &mut Vec<Event>) {
        if let Some(id) = self.primary.take()
            && let Some(p) = self.pointers.get_mut(&id)
            && let Some(button) = p.holding.take()
        {
            out.push(press(p.last, button, false));
        }
    }

    fn finger_move(&mut self, s: &Sample, pos: Pos2, out: &mut Vec<Event>) {
        let Some(p) = self.pointers.get_mut(&key_of_sample(s)) else {
            return;
        };
        if p.role == Role::Ignored {
            return;
        }
        p.note_move(pos);
        out.push(touch(key_of_sample(s), TouchPhase::Move, pos, None));
        if self.primary == Some(key_of_sample(s)) {
            out.push(Event::PointerMoved(pos));
        }
    }

    fn finger_up(&mut self, s: &Sample, pos: Pos2, out: &mut Vec<Event>) {
        let Some(p) = self.pointers.get(&key_of_sample(s)).copied() else {
            return;
        };
        if p.role == Role::Ignored {
            self.pointers.remove(&key_of_sample(s));
            return;
        }
        if self.primary == Some(key_of_sample(s)) {
            out.push(Event::PointerMoved(pos));
            self.release_primary(out);
            out.push(Event::PointerGone);
        }
        let phase = if s.action == Action::Up {
            TouchPhase::End
        } else {
            TouchPhase::Cancel
        };
        out.push(touch(key_of_sample(s), phase, pos, None));
        self.pointers.remove(&key_of_sample(s));
    }

    /// Ends every finger that is down, because a stylus has come: they were a hand resting on the screen.
    fn cancel_fingers(&mut self, out: &mut Vec<Event>) {
        let fingers: Vec<(Key2, Pointer)> = self
            .pointers
            .iter()
            .filter(|(_, p)| p.tool == Tool::Finger && p.role != Role::Ignored)
            .map(|(id, p)| (*id, *p))
            .collect();
        for (id, p) in fingers {
            if self.primary == Some(id) {
                self.release_primary(out);
                out.push(Event::PointerGone);
            }
            out.push(touch(id, TouchPhase::Cancel, p.last, None));
            if let Some(slot) = self.pointers.get_mut(&id) {
                slot.role = Role::Ignored;
            }
        }
    }

    /// A mouse: its position, and its buttons as they change.
    fn mouse_sample(&mut self, s: &Sample, pos: Pos2, out: &mut Vec<Event>) {
        if s.action == Action::Exit {
            out.push(Event::PointerGone);
            return;
        }
        let entry = self.pointers.entry(key_of_sample(s)).or_insert(Pointer {
            role: Role::Pen,
            tool: Tool::Mouse,
            ..Pointer::at(pos, 0.0, None)
        });
        entry.last = pos;
        out.push(Event::PointerMoved(pos));
        let now_down = if matches!(s.action, Action::Up | Action::Cancel) {
            0
        } else {
            s.buttons
        };
        for (bit, button) in [
            (1, PointerButton::Primary),
            (2, PointerButton::Secondary),
            (4, PointerButton::Middle),
        ] {
            let (was, is) = (entry.pressed & bit != 0, now_down & bit != 0);
            if was != is {
                out.push(press(pos, button, is));
            }
        }
        // A touch that begins with no button reported is a primary press.
        if s.action == Action::Down && now_down == 0 && entry.pressed == 0 {
            out.push(press(pos, PointerButton::Primary, true));
            entry.pressed = 1;
            return;
        }
        entry.pressed = now_down;
    }
}

// --- keys ------------------------------------------------------------------------------------------------------

/// The modifiers in Android's `KeyEvent.META_*` bits.
#[must_use]
pub fn modifiers_of(meta: u32) -> Modifiers {
    let ctrl = meta & 0x1000 != 0;
    Modifiers {
        alt: meta & 0x02 != 0,
        ctrl,
        shift: meta & 0x01 != 0,
        mac_cmd: false,
        command: ctrl,
    }
}

/// The egui key for one of Android's `KeyEvent.KEYCODE_*`.
#[must_use]
pub fn key_of(code: i32) -> Option<Key> {
    Some(match code {
        7..=16 => [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ][(code - 7) as usize],
        29..=54 => [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
        ][(code - 29) as usize],
        19 => Key::ArrowUp,
        20 => Key::ArrowDown,
        21 => Key::ArrowLeft,
        22 => Key::ArrowRight,
        55 => Key::Comma,
        56 => Key::Period,
        61 => Key::Tab,
        62 => Key::Space,
        66 | 160 => Key::Enter,
        67 => Key::Backspace,
        69 => Key::Minus,
        70 => Key::Equals,
        71 => Key::OpenBracket,
        72 => Key::CloseBracket,
        76 => Key::Slash,
        92 => Key::PageUp,
        93 => Key::PageDown,
        111 => Key::Escape,
        112 => Key::Delete,
        122 => Key::Home,
        123 => Key::End,
        124 => Key::Insert,
        131..=142 => [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
        ][(code - 131) as usize],
        _ => return None,
    })
}

/// The events for a key going down or up: the key, and the text it types when it types any.
#[must_use]
pub fn key_events(key: &KeySample) -> Vec<Event> {
    let modifiers = modifiers_of(key.meta);
    let mut out = Vec::new();
    if let Some(k) = key_of(key.code) {
        out.push(Event::Key {
            key: k,
            physical_key: None,
            pressed: key.down,
            repeat: key.repeat,
            modifiers,
        });
    }
    if key.down
        && !modifiers.ctrl
        && !modifiers.alt
        && let Some(c) = char::from_u32(key.unicode).filter(|c| !c.is_control())
    {
        out.push(Event::Text(c.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(action: Action, pointer: i32, tool: Tool, at: (f32, f32)) -> Sample {
        Sample {
            action,
            device: if tool == Tool::Finger { 1 } else { 2 },
            pointer,
            tool,
            x: at.0,
            y: at.1,
            pressure: 0.5,
            contact: 50.0,
            buttons: 0,
            time_ms: 0,
        }
    }

    fn finger(action: Action, pointer: i32, at: (f32, f32)) -> Sample {
        sample(action, pointer, Tool::Finger, at)
    }

    fn pen(action: Action, at: (f32, f32)) -> Sample {
        sample(action, 9, Tool::Stylus, at)
    }

    const TABLET: Policy = Policy {
        fingers: FingerDrawing::Automatic,
        palm_rejection: true,
        stylus_device: true,
        pen_hold: None,
    };
    const HOLDING: Policy = Policy {
        pen_hold: Some(2.0),
        ..TABLET
    };
    const PHONE: Policy = Policy {
        stylus_device: false,
        ..TABLET
    };

    fn on_paper(_: Pos2) -> Place {
        Place::Canvas
    }

    fn run(filter: &mut TouchFilter, now: f64, policy: Policy, samples: &[Sample]) -> Vec<Event> {
        filter.process(now, samples, 1.0, policy, on_paper)
    }

    /// How many touch events there are: a finger that is down is one for the pinch and the taps.
    fn touches(events: &[Event]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, Event::Touch { .. }))
            .count()
    }

    #[test]
    fn the_stylus_draws_with_its_pressure_and_the_touch_comes_before_the_press() {
        let mut f = TouchFilter::default();
        let events = run(&mut f, 1.0, TABLET, &[pen(Action::Down, (10.0, 20.0))]);
        assert!(matches!(
            events[0],
            Event::Touch {
                device_id: TouchDeviceId(2),
                phase: TouchPhase::Start,
                force: Some(p),
                ..
            } if (p - 0.5).abs() < 1e-6
        ));
        assert_eq!(presses(&events), [(PointerButton::Primary, true)]);
        let moved = run(&mut f, 1.1, TABLET, &[pen(Action::Move, (30.0, 40.0))]);
        assert!(
            moved
                .iter()
                .any(|e| matches!(e, Event::PointerMoved(p) if *p == Pos2::new(30.0, 40.0)))
        );
        let up = run(&mut f, 1.2, TABLET, &[pen(Action::Up, (30.0, 40.0))]);
        assert_eq!(presses(&up), [(PointerButton::Primary, false)]);
    }

    #[test]
    fn a_stylus_above_the_screen_only_moves_the_pointer() {
        let mut f = TouchFilter::default();
        let events = run(&mut f, 1.0, TABLET, &[pen(Action::Hover, (5.0, 5.0))]);
        assert_eq!(events, [Event::PointerMoved(Pos2::new(5.0, 5.0))]);
    }

    #[test]
    fn on_a_device_with_a_stylus_a_finger_on_the_paper_drags_the_page_and_does_not_draw() {
        let mut f = TouchFilter::default();
        let down = run(
            &mut f,
            1.0,
            TABLET,
            &[finger(Action::Down, 1, (100.0, 100.0))],
        );
        assert_eq!(
            presses(&down),
            [(PointerButton::Secondary, true)],
            "a secondary drag pans"
        );
        let up = run(
            &mut f,
            1.5,
            TABLET,
            &[finger(Action::Up, 1, (120.0, 140.0))],
        );
        assert_eq!(presses(&up), [(PointerButton::Secondary, false)]);
        assert!(up.contains(&Event::PointerGone));
    }

    #[test]
    fn a_finger_on_a_button_presses_it_even_when_fingers_do_not_draw() {
        let mut f = TouchFilter::default();
        let on_panel = |_: Pos2| Place::Panel;
        let down = f.process(
            1.0,
            &[finger(Action::Down, 1, (5.0, 5.0))],
            1.0,
            TABLET,
            on_panel,
        );
        assert_eq!(presses(&down), [(PointerButton::Primary, true)]);
    }

    #[test]
    fn on_a_device_with_no_stylus_a_finger_draws() {
        let mut f = TouchFilter::default();
        let down = run(
            &mut f,
            1.0,
            PHONE,
            &[finger(Action::Down, 1, (100.0, 100.0))],
        );
        assert_eq!(presses(&down), [(PointerButton::Primary, true)]);
    }

    #[test]
    fn once_a_stylus_has_been_used_fingers_stop_drawing_even_without_a_stylus_device() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, PHONE, &[pen(Action::Hover, (1.0, 1.0))]);
        let down = run(
            &mut f,
            9.0,
            PHONE,
            &[finger(Action::Down, 1, (100.0, 100.0))],
        );
        assert_eq!(presses(&down), [(PointerButton::Secondary, true)]);
    }

    #[test]
    fn the_setting_can_make_fingers_always_or_never_draw() {
        let always = Policy {
            fingers: FingerDrawing::Always,
            ..TABLET
        };
        let mut f = TouchFilter::default();
        let down = run(&mut f, 1.0, always, &[finger(Action::Down, 1, (1.0, 1.0))]);
        assert_eq!(presses(&down), [(PointerButton::Primary, true)]);
        let never = Policy {
            fingers: FingerDrawing::Never,
            ..PHONE
        };
        let mut f = TouchFilter::default();
        let down = run(&mut f, 1.0, never, &[finger(Action::Down, 1, (1.0, 1.0))]);
        assert_eq!(presses(&down), [(PointerButton::Secondary, true)]);
    }

    #[test]
    fn a_hand_that_lands_while_the_stylus_is_near_is_ignored_until_it_lifts() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Hover, (1.0, 1.0))]);
        let palm = run(
            &mut f,
            1.3,
            TABLET,
            &[finger(Action::Down, 1, (150.0, 150.0))],
        );
        assert_eq!(palm, Vec::<Event>::new(), "{palm:?}");
        let later = run(
            &mut f,
            5.0,
            TABLET,
            &[finger(Action::Move, 1, (210.0, 210.0))],
        );
        assert!(
            later.is_empty(),
            "a palm stays a palm after the stylus has gone"
        );
        let lift = run(
            &mut f,
            5.1,
            TABLET,
            &[finger(Action::Up, 1, (210.0, 210.0))],
        );
        assert_eq!(lift, Vec::<Event>::new());
        // After a while a finger counts again.
        let again = run(
            &mut f,
            6.0,
            TABLET,
            &[finger(Action::Down, 2, (200.0, 200.0))],
        );
        assert_eq!(presses(&again), [(PointerButton::Secondary, true)]);
    }

    #[test]
    fn a_finger_far_from_the_hovering_pen_is_the_other_hand_and_is_meant() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Hover, (100.0, 100.0))]);
        let far = run(
            &mut f,
            1.1,
            TABLET,
            &[finger(Action::Down, 1, (700.0, 500.0))],
        );
        assert_eq!(
            presses(&far),
            [(PointerButton::Secondary, true)],
            "it moves the page at once, and not on the next touch"
        );
    }

    #[test]
    fn a_finger_that_lands_just_after_the_pen_left_the_screen_is_meant() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Down, (100.0, 100.0))]);
        run(&mut f, 1.1, TABLET, &[pen(Action::Up, (100.0, 100.0))]);
        run(&mut f, 1.12, TABLET, &[pen(Action::Hover, (100.0, 100.0))]);
        let gone = run(&mut f, 1.2, TABLET, &[pen(Action::Exit, (100.0, 100.0))]);
        assert_eq!(
            gone,
            [Event::PointerGone],
            "the pointer is gone with the pen"
        );
        let finger_down = run(
            &mut f,
            1.21,
            TABLET,
            &[finger(Action::Down, 1, (140.0, 140.0))],
        );
        assert_eq!(
            presses(&finger_down),
            [(PointerButton::Secondary, true)],
            "a finger counts as soon as the pen has gone, even close to where it was"
        );
    }

    #[test]
    fn the_hand_beside_the_pen_stops_being_a_palm_a_moment_after_the_last_hover() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Hover, (100.0, 100.0))]);
        let soon = run(
            &mut f,
            1.1,
            TABLET,
            &[finger(Action::Down, 1, (140.0, 140.0))],
        );
        assert_eq!(
            soon,
            Vec::<Event>::new(),
            "near, and the pen is still around"
        );
        run(
            &mut f,
            1.2,
            TABLET,
            &[finger(Action::Up, 1, (140.0, 140.0))],
        );
        let later = run(
            &mut f,
            1.0 + PALM_HOLD + 0.05,
            TABLET,
            &[finger(Action::Down, 2, (140.0, 140.0))],
        );
        assert_eq!(
            presses(&later),
            [(PointerButton::Secondary, true)],
            "and counts once the pen has been quiet for a moment"
        );
    }

    #[test]
    fn while_the_pen_touches_the_screen_every_finger_is_the_hand_wherever_it_lands() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Down, (100.0, 100.0))]);
        let anywhere = run(
            &mut f,
            1.1,
            TABLET,
            &[finger(Action::Down, 1, (800.0, 900.0))],
        );
        assert_eq!(anywhere, Vec::<Event>::new());
    }

    #[test]
    fn a_mouse_leaving_the_window_takes_the_pointer_with_it() {
        let mut f = TouchFilter::default();
        let mut leave = sample(Action::Exit, 3, Tool::Mouse, (5.0, 5.0));
        leave.device = 4;
        assert_eq!(run(&mut f, 1.0, TABLET, &[leave]), [Event::PointerGone]);
    }

    #[test]
    fn a_finger_that_was_down_when_the_stylus_touches_is_cancelled() {
        let mut f = TouchFilter::default();
        run(
            &mut f,
            1.0,
            TABLET,
            &[finger(Action::Down, 1, (200.0, 200.0))],
        );
        let events = run(&mut f, 1.2, TABLET, &[pen(Action::Down, (10.0, 10.0))]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Touch {
                device_id: TouchDeviceId(1),
                phase: TouchPhase::Cancel,
                ..
            }
        )));
        assert!(
            presses(&events).contains(&(PointerButton::Secondary, false)),
            "its drag is let go"
        );
        let more = run(
            &mut f,
            1.3,
            TABLET,
            &[finger(Action::Move, 1, (205.0, 205.0))],
        );
        assert_eq!(more, Vec::<Event>::new());
    }

    #[test]
    fn a_very_large_contact_is_a_palm_even_with_no_stylus_around() {
        let mut f = TouchFilter::default();
        let mut big = finger(Action::Down, 1, (100.0, 100.0));
        big.contact = 400.0;
        assert_eq!(run(&mut f, 1.0, PHONE, &[big]), Vec::<Event>::new());
        let mut small = finger(Action::Down, 2, (100.0, 100.0));
        small.contact = 50.0;
        assert_ne!(run(&mut f, 1.1, PHONE, &[small]), Vec::<Event>::new());
    }

    #[test]
    fn palm_rejection_can_be_switched_off() {
        let policy = Policy {
            palm_rejection: false,
            ..TABLET
        };
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, policy, &[pen(Action::Hover, (1.0, 1.0))]);
        let down = run(
            &mut f,
            1.1,
            policy,
            &[finger(Action::Down, 1, (100.0, 100.0))],
        );
        assert_ne!(down, Vec::<Event>::new());
    }

    #[test]
    fn a_second_finger_makes_a_gesture_of_the_two_and_the_first_stops_dragging() {
        let mut f = TouchFilter::default();
        run(
            &mut f,
            1.0,
            TABLET,
            &[finger(Action::Down, 1, (100.0, 100.0))],
        );
        let second = run(
            &mut f,
            1.1,
            TABLET,
            &[finger(Action::Down, 2, (300.0, 100.0))],
        );
        assert_eq!(presses(&second), [(PointerButton::Secondary, false)]);
        assert_eq!(
            touches(&second),
            1,
            "both fingers are touches, for the pinch and the taps"
        );
        let moved = run(
            &mut f,
            1.2,
            TABLET,
            &[finger(Action::Move, 2, (320.0, 100.0))],
        );
        assert!(
            !moved.iter().any(|e| matches!(e, Event::PointerMoved(_))),
            "only the first stands for the pointer"
        );
    }

    #[test]
    fn a_finger_and_a_stylus_may_have_the_same_pointer_number_and_stay_apart() {
        // The pen digitizer and the touchscreen are two devices; each numbers its pointers from zero.
        let mut f = TouchFilter::default();
        let mut pen_down = pen(Action::Down, (10.0, 10.0));
        pen_down.pointer = 0;
        run(&mut f, 1.0, TABLET, &[pen_down]);
        let mut palm = finger(Action::Down, 0, (300.0, 300.0));
        palm.pointer = 0;
        assert!(
            run(&mut f, 1.1, TABLET, &[palm]).is_empty(),
            "the finger is a palm"
        );
        let mut palm_up = finger(Action::Up, 0, (300.0, 300.0));
        palm_up.pointer = 0;
        run(&mut f, 1.2, TABLET, &[palm_up]);
        let mut pen_move = pen(Action::Move, (50.0, 50.0));
        pen_move.pointer = 0;
        let moved = run(&mut f, 1.3, TABLET, &[pen_move]);
        assert!(
            moved.iter().any(|e| matches!(e, Event::PointerMoved(_))),
            "the stylus stroke goes on after the palm lifts: {moved:?}"
        );
    }

    #[test]
    fn samples_are_in_pixels_and_events_in_points() {
        let mut f = TouchFilter::default();
        let events = f.process(
            1.0,
            &[pen(Action::Hover, (300.0, 600.0))],
            3.0,
            TABLET,
            on_paper,
        );
        assert_eq!(events, [Event::PointerMoved(Pos2::new(100.0, 200.0))]);
    }

    #[test]
    fn a_stylus_pressed_with_its_barrel_button_held_drags_the_page_instead_of_drawing() {
        let mut f = TouchFilter::default();
        let mut down = pen(Action::Down, (10.0, 10.0));
        down.buttons = 0x20;
        let events = run(&mut f, 1.0, TABLET, &[down]);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PointerButton {
                    button: PointerButton::Secondary,
                    pressed: true,
                    ..
                }
            )),
            "a secondary press pans: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                Event::PointerButton {
                    button: PointerButton::Primary,
                    ..
                }
            )),
            "and nothing is drawn: {events:?}"
        );
        let up = run(&mut f, 1.1, TABLET, &[pen(Action::Up, (10.0, 10.0))]);
        assert!(
            up.iter().any(|e| matches!(
                e,
                Event::PointerButton {
                    button: PointerButton::Secondary,
                    pressed: false,
                    ..
                }
            )),
            "the button that went down comes up: {up:?}"
        );
    }

    fn presses(events: &[Event]) -> Vec<(PointerButton, bool)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::PointerButton {
                    button, pressed, ..
                } => Some((*button, *pressed)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_pen_held_still_for_the_time_stops_drawing_and_drags_the_page() {
        let mut f = TouchFilter::default();
        let down = run(&mut f, 1.0, HOLDING, &[pen(Action::Down, (50.0, 50.0))]);
        assert_eq!(
            presses(&down),
            [(PointerButton::Primary, true)],
            "it draws to begin with"
        );
        let (early, came) = f.tick(2.5);
        assert!(early.is_empty() && !came, "not yet: {early:?}");
        let (events, came) = f.tick(3.1);
        assert!(came, "the hold came");
        assert_eq!(
            presses(&events),
            [
                (PointerButton::Primary, false),
                (PointerButton::Secondary, true)
            ],
            "the press that draws is let go and the one that pans takes its place"
        );
        let (again, came) = f.tick(3.2);
        assert!(again.is_empty() && !came, "it comes once");
        let moved = run(&mut f, 3.3, HOLDING, &[pen(Action::Move, (50.0, 90.0))]);
        assert!(moved.iter().any(|e| matches!(e, Event::PointerMoved(_))));
        let up = run(&mut f, 3.4, HOLDING, &[pen(Action::Up, (50.0, 90.0))]);
        assert_eq!(
            presses(&up),
            [(PointerButton::Secondary, false)],
            "and lets go of the pan"
        );
    }

    #[test]
    fn a_pen_that_moves_away_is_drawing_and_is_never_held() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, HOLDING, &[pen(Action::Down, (50.0, 50.0))]);
        run(&mut f, 1.2, HOLDING, &[pen(Action::Move, (50.0, 80.0))]);
        let (events, came) = f.tick(5.0);
        assert!(events.is_empty() && !came, "{events:?}");
        assert!(f.pen_hold(5.0).is_none());
    }

    #[test]
    fn a_hand_that_is_not_quite_steady_still_holds() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, HOLDING, &[pen(Action::Down, (50.0, 50.0))]);
        run(&mut f, 1.5, HOLDING, &[pen(Action::Move, (52.0, 51.0))]);
        run(&mut f, 2.0, HOLDING, &[pen(Action::Move, (49.0, 53.0))]);
        let (_, came) = f.tick(3.2);
        assert!(came, "a wobble of a few points is not a move");
    }

    #[test]
    fn with_the_hold_off_a_pen_held_for_ever_only_draws() {
        let mut f = TouchFilter::default();
        run(&mut f, 1.0, TABLET, &[pen(Action::Down, (50.0, 50.0))]);
        let (events, came) = f.tick(60.0);
        assert!(events.is_empty() && !came);
        assert!(f.pen_hold(60.0).is_none());
    }

    #[test]
    fn a_finger_that_draws_can_be_held_too_and_a_finger_that_pans_is_not() {
        let mut drawing = TouchFilter::default();
        let phone = Policy {
            pen_hold: Some(2.0),
            ..PHONE
        };
        run(
            &mut drawing,
            1.0,
            phone,
            &[finger(Action::Down, 0, (50.0, 50.0))],
        );
        let (_, came) = drawing.tick(3.1);
        assert!(came, "a finger that draws is held like a pen");
        let mut panning = TouchFilter::default();
        run(
            &mut panning,
            1.0,
            HOLDING,
            &[finger(Action::Down, 0, (50.0, 50.0))],
        );
        let (events, came) = panning.tick(9.0);
        assert!(
            events.is_empty() && !came,
            "a finger that already moves the page has nothing to turn into"
        );
    }

    #[test]
    fn the_readout_of_a_hold_runs_from_nothing_to_the_whole_wait_and_then_says_it_pans() {
        let mut f = TouchFilter::default();
        assert!(f.pen_hold(0.0).is_none(), "nothing is down");
        run(&mut f, 1.0, HOLDING, &[pen(Action::Down, (50.0, 50.0))]);
        let half = f.pen_hold(2.0).expect("held");
        assert!(
            (half.progress - 0.5).abs() < 1e-4 && !half.panning,
            "{half:?}"
        );
        f.tick(3.5);
        let on = f.pen_hold(3.6).expect("panning");
        assert!(on.panning && (on.progress - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_stylus_without_its_button_draws() {
        let mut f = TouchFilter::default();
        let events = run(&mut f, 1.0, TABLET, &[pen(Action::Down, (10.0, 10.0))]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::PointerButton {
                button: PointerButton::Primary,
                pressed: true,
                ..
            }
        )));
    }

    #[test]
    fn the_filter_says_when_a_stylus_has_been_seen() {
        let mut f = TouchFilter::default();
        assert!(!f.stylus_seen());
        run(&mut f, 1.0, PHONE, &[finger(Action::Down, 0, (5.0, 5.0))]);
        assert!(!f.stylus_seen(), "a finger is not a stylus");
        run(&mut f, 1.1, PHONE, &[pen(Action::Hover, (9.0, 9.0))]);
        assert!(f.stylus_seen());
    }

    #[test]
    fn a_stylus_that_has_never_been_used_leaves_the_fingers_drawing() {
        // A phone, or a tablet whose pen was never taken out of its box: nothing remembered, so a finger draws.
        let mut f = TouchFilter::default();
        let events = run(&mut f, 1.0, PHONE, &[finger(Action::Down, 0, (50.0, 50.0))]);
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                }
            )),
            "{events:?}"
        );
    }

    #[test]
    fn a_mouse_presses_the_buttons_it_reports() {
        let mut f = TouchFilter::default();
        let mut down = sample(Action::Down, 3, Tool::Mouse, (10.0, 10.0));
        down.buttons = 2;
        let events = run(&mut f, 1.0, TABLET, &[down]);
        assert_eq!(presses(&events), [(PointerButton::Secondary, true)]);
        let up = sample(Action::Up, 3, Tool::Mouse, (10.0, 10.0));
        assert_eq!(
            presses(&run(&mut f, 1.1, TABLET, &[up])),
            [(PointerButton::Secondary, false)]
        );
    }

    #[test]
    fn the_scroll_wheel_scrolls() {
        let mut f = TouchFilter::default();
        let wheel = sample(Action::Scroll, 3, Tool::Mouse, (0.0, -2.0));
        let events = run(&mut f, 1.0, TABLET, &[wheel]);
        assert!(matches!(
            events[0],
            Event::MouseWheel { delta, .. } if delta == Vec2::new(0.0, -2.0)
        ));
    }

    #[test]
    fn keys_become_egui_keys_with_their_modifiers_and_text() {
        // Ctrl+Z.
        let undo = KeySample {
            down: true,
            code: 54,
            meta: 0x1000,
            unicode: 0,
            repeat: false,
        };
        let events = key_events(&undo);
        assert!(matches!(
            events[..],
            [Event::Key { key: Key::Z, pressed: true, modifiers, .. }] if modifiers.ctrl && modifiers.command
        ));
        // A plain letter types its character as well.
        let letter = KeySample {
            down: true,
            code: 29,
            meta: 0,
            unicode: u32::from('a'),
            repeat: false,
        };
        let events = key_events(&letter);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1], Event::Text("a".to_owned()));
        // Shift alone makes nothing.
        let shift = KeySample {
            down: true,
            code: 59,
            meta: 1,
            unicode: 0,
            repeat: false,
        };
        assert_eq!(key_events(&shift), Vec::<Event>::new());
        assert_eq!(key_of(131), Some(Key::F1));
        assert_eq!(key_of(7), Some(Key::Num0));
    }

    #[test]
    fn what_waits_in_the_queue_is_taken_once_and_in_order() {
        let _ = take_pointers();
        push_pointer(0, 7, 1, 1, 1.0, 2.0, 0.5, 100.0, 0, 5);
        push_pointer(2, 7, 1, 1, 3.0, 4.0, 0.5, 100.0, 0, 6);
        push_pointer(99, 7, 1, 1, 0.0, 0.0, 0.0, 0.0, 0, 7);
        let taken = take_pointers();
        assert_eq!(taken.len(), 2, "an action nobody knows is dropped");
        assert_eq!(
            (taken[0].action, taken[1].action),
            (Action::Down, Action::Move)
        );
        assert_eq!(take_pointers().len(), 0);
    }
}
