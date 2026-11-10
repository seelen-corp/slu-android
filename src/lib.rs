mod error;
#[path = "system-modules/mod.rs"]
mod system_modules;

use std::cell::RefCell;
use std::collections::HashMap;

use error::Result;
use slint::{ComponentHandle, ModelRc, VecModel};
use system_modules::config::{AppRef, Slot};
use system_modules::{activity, android_ctx, apps, config, locale, system, theme};

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

impl AppItem {
    fn app_ref(&self) -> AppRef {
        AppRef::new(&self.package_name, &self.activity_name)
    }
}

/// Logical size (px) app icons are shown at, in the list and grid views
/// (`GridMetrics.icon-size` in `ui/components/app-grid.slint`).
const ICON_SIZE: f32 = 48.0;

thread_local! {
    /// Rendered app icons by (package, activity, size). Lives on the UI thread
    /// (`slint::Image` is not `Send`); cleared when packages change, since an
    /// update can change the icon.
    static ICONS: RefCell<HashMap<(String, String, u32), slint::Image>> = Default::default();
    /// Every launchable app, before the Apps Menu search.
    static ALL_APPS: RefCell<Vec<AppItem>> = const { RefCell::new(Vec::new()) };
    /// Current Apps Menu search.
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
    /// The main window, for calls coming from Java (`nativeOnHomeIntent`).
    static MAIN_WINDOW: RefCell<Option<slint::Weak<MainWindow>>> = const { RefCell::new(None) };
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

/// Whether an app is installed (as of the last refresh). Home edits drop the
/// entries of apps that are not.
fn is_installed(app: &AppRef) -> bool {
    ALL_APPS.with_borrow(|apps| {
        apps.iter().any(|item| {
            item.package_name == app.package_name && item.activity_name == app.activity_name
        })
    })
}

/// Lowercase, without the accents of Latin letters, so "camara" finds "Cámara".
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Shows in the Apps Menu the apps matching the current search: titles that
/// start with it first, then the ones that contain it.
fn apply_search(window: &MainWindow) {
    let query = QUERY.with_borrow(|query| fold(query.trim()));
    let items: Vec<AppItem> = ALL_APPS.with_borrow(|apps| {
        if query.is_empty() {
            return apps.clone();
        }
        let (mut starts, mut contains): (Vec<AppItem>, Vec<AppItem>) = (Vec::new(), Vec::new());
        for app in apps {
            let title = fold(&app.title);
            if title.starts_with(&query) {
                starts.push(app.clone());
            } else if title.contains(&query) {
                contains.push(app.clone());
            }
        }
        starts.extend(contains);
        starts
    });
    window.set_apps(ModelRc::new(VecModel::from(items)));
}

/// System accent → `Tokens.system-accent-*`.
fn apply_accent(tokens: Tokens<'_>) {
    match theme::accent_colors() {
        Ok(accent) => {
            tokens.set_system_accent_color(accent.color);
            tokens.set_system_accent_light_color(accent.light);
            tokens.set_system_accent_lighter_color(accent.lighter);
            tokens.set_system_accent_lightest_color(accent.lightest);
            tokens.set_system_accent_dark_color(accent.dark);
            tokens.set_system_accent_darker_color(accent.darker);
            tokens.set_system_accent_darkest_color(accent.darkest);
        }
        Err(err) => log::error!("failed to read the system accent: {err}"),
    }
}

/// Wallpaper brightness → labels over the wallpaper and system bar icons.
fn apply_wallpaper(tokens: Tokens<'_>) {
    let dark_text = theme::wallpaper_supports_dark_text().unwrap_or_else(|err| {
        log::error!("failed to read the wallpaper colors: {err}");
        false
    });
    tokens.set_wallpaper_supports_dark_text(dark_text);
    log_error("set system bars", system::set_light_system_bars(dark_text));
}

fn to_index(value: i32) -> u32 {
    value.max(0) as u32
}

/// Reloads the config, the installed apps and the system look into the main
/// window.
fn refresh(window: &MainWindow) -> Result<()> {
    let mut config = config::load()?;
    // e.g. the Home grid was made smaller in Settings (another process).
    if config.clone().normalize() {
        config = config::update(|config| {
            config.normalize();
        })?;
    }

    apply_accent(window.global::<Tokens>());
    apply_wallpaper(window.global::<Tokens>());

    // The text list shows no icons: don't render them.
    let icon_size = (config.apps_view != config::AppsView::TextList)
        .then(|| (ICON_SIZE * window.window().scale_factor()).round() as u32);

    let items: Vec<AppItem> = apps::get_installed_apps()?
        .into_iter()
        .map(|app| AppItem {
            icon: icon_size.map_or_else(slint::Image::default, |size| app_icon(&app, size)),
            pinned: config.is_on_home(&AppRef::new(&app.package_name, &app.activity_name)),
            title: app.title.into(),
            package_name: app.package_name.into(),
            activity_name: app.activity_name.into(),
            uninstallable: app.uninstallable,
        })
        .collect();
    let by_ref: HashMap<(&str, &str), &AppItem> = items
        .iter()
        .map(|item| {
            (
                (item.package_name.as_str(), item.activity_name.as_str()),
                item,
            )
        })
        .collect();
    let find = |app: &AppRef| {
        by_ref
            .get(&(app.package_name.as_str(), app.activity_name.as_str()))
            .map(|item| (*item).clone())
    };

    // Entries of apps that are not installed are skipped, not deleted: they come
    // back if the app is reinstalled (or mid-update, when it's briefly gone).
    let mut home_items = Vec::new();
    let mut dock_items = Vec::new();
    for entry in &config.home {
        let Some(app) = find(&entry.app) else {
            continue;
        };
        match entry.slot {
            Slot::Grid { page, row, col } => home_items.push(HomeItem {
                app,
                dock: false,
                page: page as i32,
                col: col as i32,
                row: row as i32,
            }),
            Slot::Dock { col } => dock_items.push(HomeItem {
                app,
                dock: true,
                page: 0,
                col: col as i32,
                row: 0,
            }),
        }
    }
    let home_list: Vec<AppItem> = config
        .list_order()
        .into_iter()
        .filter_map(|i| find(&config.home[i].app))
        .collect();

    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.set_home_columns(config.home_columns as i32);
    window.set_home_rows(config.home_rows as i32);
    window.set_home_pages(config.home_pages() as i32);
    window.set_home_items(ModelRc::new(VecModel::from(home_items)));
    window.set_dock_items(ModelRc::new(VecModel::from(dock_items)));
    window.set_home_list(ModelRc::new(VecModel::from(home_list)));

    ALL_APPS.with_borrow_mut(|apps| *apps = items);
    apply_search(window);
    Ok(())
}

/// Applies a Home edit to the config and shows the result.
fn edit_home(
    weak: &slint::Weak<MainWindow>,
    action: &str,
    change: impl FnOnce(&mut config::Config),
) {
    let result = config::update(change)
        .and_then(|_| weak.upgrade().map_or(Ok(()), |window| refresh(&window)));
    log_error(action, result);
}

fn run_settings(locale: &str) -> Result<()> {
    let window = SettingsWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    apply_accent(window.global::<Tokens>());

    let config = config::load()?;
    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.set_home_columns(config.home_columns as i32);
    window.set_home_rows(config.home_rows as i32);

    let weak = window.as_weak();
    window.on_launcher_screens_changed(move |value| {
        let result = config::update(|config| config.launcher_screens = value.into());
        if let (Ok(config), Some(window)) = (&result, weak.upgrade()) {
            window.set_launcher_screens(config.launcher_screens.into());
        }
        log_error("save launcher screens", result.map(|_| ()));
    });
    let weak = window.as_weak();
    window.on_apps_view_changed(move |value| {
        let result = config::update(|config| config.apps_view = value.into());
        if let (Ok(config), Some(window)) = (&result, weak.upgrade()) {
            window.set_apps_view(config.apps_view.into());
        }
        log_error("save apps view", result.map(|_| ()));
    });
    let weak = window.as_weak();
    window.on_home_grid_changed(move |columns, rows| {
        let result = config::update(|config| {
            config.home_columns = to_index(columns).clamp(
                *config::HOME_COLUMNS_RANGE.start(),
                *config::HOME_COLUMNS_RANGE.end(),
            );
            config.home_rows = to_index(rows).clamp(
                *config::HOME_ROWS_RANGE.start(),
                *config::HOME_ROWS_RANGE.end(),
            );
            config.normalize();
        });
        if let (Ok(config), Some(window)) = (&result, weak.upgrade()) {
            window.set_home_columns(config.home_columns as i32);
            window.set_home_rows(config.home_rows as i32);
        }
        log_error("save home grid", result.map(|_| ()));
    });
    window.on_open_wallpaper_picker(|| {
        log_error("open wallpaper picker", system::open_wallpaper_picker())
    });
    window.on_open_default_launcher_settings(|| {
        log_error(
            "open default launcher settings",
            system::open_default_launcher_settings(),
        )
    });
    window.run()?;
    Ok(())
}

fn run_home(locale: &str, home_window: &HomeWindowCell) -> Result<()> {
    let window = MainWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    refresh(&window)?;
    *home_window.borrow_mut() = Some(window.as_weak());
    MAIN_WINDOW.with_borrow_mut(|main| *main = Some(window.as_weak()));

    let launcher = window.global::<LauncherActions>();
    launcher.on_open_settings(|| log_error("open settings", activity::open_settings()));
    launcher.on_open_wallpaper_picker(|| {
        log_error("open wallpaper picker", system::open_wallpaper_picker())
    });
    launcher.on_expand_notifications(|| {
        log_error("expand notifications", system::expand_notifications())
    });
    launcher.on_haptic(|| log_error("haptic feedback", system::long_press_haptic()));
    let weak = window.as_weak();
    launcher.on_search(move |query| {
        QUERY.with_borrow_mut(|current| *current = query.to_string());
        if let Some(window) = weak.upgrade() {
            apply_search(&window);
        }
    });

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
        edit_home(&weak, "add app to home", |config| {
            config.add_to_home(app.app_ref(), is_installed)
        });
    });
    let weak = window.as_weak();
    actions.on_remove_from_home(move |app| {
        edit_home(&weak, "remove app from home", |config| {
            config.remove_from_home(&app.app_ref(), is_installed)
        });
    });

    let home = window.global::<HomeActions>();
    let weak = window.as_weak();
    home.on_move_to(move |app, dock, page, col, row| {
        let slot = if dock {
            Slot::Dock { col: to_index(col) }
        } else {
            Slot::Grid {
                page: to_index(page),
                row: to_index(row),
                col: to_index(col),
            }
        };
        edit_home(&weak, "move app on home", |config| {
            config.move_to(&app.app_ref(), slot, is_installed)
        });
    });
    let weak = window.as_weak();
    home.on_move_to_index(move |app, index| {
        edit_home(&weak, "reorder home", |config| {
            config.move_to_index(&app.app_ref(), to_index(index) as usize, is_installed)
        });
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
type HomeWindowCell = std::rc::Rc<RefCell<Option<slint::Weak<MainWindow>>>>;

fn run(app: slint::android::AndroidApp) -> Result<()> {
    use slint::android::android_activity::{MainEvent, PollEvent};

    android_ctx::init(&app)?;
    apps::init()?;
    config::init(app.internal_data_path())?;

    let home_window = HomeWindowCell::default();
    let listener_window = home_window.clone();
    slint::android::init_with_event_listener(app, move |event| {
        // Back on screen (after opening an app, Settings, the wallpaper picker...):
        // Settings runs in another process, so reload the config, and start again
        // from Home.
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

/// Called from `HomeActivity.onNewIntent` (Android UI thread) when the Home
/// button is pressed while the launcher is in its task. `already_on_home`: it
/// was already in front, so the UI goes back to its first screen.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_HomeActivity_nativeOnHomeIntent(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    already_on_home: jni::sys::jboolean,
) {
    let already_on_home = already_on_home != 0;
    slint::invoke_from_event_loop(move || {
        let window = MAIN_WINDOW.with_borrow(|main| main.as_ref().and_then(|weak| weak.upgrade()));
        if let Some(window) = window {
            if already_on_home {
                window.invoke_home_pressed();
            } else {
                window.invoke_show_home();
            }
        }
    })
    .ok();
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
