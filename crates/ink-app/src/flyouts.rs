//! Flyouts that open beside the rail: the size and opacity of the pen in use, and the case with every pen.

use eframe::egui::{
    Align, Align2, Area, Color32, Context, CornerRadius, Id, Layout, Margin, Order, RichText,
    ScrollArea, Sense, Slider, Stroke, StrokeKind, Ui, pos2, vec2,
};

use crate::app::InkApp;
use crate::brushes::{self, BRUSHES, BrushInfo, Group};
use crate::config::{BrushConfig, EraserKind, NudgeVariant, SelectShape, SliceMode, ToolsConfig};
use crate::icons::Icon;
use crate::pen_art::{self, Art, Placement};
use crate::rail;
use crate::render;
use crate::settings::Section;
use crate::state::Mode;
use crate::theme;
use crate::widgets;

/// Space kept between a flyout and the top or bottom of the window.
const EDGE: f32 = 16.0;
/// The space between the edge of a flyout and what is in it.
const FLYOUT_MARGIN: f32 = 18.0;
/// Marks a flyout whose height has stopped changing since it opened.
const SETTLED: u32 = u32::MAX;
/// The most frames a flyout stays invisible while its height is found, if it will not stop changing.
const SETTLE_FRAMES: u32 = 6;

/// Shows `content` on a raised surface beside the rail, level with `anchor_y`, easing in and out as `open`
/// changes. Returns what `content` returned while it is showing.
pub fn flyout<R>(
    ctx: &Context,
    app: &mut InkApp,
    name: &'static str,
    open: bool,
    anchor_y: f32,
    estimated_height: f32,
    content: impl FnOnce(&mut Ui, &mut InkApp) -> R,
) -> Option<R> {
    let t = theme::pop(ctx, Id::unique((name, "pop")), open);
    let settling_id = Id::unique((name, "settling"));
    if t <= 0.001 {
        ctx.data_mut(|d| d.remove_temp::<(u64, u32)>(settling_id));
        return None;
    }
    let lefty = app.config.interface.left_handed;
    let screen = ctx.content_rect();
    // How tall a flyout is, is only known once it has been laid out, and that takes a frame or two after it opens:
    // until the height stops changing it is laid out where its height would be guessed. Showing that would make it
    // jump as it opens, so it stays invisible (and out of the way of the pointer) until then.
    let frame = ctx.cumulative_pass_nr();
    let (_, tries) = ctx
        .data(|d| d.get_temp::<(u64, u32)>(settling_id))
        .filter(|&(last, _)| frame.saturating_sub(last) <= 2)
        .unwrap_or((frame, 0));
    let settled = tries == SETTLED || tries >= SETTLE_FRAMES;
    // The height it had last time is the best guess; the first frame uses the estimate.
    let height_id = Id::unique((name, "height"));
    let height = ctx
        .data(|d| d.get_temp::<f32>(height_id))
        .unwrap_or(estimated_height);
    let half = (height / 2.0).min(screen.height() / 2.0 - EDGE);
    let y = anchor_y.clamp(
        EDGE + half,
        (screen.height() - EDGE - half).max(EDGE + half),
    );
    let slide = (1.0 - t) * 14.0;
    // With the rail a dock along the bottom the flyout opens above it, in the middle; beside the rail otherwise.
    let dock_top = app.ui.anchors.dock_top;
    let docked = dock_top > 0.0;
    let (pivot, x, y) = if docked {
        (
            Align2::CENTER_BOTTOM,
            screen.center().x,
            dock_top - EDGE * 0.6 + slide,
        )
    } else if lefty {
        (
            Align2::RIGHT_CENTER,
            screen.right() - rail::flyout_offset() + slide,
            y,
        )
    } else {
        (Align2::LEFT_CENTER, rail::flyout_offset() - slide, y)
    };
    let alpha = if settled { t * app.chrome_alpha } else { 0.0 };
    // However small the window, the flyout is never taller than it: what does not fit scrolls.
    let room = if docked {
        dock_top - screen.top()
    } else {
        screen.height()
    };
    let most = (room - 2.0 * EDGE - 2.0 * FLYOUT_MARGIN).max(120.0);
    let area = Area::new(Id::unique(("flyout", name)))
        .pivot(pivot)
        .fixed_pos(pos2(x, y))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            // An area lays its contents out inside the size it had last time (at first, egui's default of 350 tall),
            // which would clip a scroll area to that and keep it that size. The window is the real limit.
            ui.set_max_height(screen.height());
            theme::floating(ui, [24.0; 4], Margin::same(FLYOUT_MARGIN as i8), |ui| {
                ScrollArea::vertical()
                    .max_height(most)
                    .auto_shrink([true, true])
                    .show(ui, |ui| content(ui, app))
                    .inner
            })
        });
    let shown = area.inner.response.rect;
    ctx.data_mut(|d| d.insert_temp(height_id, shown.height()));
    let steady = (shown.height() - height).abs() < 0.5;
    let tries = if settled || steady {
        SETTLED
    } else {
        tries + 1
    };
    ctx.data_mut(|d| d.insert_temp(settling_id, (frame, tries)));
    if tries < SETTLED {
        ctx.request_repaint();
    }
    if settled {
        app.chrome.push(shown);
    }
    Some(area.inner.inner)
}

pub fn show_dial(ctx: &Context, app: &mut InkApp) {
    let open = app.ui.dial_open && app.tool.mode != Mode::Pan;
    let anchor = app.ui.anchors.pen;
    flyout(ctx, app, "dial", open, anchor, 330.0, |ui, app| {
        ui.set_width(268.0);
        if app.tool.mode == Mode::Draw {
            dial(ui, app);
        } else {
            tool_options(ui, app);
        }
    });
}

/// The name of a tool and what it does, with a button for its settings. Returns `true` when that was pressed.
fn header(ui: &mut Ui, name: &str, blurb: &str, tooltip: &str) -> bool {
    let mut settings = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width() - 44.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(name).size(18.0).strong());
            ui.label(RichText::new(blurb).size(12.0).color(theme::TEXT_DIM));
        });
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            settings = widgets::round_button(ui, 32.0, Icon::Gear, true, false)
                .on_hover_text(tooltip)
                .clicked();
        });
    });
    settings
}

/// What the eraser, the select tool and the slice tool can be set to without leaving the paper.
fn tool_options(ui: &mut Ui, app: &mut InkApp) {
    let [r, g, b] = app.tool.color.to_srgb();
    let ink = Color32::from_rgb(r, g, b);
    let (name, blurb) = match app.tool.mode {
        Mode::Erase if app.config.tools.eraser_kind == EraserKind::Soft => {
            ("Soft eraser", "Rub out only the part you touch")
        }
        Mode::Erase => ("Eraser", "Touch a stroke to remove it"),
        Mode::Select => ("Select", "Loop strokes, then move, scale or turn them"),
        Mode::Nudge => ("Nudge", "Pull or push a stroke into a new shape"),
        _ => ("Slice", "Draw a blade across strokes to cut them"),
    };
    let settings = header(ui, name, blurb, "More settings for the tools");
    ui.add_space(8.0);
    let stored = app.config.tools.clone();
    let tools = &mut app.config.tools;
    match app.tool.mode {
        Mode::Erase => {
            caption_row(ui, "TAKES", "");
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[
                        (EraserKind::Hard, "Whole stroke"),
                        (EraserKind::Soft, "Only a part"),
                    ],
                    &mut tools.eraser_kind,
                );
            });
            ui.add_space(6.0);
            caption_row(ui, "REACH", &format!("{:.0}", tools.eraser_radius));
            let (low, high) = ToolsConfig::ERASER_RANGE;
            widgets::wedge(ui, &mut tools.eraser_radius, low..=high, ink);
        }
        Mode::Select => {
            caption_row(ui, "DRAW A", "");
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[
                        (SelectShape::Lasso, SelectShape::Lasso.label()),
                        (SelectShape::Rectangle, SelectShape::Rectangle.label()),
                    ],
                    &mut tools.select_shape,
                );
            });
            ui.add_space(6.0);
            let label = if tools.select_coverage <= 0.0 {
                "touched".to_owned()
            } else {
                format!("{:.0}% inside", tools.select_coverage * 100.0)
            };
            caption_row(ui, "TAKES A STROKE WHEN", &label);
            widgets::slider(
                ui,
                Slider::new(&mut tools.select_coverage, 0.0..=1.0).show_value(false),
            );
        }
        Mode::Nudge => {
            caption_row(ui, "TOOL", "");
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[
                        (NudgeVariant::Pull, NudgeVariant::Pull.label()),
                        (NudgeVariant::Push, NudgeVariant::Push.label()),
                    ],
                    &mut tools.nudge_variant,
                );
            });
            ui.add_space(6.0);
            caption_row(ui, "REACH", &format!("{:.0}", tools.nudge_reach));
            let (low, high) = ToolsConfig::NUDGE_REACH_RANGE;
            widgets::wedge(ui, &mut tools.nudge_reach, low..=high, ink);
        }
        _ => {
            caption_row(ui, "BLADE", "");
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[
                        (SliceMode::Line, SliceMode::Line.label()),
                        (SliceMode::Freehand, SliceMode::Freehand.label()),
                    ],
                    &mut tools.slice_mode,
                );
            });
        }
    }
    if app.config.tools != stored {
        app.mark_config_changed();
    }
    if settings {
        app.open_settings(Section::Tools);
    }
}

pub fn show_case(ctx: &Context, app: &mut InkApp) {
    let open = app.ui.case_open;
    let anchor = app.ui.anchors.case;
    flyout(ctx, app, "case", open, anchor, 700.0, |ui, app| {
        ui.set_width(3.0 * CARD_WIDTH + 2.0 * CARD_GAP);
        case(ui, app);
    });
}

/// A small label on the left and a value on the right.
fn caption_row(ui: &mut Ui, left: &str, right: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(left).size(11.5).color(theme::TEXT_DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(right).size(13.0).monospace());
        });
    });
}

/// The size and opacity of the pen in use, with a sample at the size it writes.
fn dial(ui: &mut Ui, app: &mut InkApp) {
    let pen = app.tool.pen;
    let info = brushes::info(pen);
    let stored = app.config.brush(pen);
    let mut brush = stored;
    let [r, g, b] = app.tool.color.to_srgb();
    let ink = Color32::from_rgb(r, g, b);
    let mut settings = false;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width() - 44.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(info.name).size(18.0).strong());
            ui.label(RichText::new(info.blurb).size(12.0).color(theme::TEXT_DIM));
        });
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            settings = widgets::round_button(ui, 32.0, Icon::Gear, true, false)
                .on_hover_text("More settings for this pen")
                .clicked();
        });
    });
    ui.add_space(8.0);

    let (sample, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    ui.painter().rect_filled(
        sample,
        CornerRadius::same(16),
        Color32::from_rgb(0xFB, 0xFA, 0xF7),
    );
    render::draw_brush_preview(
        ui.painter(),
        sample,
        pen,
        app.tool.stroke_color(brush.opacity),
        brush.size * app.view.zoom,
    );
    ui.painter().rect_stroke(
        sample,
        CornerRadius::same(16),
        Stroke::new(1.0, Color32::from_black_alpha(40)),
        StrokeKind::Inside,
    );
    ui.add_space(10.0);

    caption_row(ui, "SIZE", &format!("{:.1}", brush.size));
    widgets::wedge(
        ui,
        &mut brush.size,
        BrushConfig::MIN_SIZE..=BrushConfig::MAX_SIZE,
        ink,
    );
    ui.add_space(4.0);
    caption_row(ui, "OPACITY", &format!("{:.0}%", brush.opacity * 100.0));
    widgets::fade_bar(ui, &mut brush.opacity, BrushConfig::MIN_OPACITY..=1.0, ink);

    if brush != stored {
        *app.config.brush_mut(pen) = brush;
        app.mark_config_changed();
    }
    if settings {
        app.open_settings(Section::Brushes);
    }
}

const CARD_WIDTH: f32 = 104.0;
const CARD_HEIGHT: f32 = 88.0;
const CARD_GAP: f32 = 8.0;

/// Every pen, laid out as cards.
fn case(ui: &mut Ui, app: &mut InkApp) {
    ui.label(RichText::new("All pens").size(18.0).strong());
    ui.add_space(6.0);
    let mut picked = None;
    // The case grows with the number of pens, so it scrolls instead of running off the screen.
    let room = (ui.ctx().content_rect().height() - 190.0).clamp(240.0, 640.0);
    ScrollArea::vertical().max_height(room).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(CARD_GAP, CARD_GAP);
        for group in Group::ALL {
            ui.add_space(4.0);
            ui.label(
                RichText::new(group.label().to_uppercase())
                    .size(11.5)
                    .strong()
                    .color(theme::TEXT_DIM),
            );
            let members: Vec<&BrushInfo> = BRUSHES.iter().filter(|b| b.group == group).collect();
            for row in members.chunks(3) {
                ui.horizontal(|ui| {
                    for brush in row {
                        if card(ui, app, brush) {
                            picked = Some(brush.pen);
                        }
                    }
                });
            }
        }
    });
    if let Some(pen) = picked {
        app.select_pen(pen);
        app.tool.mode = Mode::Draw;
        app.ui.close_popovers();
    }
}

/// One pen in the case. Returns `true` when it was tapped.
fn card(ui: &mut Ui, app: &InkApp, brush: &BrushInfo) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::click());
    let selected = app.tool.mode == Mode::Draw && app.tool.pen == brush.pen;
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter().with_clip_rect(rect);
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.16)
    } else {
        theme::CARD.lerp_to_gamma(theme::CHROME_HOVER, hover)
    };
    painter.rect_filled(rect, CornerRadius::same(18), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(18),
        if selected {
            Stroke::new(1.5, theme::ACCENT)
        } else {
            Stroke::new(1.0, Color32::from_white_alpha(14))
        },
        StrokeKind::Inside,
    );
    let [r, g, b] = app.tool.color.to_srgb();
    pen_art::paint(
        &painter,
        Art::of(brush.pen),
        Placement {
            // The card lights up on hover; the pen stays where it is, so it cannot shake under the pointer.
            tip: pos2(rect.right() - 9.0, rect.top() + 36.0),
            scale: 0.8,
            direction: 1.0,
            lift: if selected { 0.6 } else { 0.0 },
        },
        Color32::from_rgb(r, g, b),
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 15.0),
        Align2::CENTER_CENTER,
        brush.name,
        eframe::egui::FontId::proportional(12.5),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    response.on_hover_text(brush.blurb).clicked()
}
