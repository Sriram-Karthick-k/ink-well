//! Desktop entry point. Android uses the library crate directly.
// A release build for Windows is a window app: it does not open a console behind the window.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;

/// A pen by its lowercase name, without spaces.
fn pen_named(name: &str) -> Option<ink_core::Pen> {
    use ink_core::Pen;
    Some(match name.to_ascii_lowercase().as_str() {
        "pen" => Pen::Pen,
        "fountain" => Pen::Fountain,
        "dynamic" | "dynamicpen" => Pen::DynamicPen,
        "fixed" | "fixedwidth" => Pen::FixedWidth,
        "wire" => Pen::Wire,
        "softpencil" => Pen::SoftPencil,
        "hardpencil" => Pen::HardPencil,
        "marker" => Pen::Marker,
        "watercolor" => Pen::Watercolor,
        "airbrush" => Pen::Airbrush,
        "fill" => Pen::Fill,
        "dotted" => Pen::Dotted,
        "highlighter" => Pen::Highlighter,
        "brushpen" => Pen::BrushPen,
        "felttip" => Pen::FeltTip,
        "charcoal" => Pen::Charcoal,
        "chalk" => Pen::Chalk,
        "crayon" => Pen::Crayon,
        "pastel" => Pen::Pastel,
        "spray" | "spraypaint" => Pen::SprayPaint,
        "stipple" => Pen::Stipple,
        "inkbrush" => Pen::InkBrush,
        _ => {
            eprintln!("unknown pen {name}");
            return None;
        }
    })
}

/// `40,300` as a place on the screen.
fn point(text: &str) -> Option<[f32; 2]> {
    let (x, y) = text.split_once(',')?;
    Some([x.parse().ok()?, y.parse().ok()?])
}

/// `800x520` as the size of a window.
fn window_size(text: &str) -> Option<[f32; 2]> {
    let (w, h) = text.split_once('x')?;
    Some([w.parse().ok()?, h.parse().ok()?])
}

fn main() -> eframe::Result {
    let mut options = ink_app::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--open" => options.open.extend(args.next().map(PathBuf::from)),
            "--screenshot" => options.screenshot = args.next().map(PathBuf::from),
            "--library" => options.library = true,
            "--library-demo" => options.library_demo = args.next(),
            "--library-folder" => options.library_folder = args.next().map(PathBuf::from),
            "--dial" => options.show_dial = true,
            "--menu" => options.show_menu = true,
            "--precision" => options.show_precision = true,
            "--snap" => options.snap = true,
            "--guide" => options.guides.extend(args.next()),
            "--artboard" => options.artboard = args.next(),
            "--turn" => options.turn = args.next().and_then(|v| v.parse().ok()),
            "--picker" => options.show_color_picker = true,
            "--brushes" => options.show_brushes = true,
            "--settings" => options.settings = Some(args.next().unwrap_or_default()),
            "--left-handed" => options.left_handed = true,
            "--config" => options.config = args.next().map(PathBuf::from),
            "--pen" => options.pen = args.next().and_then(|name| pen_named(&name)),
            "--size" => options.size = args.next().and_then(|v| v.parse().ok()),
            "--window" => options.window = args.next().and_then(|v| window_size(&v)),
            "--hover" => options.hover = args.next().and_then(|v| point(&v)),
            "--cpu" => options.cpu_render = true,
            "--textures" => options.textures = args.next().map(PathBuf::from),
            "--demo-stroke" => options.demo_stroke = true,
            "--demo-select" => options.demo_select = true,
            other if !other.starts_with('-') => options.open.push(PathBuf::from(other)),
            other => eprintln!("ignoring unknown option {other}"),
        }
    }
    ink_app::run(options)
}
