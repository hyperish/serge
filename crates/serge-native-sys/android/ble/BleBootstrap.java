package org.hyperish.serge;

public final class BleBootstrap {
    private BleBootstrap() {}
    // Entry through this class establishes the adapter's application class loader for JNI.
    public static native void initialize();
}
