package com.seelen.slu_launcher;

/**
 * The HOME/LAUNCHER Activity. Needs to be its own class (rather than the
 * bare android.app.NativeActivity referenced directly in the manifest) so it
 * can share SluNativeActivity's status-bar-icon-contrast fix with
 * SettingsActivity.
 */
public class HomeActivity extends SluNativeActivity {}
