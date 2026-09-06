package com.seelen.slu_launcher;

/**
 * Separate Activity/task for the Settings screen. Unlike the HOME activity
 * (which Android never lists in the Recents/multitasking view), a normal
 * Activity like this one does show up there. It shares the same native
 * library and android_main entry point as the launcher; Rust picks which
 * Slint window to show based on which Activity class launched it (see
 * activity_class_name in system-modules/apps.rs).
 */
public class SettingsActivity extends SluNativeActivity {}
