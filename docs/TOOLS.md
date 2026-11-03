# Editing tools

What inkwell's Select, Slice, Nudge and Eraser tools and its keyboard system do.

| Tool | Behavior |
| --- | --- |
| Select | Lasso or box (setting). A stroke is taken when at least a chosen share of it is inside; zero takes any touch. Tap picks, Shift or Ctrl adds. Box handles move, scale from the opposite corner, and turn. |
| Selection bar | Floats under the tab bar. First row: move, flip, duplicate, delete. Second row: group and ungroup, lock and unlock everything, move to the layer below or above, and the numbers. Every button is a command, so a tablet needs no keyboard. In a narrow window the words go on a row of their own so the bar stays clear of the rail. |
| Group | `Ctrl+G` groups the selection, `Ctrl+Shift+G` breaks it up. A tap or a marquee that takes one stroke of a group takes the whole group; Shift-tap adds or takes out the whole group. |
| Lock | `Ctrl+Shift+L` locks the selection, `Ctrl+Alt+L` unlocks everything. A locked stroke cannot be selected, erased, nudged or sliced, and select all skips it. With nothing selected and strokes locked, a small bar says how many and has the unlock button. |
| Move to layer | `Ctrl+Alt+Up` and `Ctrl+Alt+Down` move the selected strokes to the nearest layer above or below that holds strokes, on top of it, as one step; they stay selected. |
| Numbers | The size button opens, under the bar, the X and Y of the selection's box (Y counts down), its width and height, a turn-by field and Keep shape. They are in the unit the settings choose. Dragging or typing a number changes the strokes as you go, and the whole drag or typed number is one step of undo; a turn goes about one point for the whole edit. A box cannot be made smaller than one unit, and a flat box cannot be stretched along its flat side. |
| Eraser | Hard removes the whole stroke it touches (the default). Soft takes only what the eraser passes over and cuts the stroke there, keeping the rest; one sweep is one step. `E` toggles, `Shift+E` chooses soft, `Alt+E` hard; the flyout and Settings, Tools have the choice. Locked strokes are left alone. |
| Nudge | Pull or push (setting). Pull follows the stroke by distance along it; push by distance from the tip. Smooth cosine falloff. The stroke gets extra points first. One undo step; Esc reverts. The stroke under the pen is outlined. |
| Slice | Straight or freehand blade across the stroke; cut points are marked while the blade moves; one undo step. A blade that crosses the stroke's line cuts there. A blade that ends inside the thickness of a thick stroke, or only touches its edge, cuts at the nearest point of its line, as it looks as if it did; a cut is never made within two half-widths of another. The first piece keeps the stroke's id. Locked strokes are not cut. |
| Arrow keys | Move the selection; Shift is a big step, Alt a fine one; repeats while held. |
| Shortcuts | Every command has keys in the settings; click to change, plus for a second key, cross to remove, reset per command or all. A key another command had moves over, with a message. Previous tool is `Q`. |

Dragging the selection (like nudging) leaves the drawing alone while the pen is down: the strokes as they look are
drawn over the paper, and go into the drawing once, when the pen is lifted. Esc gives the drag up without having
touched anything. Moving snaps when Precision is on (see `PRECISION.md`).

Locks and groups are not part of undo (they are marks on the drawing, not edits of it); they are saved in the
drawing file, see `FORMAT.md`.

Slicing gives the later pieces new ids, so a textured pen scatters differently on them. Nudged strokes keep their id.
Strokes with only a stored path and no samples cannot be sliced or nudged.

## Not done yet

- The ends of a cut are plain: they are not shaped to follow the blade.
- Snapping a selection to where strokes cross.
