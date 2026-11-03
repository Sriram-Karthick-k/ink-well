# Precision

Snapping, shape guides, shape recognition, units, canvas rotation and artboards: what inkwell does.

| Part | Behavior |
| --- | --- |
| Snap on or off | `G`, the crosshair button at the top right, or Settings, Precision. Off to begin with. |
| To the grid | The crossings of dots and squares, the lines of ruled paper, and the points where the lines of the isometric and the triangle paper meet. Not for perspective paper, and not while the grid is too small to be drawn. |
| To key points | The two ends of every stroke on a visible layer. |
| To middles | Halfway along the length of every stroke on a visible layer (off to begin with). While drawing and while editing. |
| To crossings | Where strokes cross each other or themselves, found near the pen as it moves (off to begin with). For a stroke being drawn and for the handles of a guide; a selection being dragged does not snap to them. |
| While drawing | The first point snaps when the pen goes down. The last point is drawn at the target while the pen is within reach and goes back to the pen when it moves away; the points between are never changed. |
| While editing | A selection being moved lands whichever of its box corners, its middle or its stroke ends is nearest a target. A corner being scaled lands on a target as near as the diagonal allows. A turn lands on every 15 degrees when within 4. |
| Shape guides | Line, arc, angle, circle and square, from the panel or `Alt+1` to `Alt+5`, one of each kind at most. Drag the dots to reshape them; the middle dot moves the guide (and snaps). A stroke that begins within 20 points of a guide follows it exactly, keeps its corners and curves between the pen's samples, and is not smoothed. A label gives the length, angle, size or diameter. Guides are saved in the drawing file. |
| Shape recognition | Off to begin with. With it on, a stroke whose pen is held still at its end for 0.6 s (0.3 to 2) becomes a line, an angle (two straight arms and one corner), an arc, a circle, an ellipse, a triangle or a rectangle, if it is clearly one; a scribble is left alone. An angle close to 30, 45, 60, 90, 120 or 135 degrees is made exactly that, and an arm close to level or upright is made so. The shape is in what is shown and what is kept. |
| Canvas rotation | Two fingers twisting, `Ctrl+Left` and `Ctrl+Right` (15 degrees), `Ctrl+Shift+0` for upright, the angle chip beside the zoom chip. Tapping the chip, or `Ctrl+Shift+R`, opens a small panel where the angle is dragged or typed (counterclockwise is positive) and an Upright button. Strokes, paper pattern, selection box and handles all turn; the GPU and CPU renderers agree. A horizon line and the angle show while turning. A turn that stops within 4 degrees of a multiple of 15 settles on it. Fitting the view makes it upright. |
| Measurement | Unit (px, pt, mm, cm, m, km, in, ft, yd, mi), format, places, pixels per inch, drawing scale and auto-scale, in Settings, Precision. Used by the stroke length, the selection size and the guide labels. |
| Artboard | Settings, Precision: on or off, a preset (A series, US paper, cards, screen, phones, social) or a size in the chosen unit, turn it on its side, keep drawing inside it, show the pattern only inside it, show the whole page. The page is shaded round and labelled. It is saved in the drawing file. |
| Reach | 4 to 40 screen points (14 to begin with), so it is the same to the hand at any zoom. |

The artboard, the guides, the locked strokes and the groups are saved in the drawing file (`extras.json` inside the
`.ink` archive, see `FORMAT.md`). A change to any of them marks the drawing as changed. Locks and groups are not
part of undo.

What is drawn while the pen is down is what is stored when it lifts: the builder keeps a snapped end
(`StrokeBuilder::set_end`) or a recognized shape (`StrokeBuilder::replace_with`) as part of the stroke it shows and
the one it ends with, so a stroke does not change on pen-up.

The geometry is in `ink_core` (`snap`, `guide`, `recognize`, `units`, `artboard`, `marks`), tested without a window; the
settings and the panel are `precision.rs`, the guides over the paper `guides.rs`. The lattice nodes are checked
against the lines the paper really draws (`precision.rs` tests).

## Not done yet

- The page of an artboard cannot be dragged.
- Snapping a selection to where strokes cross, and snapping to perspective paper.
- Rotation by the mouse.
- Snap to the grid from the pen's side button, which waits on telling a pen from a finger.
- Tilt, low-latency mode, draw and hold for other things, tap and hold.
