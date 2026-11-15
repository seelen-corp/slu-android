package com.seelen.slu_launcher;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;

/**
 * Registered dynamically (not in the manifest) by the Rust side in
 * system-modules/apps.rs::init, via Context.registerReceiver. Forwards
 * package add/remove/replace events into native code.
 */
public class PackageChangeReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("slu_launcher");
    }

    /**
     * @param installedPackage the package that was just installed (not an
     *                         update of an installed one), or null for any
     *                         other change.
     */
    private static native void nativeOnPackageChanged(String installedPackage);

    @Override
    public void onReceive(Context context, Intent intent) {
        String installed = null;
        Uri data = intent.getData();
        if (Intent.ACTION_PACKAGE_ADDED.equals(intent.getAction())
                && !intent.getBooleanExtra(Intent.EXTRA_REPLACING, false)
                && data != null) {
            installed = data.getSchemeSpecificPart();
        }
        nativeOnPackageChanged(installed);
    }
}
