package org.hyperish.serge;

import android.app.Activity;
import android.graphics.SurfaceTexture;
import android.hardware.camera2.*;
import android.hardware.camera2.params.StreamConfigurationMap;
import android.opengl.EGL14;
import android.opengl.EGLContext;
import android.os.Handler;
import android.os.HandlerThread;
import android.util.Size;
import android.view.Surface;
import java.util.Arrays;
import java.util.Collections;

/** Camera2 is the producer and the caller's GL_TEXTURE_EXTERNAL_OES is the consumer. */
public final class CameraTexture implements AutoCloseable {
    private final EGLContext context;
    private final SurfaceTexture texture;
    private final Surface output;
    private final HandlerThread thread;
    private CameraDevice device;
    private CameraCaptureSession session;
    private boolean available, closed;
    private String error;

    public static String[] ids(Activity a) throws CameraAccessException {
        return a.getSystemService(CameraManager.class).getCameraIdList();
    }
    public static int[] sizes(Activity a, String id) throws CameraAccessException {
        StreamConfigurationMap map = a.getSystemService(CameraManager.class).getCameraCharacteristics(id)
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);
        Size[] sizes = map == null ? null : map.getOutputSizes(SurfaceTexture.class);
        int[] pairs = new int[sizes == null ? 0 : sizes.length * 2];
        for (int i = 0; i < pairs.length / 2; i++) {
            pairs[2*i] = sizes[i].getWidth();
            pairs[2*i+1] = sizes[i].getHeight();
        }
        return pairs;
    }
    public CameraTexture(Activity a, String id, int width, int height, int name) throws CameraAccessException {
        this(a, id, width, height, name, null);
    }

    /** Owns the Java Surface reference passed by the NDK ImageReader bridge. */
    public CameraTexture(Activity a, String id, int width, int height, int name, Surface readerOutput) throws CameraAccessException {
        CameraManager manager = a.getSystemService(CameraManager.class);
        StreamConfigurationMap map = manager.getCameraCharacteristics(id).get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);
        Size[] sizes = map == null ? null : map.getOutputSizes(android.graphics.ImageFormat.PRIVATE);
        if (sizes == null || !Arrays.asList(sizes).contains(new Size(width, height)))
            throw new IllegalArgumentException("unsupported private camera size");
        if (readerOutput == null) {
            context = EGL14.eglGetCurrentContext();
            if (name == 0 || EGL14.EGL_NO_CONTEXT.equals(context))
                throw new IllegalStateException("CameraTexture requires the consumer's current GLES context");
            texture = new SurfaceTexture(name);
            texture.setDefaultBufferSize(width, height);
            output = new Surface(texture);
        } else {
            context = null;
            texture = null;
            output = readerOutput;
        }
        thread = new HandlerThread("Serge camera");
        thread.start();
        Handler handler = new Handler(thread.getLooper());
        if (texture != null) texture.setOnFrameAvailableListener(t -> { synchronized (CameraTexture.this) { if (!closed) available = true; } }, handler);
        try {
            manager.openCamera(id, new CameraDevice.StateCallback() {
                public void onOpened(CameraDevice camera) {
                    synchronized (CameraTexture.this) {
                        if (closed) { camera.close(); return; }
                        device = camera;
                        try {
                            final CaptureRequest.Builder request = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
                            request.addTarget(output);
                            camera.createCaptureSession(Collections.singletonList(output), new CameraCaptureSession.StateCallback() {
                                public void onConfigured(CameraCaptureSession s) {
                                    synchronized (CameraTexture.this) {
                                        if (closed) { s.close(); return; }
                                        session = s;
                                        try { s.setRepeatingRequest(request.build(), null, handler); }
                                        catch (CameraAccessException | RuntimeException e) { fail(e.toString()); }
                                    }
                                }
                                public void onConfigureFailed(CameraCaptureSession s) { s.close(); fail("camera stream configuration failed"); }
                            }, handler);
                        } catch (CameraAccessException | RuntimeException e) { fail(e.toString()); }
                    }
                }
                public void onDisconnected(CameraDevice camera) { camera.close(); fail("camera disconnected"); }
                public void onError(CameraDevice camera, int code) { camera.close(); fail("camera error " + code); }
            }, handler);
        } catch (CameraAccessException | RuntimeException e) { close(); throw e; }
    }
    private synchronized void fail(String message) { error = message; close(); }

    public synchronized void check() {
        if (closed) throw new IllegalStateException(error == null ? "camera closed" : error);
    }

    /** -1 means no new frame. Acquiring an image performs the platform's producer/consumer synchronization. */
    public synchronized long update(float[] matrix) {
        if (closed) throw new IllegalStateException(error == null ? "camera stream closed" : error);
        if (!context.equals(EGL14.eglGetCurrentContext())) throw new IllegalStateException("camera consumer context changed");
        if (!available) return -1;
        available = false;
        texture.updateTexImage();
        texture.getTransformMatrix(matrix);
        return texture.getTimestamp();
    }

    @Override public synchronized void close() {
        if (closed) return;
        closed = true;
        if (session != null) session.close();
        if (device != null) device.close();
        if (texture != null) texture.setOnFrameAvailableListener(null);
        output.release();
        if (texture != null) texture.release();
        thread.quitSafely();
    }
}
