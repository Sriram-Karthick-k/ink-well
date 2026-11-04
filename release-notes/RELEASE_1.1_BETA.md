## inkwell 1.1 beta: now on Android, with shapes and text

This is the first release with an **Android app**. The Windows app gets shapes, text and a lot of polish too.

### Downloads

| File | For |
|---|---|
| `inkwell-android-arm64.apk` | Android 8 or newer, 64-bit ARM (nearly every phone and tablet made since 2017) |
| `inkwell-windows-x64.exe` | Windows 10 and 11, one file, nothing to install |
| `inkwell-windows-x64.zip` | the same app with a short readme |

`SHA256SUMS.txt` has the checksums. Neither download is store-signed: on Android, open the APK and allow installs from the
app you opened it with; on Windows, SmartScreen may ask you to confirm once. This is a beta, so keep a copy of drawings that
matter (they are saved automatically as `.ink` files and can be exported from the menu).

### New

**Android app.** An interface of its own for a finger and a stylus, not a port of the desktop one.
- A Library of cards with sheets from the bottom, a tool bar along the bottom, and a settings page made for touch.
- The stylus draws, and so do your fingers: two fingers pan, pinch and turn the page, and holding still pans too. A hand
  resting by the stylus is ignored.
- Open, import and save a copy with the system's own file pickers. Back steps out one thing at a time.

**Shapes.** Twenty-five ready-made shapes (lines, arrows, rectangles, circles, polygons, a star, a heart, a cube, a speech
bubble and more).
- Put one on the page with a tap, or keep the **shapes drawer** open at the side of the page and drag shapes out of it to
  build a diagram. Choose the left or the right side.
- Shapes can be **filled** with a color, with or without an outline.
- Select anything you drew, **save it as an object**, and it is in your shapes list for every drawing.

**Text, on the page itself.** Tap where the words go and type; they appear in your ink as you type, fitted inside the shape if
one is selected (broken into lines when that makes them bigger). Words are real shapes, so they move, resize and erase like
anything else, and they can be changed later.
- **Double-tap or double-click** a shape or some words to write in them or change them, whatever tool is in use.

**A tool bar you arrange.** Choose what is on the bottom bar, in what order, and pin your favorite shapes to it.

**Better Library thumbnails.** Filled shapes and text show as they are drawn.

### Fixed

- The on-screen keyboard no longer opens and closes by itself or flickers the whole screen.
- A dark overlay that sometimes covered the app after opening and closing the top menus quickly.
- Leaving a drawing while typing no longer leaves the top bar dead when you come back.
- Small text no longer looks bolder once it is placed than it did while typing.

### Known issues

- The selection bar, the color picker and the pen menus still look like the desktop ones on touch devices.
- The APK is signed with a key made for this build only. Until the project has a fixed signing key, a later release cannot be
  installed over this one: remove this one first (its drawings stay in the app's Library folder only until it is removed, so
  save copies of anything you want to keep).
- Android builds are 64-bit ARM only; there is no 32-bit or Intel build.
