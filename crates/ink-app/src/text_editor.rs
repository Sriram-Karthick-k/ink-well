//! Typing words on the paper itself.
//!
//! There is no box beside the paper or sheet over it: the words appear on the drawing as they are typed, in the ink in use
//! and at the size and place they will have, inside the shape that is selected if there is one (see `text_ops`). A frame
//! and a caret show that they are being typed; two buttons finish or give up; a touch anywhere else on the paper finishes.
//! Where an on-screen keyboard would cover the words, the paper is moved up to keep them in sight and put back after.
//!
//! The same for the computer and for a device used by touch: only the size of the buttons differs (see `density`).

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{
    Align2, Area, Button, Color32, Context, CornerRadius, FontId, Frame, Id, Key, LayerId, Margin,
    Mesh, Order, Painter, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, pos2, vec2,
};
use ink_core::Point;

use crate::app::InkApp;
use crate::density;
use crate::platform;
use crate::text_art::Lettering;
use crate::text_ops::{Placement, TextEntry};
use crate::theme;

/// The space between the letters and the frame round them.
const PAD: f32 = 14.0;
/// The space between the frame and the buttons.
const GAP: f32 = 10.0;
/// How the blinking caret is on: the part of each second.
const CARET_ON: f64 = 0.6;

/// Shows what is being written, while something is.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let Some(mut entry) = app.ui.text.take() else {
        return;
    };
    if entry.doc == 0 {
        entry.doc = app.doc_id;
    }
    // Another drawing is open than the one the words were for: they are not shown over it, and the view is not theirs to
    // put back.
    if entry.doc != app.doc_id {
        return;
    }
    let Some(paper) = app.canvas_rect else {
        app.ui.text = Some(entry);
        return;
    };
    // Something else was opened over the paper: the words are finished.
    if app.ui.any_popover() {
        app.ui.text = Some(entry);
        app.commit_text(ctx);
        return;
    }
    if entry.placing {
        placing(ctx, app, entry, paper);
    } else {
        typing(ctx, app, entry, paper);
    }
}

/// How big a button is: a fingertip for a device used by touch.
fn button_size(ctx: &Context) -> eframe::egui::Vec2 {
    density::get(ctx).choice
}

/// The buttons, side by side with their left edge at `at`: give up (`false`) and, when there is something to finish
/// with, finish (`true`) as the answer.
fn buttons(ctx: &Context, app: &mut InkApp, at: Pos2, finish: Option<&str>) -> Option<bool> {
    let mut answer = None;
    let area = Area::new(Id::unique("text_buttons"))
        .fixed_pos(at)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            theme::floating(ui, [18.0; 4], Margin::same(6), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let size = button_size(ctx);
                    if ui
                        .add(
                            Button::new(RichText::new("Cancel").size(15.0))
                                .fill(theme::CHROME_HI)
                                .min_size(size),
                        )
                        .clicked()
                    {
                        answer = Some(false);
                    }
                    if let Some(finish) = finish
                        && ui
                            .add(
                                Button::new(
                                    RichText::new(finish)
                                        .size(15.0)
                                        .color(Color32::from_rgb(0x1B, 0x1A, 0x18)),
                                )
                                .fill(theme::ACCENT)
                                .min_size(size),
                            )
                            .clicked()
                    {
                        answer = Some(true);
                    }
                });
            });
        });
    // A press on the buttons is not a press outside the panels.
    app.chrome.push(area.response.rect);
    answer
}

/// Waiting for a touch of the paper to say where the words go.
fn placing(ctx: &Context, app: &mut InkApp, mut entry: TextEntry, paper: Rect) {
    let catcher = Area::new(Id::unique("text_place"))
        .fixed_pos(paper.min)
        .order(Order::Middle)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(paper.size(), Sense::click());
            response
        });
    let hint = Area::new(Id::unique("text_hint"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, paper.top() + 16.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            theme::floating(ui, [18.0; 4], Margin::symmetric(16, 10), |ui| {
                ui.label(RichText::new("Touch where the words go").size(16.0));
            });
        });
    app.chrome.push(hint.response.rect);
    let side = hint.response.rect;
    let answer = buttons(
        ctx,
        app,
        pos2(side.center().x - 40.0, side.bottom() + GAP),
        None,
    );
    if answer == Some(false) || ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.ui.text = Some(entry);
        app.cancel_text();
        return;
    }
    if let Some(at) = catcher
        .inner
        .interact_pointer_pos()
        .filter(|_| catcher.inner.clicked())
    {
        entry.at = Some(app.view.to_canvas(at));
        entry.placing = false;
    }
    app.ui.text = Some(entry);
}

/// The letters of `plan` as one mesh on the screen: each region is cut into triangles on the paper, so a turned paper
/// turns them too.
fn paint_letters(
    painter: &Painter,
    app: &InkApp,
    letters: &Lettering,
    placement: Placement,
    color: Color32,
) -> Rect {
    let to_paper = |p: &[f32; 2]| {
        Point::new(
            placement.center.x + p[0] * placement.scale,
            placement.center.y + p[1] * placement.scale,
        )
    };
    let mut mesh = Mesh::default();
    let mut bounds = Rect::NOTHING;
    for region in &letters.regions {
        let points: Vec<Point> = region.iter().map(to_paper).collect();
        for p in &points {
            bounds.extend_with(app.view.to_screen(*p));
        }
        let triangles = ink_brush::fill::triangulate(&points);
        let base = mesh.vertices.len() as u32;
        for v in triangles.as_chunks::<2>().0 {
            mesh.colored_vertex(app.view.to_screen(Point::new(v[0], v[1])), color);
        }
        mesh.indices.extend(base..mesh.vertices.len() as u32);
    }
    painter.add(mesh);
    bounds
}

/// The ink in use, as the color the letters are drawn in; the color of the words being changed when there are some.
fn letter_color(app: &InkApp, entry: &TextEntry, placeholder: bool) -> Color32 {
    let [r, g, b] = app.tool.color.to_srgb();
    let color = entry.color().map_or(Color32::from_rgb(r, g, b), |c| {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
    });
    if placeholder {
        color.gamma_multiply(0.3)
    } else {
        color
    }
}

/// The caret: after the last letter, or at the start of the hint while nothing is typed. It blinks.
fn paint_caret(
    ctx: &Context,
    painter: &Painter,
    app: &InkApp,
    letters: &Lettering,
    placement: Placement,
    placeholder: bool,
) {
    let caret_x = if placeholder {
        letters
            .regions
            .iter()
            .flatten()
            .map(|p| p[0])
            .fold(f32::MAX, f32::min)
    } else {
        letters.caret[0]
    };
    let at = |x: f32, y: f32| {
        app.view.to_screen(Point::new(
            placement.center.x + x * placement.scale,
            placement.center.y + y * placement.scale,
        ))
    };
    let (top, bottom) = (
        at(caret_x, letters.caret[1] + 0.78 * letters.line),
        at(caret_x, letters.caret[1] - 0.22 * letters.line),
    );
    if ctx.input(|i| i.time).fract() < CARET_ON {
        painter.line_segment([top, bottom], Stroke::new(2.0, theme::ACCENT));
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

/// What takes the typing: a field where the frame is that shows nothing, for the letters on the paper are what is seen.
fn type_into(ctx: &Context, entry: &mut TextEntry, frame: Rect) {
    let id = Id::unique("text_field");
    Area::new(Id::unique("text_input"))
        .fixed_pos(frame.min)
        .order(Order::Foreground)
        .interactable(true)
        .constrain(false)
        .show(ctx, |ui| {
            ui.scope(|ui| {
                let visuals = ui.visuals_mut();
                visuals.selection.bg_fill = Color32::TRANSPARENT;
                visuals.selection.stroke = Stroke::NONE;
                visuals.text_cursor.stroke = Stroke::NONE;
                let field = ui.add_sized(
                    frame.size().max(vec2(40.0, 24.0)),
                    TextEdit::multiline(&mut entry.text)
                        .id(id)
                        .frame(Frame::NONE)
                        .font(FontId::proportional(14.0))
                        .text_color(Color32::TRANSPARENT)
                        .desired_rows(1),
                );
                // Asked once: asking interrupts the keyboard (it is hidden and shown again), so asking every frame would
                // keep it from ever staying.
                if !field.has_focus() {
                    field.request_focus();
                }
            });
        });
    // Letters are added and taken away at the end: the caret does not wander into the middle of them.
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = CCursorRange::one(CCursor::new(entry.text.chars().count()));
    if state.cursor.char_range() != Some(end) {
        state.cursor.set_char_range(Some(end));
        state.store(ctx, id);
    }
}

/// The words are being typed: shown as they will be, with a frame and a caret, and the buttons.
fn typing(ctx: &Context, app: &mut InkApp, mut entry: TextEntry, paper: Rect) {
    let Some((letters, placement, placeholder)) = app.plan_text(&entry) else {
        app.ui.text = Some(entry);
        return;
    };
    let painter = ctx
        .layer_painter(LayerId::new(Order::Middle, Id::unique("text_preview")))
        .with_clip_rect(paper);
    let color = letter_color(app, &entry, placeholder);
    let ink = paint_letters(&painter, app, &letters, placement, color);

    // The frame, a little bigger than the letters.
    let frame = ink.expand(PAD);
    painter.rect_filled(
        frame,
        CornerRadius::same(12),
        theme::ACCENT.gamma_multiply(0.07),
    );
    painter.rect_stroke(
        frame,
        CornerRadius::same(12),
        Stroke::new(1.5, theme::ACCENT),
        StrokeKind::Outside,
    );

    paint_caret(ctx, &painter, app, &letters, placement, placeholder);
    type_into(ctx, &mut entry, frame);

    // The buttons, over the frame if there is room and under it if there is not.
    let size = button_size(ctx);
    let high = frame.top() - GAP - size.y - 12.0;
    let y = if high >= paper.top() + 4.0 {
        high
    } else {
        frame.bottom() + GAP
    };
    let x = (frame.center().x - size.x - 10.0)
        .clamp(paper.left() + 6.0, paper.right() - 2.0 * size.x - 30.0);
    let answer = buttons(
        ctx,
        app,
        pos2(x, y),
        Some(if entry.editing { "Done" } else { "Add" }),
    );
    keep_in_sight(ctx, app, frame, paper, size.y);

    // A touch of the paper outside the frame finishes the words.
    let catcher = Area::new(Id::unique("text_outside"))
        .fixed_pos(paper.min)
        .order(Order::Middle)
        .show(ctx, |ui| {
            ui.allocate_exact_size(paper.size(), Sense::click()).1
        });
    let outside = catcher.inner.clicked()
        && catcher
            .inner
            .interact_pointer_pos()
            .is_some_and(|p| !frame.contains(p));
    let enter = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    app.ui.text = Some(entry);
    if escape || answer == Some(false) {
        app.cancel_text();
    } else if outside || enter || answer == Some(true) {
        app.commit_text(ctx);
    }
}

/// Moves the paper up, a little each frame, while the on-screen keyboard (and the buttons) would cover `frame`.
fn keep_in_sight(ctx: &Context, app: &mut InkApp, frame: Rect, paper: Rect, button: f32) {
    let lift = platform::keyboard_lift(ctx);
    if lift <= 0.0 {
        return;
    }
    let clear = paper.bottom() - lift - 12.0;
    let covered = frame.bottom() + GAP - clear;
    // The buttons are over the frame, so only what is under the keyboard matters; keep the buttons on the screen too.
    let too_high = paper.top() + button + 2.0 * GAP - frame.top();
    if covered > 1.0 {
        app.view.origin.y -= (covered * 0.3).max(1.0).min(covered);
        ctx.request_repaint();
    } else if too_high > 1.0 {
        app.view.origin.y += (too_high * 0.3).max(1.0).min(too_high);
        ctx.request_repaint();
    }
}
