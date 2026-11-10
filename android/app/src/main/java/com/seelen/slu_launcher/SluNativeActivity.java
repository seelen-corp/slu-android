package com.seelen.slu_launcher;

import android.app.NativeActivity;
import android.graphics.Color;
import android.os.Bundle;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.Window;

/**
 * Common base for every Activity in this app. Holds the few things the Rust side
 * can't safely do itself because they must run on the Android UI thread (View
 * state): system bar colors/icon contrast and haptic feedback. Rust calls the
 * public methods through JNI (see system-modules/theme.rs and system.rs); they
 * post the work to the UI thread.
 */
public class SluNativeActivity extends NativeActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // Edge to edge: the UI draws behind the system bars (Slint reports their
        // insets as the window's safe area).
        Window window = getWindow();
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        View decorView = window.getDecorView();
        decorView.setSystemUiVisibility(decorView.getSystemUiVisibility()
            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
    }

    /**
     * Dark system bar icons (for light content behind the bars) or light ones
     * (for dark content). Called from any thread.
     */
    @SuppressWarnings("deprecation")
    public void setLightSystemBars(final boolean light) {
        runOnUiThread(() -> {
            View decorView = getWindow().getDecorView();
            int flags = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            int visibility = decorView.getSystemUiVisibility();
            decorView.setSystemUiVisibility(light ? (visibility | flags) : (visibility & ~flags));
        });
    }

    /**
     * Long-press haptic feedback. Goes through the View, so it follows the user's
     * "touch feedback" setting and needs no permission. Called from any thread.
     */
    public void performLongPressHaptic() {
        runOnUiThread(() -> getWindow().getDecorView()
            .performHapticFeedback(HapticFeedbackConstants.LONG_PRESS));
    }
}
