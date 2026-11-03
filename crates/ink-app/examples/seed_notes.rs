//! Writes sample drawings, in a few folders, for trying out the Library.
//!
//! ```sh
//! cargo run -p ink-app --example seed_notes -- some/folder
//! cargo run -p ink-app -- --library-folder some/folder --library
//! ```

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ink_core::{Color, Editor, IdGen, Pen, Point, StrokeBuilder};
use ink_format::WriteOptions;

/// A small deterministic pseudo-random sequence, so the sample notes are the same every time.
struct Noise(u64);

impl Noise {
    /// The next value in `0.0..1.0`.
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / (1_u64 << 24) as f32
    }
}

fn stroke(editor: &mut Editor, pen: Pen, width: f32, color: Color, points: &[Point]) {
    let mut builder = StrokeBuilder::new(pen, Some(width), color, 1.0, 0.1);
    for (i, p) in points.iter().enumerate() {
        builder.push(*p, None, i as f64 * 0.008);
    }
    let finished = builder.with_id(editor.next_id()).finish();
    editor.commit_stroke(0, finished);
}

const INK: Color = Color::new(0.09, 0.09, 0.07, 1.0);
const BLUE: Color = Color::new(0.18, 0.48, 0.88, 1.0);
const RED: Color = Color::new(0.9, 0.29, 0.29, 1.0);
const GREEN: Color = Color::new(0.35, 0.72, 0.36, 1.0);
const AMBER: Color = Color::new(0.95, 0.7, 0.2, 0.8);

/// Lines of handwriting-like scribble.
fn lines_of_text(editor: &mut Editor, lines: usize, noise: &mut Noise) {
    for line in 0..lines {
        let y = -(line as f32) * 34.0;
        let mut x = 0.0;
        let end = 330.0 - noise.next() * 120.0;
        while x < end {
            let word = 20.0 + noise.next() * 50.0;
            let wiggles = 5.0 + noise.next() * 3.0;
            let points: Vec<Point> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    Point::new(x + t * word, y + (t * wiggles * TAU).sin() * 7.0)
                })
                .collect();
            stroke(editor, Pen::Fountain, 2.6, INK, &points);
            x += word + 14.0;
        }
    }
}

fn waves(editor: &mut Editor) {
    for (k, color) in [BLUE, GREEN, RED].into_iter().enumerate() {
        let points: Vec<Point> = (0..=80)
            .map(|i| {
                let x = i as f32 * 4.0;
                Point::new(
                    x,
                    (x / 38.0 + k as f32 * 1.2).sin() * 46.0 - k as f32 * 36.0,
                )
            })
            .collect();
        stroke(editor, Pen::Marker, 12.0, color, &points);
    }
}

fn spirals(editor: &mut Editor) {
    for (cx, color) in [(0.0, BLUE), (150.0, RED), (300.0, GREEN)] {
        let points: Vec<Point> = (0..=140)
            .map(|i| {
                let t = i as f32 / 140.0;
                let a = t * 5.0 * TAU;
                Point::new(cx + a.cos() * t * 62.0, a.sin() * t * 62.0)
            })
            .collect();
        stroke(editor, Pen::Pen, 3.0, color, &points);
    }
}

/// A straight line from `a` to `b`, as many small steps.
fn line(a: Point, b: Point) -> Vec<Point> {
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        })
        .collect()
}

fn floor_plan(editor: &mut Editor) {
    let rooms = [
        (0.0, 0.0, 180.0, 120.0),
        (180.0, 0.0, 120.0, 120.0),
        (0.0, -120.0, 120.0, 120.0),
        (120.0, -120.0, 180.0, 120.0),
    ];
    for (x, y, w, h) in rooms {
        let corners = [
            Point::new(x, y),
            Point::new(x + w, y),
            Point::new(x + w, y - h),
            Point::new(x, y - h),
            Point::new(x, y),
        ];
        let outline: Vec<Point> = corners
            .windows(2)
            .flat_map(|pair| line(pair[0], pair[1]))
            .collect();
        stroke(editor, Pen::Wire, 1.5, INK, &outline);
    }
    stroke(
        editor,
        Pen::Marker,
        8.0,
        AMBER,
        &line(Point::new(170.0, -40.0), Point::new(190.0, -40.0)),
    );
}

fn table(editor: &mut Editor, noise: &mut Noise) {
    for row in 0..=4 {
        let y = -(row as f32) * 44.0;
        stroke(
            editor,
            Pen::Pen,
            2.0,
            INK,
            &line(Point::new(0.0, y), Point::new(320.0, y)),
        );
    }
    for col in 0..=4 {
        let x = col as f32 * 80.0;
        stroke(
            editor,
            Pen::Pen,
            2.0,
            INK,
            &line(Point::new(x, 0.0), Point::new(x, -176.0)),
        );
    }
    for row in 0..4 {
        for col in 0..4 {
            if noise.next() < 0.6 {
                let (x, y) = (col as f32 * 80.0 + 14.0, -(row as f32) * 44.0 - 22.0);
                let points: Vec<Point> = (0..=14)
                    .map(|i| {
                        let t = i as f32 / 14.0;
                        Point::new(x + t * 46.0, y + (t * 4.0 * TAU).sin() * 5.0)
                    })
                    .collect();
                stroke(editor, Pen::Fountain, 2.4, BLUE, &points);
            }
        }
    }
}

fn doodles(editor: &mut Editor, noise: &mut Noise) {
    for i in 0..9 {
        let (cx, cy) = ((i % 3) as f32 * 110.0, -((i / 3) as f32) * 100.0);
        let loops = 2.0 + noise.next() * 3.0;
        let radius = 26.0 + noise.next() * 14.0;
        let points: Vec<Point> = (0..=60)
            .map(|k| {
                let t = k as f32 / 60.0;
                let a = t * loops * TAU;
                Point::new(
                    cx + a.cos() * radius * (0.5 + t * 0.5),
                    cy + a.sin() * radius,
                )
            })
            .collect();
        let color = [BLUE, RED, GREEN, INK][i % 4];
        stroke(editor, Pen::SoftPencil, 4.0, color, &points);
    }
}

fn write_note(folder: &Path, name: &str, build: impl FnOnce(&mut Editor), age: u64) -> PathBuf {
    let mut editor = Editor::blank(IdGen::new(name.len() as u64 * 977 + 13));
    build(&mut editor);
    let bytes = ink_format::write(
        editor.document(),
        &ink_format::Extras::default(),
        &WriteOptions {
            now_unix_ms: 1_760_000_000_000,
        },
    )
    .expect("the document can be written");
    let path = folder.join(format!("{name}.ink"));
    std::fs::write(&path, bytes).expect("the note can be saved");
    let file = std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("the note can be opened");
    let when = SystemTime::now()
        .checked_sub(Duration::from_secs(age))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    file.set_modified(when).expect("the time can be set");
    path
}

fn main() {
    let folder = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: seed_notes <folder to write the notes into>"),
    );
    let (minute, hour, day) = (60, 3600, 86_400);
    let work = folder.join("Work");
    let school = folder.join("School");
    let sketches = folder.join("Sketches");
    for dir in [
        &work,
        &work.join("Meetings"),
        &school,
        &sketches,
        &folder.join("Archive"),
    ] {
        std::fs::create_dir_all(dir).expect("the folder can be created");
    }
    write_note(
        &work,
        "Meeting notes",
        |e| lines_of_text(e, 7, &mut Noise(11)),
        4 * minute,
    );
    write_note(&work, "Floor plan", floor_plan, 3 * hour);
    write_note(
        &work,
        "Weekly planner",
        |e| table(e, &mut Noise(3)),
        day + 2 * hour,
    );
    write_note(
        &work.join("Meetings"),
        "Standup",
        |e| lines_of_text(e, 3, &mut Noise(5)),
        5 * day,
    );
    write_note(&sketches, "Sketch - waves", waves, 2 * day);
    write_note(&sketches, "Doodles", |e| doodles(e, &mut Noise(7)), 9 * day);
    write_note(&sketches, "Spirals", spirals, 20 * day);
    write_note(
        &school,
        "Math homework",
        |e| lines_of_text(e, 5, &mut Noise(29)),
        4 * day,
    );
    write_note(
        &folder,
        "Grocery list",
        |e| lines_of_text(e, 4, &mut Noise(41)),
        40 * day,
    );
    write_note(&folder, "Empty page", |_| {}, 60 * day);
    println!("wrote 10 drawings in 5 folders to {}", folder.display());
}
