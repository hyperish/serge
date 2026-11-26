package org.hyperish.serge;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.DatePickerDialog;
import android.app.Dialog;
import android.app.TimePickerDialog;
import android.content.Context;
import android.hardware.biometrics.BiometricPrompt;
import android.os.Build;
import android.os.CancellationSignal;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.AtomicFile;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.RandomAccessFile;
import java.nio.charset.StandardCharsets;
import java.nio.channels.FileLock;
import java.security.KeyStore;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Calendar;
import java.util.Locale;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Platform objects for the Rust NativeActivity adapter. */
public final class NativeServices {
    public static StylusPad stylusPad(Activity activity, int x, int y, int width, int height) {
        return new StylusPad(activity, x, y, width, height);
    }
    private NativeServices() {}

    /** A Jetpack WindowManager subscription supplied by the application's window_layout option. */
    public static Object posture(Activity activity) throws ReflectiveOperationException {
        return Class.forName("org.hyperish.serge.window.WindowPosture", true, activity.getClassLoader())
            .getConstructor(Activity.class).newInstance(activity);
    }

    public static String[] cameraIds(android.app.Activity a) throws Exception { return CameraTexture.ids(a); }
    public static int[] cameraSizes(android.app.Activity a, String id) throws Exception { return CameraTexture.sizes(a, id); }
    public static CameraTexture camera(android.app.Activity a, String id, int w, int h, int texture) throws Exception {
        return new CameraTexture(a, id, w, h, texture);
    }

    public static CameraTexture cameraSurface(Activity a, String id, int w, int h, android.view.Surface surface) throws Exception {
        return new CameraTexture(a, id, w, h, 0, surface);
    }

    public static SigningKey signingKey(Context context, String name, boolean requireHardware) throws Exception {
        return SigningKey.open(context, name, requireHardware);
    }

    /** Locate the generated bridge in the signed application's own class loader. */
    public static Class<?> widgetBridge(android.app.Activity activity) throws Exception {
        android.content.pm.ApplicationInfo app = activity.getPackageManager().getApplicationInfo(
            activity.getPackageName(), android.content.pm.PackageManager.GET_META_DATA);
        String name = app.metaData == null ? null : app.metaData.getString("org.hyperish.serge.widgetBridge");
        if (name == null) throw new IllegalStateException("No generated Serge widgets in this application");
        return Class.forName(name, true, activity.getClassLoader());
    }

    /** An activity-scoped UI result. Rust polls it without blocking the event loop. */
    public static final class Prompt {
        private final Activity activity;
        private Dialog dialog;
        private CancellationSignal signal;
        private volatile int status;
        private volatile String value;

        private Prompt(Activity activity) { this.activity = activity; }
        private synchronized void finish(int status, String value) {
            if (this.status == 0) { this.value = value; this.status = status; }
        }
        public int status() { return status; }
        public String value() { return value; }
        public void cancel() {
            finish(1, null);
            activity.runOnUiThread(() -> {
                if (dialog != null) dialog.dismiss();
                if (signal != null) signal.cancel();
            });
        }
    }

    public static Prompt prompt(Activity activity, String kind, String title, String message, String[] items) {
        Prompt p = new Prompt(activity);
        activity.runOnUiThread(() -> {
            if (p.status != 0 || activity.isFinishing() || activity.isDestroyed()) {
                p.finish(1, null);
                return;
            }
            try {
                Calendar now = Calendar.getInstance();
                switch (kind) {
                    case "pip":
                        android.util.Rational ratio = new android.util.Rational(
                            Integer.parseInt(items[0]), Integer.parseInt(items[1]));
                        boolean entered = activity.enterPictureInPictureMode(
                            new android.app.PictureInPictureParams.Builder().setAspectRatio(ratio).build());
                        p.finish(entered ? 2 : 3, entered ? "entered" : "Android declined picture-in-picture");
                        return;
                    case "alert":
                        p.dialog = new AlertDialog.Builder(activity).setTitle(title).setMessage(message)
                            .setPositiveButton(android.R.string.ok, (d, n) -> p.finish(2, "ok")).create();
                        break;
                    case "options":
                        p.dialog = new AlertDialog.Builder(activity).setTitle(title)
                            .setItems(items, (d, n) -> p.finish(2, Integer.toString(n))).create();
                        break;
                    case "date":
                        p.dialog = new DatePickerDialog(activity, (v, y, m, d) -> p.finish(2,
                            String.format(Locale.ROOT, "%04d-%02d-%02d", y, m + 1, d)),
                            now.get(Calendar.YEAR), now.get(Calendar.MONTH), now.get(Calendar.DAY_OF_MONTH));
                        break;
                    case "time":
                        p.dialog = new TimePickerDialog(activity, (v, h, m) -> p.finish(2,
                            String.format(Locale.ROOT, "%02d:%02d", h, m)), now.get(Calendar.HOUR_OF_DAY),
                            now.get(Calendar.MINUTE), android.text.format.DateFormat.is24HourFormat(activity));
                        break;
                    case "biometric":
                        if (Build.VERSION.SDK_INT < 28) { p.finish(3, "BiometricPrompt requires Android 9"); return; }
                        Biometric.start(p, title, message);
                        return;
                    default: throw new IllegalArgumentException("unknown system prompt");
                }
                p.dialog.setOnDismissListener(d -> p.finish(1, null));
                p.dialog.show();
            } catch (RuntimeException error) { p.finish(3, error.toString()); }
        });
        return p;
    }

    private static final class Biometric {
        static void start(Prompt p, String title, String message) {
            p.signal = new CancellationSignal();
            new BiometricPrompt.Builder(p.activity).setTitle(title).setDescription(message)
                .setNegativeButton(p.activity.getText(android.R.string.cancel), p.activity.getMainExecutor(),
                    (dlg, button) -> p.finish(1, null))
                .build().authenticate(p.signal, p.activity.getMainExecutor(), new BiometricPrompt.AuthenticationCallback() {
                    @Override public void onAuthenticationSucceeded(BiometricPrompt.AuthenticationResult result) { p.finish(2, "authenticated"); }
                    @Override public void onAuthenticationError(int code, CharSequence error) {
                        if (code == BiometricPrompt.BIOMETRIC_ERROR_CANCELED
                                || code == BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED) {
                            p.finish(1, null);
                        } else {
                            p.finish(3, error.toString());
                        }
                    }
                });
        }
    }

    public static void requestPermission(Activity activity, String permission, int requestId) {
        activity.runOnUiThread(() -> activity.requestPermissions(new String[] { permission }, requestId));
    }

    /** Store one bounded encrypted value. The AES key never leaves AndroidKeyStore. */
    public static synchronized byte[] credential(Context context, String service, String account, byte[] update, boolean remove) throws Exception {
        if (update != null && update.length > 65536) throw new IOException("credential exceeds 64 KiB");
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        digest.update(service.getBytes(StandardCharsets.UTF_8));
        digest.update((byte) 0);
        byte[] identity = digest.digest(account.getBytes(StandardCharsets.UTF_8));
        StringBuilder hex = new StringBuilder("serge-");
        for (byte b : identity) hex.append(String.format(Locale.ROOT, "%02x", b & 255));
        String alias = hex.toString();
        File directory = new File(context.getNoBackupFilesDir(), "serge-credentials");
        if (!directory.isDirectory() && !directory.mkdirs() && !directory.isDirectory()) throw new IOException("credential directory unavailable");
        AtomicFile file = new AtomicFile(new File(directory, alias));
        try (RandomAccessFile lock = new RandomAccessFile(new File(directory, "lock"), "rw");
             FileLock held = lock.getChannel().lock()) {
            KeyStore store = KeyStore.getInstance("AndroidKeyStore");
            store.load(null);
            if (remove) {
                if (store.containsAlias(alias)) store.deleteEntry(alias);
                file.delete();
                return null;
            }
            if (update == null && !file.getBaseFile().exists() && !new File(file.getBaseFile().getPath() + ".bak").exists()) return null;
            SecretKey key = (SecretKey) store.getKey(alias, null);
            if (key == null && update == null) throw new IOException("credential key unavailable");
            if (key == null) {
                KeyGenerator gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
                gen.init(new KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());
                key = gen.generateKey();
            }
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            if (update != null) {
                cipher.init(Cipher.ENCRYPT_MODE, key);
                cipher.updateAAD(identity);
                byte[] iv = cipher.getIV(), sealed = cipher.doFinal(update);
                if (iv.length != 12) throw new IOException("unexpected GCM nonce length");
                FileOutputStream out = file.startWrite();
                try { out.write(iv); out.write(sealed); file.finishWrite(out); }
                catch (IOException failure) { file.failWrite(out); throw failure; }
                return null;
            }
            try (FileInputStream in = file.openRead()) {
                long len = in.getChannel().size();
                if (len < 28 || len > 65536 + 28) throw new IOException("invalid credential file");
                byte[] sealed = new byte[(int) len];
                new java.io.DataInputStream(in).readFully(sealed);
                cipher.init(Cipher.DECRYPT_MODE, key, new GCMParameterSpec(128, Arrays.copyOf(sealed, 12)));
                cipher.updateAAD(identity);
                return cipher.doFinal(sealed, 12, sealed.length - 12);
            }
        }
    }
}
