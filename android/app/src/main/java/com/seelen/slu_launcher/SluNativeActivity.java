package com.seelen.slu_launcher;

import android.app.NativeActivity;
import android.graphics.Color;
import android.os.Build;
import android.os.Bundle;
import android.view.Display;
import android.view.HapticFeedbackConstants;
import android.view.Surface;
import android.view.SurfaceHolder;
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

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        super.surfaceCreated(holder);
        requestPeakRefreshRate(holder.getSurface());
    }

    /**
     * Votes for the display's highest refresh rate (e.g. 120Hz) for our surface.
     * Without it some systems keep unknown apps at 60Hz. It's only a vote:
     * SurfaceFlinger ignores it while the surface isn't drawing (idle Home), so
     * the display still drops its rate when nothing moves.
     */
    private void requestPeakRefreshRate(Surface surface) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R || surface == null || !surface.isValid()) {
            return;
        }
        Display display = getDisplay();
        if (display == null) {
            return;
        }
        Display.Mode current = display.getMode();
        float peak = current.getRefreshRate();
        for (Display.Mode mode : display.getSupportedModes()) {
            if (mode.getPhysicalWidth() == current.getPhysicalWidth()
                && mode.getPhysicalHeight() == current.getPhysicalHeight()) {
                peak = Math.max(peak, mode.getRefreshRate());
            }
        }
        try {
            surface.setFrameRate(peak, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT);
        } catch (IllegalArgumentException | IllegalStateException e) {
            // Not fatal: the system just picks the rate itself.
        }
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
