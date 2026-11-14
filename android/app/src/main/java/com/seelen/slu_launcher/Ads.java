package com.seelen.slu_launcher;

import android.app.Activity;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.widget.PopupWindow;

import androidx.annotation.NonNull;

import com.google.android.gms.ads.AdListener;
import com.google.android.gms.ads.AdError;
import com.google.android.gms.ads.AdRequest;
import com.google.android.gms.ads.AdSize;
import com.google.android.gms.ads.AdView;
import com.google.android.gms.ads.FullScreenContentCallback;
import com.google.android.gms.ads.LoadAdError;
import com.google.android.gms.ads.MobileAds;
import com.google.android.gms.ads.rewarded.RewardedAd;
import com.google.android.gms.ads.rewarded.RewardedAdLoadCallback;
import com.google.android.ump.ConsentInformation;
import com.google.android.ump.ConsentRequestParameters;
import com.google.android.ump.UserMessagingPlatform;

import java.util.concurrent.atomic.AtomicBoolean;

/**
 * Ads of the Settings screen (only there, never on Home): a banner at the
 * bottom and a rewarded ad the user can choose to watch to support the
 * project. Rust drives it through {@link SettingsActivity} and is told what
 * happens through the native callbacks below (system-modules/ads.rs).
 *
 * The banner is a real {@link AdView} in its own {@link PopupWindow}:
 * NativeActivity takes the window's surface and input queue, so views added to
 * the activity's own window are neither drawn nor touched. Ads must never be
 * drawn offscreen and shown as a picture (like the widgets are): that's invalid
 * traffic for AdMob.
 *
 * Ads are requested only once the user's consent (UMP, GDPR) allows it.
 */
final class Ads {
    static {
        // Settings runs in its own process, where NativeActivity loads the
        // library without registering it for JNI lookups.
        System.loadLibrary("slu_launcher");
    }

    private static final String TAG = "slu_launcher";

    // States of the rewarded ad (`RewardedState` in system-modules/ads.rs).
    static final int REWARDED_IDLE = 0;
    static final int REWARDED_LOADING = 1;
    static final int REWARDED_FAILED = 2;
    static final int REWARDED_THANKS = 3;

    /** Space the banner takes at the bottom, in dp (0 = no banner). */
    private static native void nativeOnBannerHeight(float heightDp);

    /** The rewarded ad changed state ({@code REWARDED_*}). */
    private static native void nativeOnRewardedState(int state);

    /** Whether the user must be able to reopen the consent form (privacy options). */
    private static native void nativeOnPrivacyOptions(boolean required);

    private final Activity activity;
    private final ConsentInformation consent;
    private final AtomicBoolean initStarted = new AtomicBoolean(false);
    private boolean initialized;
    private boolean destroyed;

    private PopupWindow bannerPopup;
    private AdView banner;
    /** Width (dp) the current banner was requested for. */
    private int bannerWidthDp;

    private boolean rewardedBusy;
    private boolean rewardEarned;

    Ads(Activity activity) {
        this.activity = activity;
        this.consent = UserMessagingPlatform.getConsentInformation(activity);
    }

    // Lifecycle (Android UI thread).

    void start() {
        ConsentRequestParameters params = new ConsentRequestParameters.Builder().build();
        consent.requestConsentInfoUpdate(activity, params,
            () -> UserMessagingPlatform.loadAndShowConsentFormIfRequired(activity, error -> {
                if (error != null) {
                    Log.w(TAG, "consent form: " + error.getMessage());
                }
                reportPrivacyOptions();
                initIfAllowed();
            }),
            error -> {
                Log.w(TAG, "consent info update: " + error.getMessage());
                initIfAllowed();
            });
        // Consent given in an earlier session: no need to wait for the update.
        initIfAllowed();
    }

    void resume() {
        if (banner != null) {
            banner.resume();
        }
    }

    void pause() {
        if (banner != null) {
            banner.pause();
        }
    }

    void destroy() {
        destroyed = true;
        removeBanner();
    }

    /** The screen width may have changed: the banner is requested again for it. */
    void onConfigurationChanged() {
        if (initialized && banner != null && currentWidthDp() != bannerWidthDp) {
            removeBanner();
            loadBanner();
        }
    }

    // Called from Rust (any thread).

    /** Loads and shows a rewarded ad. */
    void showRewarded() {
        activity.runOnUiThread(this::loadRewarded);
    }

    /** Reopens the consent form (Settings > privacy options). */
    void showPrivacyOptions() {
        activity.runOnUiThread(() -> UserMessagingPlatform.showPrivacyOptionsForm(activity, error -> {
            if (error != null) {
                Log.w(TAG, "privacy options form: " + error.getMessage());
            }
            initIfAllowed();
        }));
    }

    // Internals (Android UI thread).

    private void reportPrivacyOptions() {
        nativeOnPrivacyOptions(consent.getPrivacyOptionsRequirementStatus()
            == ConsentInformation.PrivacyOptionsRequirementStatus.REQUIRED);
    }

    private void initIfAllowed() {
        if (!consent.canRequestAds() || !initStarted.compareAndSet(false, true)) {
            return;
        }
        // Initialization does disk/network work: off the UI thread.
        new Thread(() -> MobileAds.initialize(activity, status -> activity.runOnUiThread(() -> {
            initialized = true;
            if (!destroyed) {
                loadBanner();
            }
        })), "slu-ads-init").start();
    }

    private int currentWidthDp() {
        DisplayMetrics metrics = activity.getResources().getDisplayMetrics();
        return (int) (metrics.widthPixels / metrics.density);
    }

    private void loadBanner() {
        bannerWidthDp = currentWidthDp();
        final AdView view = new AdView(activity);
        view.setAdUnitId(BuildConfig.ADMOB_BANNER_ID);
        view.setAdSize(AdSize.getCurrentOrientationAnchoredAdaptiveBannerAdSize(activity, bannerWidthDp));
        view.setAdListener(new AdListener() {
            @Override
            public void onAdLoaded() {
                if (view == banner) {
                    showBanner();
                }
            }

            @Override
            public void onAdFailedToLoad(@NonNull LoadAdError error) {
                // The banner refreshes by itself; while it has nothing, it takes
                // no space.
                Log.w(TAG, "banner: " + error.getMessage());
                if (view == banner) {
                    hideBanner();
                }
            }
        });
        banner = view;
        bannerPopup = new PopupWindow(view,
            WindowManager.LayoutParams.MATCH_PARENT, WindowManager.LayoutParams.WRAP_CONTENT);
        // Not focusable: keys (Back) and touches outside it stay with the activity.
        bannerPopup.setFocusable(false);
        bannerPopup.setTouchable(true);
        bannerPopup.setOutsideTouchable(false);
        view.loadAd(new AdRequest.Builder().build());
    }

    /** Bottom system bar height (px): the banner sits above it. */
    private int bottomInset(View decor) {
        WindowInsets insets = decor.getRootWindowInsets();
        return insets == null ? 0 : insets.getStableInsetBottom();
    }

    private void showBanner() {
        final View decor = activity.getWindow().getDecorView();
        if (destroyed || bannerPopup == null) {
            return;
        }
        if (decor.getWindowToken() == null) {
            // Not attached yet (the ad loaded very early): try again once it is.
            decor.post(this::showBanner);
            return;
        }
        final int inset = bottomInset(decor);
        if (!bannerPopup.isShowing()) {
            bannerPopup.showAtLocation(decor, Gravity.BOTTOM | Gravity.CENTER_HORIZONTAL, 0, inset);
        }
        // Whether the popup's offset counts from the screen bottom or from above
        // the system bar depends on the Android version: measured and corrected
        // so it always ends right on top of the bar.
        banner.post(() -> {
            if (bannerPopup == null || !bannerPopup.isShowing()) {
                return;
            }
            int[] location = new int[2];
            banner.getLocationOnScreen(location);
            int screenHeight = decor.getRootView().getHeight();
            int gap = screenHeight - (location[1] + banner.getHeight());
            if (gap != inset) {
                bannerPopup.update(0, inset + (inset - gap), -1, -1);
            }
            float density = activity.getResources().getDisplayMetrics().density;
            nativeOnBannerHeight(banner.getHeight() / density);
        });
    }

    private void hideBanner() {
        if (bannerPopup != null && bannerPopup.isShowing()) {
            bannerPopup.dismiss();
        }
        nativeOnBannerHeight(0);
    }

    private void removeBanner() {
        hideBanner();
        if (banner != null) {
            banner.destroy();
        }
        banner = null;
        bannerPopup = null;
    }

    private void loadRewarded() {
        if (rewardedBusy) {
            return;
        }
        if (!initialized) {
            // No consent yet, or the SDK is still starting.
            nativeOnRewardedState(REWARDED_FAILED);
            return;
        }
        rewardedBusy = true;
        rewardEarned = false;
        nativeOnRewardedState(REWARDED_LOADING);
        RewardedAd.load(activity, BuildConfig.ADMOB_REWARDED_ID, new AdRequest.Builder().build(),
            new RewardedAdLoadCallback() {
                @Override
                public void onAdLoaded(@NonNull RewardedAd ad) {
                    if (destroyed) {
                        rewardedBusy = false;
                        return;
                    }
                    ad.setFullScreenContentCallback(new FullScreenContentCallback() {
                        @Override
                        public void onAdDismissedFullScreenContent() {
                            rewardedBusy = false;
                            nativeOnRewardedState(rewardEarned ? REWARDED_THANKS : REWARDED_IDLE);
                        }

                        @Override
                        public void onAdFailedToShowFullScreenContent(@NonNull AdError error) {
                            Log.w(TAG, "rewarded show: " + error.getMessage());
                            rewardedBusy = false;
                            nativeOnRewardedState(REWARDED_FAILED);
                        }
                    });
                    ad.show(activity, reward -> rewardEarned = true);
                }

                @Override
                public void onAdFailedToLoad(@NonNull LoadAdError error) {
                    Log.w(TAG, "rewarded load: " + error.getMessage());
                    rewardedBusy = false;
                    nativeOnRewardedState(REWARDED_FAILED);
                }
            });
    }
}
