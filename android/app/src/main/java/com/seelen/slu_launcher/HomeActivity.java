package com.seelen.slu_launcher;

/**
 * The HOME/LAUNCHER Activity. Needs to be its own class (rather than the
 * bare android.app.NativeActivity referenced directly in the manifest) so it
 * can share SluNativeActivity's status-bar-icon-contrast fix with
 * SettingsActivity.
 */
public class HomeActivity extends SluNativeActivity {
    /**
     * A launcher is never closed by Back. The Slint UI handles Back itself
     * (Apps Menu -> Home); this only catches the events it leaves unhandled,
     * e.g. while a Slint popup (context menu) has the focus, which would
     * otherwise finish this Activity.
     */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {}
}
