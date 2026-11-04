# The `.ink` file format

A drawing is saved as a `.ink` file: a ZIP archive of JSON. It holds the layers and strokes, and what a drawing
keeps besides them. `ink-format` reads and writes it (`read`, `write`); the model it maps to is in `ink-core`
(`Document`, `Layer`, `Stroke`).

## Archive layout

| Entry | Required | Holds |
| --- | --- | --- |
| `manifest.json` | yes | `{"format":"ink","version":1,"writer":"inkwell 0.1.0"}`. `format` must be `ink`; a `version` higher than the reader's is refused with a message to update the app. |
| `document.json` | yes | The document: its metadata and layers. |
| `extras.json` | no | The artboard, the shape guides, the locked strokes, the groups and the words written on the drawing. Left out when there is none of them. A damaged one is ignored and the drawing opens without it. |

Entries are deflate-compressed. A reader refuses an entry that is larger than 512 MB once unpacked (16 MB for
`extras.json`, 64 KB for the manifest), whatever size the archive declares, so a small file cannot ask for all the
memory there is. Reading never panics: a file that is not a ZIP, is not a drawing, is damaged or is from a newer
version gives an error with a plain message.

## `document.json`

```json
{
  "metadata": {
    "backgroundSolidColor": "#F9F9F9",
    "creationTime": "2026-10-30T20:54:33.371Z",
    "modificationTime": "2026-10-30T20:55:29.637Z",
    "formatVersion": 1,
    "identifier": "66cb6d04-7a3c-41be-a23d-0116c19df02a"
  },
  "layers": [ { "id": "...", "content": true, "label": {"Counter": 1}, "visible": true, "opacity": 1.0, "strokes": [ ... ] } ]
}
```

- **Layer:** `id` (UUID), `content` (`true` for layers that hold strokes; at least one is needed to open the
  drawing), `label` (`{"Name": "Notes"}`, `{"Counter": 3}` for "Layer 3", or `"None"`), `visible`, `opacity`
  (`0.0` to `1.0`) and `strokes` in drawing order, bottom first.
- **Stroke:** `id` (UUID), `pen`, `width` (the size at draw time; `null` for pens without a width), `color`,
  `path` (stored anchor points), `samples` (the raw input samples) and `pressure` (one `0.0` to `1.0` value per
  sample, or empty for a mouse). The points to draw are the samples when there are at least two, else the path.
- **Pen:** the name of a built-in pen (`"Pen"`, `"Fountain"`, `"Marker"`, `"SoftPencil"`, ...), or
  `{"Unknown": n}` for one this version does not know; it is kept and written back as it was.
- **Sample:** `point` (`x`, `y` in canvas units, `y` up) and `attrs`, four values scaled to `0` to `65535`: time,
  pressure, altitude and azimuth.
- **Color:** `r`, `g`, `b`, `a`, each `0.0` to `1.0`.

## `extras.json`

```json
{ "artboard": { ... }, "guides": [ ... ], "groups": [ ["<stroke id>", "<stroke id>"] ], "locked": ["<stroke id>"],
  "labels": [ { "text": "Hello", "size": 24.0, "height": 17.5, "ids": ["<stroke id>"] } ] }
```

Every field is optional. Locks and groups are not part of undo; the artboard and guides are described in
`PRECISION.md`.

**Labels** remember the words behind letters that were written with the text tool. The letters themselves are ordinary
strokes of the filling pen (see below), so any reader draws them; a label only says what they spell, so the words can be
changed later. `ids` are the strokes that make the letters, `size` is how tall the letters were made (per em, in canvas
units) and `height` how tall all of them together were, to tell how much they have been scaled since. A reader that does
not know labels loses nothing it could draw.

**The filling pen.** A stroke of the pen `fill` with three or more points is a region: its points are the edge of a polygon,
closed by a line from the last point back to the first, and everything inside is covered (even and odd: a path that crosses
itself, or goes in along a line and comes back along it to a hole, leaves the hole empty). Shapes put on the drawing from
the shapes menu are one or two such strokes (the fill, then the outline) tied in a group.

## Writing

The file is written through a temporary file and renamed, so a failure cannot damage an existing drawing. The
modification time is set to the time of the save, and the creation time if the drawing has none.

## Versions

`version` is `1`. A change that older readers could not understand raises it; one that only adds optional fields
does not.
