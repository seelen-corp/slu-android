package com.seelen.slu_launcher;

import android.content.Intent;
import android.graphics.PixelFormat;
import android.os.Bundle;

/**
 * The HOME/LAUNCHER Activity. Its theme (manifest) shows the system wallpaper
 * behind the window; the Slint UI draws over it with a transparent background.
 */
public class HomeActivity extends SluNativeActivity {
    static {
        System.loadLibrary("slu_launcher");
    }

    /** Implemented in Rust (lib.rs). */
    private static native void nativeOnHomeIntent(boolean alreadyOnHome);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // NativeActivity makes the surface opaque (RGB_565): the wallpaper behind
        // the window needs one with alpha.
        getWindow().setFormat(PixelFormat.TRANSLUCENT);
    }

    /**
     * The Home button (or gesture) while this activity is in the task: if it was
     * already the one in front, the UI goes back to its first screen.
     */
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        if (Intent.ACTION_MAIN.equals(intent.getAction())) {
            nativeOnHomeIntent(hasWindowFocus());
        }
    }

    /**
     * A launcher is never closed by Back. The Slint UI handles Back itself
     * (Apps Menu -> Home); this only catches the events it leaves unhandled,
     * e.g. while a Slint popup (menu) has the focus, which would otherwise
     * finish this Activity.
     */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {}
}
