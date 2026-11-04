package app.inkwell;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.WindowInsets;
import android.view.WindowManager;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * The activity of the app. It is a NativeActivity (the app itself is native code) that takes touch and key input
 * for itself instead of leaving it to the native glue, because the glue drops what the app most needs to know: which
 * tool touched the screen (a stylus, its eraser end, a finger, a mouse), how hard, how big the contact is, and the
 * samples that arrive between two frames. Everything is passed on to the native side, which decides what it means.
 */
public class InkActivity extends NativeActivity {
    static {
        System.loadLibrary("ink_android");
    }

    // The kinds of event, as the native side knows them.
    private static final int DOWN = 0;
    private static final int UP = 1;
    private static final int MOVE = 2;
    private static final int CANCEL = 3;
    private static final int HOVER = 4;
    private static final int SCROLL = 5;
    private static final int EXIT = 6;

    /** One sample of one pointer. Positions are in pixels of the window; the time is in milliseconds. */
    private static native void nativePointer(int action, int device, int pointerId, int tool, float x, float y, float pressure,
            float contact, float tilt, float orientation, int buttons, long timeMs);

    /** A key went down or up. `unicode` is the character it makes, or 0. */
    private static native void nativeKey(boolean down, int keyCode, int meta, int unicode, boolean repeat);

    /** Tells the native side whether the device has a stylus digitizer. */
    private static native void nativeCapabilities(boolean stylus);

    /** A file was picked: what it is for (0 open, 1 import), its name and everything in it. */
    private static native void nativeFilePicked(int purpose, String name, byte[] bytes);

    /** A copy of a drawing was written where the person chose (`ok`), or could not be. */
    private static native void nativeFileSaved(boolean ok, String name);

    /** The app is going to the background, where it may be stopped: the native side saves what is unsaved. */
    private static native void nativePaused();

    /** Back was pressed. `true` when the app takes it (it closes or leaves something); `false` when the system should. */
    private static native boolean nativeBack();

    /** How much of the bottom of the window the on-screen keyboard covers, in pixels; `0` while there is none. */
    private static native void nativeKeyboard(int bottomPixels);

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        // The native glue took the window's input queue; give it back, so input arrives in the methods below.
        getWindow().takeInputQueue(null);
        nativeCapabilities(hasStylusDigitizer());
        watchKeyboard();
    }

    /**
     * The on-screen keyboard is drawn over the app and does not resize it: a window that shrinks as the keyboard slides
     * in leaves a black gap below it for a moment and lays everything out again, which flickers the whole screen. The
     * native side is told how tall the keyboard is and lifts only what must clear it (a sheet with a text field in it).
     */
    private void watchKeyboard() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            return; // Older systems do not say how tall the keyboard is; the window is resized for it, as the manifest says.
        }
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        getWindow().getDecorView().setOnApplyWindowInsetsListener((view, insets) -> {
            nativeKeyboard(insets.getInsets(WindowInsets.Type.ime()).bottom);
            return view.onApplyWindowInsets(insets);
        });
    }

    private static boolean hasStylusDigitizer() {
        for (int id : InputDevice.getDeviceIds()) {
            InputDevice device = InputDevice.getDevice(id);
            if (device != null && (device.getSources() & InputDevice.SOURCE_STYLUS) == InputDevice.SOURCE_STYLUS) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        forward(event);
        return true;
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        // Hovering (a stylus above the screen, a mouse) and the scroll wheel.
        forward(event);
        return true;
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        int code = event.getKeyCode();
        // The system's own keys are left to the system.
        if (code == KeyEvent.KEYCODE_BACK || code == KeyEvent.KEYCODE_VOLUME_UP || code == KeyEvent.KEYCODE_VOLUME_DOWN
                || code == KeyEvent.KEYCODE_POWER || code == KeyEvent.KEYCODE_HOME) {
            return super.dispatchKeyEvent(event);
        }
        if (event.getAction() == KeyEvent.ACTION_MULTIPLE) {
            return true;
        }
        nativeKey(event.getAction() == KeyEvent.ACTION_DOWN, code, event.getMetaState(), event.getUnicodeChar(),
                event.getRepeatCount() > 0);
        return true;
    }

    private static int kind(MotionEvent event, int index) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                return DOWN;
            case MotionEvent.ACTION_POINTER_DOWN:
                return index == event.getActionIndex() ? DOWN : MOVE;
            case MotionEvent.ACTION_UP:
                return UP;
            case MotionEvent.ACTION_POINTER_UP:
                return index == event.getActionIndex() ? UP : MOVE;
            case MotionEvent.ACTION_CANCEL:
                return CANCEL;
            case MotionEvent.ACTION_HOVER_ENTER:
            case MotionEvent.ACTION_HOVER_MOVE:
                return HOVER;
            case MotionEvent.ACTION_HOVER_EXIT:
                return EXIT;
            case MotionEvent.ACTION_SCROLL:
                return SCROLL;
            default:
                return MOVE;
        }
    }

    private static void forward(MotionEvent event) {
        int count = event.getPointerCount();
        if (event.getActionMasked() == MotionEvent.ACTION_SCROLL) {
            // The wheel: the amounts come in the position's place.
            nativePointer(SCROLL, event.getDeviceId(), event.getPointerId(0), event.getToolType(0), event.getAxisValue(MotionEvent.AXIS_HSCROLL),
                    event.getAxisValue(MotionEvent.AXIS_VSCROLL), 0f, 0f, 0f, 0f, event.getButtonState(),
                    event.getEventTime());
            return;
        }
        // The samples that arrived since the last event, oldest first, then the newest.
        boolean moving = event.getActionMasked() == MotionEvent.ACTION_MOVE
                || event.getActionMasked() == MotionEvent.ACTION_HOVER_MOVE;
        if (moving) {
            for (int h = 0; h < event.getHistorySize(); h++) {
                for (int p = 0; p < count; p++) {
                    nativePointer(kind(event, p), event.getDeviceId(), event.getPointerId(p), event.getToolType(p),
                            event.getHistoricalX(p, h), event.getHistoricalY(p, h), event.getHistoricalPressure(p, h),
                            event.getHistoricalTouchMajor(p, h), event.getHistoricalAxisValue(MotionEvent.AXIS_TILT, p, h),
                            event.getHistoricalOrientation(p, h), event.getButtonState(), event.getHistoricalEventTime(h));
                }
            }
        }
        for (int p = 0; p < count; p++) {
            nativePointer(kind(event, p), event.getDeviceId(), event.getPointerId(p), event.getToolType(p), event.getX(p), event.getY(p),
                    event.getPressure(p), event.getTouchMajor(p), event.getAxisValue(MotionEvent.AXIS_TILT, p),
                    event.getOrientation(p), event.getButtonState(), event.getEventTime());
        }
    }

    // Files. The system's own pickers: the app may not open a path outside its folders, so a file is handed over as a
    // stream, and a copy is written to a place the person chose. Nothing here needs a permission.

    private static final int REQUEST_SAVE = 4101;
    private static final int REQUEST_PICK = 4102;
    private byte[] pendingSave;
    private int pendingPurpose;

    @Override
    public void onBackPressed() {
        // Back steps out of what is open in the app, and only from the top of the Library does the system take it.
        if (!nativeBack()) {
            super.onBackPressed();
        }
    }

    @Override
    protected void onPause() {
        super.onPause();
        nativePaused();
    }

    /** Asks where to save `bytes` as `name`. Called from the native side, on its own thread. */
    public void pickSaveLocation(final String name, final byte[] bytes) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                pendingSave = bytes;
                Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("application/octet-stream");
                intent.putExtra(Intent.EXTRA_TITLE, name);
                startActivityForResult(intent, REQUEST_SAVE);
            }
        });
    }

    /** Asks for files: one to open (`purpose` 0) or several to import (`purpose` 1). */
    public void pickFiles(final int purpose) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                pendingPurpose = purpose;
                Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                if (purpose == 1) {
                    intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
                }
                startActivityForResult(intent, REQUEST_PICK);
            }
        });
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == REQUEST_SAVE) {
            byte[] bytes = pendingSave;
            pendingSave = null;
            // Backing out of the picker is an answer, not a failure: nothing is said.
            if (result == RESULT_OK && data != null && data.getData() != null && bytes != null) {
                Uri uri = data.getData();
                nativeFileSaved(write(uri, bytes), displayName(uri));
            }
        } else if (request == REQUEST_PICK && result == RESULT_OK && data != null) {
            ClipData clips = data.getClipData();
            if (clips != null) {
                for (int i = 0; i < clips.getItemCount(); i++) {
                    deliver(clips.getItemAt(i).getUri());
                }
            } else if (data.getData() != null) {
                deliver(data.getData());
            }
        }
    }

    private void deliver(Uri uri) {
        try {
            nativeFilePicked(pendingPurpose, displayName(uri), read(uri));
        } catch (IOException e) {
            // A file that cannot be read is told to the native side as nothing, which it says in words.
            nativeFilePicked(pendingPurpose, displayName(uri), new byte[0]);
        }
    }

    private boolean write(Uri uri, byte[] bytes) {
        try {
            OutputStream out = getContentResolver().openOutputStream(uri, "wt");
            if (out == null) {
                return false;
            }
            try {
                out.write(bytes);
            } finally {
                out.close();
            }
            return true;
        } catch (IOException | RuntimeException e) {
            return false;
        }
    }

    private byte[] read(Uri uri) throws IOException {
        InputStream in = getContentResolver().openInputStream(uri);
        if (in == null) {
            throw new IOException("no stream");
        }
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            byte[] buffer = new byte[16384];
            int n;
            while ((n = in.read(buffer)) > 0) {
                out.write(buffer, 0, n);
            }
            return out.toByteArray();
        } finally {
            in.close();
        }
    }

    /** The name the picked file has, or a plain one when the provider does not say. */
    private String displayName(Uri uri) {
        Cursor cursor = null;
        try {
            cursor = getContentResolver().query(uri, new String[] { OpenableColumns.DISPLAY_NAME }, null, null, null);
            if (cursor != null && cursor.moveToFirst()) {
                String name = cursor.getString(0);
                if (name != null && !name.isEmpty()) {
                    return name;
                }
            }
        } catch (RuntimeException e) {
            // Falls through to the plain name.
        } finally {
            if (cursor != null) {
                cursor.close();
            }
        }
        return "Drawing.ink";
    }
}
