mod error;
#[path = "system-modules/mod.rs"]
mod system_modules;

use error::Result;
use system_modules::{activity, android_ctx, apps, config, locale};

slint::include_modules!();

rust_i18n::i18n!("i18n", fallback = "en");

/// Wires the `I18n` Slint global of a window to the translations in `i18n/`.
fn setup_i18n(i18n: I18n<'_>, locale: &str) {
    i18n.on_translate(|key, locale| {
        rust_i18n::t!(key.as_str(), locale = locale.as_str())
            .as_ref()
            .into()
    });
    i18n.set_locale(locale.into());
}

fn current_locale() -> String {
    locale::system_locale().unwrap_or_else(|err| {
        log::error!("failed to read system locale: {err}");
        "en".to_owned()
    })
}

fn log_error(action: &str, result: Result<()>) {
    if let Err(err) = result {
        log::error!("failed to {action}: {err}");
    }
}

impl From<config::LauncherScreens> for LauncherScreens {
    fn from(value: config::LauncherScreens) -> Self {
        match value {
            config::LauncherScreens::AppsMenu => Self::AppsMenu,
            config::LauncherScreens::HomeAndAppsMenu => Self::HomeAndAppsMenu,
        }
    }
}

impl From<LauncherScreens> for config::LauncherScreens {
    fn from(value: LauncherScreens) -> Self {
        match value {
            LauncherScreens::AppsMenu => Self::AppsMenu,
            LauncherScreens::HomeAndAppsMenu => Self::HomeAndAppsMenu,
        }
    }
}

impl From<config::AppsView> for AppsView {
    fn from(value: config::AppsView) -> Self {
        match value {
            config::AppsView::List => Self::List,
            config::AppsView::TextList => Self::TextList,
            config::AppsView::Grid => Self::Grid,
        }
    }
}

impl From<AppsView> for config::AppsView {
    fn from(value: AppsView) -> Self {
        match value {
            AppsView::List => Self::List,
            AppsView::TextList => Self::TextList,
            AppsView::Grid => Self::Grid,
        }
    }
}

/// Logical size (px) app icons are shown at, in the list and grid views
/// (`ui/components/app-list-item.slint`, `app-grid.slint`).
const ICON_SIZE: f32 = 48.0;

thread_local! {
    /// Rendered app icons by (package, activity, size). Lives on the UI thread
    /// (`slint::Image` is not `Send`); cleared when packages change, since an
    /// update can change the icon.
    static ICONS: std::cell::RefCell<std::collections::HashMap<(String, String, u32), slint::Image>> =
        Default::default();
}

fn app_icon(app: &apps::AppInfo, size: u32) -> slint::Image {
    // The size is part of the key: the scale factor is only known once the
    // window is shown, so the first refresh may render at another size.
    let key = (app.package_name.clone(), app.activity_name.clone(), size);
    if let Some(icon) = ICONS.with_borrow(|icons| icons.get(&key).cloned()) {
        return icon;
    }
    match apps::load_icon(&app.package_name, &app.activity_name, size) {
        Ok(buffer) => {
            let icon = slint::Image::from_rgba8(buffer);
            ICONS.with_borrow_mut(|icons| icons.insert(key, icon.clone()));
            icon
        }
        Err(err) => {
            log::error!("failed to load icon of {}: {err}", app.package_name);
            slint::Image::default()
        }
    }
}

/// Reloads the config and the installed apps into the main window.
fn refresh(window: &MainWindow) -> Result<()> {
    let config = config::load()?;
    // The text list shows no icons: don't render them.
    let icon_size = (config.apps_view != config::AppsView::TextList)
        .then(|| (ICON_SIZE * window.window().scale_factor()).round() as u32);

    let items: Vec<AppItem> = apps::get_installed_apps()?
        .into_iter()
        .map(|app| AppItem {
            icon: icon_size.map_or_else(slint::Image::default, |size| app_icon(&app, size)),
            pinned: config.is_pinned(&app.package_name, &app.activity_name),
            title: app.title.into(),
            package_name: app.package_name.into(),
            activity_name: app.activity_name.into(),
            uninstallable: app.uninstallable,
        })
        .collect();

    // Pins of apps that are no longer installed are skipped, not deleted: they
    // come back if the app is reinstalled (or mid-update, when it's briefly gone).
    let home_items: Vec<AppItem> = config
        .pinned
        .iter()
        .filter_map(|pinned| {
            items
                .iter()
                .find(|item| {
                    item.package_name == pinned.package_name
                        && item.activity_name == pinned.activity_name
                })
                .cloned()
        })
        .collect();

    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.set_apps(slint::ModelRc::new(slint::VecModel::from(items)));
    window.set_home_apps(slint::ModelRc::new(slint::VecModel::from(home_items)));
    Ok(())
}

fn run_settings(locale: &str) -> Result<()> {
    let window = SettingsWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    let config = config::load()?;
    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.on_launcher_screens_changed(|value| {
        log_error(
            "save launcher screens",
            config::update(|config| config.launcher_screens = value.into()).map(|_| ()),
        );
    });
    window.on_apps_view_changed(|value| {
        log_error(
            "save apps view",
            config::update(|config| config.apps_view = value.into()).map(|_| ()),
        );
    });
    window.run()?;
    Ok(())
}

fn run_home(locale: &str, home_window: &HomeWindowCell) -> Result<()> {
    let window = MainWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    refresh(&window)?;
    *home_window.borrow_mut() = Some(window.as_weak());

    window.on_open_settings(|| log_error("open settings", activity::open_settings()));

    let actions = window.global::<AppActions>();
    actions.on_launch(|app| {
        log_error(
            "launch app",
            apps::launch(&app.package_name, &app.activity_name),
        );
    });
    actions.on_app_info(|app| log_error("open app info", apps::open_app_info(&app.package_name)));
    actions.on_uninstall(|app| log_error("uninstall app", apps::uninstall(&app.package_name)));

    let weak = window.as_weak();
    actions.on_add_to_home(move |app| {
        let result = config::update(|config| {
            if !config.is_pinned(&app.package_name, &app.activity_name) {
                config.pinned.push(config::PinnedApp {
                    package_name: app.package_name.to_string(),
                    activity_name: app.activity_name.to_string(),
                });
            }
        })
        .and_then(|_| weak.upgrade().map_or(Ok(()), |window| refresh(&window)));
        log_error("add app to home", result);
    });

    let weak = window.as_weak();
    actions.on_remove_from_home(move |app| {
        let result = config::update(|config| {
            config.pinned.retain(|pinned| {
                pinned.package_name != app.package_name.as_str()
                    || pinned.activity_name != app.activity_name.as_str()
            });
        })
        .and_then(|_| weak.upgrade().map_or(Ok(()), |window| refresh(&window)));
        log_error("remove app from home", result);
    });

    let weak = window.as_weak();
    apps::subscribe(move || {
        weak.upgrade_in_event_loop(|window| {
            ICONS.with_borrow_mut(|icons| icons.clear());
            log_error("refresh apps", refresh(&window));
        })
        .ok();
    });

    window.run()?;
    Ok(())
}

/// The main window, once created. Read by the lifecycle listener, which is
/// installed before any window exists.
type HomeWindowCell = std::rc::Rc<std::cell::RefCell<Option<slint::Weak<MainWindow>>>>;

fn run(app: slint::android::AndroidApp) -> Result<()> {
    use slint::android::android_activity::{MainEvent, PollEvent};

    android_ctx::init(&app)?;
    apps::init()?;
    config::init(app.internal_data_path())?;

    let home_window = HomeWindowCell::default();
    let listener_window = home_window.clone();
    slint::android::init_with_event_listener(app, move |event| {
        // Back on screen (after opening an app, Settings, ...): Settings runs in
        // another process, so reload the config, and start again from Home.
        if let PollEvent::Main(MainEvent::Resume { .. }) = event
            && let Some(weak) = listener_window.borrow().as_ref()
        {
            weak.upgrade_in_event_loop(|window| {
                log_error("refresh on resume", refresh(&window));
                window.invoke_show_home();
            })
            .ok();
        }
    })?;

    let locale = current_locale();
    if activity::current_class_name()? == "SettingsActivity" {
        run_settings(&locale)
    } else {
        run_home(&locale, &home_window)
    }
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
