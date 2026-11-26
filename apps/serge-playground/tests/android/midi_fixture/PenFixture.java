package org.hyperish.midifixture;

import android.app.Activity;
import android.app.Instrumentation;
import android.app.UiAutomation;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.MotionEvent;

/** Sends deterministic pen axes through the emulator's normal input dispatcher. */
public final class PenFixture extends Instrumentation {
    @Override public void onCreate(Bundle options) { super.onCreate(options); start(); }

    private void stroke(UiAutomation ui, int tool, boolean cancel) {
        long down = SystemClock.uptimeMillis();
        MotionEvent.PointerProperties pointer = new MotionEvent.PointerProperties();
        pointer.id = 0;
        pointer.toolType = tool;
        int[] actions = {MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE,
            cancel ? MotionEvent.ACTION_CANCEL : MotionEvent.ACTION_UP};
        for (int i = 0; i < actions.length; i++) {
            MotionEvent.PointerCoords coords = new MotionEvent.PointerCoords();
            coords.x = 220 + 12 * i;
            coords.y = 260 + 8 * i;
            coords.pressure = i == 0 ? 0.25f : 0.75f;
            coords.setAxisValue(MotionEvent.AXIS_TILT, i == 0 ? 0.2f : 0.6f);
            MotionEvent e = MotionEvent.obtain(down, SystemClock.uptimeMillis(), actions[i], 1,
                new MotionEvent.PointerProperties[]{pointer}, new MotionEvent.PointerCoords[]{coords},
                0, 0, 1, 1, 0, 0, tool == MotionEvent.TOOL_TYPE_FINGER
                    ? InputDevice.SOURCE_TOUCHSCREEN : InputDevice.SOURCE_STYLUS, 0);
            try {
                if (!ui.injectInputEvent(e, true)) throw new IllegalStateException("input dispatcher refused event");
            } finally { e.recycle(); }
            SystemClock.sleep(12);
        }
    }

    @Override public void onStart() {
        Bundle out = new Bundle();
        try {
            UiAutomation ui = getUiAutomation();
            stroke(ui, MotionEvent.TOOL_TYPE_FINGER, false);
            stroke(ui, MotionEvent.TOOL_TYPE_STYLUS, false);
            stroke(ui, MotionEvent.TOOL_TYPE_ERASER, true);
            out.putString("pen", "delivered");
            finish(Activity.RESULT_OK, out);
        } catch (RuntimeException error) {
            out.putString("error", error.toString());
            finish(Activity.RESULT_CANCELED, out);
        }
    }
}
