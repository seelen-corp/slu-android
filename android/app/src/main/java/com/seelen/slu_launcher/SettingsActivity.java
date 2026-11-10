package com.seelen.slu_launcher;

import android.content.res.Configuration;
import android.os.Bundle;

/**
 * Separate Activity/task for the Settings screen. Unlike the HOME activity
 * (which Android never lists in the Recents/multitasking view), a normal
 * Activity like this one does show up there. It shares the same native
 * library and android_main entry point as the launcher; Rust picks which
 * Slint window to show based on which Activity class launched it (see
 * activity::current_class_name).
 */
public class SettingsActivity extends SluNativeActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        updateSystemBars(getResources().getConfiguration());
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        updateSystemBars(newConfig);
    }

    /** System bar icons contrast with the settings surface (follows the scheme). */
    private void updateSystemBars(Configuration config) {
        int night = config.uiMode & Configuration.UI_MODE_NIGHT_MASK;
        setLightSystemBars(night != Configuration.UI_MODE_NIGHT_YES);
    }
}
