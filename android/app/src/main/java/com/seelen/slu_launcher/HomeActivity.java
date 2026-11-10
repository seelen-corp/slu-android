package com.seelen.slu_launcher;

import android.content.ActivityNotFoundException;
import android.content.ContentResolver;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Matrix;
import android.graphics.PixelFormat;
import android.media.ExifInterface;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;

/**
 * The HOME/LAUNCHER Activity. Its theme (manifest) shows the system wallpaper
 * behind the window; the Slint UI draws over it with a transparent background.
 */
public class HomeActivity extends SluNativeActivity {
    static {
        System.loadLibrary("slu_launcher");
    }

    private static final String TAG = "slu_launcher";
    private static final int REQUEST_PICK_IMAGE = 1;
    /** Longest side of the images copied for Home; bigger ones are scaled down. */
    private static final int MAX_IMAGE_SIDE = 1600;

    /** Implemented in Rust (lib.rs). */
    private static native void nativeOnHomeIntent(boolean alreadyOnHome);

    /**
     * Implemented in Rust (lib.rs): an image picked with {@link #pickImage} was
     * copied into {@code files/images/} (system-modules/images.rs) as {@code fileName}.
     */
    private static native void nativeOnImagePicked(String fileName);

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

    /**
     * Opens the system image picker, to put an image on Home. Called from any
     * thread (Rust, system-modules/images.rs); the result arrives in
     * {@link #onActivityResult}.
     */
    @SuppressWarnings("deprecation")
    public void pickImage() {
        runOnUiThread(() -> {
            Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
            intent.setType("image/*");
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            try {
                startActivityForResult(intent, REQUEST_PICK_IMAGE);
            } catch (ActivityNotFoundException e) {
                Log.e(TAG, "no image picker", e);
            }
        });
    }

    @Override
    @SuppressWarnings("deprecation")
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_PICK_IMAGE || resultCode != RESULT_OK
                || data == null || data.getData() == null) {
            return;
        }
        final Uri uri = data.getData();
        // Decoding and writing a photo takes too long for the UI thread.
        new Thread(() -> {
            try {
                nativeOnImagePicked(copyImage(uri));
            } catch (Exception e) {
                Log.e(TAG, "failed to copy the picked image", e);
            }
        }, "slu-pick-image").start();
    }

    /**
     * Copies the image at {@code uri} into {@code files/images/}: upright (EXIF
     * orientation applied), at most {@link #MAX_IMAGE_SIDE} pixels on its longest
     * side, as JPEG (PNG if it has transparency). Returns the new file's name.
     */
    private String copyImage(Uri uri) throws IOException {
        ContentResolver resolver = getContentResolver();

        BitmapFactory.Options bounds = new BitmapFactory.Options();
        bounds.inJustDecodeBounds = true;
        try (InputStream in = resolver.openInputStream(uri)) {
            BitmapFactory.decodeStream(in, null, bounds);
        }
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) {
            throw new IOException("not an image: " + uri);
        }

        BitmapFactory.Options options = new BitmapFactory.Options();
        int longest = Math.max(bounds.outWidth, bounds.outHeight);
        options.inSampleSize = 1;
        while (longest / (options.inSampleSize * 2) >= MAX_IMAGE_SIDE) {
            options.inSampleSize *= 2;
        }
        Bitmap bitmap;
        try (InputStream in = resolver.openInputStream(uri)) {
            bitmap = BitmapFactory.decodeStream(in, null, options);
        }
        if (bitmap == null) {
            throw new IOException("can't decode " + uri);
        }

        int orientation = ExifInterface.ORIENTATION_NORMAL;
        try (InputStream in = resolver.openInputStream(uri)) {
            if (in != null) {
                orientation = new ExifInterface(in)
                    .getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL);
            }
        } catch (IOException e) {
            // No EXIF data: drawn as it is.
        }
        Matrix matrix = new Matrix();
        switch (orientation) {
            case ExifInterface.ORIENTATION_ROTATE_90: matrix.postRotate(90); break;
            case ExifInterface.ORIENTATION_ROTATE_180: matrix.postRotate(180); break;
            case ExifInterface.ORIENTATION_ROTATE_270: matrix.postRotate(270); break;
            case ExifInterface.ORIENTATION_FLIP_HORIZONTAL: matrix.postScale(-1, 1); break;
            case ExifInterface.ORIENTATION_FLIP_VERTICAL: matrix.postScale(1, -1); break;
            case ExifInterface.ORIENTATION_TRANSPOSE: matrix.postRotate(90); matrix.postScale(-1, 1); break;
            case ExifInterface.ORIENTATION_TRANSVERSE: matrix.postRotate(-90); matrix.postScale(-1, 1); break;
            default: break;
        }
        float scale = (float) MAX_IMAGE_SIDE / Math.max(bitmap.getWidth(), bitmap.getHeight());
        if (scale < 1f) {
            matrix.postScale(scale, scale);
        }
        if (!matrix.isIdentity()) {
            Bitmap transformed = Bitmap.createBitmap(
                bitmap, 0, 0, bitmap.getWidth(), bitmap.getHeight(), matrix, true);
            if (transformed != bitmap) {
                bitmap.recycle();
                bitmap = transformed;
            }
        }

        File dir = new File(getFilesDir(), "images");
        if (!dir.isDirectory() && !dir.mkdirs()) {
            throw new IOException("can't create " + dir);
        }
        boolean alpha = bitmap.hasAlpha();
        String name = "image-" + System.currentTimeMillis() + (alpha ? ".png" : ".jpg");
        try (FileOutputStream out = new FileOutputStream(new File(dir, name))) {
            if (!bitmap.compress(alpha ? Bitmap.CompressFormat.PNG : Bitmap.CompressFormat.JPEG, 90, out)) {
                throw new IOException("can't encode the image");
            }
        } finally {
            bitmap.recycle();
        }
        return name;
    }
}
