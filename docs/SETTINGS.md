# Settings

What can be configured in inkwell, and what is not there yet. Settings are saved as JSON as they change (see the
README for where).

| Setting | Where | Notes |
| --- | --- | --- |
| Zoom the canvas on or off | Workspace | Locks pinch, wheel and the zoom keys |
| Snap to round zoom levels | Workspace | 25, 50, 100, 200, 400, 800 percent; settles after the zoom pauses, so it never holds a pinch still |
| Brush preview on hover | Workspace | A ring the size of the brush |
| Show stroke length | Workspace | In canvas units, beside the pen |
| Show selection size | Workspace | Width and height, on the selection bar |
| Pen rail: 3 to 8 places, which pen in each | Workspace | Each place has a key (1 to 8) |
| Keyboard shortcuts on or off | Workspace | The keys themselves are under Shortcuts |
| Pen pressure on or off, pressure response | Interaction | Two sliders and a curve; applied to what the pen reports |
| Two-finger pan and zoom | Interaction | |
| Two, three and four finger taps | Interaction | Undo, redo, hide panels, zoom to fit, previous tool, or nothing. Two is undo and three is redo to start with. A tap right after a stroke is ignored, so a resting hand is not a tap |
| Mouse wheel zooms | Interaction | |
| Grid: main lines every N, pattern color, line weight | Canvas | Along with the pattern, spacing, visibility and paper color |
| Nine patterns: blank, dots, grid, lines, isometric, triangle, perspective with 1, 2 or 3 points | Canvas, note menu | Drawn from lines clipped to the window, so they run on at any zoom |
| Panels over the paper stay inside the window | | A panel taller than the window scrolls instead of running off it; a flyout stays hidden until its height is known |
| Hide the panels | key (Ctrl+Period) or tap | |
| Snap: to the grid, to key points, to middles, to crossings, while drawing, while editing, reach | Precision | `G` switches it; middles and crossings are off to begin with; see `PRECISION.md` |
| Shape recognition on or off, how long to hold | Precision | `Shift+R`; off to begin with |
| Shape guides | Precision panel | `Alt+1` to `Alt+5` |
| Unit, format, places, pixels per inch, drawing scale, auto-scale | Precision | Used by the length, size and guide labels |
| Artboard: preset or size, on its side, keep drawing inside, pattern only inside | Precision | Saved in the drawing file |
| Turn the canvas on or off, snap the turn to every 15 degrees | Workspace | Two fingers twisting, `Ctrl+Left`, `Ctrl+Right`, `Ctrl+Shift+0` |
| Tools: select shape and coverage, slice blade, nudge, eraser reach and what it takes (whole strokes or only what it touches), arrow-key steps | Tools | See `TOOLS.md` |
| Renderer, texture folder | Rendering | The texture folder is optional: stamp textures for the pencils, marker, airbrush and watercolor, none of which are shipped |
| About 60 keyboard commands | Shortcuts | Click to change, add, remove, reset |

## Not done yet, and why

- **Stylus against finger, the eraser end and the side button:** these need to tell a pen from a finger, and the
  event layer does not say which it is. Doing it by guesswork would draw with the wrong input, so it waits until it
  can be tried on the tablet.
- **Tilt, low-latency mode, tap and hold:** not built.
- **Handedness presets** that also change palm rejection: left-handed layout exists; palm rejection does not.
