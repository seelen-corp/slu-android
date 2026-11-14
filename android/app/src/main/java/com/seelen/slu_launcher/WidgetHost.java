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
import android.graphics.HardwareRenderer;
import android.graphics.PixelFormat;
import android.graphics.RecordingCanvas;
import android.graphics.Rect;
import android.graphics.RenderNode;
import android.graphics.drawable.Drawable;
import android.hardware.HardwareBuffer;
import android.hardware.SyncFence;
import android.media.Image;
import android.media.ImageReader;
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
import android.view.ViewParent;
import android.widget.FrameLayout;
import android.widget.RemoteViews;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.time.Duration;
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
 * collections like on any launcher), and each one is drawn whenever it changes
 * ({@link WidgetView#capture}): on the GPU into a buffer that Rust shows as a
 * texture (Android 10+, {@link GpuFrames}), or into a bitmap. Rust shows those
 * frames and forwards the touches on them ({@link #touch}).
 *
 * <p>Everything here runs on the UI thread except {@link #providers}.
 */
final class WidgetHost {
    private static final String TAG = "slu_launcher";
    /** Any constant: it identifies this launcher's widgets to the system. */
    private static final int HOST_ID = 0x51;
    static final int REQUEST_BIND = 2;
    static final int REQUEST_CONFIGURE = 3;
    /** At most ~30 bitmap frames per second per widget. */
    private static final long FRAME_INTERVAL_MS = 33;
    /** At most ~60 GPU frames per second per widget. */
    private static final long GPU_FRAME_INTERVAL_MS = 16;
    /** Longest wait for the GPU to finish drawing a frame ({@link GpuFrames}). */
    private static final Duration GPU_FENCE_TIMEOUT = Duration.ofMillis(500);
    /** Longest side of the previews shown in the widget picker. */
    private static final int PREVIEW_SIDE_DP = 160;
    /** Lost frames of a widget drawn again in a row, at most. */
    private static final int MAX_RETRIES = 30;

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

    /**
     * Implemented in Rust (widgets.rs): frame {@code seq} of a widget, drawn on
     * the GPU. Rust holds the buffer until {@link #frameDone}.
     */
    private static native void nativeOnWidgetBuffer(
        int appWidgetId, int seq, int width, int height, HardwareBuffer buffer);

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
    /** Frames drawn on the GPU ({@link GpuFrames}), unless Rust can't show them. */
    private boolean gpuFrames = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q;
    /** Number of the next GPU frame, unique across widgets. */
    private int nextSeq;

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
        container = new FrameLayout(activity) {
            // A widget view invalidated itself (its own changes arrive in
            // WidgetView#invalidateChildInParent).
            @Override
            @SuppressWarnings("deprecation")
            public ViewParent invalidateChildInParent(int[] location, Rect dirty) {
                for (int i = 0; i < views.size(); i++) {
                    WidgetView view = views.valueAt(i);
                    if (view.isDirty()) {
                        view.scheduleCapture();
                    }
                }
                return super.invalidateChildInParent(location, dirty);
            }
        };
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
        // Their GPU buffers aren't needed off screen (start draws them again).
        for (int i = 0; i < views.size(); i++) {
            views.valueAt(i).releaseGpu();
        }
    }

    /** Rust is done with GPU frame {@code seq} of widget {@code id}. */
    void frameDone(int id, int seq) {
        WidgetView view = views.get(id);
        if (view == null || view.gpu == null) {
            return;
        }
        view.gpu.done(seq);
        if (view.waitingForFrame) {
            view.waitingForFrame = false;
            view.scheduleCapture();
        }
    }

    /** Draws every widget again (Rust lost its frames). */
    void redraw() {
        for (int i = 0; i < views.size(); i++) {
            views.valueAt(i).scheduleCapture();
        }
    }

    /** Rust can't show GPU frames: bitmaps from now on. */
    void useBitmaps() {
        if (!gpuFrames) {
            return;
        }
        gpuFrames = false;
        for (int i = 0; i < views.size(); i++) {
            WidgetView view = views.valueAt(i);
            view.releaseGpu();
            view.scheduleCapture();
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
        // Home shows it again (e.g. its entry was rebuilt): it may have no
        // frame yet, even at the same size.
        view.scheduleCapture();
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

    /**
     * A widget view that draws itself whenever it changes: on the GPU
     * ({@link GpuFrames}), or into a bitmap.
     */
    private final class WidgetView extends AppWidgetHostView {
        private final int id;
        private int width;
        private int height;
        private boolean scheduled;
        private long lastCapture;
        long downTime;
        /** Not drawn: Rust held every free GPU frame ({@link #frameDone}). */
        boolean waitingForFrame;
        /** Frames lost in a row ({@link #retryCapture}). */
        private int retries;
        private final Runnable capture = this::capture;
        // Bitmap frames.
        private Bitmap bitmap;
        private Canvas canvas;
        private ByteBuffer pixels;
        /** GPU frames (Android 10+), or null. */
        GpuFrames gpu;

        WidgetView(Context context, int id) {
            super(context);
            this.id = id;
        }

        // AppWidgetHostView pads the widget by the system's default (~8dp per
        // side, `getDefaultPaddingForWidget`), on top of the inset Home already
        // gives every entry (`GridMetrics.cell-padding`): widgets would look
        // smaller than the pictures and folders next to them. Launcher3 drops it
        // the same way.
        @Override
        public void setAppWidget(int appWidgetId, AppWidgetProviderInfo info) {
            super.setAppWidget(appWidgetId, info);
            setPadding(0, 0, 0, 0);
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

        // The window draws in software (its surface is Slint's), so a change
        // inside the widget (an image loaded, a list filled, a clock ticking)
        // arrives here. onDescendantInvalidated is the hardware path.
        @Override
        @SuppressWarnings("deprecation")
        public ViewParent invalidateChildInParent(int[] location, Rect dirty) {
            scheduleCapture();
            return super.invalidateChildInParent(location, dirty);
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

        /** Draws it soon (coalesced, at most one frame per interval). */
        void scheduleCapture() {
            // Also called by the constructor of AppWidgetHostView, before the
            // fields of this class are set.
            if (capture == null || scheduled || !started) {
                return;
            }
            scheduled = true;
            long interval = gpuFrames ? GPU_FRAME_INTERVAL_MS : FRAME_INTERVAL_MS;
            long delay = Math.max(0, lastCapture + interval - SystemClock.uptimeMillis());
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
            if (gpuFrames) {
                captureGpu();
            } else {
                captureBitmap();
            }
        }

        private void captureGpu() {
            if (gpu == null || gpu.width != width || gpu.height != height) {
                releaseGpu();
                try {
                    gpu = new GpuFrames(id, width, height);
                } catch (RuntimeException e) {
                    Log.e(TAG, "can't draw the widget " + id + " on the GPU", e);
                    return;
                }
            }
            if (!gpu.canDraw()) {
                waitingForFrame = true;
                return;
            }
            waitingForFrame = false;
            int result;
            try {
                result = gpu.draw(this);
            } catch (RuntimeException e) {
                Log.w(TAG, "can't draw the widget " + id, e);
                retryCapture();
                return;
            }
            // No frame came out of it (e.g. the first ones of a new renderer on
            // a real GPU), or the views want another one: draw again, or the
            // widget stays blank until it changes by itself.
            if ((result & (HardwareRenderer.SYNC_FRAME_DROPPED
                | HardwareRenderer.SYNC_LOST_SURFACE_REWARD_IF_FOUND
                | HardwareRenderer.SYNC_CONTEXT_IS_STOPPED)) != 0) {
                retryCapture();
            } else if ((result & HardwareRenderer.SYNC_REDRAW_REQUESTED) != 0) {
                scheduleCapture();
            }
        }

        /** A frame was lost on its way to Rust: draws it again (a few times). */
        void retryCapture() {
            if (retries >= MAX_RETRIES) {
                Log.w(TAG, "giving up drawing the widget " + id + " again");
                return;
            }
            retries++;
            scheduleCapture();
        }

        /** A frame reached Rust. */
        void frameDelivered() {
            retries = 0;
        }

        private void captureBitmap() {
            if (bitmap == null || bitmap.getWidth() != width || bitmap.getHeight() != height) {
                releaseBitmap();
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

        void releaseGpu() {
            waitingForFrame = false;
            if (gpu != null) {
                gpu.close();
                gpu = null;
            }
        }

        private void releaseBitmap() {
            if (bitmap != null) {
                bitmap.recycle();
                bitmap = null;
                canvas = null;
                pixels = null;
            }
        }

        void release() {
            handler.removeCallbacks(capture);
            scheduled = false;
            releaseBitmap();
            releaseGpu();
        }
    }

    /**
     * Frames of a widget drawn on the GPU (Android 10+): its views are recorded
     * into a {@link RenderNode} and drawn by a {@link HardwareRenderer} into an
     * {@link ImageReader}, whose buffers go to Rust without copies
     * ({@link #nativeOnWidgetBuffer}). Rust holds each one until it's done with
     * it ({@link #frameDone}); with all of them held, the next frame waits.
     */
    private final class GpuFrames implements ImageReader.OnImageAvailableListener {
        /** One being drawn, one shown, one waiting to be shown. */
        private static final int MAX_IMAGES = 3;

        final int id;
        final int width;
        final int height;
        private final ImageReader reader;
        private final RenderNode node;
        private final HardwareRenderer renderer;
        /** Images given to Rust, by frame number. */
        private final SparseArray<Image> held = new SparseArray<>();
        private boolean closed;

        GpuFrames(int id, int width, int height) {
            this.id = id;
            this.width = width;
            this.height = height;
            reader = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, MAX_IMAGES,
                HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE | HardwareBuffer.USAGE_GPU_COLOR_OUTPUT);
            reader.setOnImageAvailableListener(this, handler);
            node = new RenderNode("widget-" + id);
            node.setPosition(0, 0, width, height);
            renderer = new HardwareRenderer();
            renderer.setOpaque(false);
            renderer.setContentRoot(node);
            renderer.setSurface(reader.getSurface());
        }

        /** A buffer is free for one more frame. */
        boolean canDraw() {
            return held.size() < MAX_IMAGES - 1;
        }

        /** Returns the {@code HardwareRenderer.SYNC_*} flags of the frame. */
        int draw(View view) {
            RecordingCanvas canvas = node.beginRecording(width, height);
            try {
                view.draw(canvas);
            } finally {
                node.endRecording();
            }
            return renderer.createRenderRequest().syncAndDraw();
        }

        @Override
        public void onImageAvailable(ImageReader reader) {
            if (closed) {
                return;
            }
            Image image;
            try {
                image = reader.acquireLatestImage();
            } catch (IllegalStateException e) {
                Log.w(TAG, "no image free for the widget " + id, e);
                retry();
                return;
            }
            if (image == null) {
                return;
            }
            if (!awaitDrawn(image)) {
                image.close();
                retry();
                return;
            }
            HardwareBuffer buffer = image.getHardwareBuffer();
            if (buffer == null) {
                image.close();
                retry();
                return;
            }
            int seq = nextSeq++;
            held.put(seq, image);
            WidgetView view = views.get(id);
            if (view != null && view.gpu == this) {
                view.frameDelivered();
            }
            nativeOnWidgetBuffer(id, seq, width, height, buffer);
            buffer.close();
        }

        /**
         * This frame never reached Rust: without another one the widget stays
         * blank (a new widget draws only a few frames, unlike on startup, where
         * they're all drawn again once Slint's renderer is set up).
         */
        private void retry() {
            WidgetView view = views.get(id);
            if (view != null && view.gpu == this) {
                view.retryCapture();
            }
        }

        /**
         * Waits until the GPU finished drawing {@code image}: it's handed over as
         * soon as it's queued, while the GPU may still be drawing it (always with
         * HWUI on Vulkan, the default on recent phones), and Rust would show a
         * blank frame. Returns false if it isn't drawn in time (frame skipped).
         */
        private boolean awaitDrawn(Image image) {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
                return true;
            }
            try (SyncFence fence = image.getFence()) {
                if (fence.isValid() && !fence.await(GPU_FENCE_TIMEOUT)) {
                    Log.w(TAG, "the widget " + id + " took too long to draw");
                    return false;
                }
            } catch (IOException | RuntimeException e) {
                Log.w(TAG, "can't wait for the widget " + id + " to draw", e);
            }
            return true;
        }

        /** Rust is done with frame {@code seq} (if it's one of these). */
        void done(int seq) {
            Image image = held.get(seq);
            if (image != null) {
                held.remove(seq);
                image.close();
            }
        }

        /** Rust keeps its own reference to the buffers it still holds. */
        void close() {
            closed = true;
            reader.setOnImageAvailableListener(null, null);
            renderer.destroy();
            node.discardDisplayList();
            for (int i = 0; i < held.size(); i++) {
                held.valueAt(i).close();
            }
            held.clear();
            reader.close();
        }
    }
}
