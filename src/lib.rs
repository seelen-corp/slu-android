mod error;
#[path = "system-modules/mod.rs"]
mod system_modules;

use error::Result;
use system_modules::{activity, android_ctx, apps};

slint::include_modules!();

fn apps_model() -> Result<slint::ModelRc<slint::SharedString>> {
    let titles: Vec<slint::SharedString> = apps::get_installed_apps()?
        .into_iter()
        .map(|app| app.title.into())
        .collect();
    Ok(slint::ModelRc::new(slint::VecModel::from(titles)))
}

fn run(app: slint::android::AndroidApp) -> Result<()> {
    android_ctx::init(&app)?;
    apps::init()?;
    slint::android::init(app)?;

    if activity::current_class_name()? == "SettingsActivity" {
        SettingsWindow::new()?.run()?;
        return Ok(());
    }

    let main_window = MainWindow::new()?;
    main_window.set_app_titles(apps_model()?);
    main_window.on_open_settings(|| {
        if let Err(err) = activity::open_settings() {
            log::error!("failed to open settings: {err}");
        }
    });

    let weak_window = main_window.as_weak();
    apps::subscribe(move || {
        let weak_window = weak_window.clone();
        slint::invoke_from_event_loop(move || {
            let Some(window) = weak_window.upgrade() else {
                return;
            };
            match apps_model() {
                Ok(model) => window.set_app_titles(model),
                Err(err) => log::error!("failed to refresh installed apps: {err}"),
            }
        })
        .ok();
    });

    main_window.run()?;
    Ok(())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );

    log::info!("Hello World from slu_launcher (pure Rust Android launcher skeleton)");

    if let Err(err) = run(app) {
        log::error!("fatal error in android_main: {err}");
    }
}
