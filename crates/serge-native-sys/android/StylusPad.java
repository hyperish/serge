package org.hyperish.serge;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.widget.PopupWindow;

/** Window-local drawing surface. A NativeActivity keeps its separate native input queue. */
public final class StylusPad {
    private static final int LIMIT = 512, WORDS = 8;
    private final Activity activity;
    private final long[] samples = new long[LIMIT * WORDS];
    private int start, count;
    private long dropped;
    private volatile boolean closed;
    private volatile int status;
    private PopupWindow window;

    StylusPad(Activity activity, int left, int top, int width, int height) {
        this.activity = activity;
        activity.runOnUiThread(() -> {
            if (closed || activity.isFinishing() || activity.isDestroyed()) { status = -1; return; }
            try {
                View content = activity.findViewById(android.R.id.content);
                int[] origin = new int[2];
                content.getLocationInWindow(origin);
                View pad = new View(activity);
                pad.setOnTouchListener((view, e) -> capture(e));
                window = new PopupWindow(pad, width, height, false);
                window.setBackgroundDrawable(new ColorDrawable(Color.TRANSPARENT));
                window.setInputMethodMode(PopupWindow.INPUT_METHOD_NOT_NEEDED);
                window.setOnDismissListener(() -> status = -1);
                window.showAtLocation(content, Gravity.TOP | Gravity.LEFT, origin[0] + left, origin[1] + top);
                status = 1;
            } catch (RuntimeException unavailable) { status = -1; }
        });
    }
    public int status() { return status; }

    private synchronized boolean capture(MotionEvent event) {
        int action = event.getActionMasked();
        for (int p = 0; p < event.getPointerCount(); p++) {
            int tool = event.getToolType(p);
            if (tool != MotionEvent.TOOL_TYPE_STYLUS && tool != MotionEvent.TOOL_TYPE_ERASER) continue;
            int phase;
            switch (action) {
                case MotionEvent.ACTION_DOWN:
                case MotionEvent.ACTION_POINTER_DOWN:
                    if (p != event.getActionIndex()) continue;
                    phase = 0;
                    break;
                case MotionEvent.ACTION_MOVE: phase = 1; break;
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_POINTER_UP:
                    if (p != event.getActionIndex()) continue;
                    phase = (event.getFlags() & MotionEvent.FLAG_CANCELED) == 0 ? 2 : 3;
                    break;
                case MotionEvent.ACTION_CANCEL: phase = 3; break;
                default: continue;
            }
            if (phase == 1) {
                for (int h = 0; h < event.getHistorySize(); h++) {
                    long time = Build.VERSION.SDK_INT >= 34 ? event.getHistoricalEventTimeNanos(h)
                        : event.getHistoricalEventTime(h) * 1000000L;
                    add(time, event.getPointerId(p), phase, tool, event.getHistoricalX(p, h),
                        event.getHistoricalY(p, h), event.getHistoricalPressure(p, h),
                        event.getHistoricalAxisValue(MotionEvent.AXIS_TILT, p, h));
                }
            }
            long time = Build.VERSION.SDK_INT >= 34 ? event.getEventTimeNanos() : event.getEventTime() * 1000000L;
            add(time, event.getPointerId(p), phase, tool, event.getX(p), event.getY(p),
                event.getPressure(p), event.getAxisValue(MotionEvent.AXIS_TILT, p));
        }
        return true;
    }
    private void add(long time, int id, int phase, int tool, float x, float y, float pressure, float tilt) {
        if (count == LIMIT) { start = (start + 1) % LIMIT; count--; dropped++; }
        int at = ((start + count++) % LIMIT) * WORDS;
        samples[at] = time;
        samples[at + 1] = id;
        samples[at + 2] = phase;
        samples[at + 3] = tool == MotionEvent.TOOL_TYPE_ERASER ? 1 : 0;
        samples[at + 4] = Float.floatToIntBits(x);
        samples[at + 5] = Float.floatToIntBits(y);
        samples[at + 6] = Float.floatToIntBits(pressure);
        samples[at + 7] = Float.floatToIntBits(tilt);
    }
    public synchronized long[] drain() {
        long[] out = new long[1 + count * WORDS];
        out[0] = dropped;
        int first = Math.min(count, LIMIT - start);
        System.arraycopy(samples, start * WORDS, out, 1, first * WORDS);
        System.arraycopy(samples, 0, out, 1 + first * WORDS, (count - first) * WORDS);
        start = count = 0;
        dropped = 0;
        return out;
    }
    public void close() {
        closed = true;
        activity.runOnUiThread(() -> { if (window != null) window.dismiss(); });
    }
}
