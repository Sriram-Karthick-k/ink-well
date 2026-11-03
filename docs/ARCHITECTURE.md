# Architecture

## Principles

1. **A pure core, thin shells.** All drawing logic lives in Rust crates with no platform code. Each
   platform (Android, desktop, web) is a small shell that supplies input events, a drawing surface and
   file access, and calls into the core.
2. **Determinism.** The same input produces the same strokes on every platform. The core owns its own
   PRNG and never depends on platform floating-point quirks beyond IEEE 754 `f32`.
3. **Own your data model.** The in-memory model (`ink-core`) is independent of any file format. Formats
   are adapters (`ink-format`), so the app can read other formats or add its own later.
4. **No third-party assets in the repo.** Textures for the pens are optional and loaded at runtime from a
   folder the user provides; none is shipped.

## Crates

```
ink-wasm ─┐
(android) ├─> ink-format ──> ink-core
(desktop) ┘        └──────> ink-brush ──> ink-core
                            ink-gpu ───> ink-brush
```

| Crate | Responsibility | Must not contain |
|---|---|---|
| `ink-core` | Document, layers, strokes, colors; path smoothing; undo history; snapping, guides, shape recognition, units, artboards | I/O, GPU, platform code, `unsafe` |
| `ink-format` | Read and write the `.ink` file (a ZIP of JSON) as the core model, see `FORMAT.md` | rendering |
| `ink-brush` | Brush parameter model, curves, CPU-side stamp placement, per-pen styles, procedural paper grain | GPU code, texture decoding |
| `ink-gpu` | OpenGL stroke renderer: stamps into a mask, grain reveal, composite, cached document texture | document or pen logic; the only crate that may use `unsafe` besides the wasm bindings |
| `ink-wasm` | `wasm-bindgen` bindings for a browser UI | logic (delegates to the crates above) |
| `ink-app` | The native UI: window, canvas input, tabs, the Library with folders, flyouts, note menu, settings window and the settings file | drawing logic (calls `ink-core`) |

`ink-app` draws strokes with `ink-gpu` through an egui paint callback. If the GPU renderer cannot start (OpenGL older
than 3.3 or ES 3.0) or fails while drawing, it falls back to the egui-shape renderer in `render.rs`; `--cpu` forces
that fallback.

## Data flow for a stroke

```
pointer events (shell) ──> raw samples ──> ink-core::smoothing ──> smoothed path
smoothed path + pen ──> ink-brush::pens ──> stamp batch ──> ink-gpu (mask, then grain and composite)
document ──> ink-format ──> .ink file
```

The renderer works in two stages: stamps accumulate into a single-channel mask with `mask = a + (1 - a) * mask`,
then a grain texture is revealed through the mask and the result is composited onto the layer once. Compositing a
finished stroke in one pass, from a smoothed path, is what avoids visible joins.

What is drawn while the pen is down is what is kept: the smoothed path is built incrementally and never reshaped,
and nothing is recomputed when the pen lifts.

## Platform plan

| Target | Shell | Status |
|---|---|---|
| Native (Windows, later macOS and Linux) | `ink-app` (egui) | running: tabs, Library with folders and autosave, canvas, pen rail and flyouts, note menu, editing and precision tools, settings window |
| Web | a browser UI on `ink-wasm` | bindings exist; no UI yet |
| Android | `ink-app` built for Android with winit's native-activity backend | not started |

The Android choice is deferred until stylus latency is measured on a real tablet.

## Testing

- Unit tests next to the code; property-style checks for geometry (smoothing keeps endpoints, straight
  lines stay straight).
- Integration tests in `ink-format` write and read back drawings, and check that damaged, hostile and foreign
  files are refused.
- The app's tests drive the real UI frame by frame (panels stay in the window and hold still, hovering never
  moves anything, strokes do not change on pen-up).
- CI runs fmt, clippy with warnings denied, tests on Linux, Windows and macOS, a WASM build and an
  Android `cargo check`.
