package com.seelen.slu_launcher;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * Registered dynamically (not in the manifest) by the Rust side in
 * system-modules/apps.rs::init, via Context.registerReceiver. Forwards
 * package add/remove/replace events into native code.
 */
public class PackageChangeReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("slu_launcher");
    }

    private static native void nativeOnPackageChanged();

    @Override
    public void onReceive(Context context, Intent intent) {
        nativeOnPackageChanged();
    }
}
