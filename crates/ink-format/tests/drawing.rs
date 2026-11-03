//! A drawing written and read back is the drawing that was written.

use std::io::{Cursor, Read, Write};

use ink_core::artboard::Artboard;
use ink_core::guide::{Guide, Kind};
use ink_core::marks::Marks;
use ink_core::{Color, Editor, IdGen, LayerLabel, Pen, Point, StrokeBuilder};
use ink_format::{Extras, FormatError, WriteOptions, read, write};
use uuid::Uuid;
use zip::ZipArchive;

const NOW: WriteOptions = WriteOptions {
    now_unix_ms: 1_793_393_673_371,
};

/// A drawing with two layers and strokes of three kinds: a plain one, one with pressure and one in a pen this
/// version has no name for.
fn drawing() -> Editor {
    let mut editor = Editor::blank(IdGen::new(11));
    let line = |from: (f32, f32), pen: Pen, pressure: bool, editor: &mut Editor| {
        let mut builder = StrokeBuilder::new(pen, Some(4.0), Color::BLACK, 1.0, 0.0);
        for i in 0..=12 {
            let t = i as f32 / 12.0;
            builder.push(
                Point::new(from.0 + 90.0 * t, from.1 + 30.0 * t * t),
                pressure.then_some(0.2 + 0.6 * t),
                f64::from(i) * 0.01,
            );
        }
        builder.with_id(editor.next_id()).finish()
    };
    let first = line((0.0, 0.0), Pen::Fountain, false, &mut editor);
    editor.commit_stroke(0, first);
    let second = line((0.0, 40.0), Pen::Marker, true, &mut editor);
    editor.commit_stroke(0, second);
    editor.add_layer(None);
    let third = line((0.0, 80.0), Pen::Unknown(77), false, &mut editor);
    editor.commit_stroke(1, third);
    editor.set_layer_visible(1, false);
    editor.rename_layer(1, "Notes".to_owned());
    editor
}

fn extras(editor: &Editor) -> Extras {
    let ids: Vec<Uuid> = editor.document().layers[0]
        .strokes
        .iter()
        .map(|s| s.id)
        .collect();
    let mut marks = Marks::default();
    marks.lock([ids[0]]);
    let guide = Guide::new(Kind::Circle, Point::new(5.0, 6.0), 100.0);
    Extras::of(
        Some(Artboard::centered(Point::new(0.0, 0.0), 300.0, 200.0)),
        &[guide],
        &marks,
    )
}

fn entries(bytes: &[u8]) -> Vec<String> {
    let zip = ZipArchive::new(Cursor::new(bytes)).expect("an archive");
    zip.file_names().map(str::to_owned).collect()
}

fn text(bytes: &[u8], name: &str) -> String {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("an archive");
    let mut out = String::new();
    zip.by_name(name)
        .expect("the entry")
        .read_to_string(&mut out)
        .expect("text");
    out
}

#[test]
fn what_is_written_is_read_back_with_every_layer_stroke_and_extra() {
    let editor = drawing();
    let extras = extras(&editor);
    let bytes = write(editor.document(), &extras, &NOW).expect("written");
    let back = read(&bytes).expect("read");
    assert_eq!(back.extras, extras);
    let (before, after) = (editor.document(), &back.document);
    assert_eq!(
        before.layers, after.layers,
        "layers, names, visibility, strokes, pens, pressure"
    );
    assert_eq!(after.layers[1].label, LayerLabel::Name("Notes".to_owned()));
    assert!(!after.layers[1].visible);
    assert_eq!(after.layers[1].strokes[0].pen, Pen::Unknown(77));
    assert_eq!(
        after.layers[0].strokes[1].pressure.len(),
        13,
        "one pressure for each sample"
    );
    assert_eq!(after.metadata.identifier, before.metadata.identifier);
    assert_eq!(after.metadata.format_version, 1);
}

#[test]
fn the_times_are_set_when_it_is_written_and_the_creation_time_is_kept_after_that() {
    let editor = drawing();
    let bytes = write(editor.document(), &Extras::default(), &NOW).expect("written");
    let first = read(&bytes).expect("read").document;
    assert_eq!(first.metadata.creation_time, "2026-10-30T20:54:33.371Z");
    assert_eq!(first.metadata.modification_time, "2026-10-30T20:54:33.371Z");
    let later = WriteOptions {
        now_unix_ms: NOW.now_unix_ms + 60_000,
    };
    let again = read(&write(&first, &Extras::default(), &later).expect("written")).expect("read");
    assert_eq!(
        again.document.metadata.creation_time,
        "2026-10-30T20:54:33.371Z"
    );
    assert_eq!(
        again.document.metadata.modification_time,
        "2026-10-30T20:55:33.371Z"
    );
}

#[test]
fn the_archive_holds_a_manifest_the_document_and_extras_only_when_there_are_some() {
    let editor = drawing();
    let with = write(editor.document(), &extras(&editor), &NOW).expect("written");
    let mut names = entries(&with);
    names.sort();
    assert_eq!(names, ["document.json", "extras.json", "manifest.json"]);
    let without = write(editor.document(), &Extras::default(), &NOW).expect("written");
    let mut names = entries(&without);
    names.sort();
    assert_eq!(names, ["document.json", "manifest.json"]);
    let manifest = text(&with, "manifest.json");
    assert!(
        manifest.contains(r#""format":"ink""#) && manifest.contains(r#""version":1"#),
        "{manifest}"
    );
}

#[test]
fn damaged_extras_cost_the_extras_and_not_the_drawing() {
    let editor = drawing();
    let bytes = write(editor.document(), &extras(&editor), &NOW).expect("written");
    // The same archive with its extras replaced by nonsense.
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).expect("an archive");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).expect("entry");
        let name = file.name().to_owned();
        let mut content = Vec::new();
        file.read_to_end(&mut content).expect("bytes");
        if name == "extras.json" {
            content = b"{ nonsense".to_vec();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("entry");
        out.write_all(&content).expect("bytes");
    }
    let damaged = out.finish().expect("archive").into_inner();
    let back = read(&damaged).expect("the drawing still opens");
    assert!(back.extras.is_empty());
    assert_eq!(back.document.layers.len(), 2);
}

#[test]
fn a_file_that_is_not_a_drawing_says_so() {
    assert!(matches!(
        read(b"%PDF-1.7 not a drawing"),
        Err(FormatError::Zip(_))
    ));
    let message = read(b"").expect_err("empty").to_string();
    assert!(message.starts_with("not a drawing"), "{message}");
}
