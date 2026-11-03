inkwell for Windows (portable)
==============================

Double-click inkwell.exe. There is nothing to install and it needs no administrator rights; delete the folder to
remove it. It is for 64-bit Windows 10 and 11 and draws with OpenGL, which the graphics driver of any ordinary PC
has (a remote desktop session or a bare virtual machine may not).

Where things are kept
  Your drawings    Documents\inkwell (change it in Settings, Files)
  Your settings    %APPDATA%\inkwell\config.json
  Start with other settings:  inkwell.exe --config path\to\config.json

Drawings are saved as .ink files, one file for each drawing. Double-clicking is not tied to the app yet: open a
drawing from the Library, with Ctrl+O, or by dropping the file on the window.

Pencil and airbrush textures are not part of this download, so those pens use a plain soft stamp. If you have stamp
textures of your own, put them in a folder named textures next to inkwell.exe, or point the app at a folder:
inkwell.exe --textures path\to\textures

Windows SmartScreen may warn about a program it has not seen before ("More info", then "Run anyway"): the exe is not
signed. SHA256SUMS.txt next to the download has the checksum to compare.

Shortcuts: B all pens, E eraser, V select, K slice, G snap, Ctrl+Z undo, Ctrl+, settings (every key can be changed
there).
