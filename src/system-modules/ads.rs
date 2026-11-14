//! Ads of the Settings screen (`Ads.java`, AdMob): a banner at the bottom and a
//! rewarded ad the user may watch to support the project. Never on Home. The
//! Java side handles the consent (UMP), loading and showing; this module
//! starts the actions and passes on what it reports.

use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::JClass;

use crate::error::Result;
use crate::system_modules::android_ctx;

/// State of the rewarded ad (`Ads.REWARDED_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewardedState {
    Idle,
    Loading,
    /// No ad could be loaded or shown (or ads aren't allowed yet).
    Failed,
    /// Watched to the end.
    Thanks,
}

impl RewardedState {
    fn from_java(value: i32) -> Self {
        match value {
            1 => Self::Loading,
            2 => Self::Failed,
            3 => Self::Thanks,
            _ => Self::Idle,
        }
    }
}

/// What `Ads.java` reports.
#[derive(Clone, Copy, Debug)]
pub enum AdEvent {
    /// Space the banner takes above the bottom system bar (dp = Slint logical
    /// px); 0 without banner.
    BannerHeight(f32),
    Rewarded(RewardedState),
    /// The user must be able to reopen the consent form.
    PrivacyOptions(bool),
}

type Listener = Box<dyn Fn(AdEvent) + Send + 'static>;

static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);

/// Last banner height and privacy options requirement, for a listener that
/// subscribes after Java reported them (it starts with the Activity).
static LAST: Mutex<(f32, bool)> = Mutex::new((0.0, false));

/// `on_event` runs (on the Android UI thread) for every `AdEvent`; right away
/// with the current banner height and privacy options requirement. Replaces
/// the previous listener.
pub fn subscribe(on_event: impl Fn(AdEvent) + Send + 'static) {
    let Ok(mut listener) = LISTENER.lock() else {
        return;
    };
    if let Ok(last) = LAST.lock() {
        on_event(AdEvent::BannerHeight(last.0));
        on_event(AdEvent::PrivacyOptions(last.1));
    }
    *listener = Some(Box::new(on_event));
}

fn emit(event: AdEvent) {
    // Taken first, like `subscribe`, so no event is lost between the two.
    let Ok(listener) = LISTENER.lock() else {
        return;
    };
    if let Ok(mut last) = LAST.lock() {
        match event {
            AdEvent::BannerHeight(height) => last.0 = height,
            AdEvent::PrivacyOptions(required) => last.1 = required,
            AdEvent::Rewarded(_) => {}
        }
    }
    if let Some(listener) = listener.as_ref() {
        listener(event);
    }
}

/// Loads and shows a rewarded ad (Settings only).
pub fn show_rewarded() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "showRewardedAd", "()V", &[])?;
        Ok(())
    })
}

/// Reopens the ads consent form (Settings only).
pub fn show_privacy_options() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "showAdPrivacyOptions", "()V", &[])?;
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_Ads_nativeOnBannerHeight(
    _env: JNIEnv,
    _class: JClass,
    height_dp: jni::sys::jfloat,
) {
    emit(AdEvent::BannerHeight(height_dp.max(0.0)));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_Ads_nativeOnRewardedState(
    _env: JNIEnv,
    _class: JClass,
    state: jni::sys::jint,
) {
    emit(AdEvent::Rewarded(RewardedState::from_java(state)));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_Ads_nativeOnPrivacyOptions(
    _env: JNIEnv,
    _class: JClass,
    required: jni::sys::jboolean,
) {
    emit(AdEvent::PrivacyOptions(required != 0));
}
