package com.seelen.slu_launcher;

import android.app.Activity;
import android.app.ActivityOptions;
import android.appwidget.AppWidgetHost;
import android.appwidget.AppWidgetHostView;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProviderInfo;
import android.content.ActivityNotFoundException;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.drawable.Drawable;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.util.Log;
import android.util.SizeF;
import android.util.SparseArray;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.RemoteViews;

import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Hosts the Android app widgets placed on Home (system-modules/widgets.rs).
 *
 * <p>The window's content is the Slint surface (NativeActivity takes it), so the
 * widget views are never drawn on screen by Android: they live in an invisible
 * container (attached and laid out, so they update, tick and load their
 * collections like on any launcher), and each one is drawn into a bitmap
 * whenever it changes ({@link WidgetView#capture}). Rust shows those frames and
 * forwards the touches on them ({@link #touch}).
 *
 * <p>Everything here runs on the UI thread except {@link #providers}.
 */
final class WidgetHost {
    private static final String TAG = "slu_launcher";
    /** Any constant: it identifies this launcher's widgets to the system. */
    private static final int HOST_ID = 0x51;
    static final int REQUEST_BIND = 2;
    static final int REQUEST_CONFIGURE = 3;
    /** At most ~30 frames per second per widget. */
    private static final long FRAME_INTERVAL_MS = 33;
    /** Longest side of the previews shown in the widget picker. */
    private static final int PREVIEW_SIDE_DP = 160;

    /**
     * Implemented in Rust (widgets.rs): the widget {@code appWidgetId} of
     * {@code provider} was bound (and configured, if it needs it) and can go on
     * Home.
     */
    private static native void nativeOnWidgetAdded(int appWidgetId, String provider);

    /**
     * Implemented in Rust (widgets.rs): a new image of a widget, {@code width} x
     * {@code height} premultiplied RGBA pixels. Only valid during the call.
     */
    private static native void nativeOnWidgetFrame(int appWidgetId, int width, int height, ByteBuffer pixels);

    /** A widget provider, for the picker. Its fields are read from Rust. */
    public static final class Provider {
        /** Flattened {@link ComponentName}. */
        public String component;
        public String label;
        public String appLabel;
        /** Minimum size, in dp. */
        public float minWidth;
        public float minHeight;
        /** Default size in cells (API 31+), 0 if the provider doesn't say. */
        public int targetColumns;
        public int targetRows;
        /** Premultiplied RGBA pixels, or null. */
        public byte[] preview;
        public int previewWidth;
        public int previewHeight;
    }

    private final Activity activity;
    private final AppWidgetManager manager;
    private final AppWidgetHost host;
    private final FrameLayout container;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final SparseArray<WidgetView> views = new SparseArray<>();
    private final float density;
    /** Being bound or configured (a system activity is open for it), or -1. */
    private int pendingId = -1;
    /** Between onStart and onStop: the widgets update and are drawn. */
    private boolean started;

    WidgetHost(Activity activity) {
        this.activity = activity;
        manager = AppWidgetManager.getInstance(activity);
        density = activity.getResources().getDisplayMetrics().density;
        host = new AppWidgetHost(activity, HOST_ID) {
            @Override
            protected AppWidgetHostView onCreateView(Context context, int appWidgetId, AppWidgetProviderInfo info) {
                return new WidgetView(context, appWidgetId);
            }
        };
        container = new FrameLayout(activity);
        // "Shown" (some widgets, e.g. TextClock, only tick while they're visible)
        // but never drawn on the window: alpha, not visibility.
        container.setAlpha(0f);
        activity.addContentView(container, new ViewGroup.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
    }

    void start() {
        started = true;
        try {
            host.startListening();
        } catch (RuntimeException e) {
            Log.e(TAG, "can't listen to the widget updates", e);
        }
        for (int i = 0; i < views.size(); i++) {
            views.valueAt(i).scheduleCapture();
        }
    }

    void stop() {
        started = false;
        try {
            host.stopListening();
        } catch (RuntimeException e) {
            Log.e(TAG, "can't stop listening to the widget updates", e);
        }
    }

    /** Every installed widget provider. Called from any thread. */
    Object[] providers() {
        PackageManager pm = activity.getPackageManager();
        int densityDpi = activity.getResources().getDisplayMetrics().densityDpi;
        int previewSide = Math.round(PREVIEW_SIDE_DP * density);
        Map<String, String> appLabels = new HashMap<>();
        List<Provider> providers = new ArrayList<>();
        for (AppWidgetProviderInfo info : manager.getInstalledProviders()) {
            try {
                Provider provider = new Provider();
                provider.component = info.provider.flattenToString();
                provider.label = info.loadLabel(pm);
                String packageName = info.provider.getPackageName();
                String appLabel = appLabels.get(packageName);
                if (appLabel == null) {
                    appLabel = appLabel(pm, packageName);
                    appLabels.put(packageName, appLabel);
                }
                provider.appLabel = appLabel;
                provider.minWidth = info.minWidth / density;
                provider.minHeight = info.minHeight / density;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    provider.targetColumns = info.targetCellWidth;
                    provider.targetRows = info.targetCellHeight;
                }
                Drawable preview = info.loadPreviewImage(activity, densityDpi);
                if (preview == null) {
                    preview = info.loadIcon(activity, densityDpi);
                }
                if (preview != null) {
                    renderPreview(provider, preview, previewSide);
                }
                providers.add(provider);
            } catch (RuntimeException e) {
                Log.w(TAG, "skipping the widget " + info.provider, e);
            }
        }
        return providers.toArray();
    }

    private static String appLabel(PackageManager pm, String packageName) {
        try {
            ApplicationInfo app = pm.getApplicationInfo(packageName, 0);
            return pm.getApplicationLabel(app).toString();
        } catch (PackageManager.NameNotFoundException e) {
            return packageName;
        }
    }

    private static void renderPreview(Provider provider, Drawable drawable, int maxSide) {
        int width = drawable.getIntrinsicWidth();
        int height = drawable.getIntrinsicHeight();
        if (width <= 0 || height <= 0) {
            width = maxSide;
            height = maxSide;
        }
        float scale = Math.min(1f, (float) maxSide / Math.max(width, height));
        width = Math.max(1, Math.round(width * scale));
        height = Math.max(1, Math.round(height * scale));
        Bitmap bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
        drawable.setBounds(0, 0, width, height);
        drawable.draw(new Canvas(bitmap));
        ByteBuffer pixels = ByteBuffer.allocate(width * height * 4);
        bitmap.copyPixelsToBuffer(pixels);
        bitmap.recycle();
        provider.preview = pixels.array();
        provider.previewWidth = width;
        provider.previewHeight = height;
    }

    /**
     * Starts adding a widget of {@code component}: binds it (asking the user the
     * first time, with the system dialog) and opens its configuration activity
     * if it has one. Once done, {@link #nativeOnWidgetAdded}.
     */
    void add(String component) {
        ComponentName provider = ComponentName.unflattenFromString(component);
        if (provider == null) {
            return;
        }
        cancelPending();
        int id = host.allocateAppWidgetId();
        pendingId = id;
        if (manager.bindAppWidgetIdIfAllowed(id, provider)) {
            configure(id);
            return;
        }
        Intent intent = new Intent(AppWidgetManager.ACTION_APPWIDGET_BIND);
        intent.putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, id);
        intent.putExtra(AppWidgetManager.EXTRA_APPWIDGET_PROVIDER, provider);
        try {
            activity.startActivityForResult(intent, REQUEST_BIND);
        } catch (ActivityNotFoundException e) {
            Log.e(TAG, "no activity to bind widgets", e);
            cancelPending();
        }
    }

    private void configure(int id) {
        AppWidgetProviderInfo info = manager.getAppWidgetInfo(id);
        if (info == null) {
            cancelPending();
            return;
        }
        boolean optional = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
            && (info.widgetFeatures & AppWidgetProviderInfo.WIDGET_FEATURE_CONFIGURATION_OPTIONAL) != 0;
        if (info.configure != null && !optional) {
            try {
                host.startAppWidgetConfigureActivityForResult(activity, id, 0, REQUEST_CONFIGURE, configureOptions());
                return;
            } catch (ActivityNotFoundException | SecurityException e) {
                Log.e(TAG, "can't open the widget configuration", e);
                cancelPending();
                return;
            }
        }
        added(id, info);
    }

    /**
     * The configuration activity is started by the system on behalf of this
     * launcher: since Android 14 that needs this opt-in, or it's blocked as a
     * background activity start (and the widget cancelled).
     */
    private static Bundle configureOptions() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            return null;
        }
        ActivityOptions options = ActivityOptions.makeBasic();
        options.setPendingIntentBackgroundActivityStartMode(
            ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED);
        return options.toBundle();
    }

    private void added(int id, AppWidgetProviderInfo info) {
        pendingId = -1;
        if (info == null) {
            host.deleteAppWidgetId(id);
            return;
        }
        nativeOnWidgetAdded(id, info.provider.flattenToString());
    }

    private void cancelPending() {
        if (pendingId != -1) {
            host.deleteAppWidgetId(pendingId);
            pendingId = -1;
        }
    }

    /** Result of the bind or configure activity. Returns whether it was one. */
    boolean onActivityResult(int requestCode, int resultCode) {
        if (requestCode != REQUEST_BIND && requestCode != REQUEST_CONFIGURE) {
            return false;
        }
        int id = pendingId;
        if (id == -1) {
            return true;
        }
        if (resultCode != Activity.RESULT_OK) {
            cancelPending();
        } else if (requestCode == REQUEST_BIND) {
            configure(id);
        } else {
            added(id, manager.getAppWidgetInfo(id));
        }
        return true;
    }

    /**
     * Lays out a widget of Home at {@code width} x {@code height} pixels
     * (creating its view the first time); its frames follow.
     */
    void show(int id, int width, int height) {
        if (width <= 0 || height <= 0) {
            return;
        }
        WidgetView view = views.get(id);
        if (view == null) {
            // No info (its app was uninstalled): the view shows the system's
            // error message.
            view = (WidgetView) host.createView(activity, id, manager.getAppWidgetInfo(id));
            views.put(id, view);
            container.addView(view, new FrameLayout.LayoutParams(width, height));
        }
        view.resize(width, height);
    }

    /** Removes a widget of Home: its view, and its id from the system. */
    void remove(int id) {
        WidgetView view = views.get(id);
        if (view != null) {
            views.remove(id);
            container.removeView(view);
            view.release();
        }
        host.deleteAppWidgetId(id);
    }

    /** Deletes the widgets of this host that are not on Home ({@code keep}). */
    void prune(int[] keep) {
        List<Integer> kept = new ArrayList<>();
        for (int id : keep) {
            kept.add(id);
        }
        int[] ids = host.getAppWidgetIds();
        for (int id : ids) {
            if (id != pendingId && !kept.contains(id)) {
                remove(id);
            }
        }
        for (int i = views.size() - 1; i >= 0; i--) {
            int id = views.keyAt(i);
            if (!kept.contains(id)) {
                remove(id);
            }
        }
    }

    /**
     * A touch on a widget, forwarded from the Slint UI: {@code action} is a
     * {@link MotionEvent} action (down, up, move, cancel), at pixels {@code x},
     * {@code y} of the widget.
     */
    void touch(int id, int action, float x, float y) {
        WidgetView view = views.get(id);
        if (view == null) {
            return;
        }
        long now = SystemClock.uptimeMillis();
        if (action == MotionEvent.ACTION_DOWN) {
            view.downTime = now;
        }
        MotionEvent event = MotionEvent.obtain(view.downTime, now, action, x, y, 0);
        view.dispatchTouchEvent(event);
        event.recycle();
    }

    /** A widget view that draws itself into a bitmap whenever it changes. */
    private final class WidgetView extends AppWidgetHostView {
        private final int id;
        private int width;
        private int height;
        private Bitmap bitmap;
        private Canvas canvas;
        private ByteBuffer pixels;
        private boolean scheduled;
        private long lastCapture;
        long downTime;
        private final Runnable capture = this::capture;

        WidgetView(Context context, int id) {
            super(context);
            this.id = id;
        }

        @SuppressWarnings("deprecation")
        void resize(int width, int height) {
            if (width == this.width && height == this.height) {
                return;
            }
            this.width = width;
            this.height = height;
            ViewGroup.LayoutParams params = getLayoutParams();
            if (params != null) {
                params.width = width;
                params.height = height;
                setLayoutParams(params);
            }
            float widthDp = width / density;
            float heightDp = height / density;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                updateAppWidgetSize(new Bundle(), Collections.singletonList(new SizeF(widthDp, heightDp)));
            } else {
                int w = Math.round(widthDp);
                int h = Math.round(heightDp);
                updateAppWidgetSize(null, w, h, w, h);
            }
            scheduleCapture();
        }

        @Override
        public void updateAppWidget(RemoteViews remoteViews) {
            super.updateAppWidget(remoteViews);
            scheduleCapture();
        }

        @Override
        public void onDescendantInvalidated(View child, View target) {
            super.onDescendantInvalidated(child, target);
            scheduleCapture();
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            super.onLayout(changed, left, top, right, bottom);
            scheduleCapture();
        }

        /** Draws it soon (coalesced, at most every {@link #FRAME_INTERVAL_MS}). */
        void scheduleCapture() {
            // Also called by the constructor of AppWidgetHostView, before the
            // fields of this class are set.
            if (capture == null || scheduled || !started) {
                return;
            }
            scheduled = true;
            long delay = Math.max(0, lastCapture + FRAME_INTERVAL_MS - SystemClock.uptimeMillis());
            handler.postDelayed(capture, delay);
        }

        private void capture() {
            scheduled = false;
            if (!started || width <= 0 || height <= 0 || views.get(id) != this) {
                return;
            }
            lastCapture = SystemClock.uptimeMillis();
            if (isLayoutRequested() || getWidth() != width || getHeight() != height) {
                measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY));
                layout(0, 0, width, height);
            }
            if (bitmap == null || bitmap.getWidth() != width || bitmap.getHeight() != height) {
                release();
                bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
                canvas = new Canvas(bitmap);
                pixels = ByteBuffer.allocateDirect(width * height * 4);
            }
            bitmap.eraseColor(Color.TRANSPARENT);
            try {
                draw(canvas);
            } catch (RuntimeException e) {
                // e.g. a hardware bitmap, which a software canvas can't draw.
                Log.w(TAG, "can't draw the widget " + id, e);
                return;
            }
            pixels.rewind();
            bitmap.copyPixelsToBuffer(pixels);
            pixels.rewind();
            nativeOnWidgetFrame(id, width, height, pixels);
        }

        void release() {
            handler.removeCallbacks(capture);
            scheduled = false;
            if (bitmap != null) {
                bitmap.recycle();
                bitmap = null;
                canvas = null;
                pixels = null;
            }
        }
    }
}
