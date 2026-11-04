<p align="center"><img src="assets/logo/inkwell.svg" alt="inkwell logo" width="128"></p>

# inkwell

A cross-platform sketching app written in Rust. Android tablets with a stylus are the end goal; the Windows
desktop app comes first. The goal is a smooth, high-quality inking engine that can read and write a documented
document format.

**Status:** the Windows app and the Android app work (pens, layers, library with folders, editing tools, precision
tools, shapes with fill, text, a tool bar you arrange yourself, and an interface made for a finger and a stylus on
Android). The web build is still to come. This is an independent implementation. It is not affiliated with any other
drawing app, and it contains no third-party assets. See `docs/` for the design.

## Download

Windows 10 and 11 (64-bit) and Android 8 or newer (64-bit ARM) from the
[Releases](https://github.com/Sriram-Karthick-k/ink-well/releases) page, or directly, always the newest:

- [inkwell-windows-x64.zip](https://github.com/Sriram-Karthick-k/ink-well/releases/latest/download/inkwell-windows-x64.zip)
  (the app with a short readme)
- [inkwell-windows-x64.exe](https://github.com/Sriram-Karthick-k/ink-well/releases/latest/download/inkwell-windows-x64.exe)
  (just the file)

- [inkwell-android-arm64.apk](https://github.com/Sriram-Karthick-k/ink-well/releases/latest/download/inkwell-android-arm64.apk)
  (the Android app)

Run the exe; it is one file with nothing to install. The download is not code signed, so Windows SmartScreen may ask you to confirm once ("More info", "Run anyway").
To install the APK, open it on the device and allow installs from the app you opened it with when Android asks;
it is not from a store. `SHA256SUMS.txt` has the checksums of every file. Releases are built by
`.github/workflows/release.yml` when a tag such as `v0.1.0` or `RELEASE_1.1_BETA` is pushed; the notes of a release are
the file `release-notes/<tag>.md` when there is one. Run that workflow by hand to get a build without publishing it.

To build the APK yourself see `android/build_apk.py`, which also says how to sign it with a key of your own.

## Layout

| Path | Purpose |
|---|---|
| `crates/ink-core` | Document model and stroke smoothing. Pure Rust: no I/O, no `unsafe`. |
| `crates/ink-format` | Reads and writes the `.ink` drawing file (a ZIP of JSON) as the core model. |
| `crates/ink-brush` | Brush parameters and CPU-side stamp placement. No GPU code. |
| `crates/ink-wasm` | WebAssembly bindings for a future web UI. |
| `crates/ink-gpu` | The OpenGL stroke renderer (stamps, mask, grain, composite). |
| `crates/ink-app` | The native app (egui): canvas, menus, brushes, color and layers panels, settings window. |
| `docs/` | `ARCHITECTURE.md` (design), `FORMAT.md` (the `.ink` file format), and what the precision tools, editing tools and settings do. |
| `assets/logo/` | The logo (SVG, PNG, ICO) and `make_logo.py`, which draws all of them. |
| `packaging/` | What goes in the Windows download besides the exe. |
| `android/` | The Android app's activity, manifest, icons and `build_apk.py`, which builds and signs the APK. |
| `crates/ink-android` | The Android entry point: hands touch, keys and files from the system to `ink-app`. |
| `release-notes/` | The notes of each release, named by its tag. |

## Build

 The toolchain is pinned in `rust-toolchain.toml`; `rustup` installs it on first use.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p ink-wasm --target wasm32-unknown-unknown --release   # web
python android/build_apk.py                                          # Android APK (see the script for what it needs)
```

## Run

```sh
cargo run -p ink-app                                   # blank canvas
cargo run -p ink-app -- path/to/drawing.ink              # open a file (or drop one on the window)
cargo run -p ink-app -- --cpu                            # draw with egui shapes instead of the GPU renderer
cargo run -p ink-app -- --textures path/to/textures       # optional stamp textures for the pencils and airbrush
cargo run -p ink-app -- --config path/to/config.json     # use another settings file
cargo run -p ink-app -- a.ink b.ink                      # open files, one tab each
cargo run -p ink-app -- --library-folder path/to/folder   # keep drawings in another folder
cargo run -p ink-app --example seed_notes -- path/to/folder   # sample drawings in folders, to try the Library
```

For checking the layout, `--library`, `--library-demo <menu|move|rename|undo|search|folder>`, `--brushes`, `--dial`, `--menu`, `--picker`, `--settings <section>`, `--left-handed`,
`--precision`, `--snap`, `--guide <line|arc|angle|circle|square>`, `--turn <degrees>`, `--artboard <preset>`, `--demo-select`,
`--pen <name>`, `--window 800x520` and `--screenshot out.png` start with a panel open and save a picture (screenshot runs read the
defaults and save no settings unless `--config` is given).

### Interface

The paper fills the window; everything else floats over it and fades while you write.

- **Tabs** in a toolbar docked along the top, like a browser's or Figma's: the **Library** is the first tab,
  then one tab for every open drawing, each with its own canvas, zoom and undo history. A dot marks unsaved
  changes, a cross (or a middle click) closes a tab, tabs drag into another order, and **+** opens a new one.
  Ctrl+T new tab, Ctrl+W close, Ctrl+Tab and Ctrl+Shift+Tab move through them, Ctrl+1 to 9 jump to one.
  Opening a drawing that is already open just switches to its tab. Several files on the command line open as
  several tabs.
- **Library** (the home screen, also Ctrl+L): your drawings in real **folders** that nest as deep as you like.
  The top shows what you edited **recently** across every folder, then the folders (each shows the newest
  drawings in it), then the drawings. A breadcrumb trail leads back up. Make folders, rename, duplicate and
  delete (a hidden `.deleted` folder with an Undo for a few seconds), and organize by **dragging** a drawing or
  folder onto a folder or onto a step of the trail, or with Move to. Search looks in every folder. Import copies
  `.ink` files in and never touches the originals. Everything lives in one library folder
  (`Documents\inkwell` by default, changeable in Settings, Files) and **saves itself** a moment after every
  change; a new drawing leaves no file until something is drawn. Files opened from elsewhere are never saved
  without asking.
- **Pen rail** on the screen edge: undo and redo, five pens drawn as objects lying in slots (each shows the
  current ink on its tip), the eraser, the case button, the ink well and a hand for moving the canvas. The pen
  in use is pulled out of the rail; tap it again for its size and opacity scrubbers, with a sample stroke at
  the true size. The case opens every pen; choosing one that is not on the rail swaps it in for the last slot.
- **Ink**: tap the well for a hue ring around a saturation square, a hex field, four palettes and recent colors.
- **Eraser**: touching a stroke removes it (hard), or only what the eraser passes over (soft, `Shift+E`; `Alt+E`
  for hard); one sweep is one step of undo.
- **Drawing menu** (the lines button at the right of the toolbar): Library, new drawing, open a file, save,
  save as, the paper pattern (blank, dots, grid, lines), settings and shortcuts. Layers and settings sit next to
  it, and the zoom level is at the bottom right.

Other shortcuts: `B` all pens, `E` eraser, `L` layers, `Esc` closes a flyout, Ctrl+L the Library, Ctrl+, the settings.

Every option is a setting, saved as JSON as it is changed (`%APPDATA%\inkwell\config.json` on Windows,
`~/.config/inkwell/config.json` on Linux, `~/Library/Application Support/inkwell/config.json` on macOS):
per-brush size, opacity and smoothing; the paper pattern, its spacing and the paper color; pen pressure, two-finger gestures and what the
mouse wheel does; interface size, left-handed layout and hiding panels while drawing; the renderer and texture
folder; confirmation before discarding changes. A damaged settings file is kept as `config.json.broken` and
the defaults are used. `--config path` reads and saves somewhere else. Snapping to the grid and to the ends of
strokes, shape guides (line, arc, angle, circle, square), shape recognition, units and artboards are under
Precision (the crosshair button, `G` to switch snapping, `Shift+G` for the panel); snapping can also land on the
middles of strokes, where they cross and on the nodes of the isometric and triangle papers. The canvas turns
with two fingers or `Ctrl+Left` and `Ctrl+Right`, or to a typed angle (`Ctrl+Shift+R`). The selection bar has
group, lock, move to layer and the numbers of the selection. Artboards, guides, groups and locks are saved in
the drawing file. See `docs/PRECISION.md` and `docs/TOOLS.md`.

Ctrl+N, Ctrl+O, Ctrl+S and Ctrl+Shift+S create, open, save and save as. Saving rewrites the whole file
through a temporary one, and the app warns once before a new, open or drop would discard unsaved changes. Drawings
are `.ink` files (see `docs/FORMAT.md`).

On Windows either toolchain works: MSVC (needs the Visual Studio Build Tools) or GNU (needs MinGW-w64,
no admin rights required). Development so far used the GNU host, `x86_64-pc-windows-gnu`.

## Third-party content

No brush packs or textures are bundled in this repository. The pencils, marker, airbrush and watercolor can use
stamp textures that you provide in a folder (`--textures`, or the setting); without them they draw plain soft stamps.
