package com.seelen.slu_launcher;

import android.app.NativeActivity;
import android.os.Bundle;
import android.view.View;

/**
 * Common base for every Activity in this app. Both screens (Home and
 * Settings) render a light/white background, but Android's default status
 * bar icons are the light (white) variant meant for dark backgrounds — on a
 * white background they become nearly invisible. SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
 * switches the system to the dark icon variant so it contrasts with our
 * background. Must run in onCreate (UI thread); the Rust/Slint side has no
 * safe way to touch View state directly.
 */
public class SluNativeActivity extends NativeActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        View decorView = getWindow().getDecorView();
        decorView.setSystemUiVisibility(
            decorView.getSystemUiVisibility() | View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
        );
    }
}
