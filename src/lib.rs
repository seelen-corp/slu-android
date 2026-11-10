mod error;
#[path = "system-modules/mod.rs"]
mod system_modules;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use error::Result;
use slint::{ComponentHandle, ModelRc, VecModel};
use system_modules::config::{AppRef, EntryKey, HomeContent, Slot};
use system_modules::{activity, android_ctx, apps, config, icons, images, locale, system, theme};

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

impl From<config::IconShape> for IconShape {
    fn from(value: config::IconShape) -> Self {
        match value {
            config::IconShape::System => Self::System,
            config::IconShape::Circle => Self::Circle,
            config::IconShape::Squircle => Self::Squircle,
            config::IconShape::Teardrop => Self::Teardrop,
            config::IconShape::RoundedSquare => Self::RoundedSquare,
        }
    }
}

impl From<IconShape> for config::IconShape {
    fn from(value: IconShape) -> Self {
        match value {
            IconShape::System => Self::System,
            IconShape::Circle => Self::Circle,
            IconShape::Squircle => Self::Squircle,
            IconShape::Teardrop => Self::Teardrop,
            IconShape::RoundedSquare => Self::RoundedSquare,
        }
    }
}

impl From<config::LegacyIcons> for LegacyIcons {
    fn from(value: config::LegacyIcons) -> Self {
        match value {
            config::LegacyIcons::Wrap => Self::Wrap,
            config::LegacyIcons::Original => Self::Original,
        }
    }
}

impl From<LegacyIcons> for config::LegacyIcons {
    fn from(value: LegacyIcons) -> Self {
        match value {
            LegacyIcons::Wrap => Self::Wrap,
            LegacyIcons::Original => Self::Original,
        }
    }
}

impl From<config::SwipeDownAction> for SwipeDownAction {
    fn from(value: config::SwipeDownAction) -> Self {
        match value {
            config::SwipeDownAction::Search => Self::Search,
            config::SwipeDownAction::Notifications => Self::Notifications,
        }
    }
}

impl From<SwipeDownAction> for config::SwipeDownAction {
    fn from(value: SwipeDownAction) -> Self {
        match value {
            SwipeDownAction::Search => Self::Search,
            SwipeDownAction::Notifications => Self::Notifications,
        }
    }
}

impl AppItem {
    fn app_ref(&self) -> AppRef {
        AppRef::new(&self.package_name, &self.activity_name)
    }
}

impl HomeItem {
    fn key(&self) -> EntryKey {
        if self.is_folder {
            EntryKey::Folder(to_index(self.folder.id))
        } else if self.is_picture {
            EntryKey::Image(to_index(self.picture.id))
        } else {
            EntryKey::App(self.app.app_ref())
        }
    }
}

/// A grid cell (`dock` false) or dock slot, as the UI gives them.
fn slot(dock: bool, page: i32, col: i32, row: i32) -> Slot {
    if dock {
        Slot::Dock { col: to_index(col) }
    } else {
        Slot::Grid {
            page: to_index(page),
            row: to_index(row),
            col: to_index(col),
        }
    }
}

/// Largest logical size (px) app icons are shown at in the current view, so
/// they're only ever scaled down. `None` in the text list (no icons). In the
/// grid the item size is the icon size (`GridMetrics.icon-size`, only smaller
/// if the column is too narrow); in the list, the row minus its padding
/// (`AppListItem`).
fn icon_size(config: &config::Config) -> Option<f32> {
    let item = config.item_size(config.apps_view) as f32;
    match config.apps_view {
        config::AppsView::Grid => Some(item),
        config::AppsView::List => Some(item - 2.0 * 8.0),
        config::AppsView::TextList => None,
    }
}

thread_local! {
    /// Rendered app icons by (package, activity, size, shape). Lives on the UI
    /// thread (`slint::Image` is not `Send`); cleared when packages change, since
    /// an update can change the icon.
    static ICONS: RefCell<HashMap<IconKey, slint::Image>> = Default::default();
    /// Every launchable app, before the Apps Menu search.
    static ALL_APPS: RefCell<Vec<AppItem>> = const { RefCell::new(Vec::new()) };
    /// Current Apps Menu search.
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
    /// The main window, for calls coming from Java (`nativeOnHomeIntent`).
    static MAIN_WINDOW: RefCell<Option<slint::Weak<MainWindow>>> = const { RefCell::new(None) };
    /// Folder shown in the folder view, kept up to date by `refresh`.
    static OPEN_FOLDER: Cell<Option<u32>> = const { Cell::new(None) };
    /// The icon shape alone (`AppsLayout.icon-shape`), by size and style.
    static ICON_SHAPE: RefCell<Option<(u32, config::IconStyle, slint::Image)>> = const { RefCell::new(None) };
    /// Decoded images of the pictures on Home, by file name.
    static PICTURES: RefCell<HashMap<String, slint::Image>> = Default::default();
    /// The config shown by the last `refresh`, for the checks the UI makes while
    /// dragging (`HomeActions.can-move`, `can-resize-picture`).
    static SHOWN_CONFIG: RefCell<config::Config> = RefCell::new(config::Config::default());
    /// Grid cell where the image being picked goes (`HomeActions.add-picture`).
    static PICTURE_SLOT: Cell<Option<Slot>> = const { Cell::new(None) };
}

fn picture_image(file: &str) -> slint::Image {
    if let Some(image) = PICTURES.with_borrow(|pictures| pictures.get(file).cloned()) {
        return image;
    }
    match images::load(file) {
        Ok(image) => {
            PICTURES.with_borrow_mut(|pictures| pictures.insert(file.to_owned(), image.clone()));
            image
        }
        Err(err) => {
            log::error!("failed to load a picture: {err}");
            slint::Image::default()
        }
    }
}

/// The icon shape at `size` physical pixels, for the folders.
fn icon_shape(size: u32, style: config::IconStyle) -> slint::Image {
    if let Some(image) = ICON_SHAPE.with_borrow(|shape| {
        shape
            .as_ref()
            .filter(|(s, st, _)| *s == size && *st == style)
            .map(|(_, _, image)| image.clone())
    }) {
        return image;
    }
    match icons::load_shape(size, style) {
        Ok(buffer) => {
            let image = slint::Image::from_rgba8(buffer);
            ICON_SHAPE.with_borrow_mut(|shape| *shape = Some((size, style, image.clone())));
            image
        }
        Err(err) => {
            log::error!("failed to render the icon shape: {err}");
            slint::Image::default()
        }
    }
}

type IconKey = (String, String, u32, config::IconStyle);

fn app_icon(app: &apps::AppInfo, size: u32, style: config::IconStyle) -> slint::Image {
    // The size is part of the key: the scale factor is only known once the
    // window is shown, so the first refresh may render at another size.
    let key = (
        app.package_name.clone(),
        app.activity_name.clone(),
        size,
        style,
    );
    if let Some(icon) = ICONS.with_borrow(|icons| icons.get(&key).cloned()) {
        return icon;
    }
    match icons::load_icon(&app.package_name, &app.activity_name, size, style) {
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
    apply_system_bars(tokens);
}

/// System bar icons: dark over a light wallpaper, unless it's behind the scrim
/// (`Tokens.wallpaper-dimmed`).
fn apply_system_bars(tokens: Tokens<'_>) {
    let dark_icons = tokens.get_wallpaper_supports_dark_text() && !tokens.get_wallpaper_dimmed();
    log_error("set system bars", system::set_light_system_bars(dark_icons));
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
    let icon_size = icon_size(&config)
        .map(|size| (size * window.window().scale_factor()).round().max(1.0) as u32);
    // Icons rendered at another size or style (changed in Settings) are not used
    // anymore.
    let style = config.icon_style();
    ICONS
        .with_borrow_mut(|icons| icons.retain(|key, _| Some(key.2) == icon_size && key.3 == style));

    let items: Vec<AppItem> = apps::get_installed_apps()?
        .into_iter()
        .map(|app| AppItem {
            icon: icon_size.map_or_else(slint::Image::default, |size| app_icon(&app, size, style)),
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
    // Folders show their installed apps; with none, they're skipped too.
    let home_item = |entry: &config::HomeEntry| -> Option<HomeItem> {
        let mut item = HomeItem::default();
        match &entry.content {
            HomeContent::App(app) => item.app = find(app)?,
            HomeContent::Folder(folder) => {
                let apps: Vec<AppItem> = folder.apps.iter().filter_map(&find).collect();
                if apps.is_empty() {
                    return None;
                }
                item.is_folder = true;
                item.folder = FolderItem {
                    id: folder.id as i32,
                    title: folder.title.as_str().into(),
                    apps: ModelRc::new(VecModel::from(apps)),
                };
            }
            HomeContent::Image(image) => {
                item.is_picture = true;
                item.picture = PictureItem {
                    id: image.id as i32,
                    source: picture_image(&image.file),
                    columns: image.columns as i32,
                    rows: image.rows as i32,
                };
            }
        }
        let (dock, page, col, row) = match entry.slot {
            Slot::Grid { page, row, col } => (false, page, col, row),
            Slot::Dock { col } => (true, 0, col, 0),
        };
        Some(HomeItem {
            dock,
            page: page as i32,
            col: col as i32,
            row: row as i32,
            ..item
        })
    };
    let (rows, columns) = (config.home_rows as usize, config.home_columns as usize);
    // 0 free, 1 app or folder, 2 picture (`HomeGrid.home-cells`).
    let mut home_cells = vec![0; config.home_pages() as usize * rows * columns];
    let mut dock_cells = vec![false; columns];
    let mut home_items = Vec::new();
    let mut home_pictures = Vec::new();
    let mut dock_items = Vec::new();
    for entry in &config.home {
        let Some(item) = home_item(entry) else {
            continue;
        };
        if item.dock {
            if let Some(cell) = dock_cells.get_mut(item.col as usize) {
                *cell = true;
            }
            dock_items.push(item);
            continue;
        }
        let kind = if item.is_picture { 2 } else { 1 };
        for cell in entry.cells() {
            if let Slot::Grid { page, row, col } = cell {
                let (page, row, col) = (page as usize, row as usize, col as usize);
                if row < rows
                    && col < columns
                    && let Some(cell) = home_cells.get_mut((page * rows + row) * columns + col)
                {
                    *cell = kind;
                }
            }
        }
        if item.is_picture {
            home_pictures.push(item);
        } else {
            home_items.push(item);
        }
    }
    // Images of pictures no longer on Home.
    PICTURES.with_borrow_mut(|pictures| {
        pictures.retain(|file, _| {
            config.home.iter().any(
                |entry| matches!(&entry.content, HomeContent::Image(image) if image.file == *file),
            )
        })
    });
    let home_list: Vec<HomeItem> = config
        .list_order()
        .into_iter()
        .filter_map(|i| home_item(&config.home[i]))
        .collect();

    // The open folder: updated, or closed if it's gone (e.g. left with one app).
    if let Some(id) = OPEN_FOLDER.get() {
        let open = home_items
            .iter()
            .chain(&dock_items)
            .find(|item| item.is_folder && item.folder.id == id as i32);
        match open {
            Some(item) => window.set_open_folder(item.folder.clone()),
            None => {
                OPEN_FOLDER.set(None);
                window.set_folder_open(false);
            }
        }
    }

    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.set_swipe_down_action(config.swipe_down.into());
    window.set_home_columns(config.home_columns as i32);
    window.set_home_pages(config.home_pages() as i32);
    window.set_home_rows(config.home_rows as i32);
    window.set_home_cells(ModelRc::new(VecModel::from(home_cells)));
    window.set_dock_cells(ModelRc::new(VecModel::from(dock_cells)));
    window.set_home_items(ModelRc::new(VecModel::from(home_items)));
    window.set_home_pictures(ModelRc::new(VecModel::from(home_pictures)));
    window.set_dock_items(ModelRc::new(VecModel::from(dock_items)));
    window.set_home_list(ModelRc::new(VecModel::from(home_list)));
    let layout = window.global::<AppsLayout>();
    layout.set_icon_shape(
        icon_size.map_or_else(slint::Image::default, |size| icon_shape(size, style)),
    );
    layout.set_item_size(config.item_size(config.apps_view) as f32);
    layout.set_padding(config.apps_padding as f32);
    layout.set_label_lines(config.label_lines as i32);

    ALL_APPS.with_borrow_mut(|apps| *apps = items);
    SHOWN_CONFIG.with_borrow_mut(|shown| *shown = config);
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

/// Shows a config in the Settings window.
fn show_settings(window: &SettingsWindow, config: &config::Config) {
    window.set_launcher_screens(config.launcher_screens.into());
    window.set_apps_view(config.apps_view.into());
    window.set_swipe_down_action(config.swipe_down.into());
    window.set_icon_shape(config.icon_shape.into());
    window.set_icon_corner_radius(config.icon_corner_radius as i32);
    window.set_legacy_icons(config.legacy_icons.into());
    window.set_home_columns(config.home_columns as i32);
    window.set_item_size(config.item_size(config.apps_view) as i32);
    window.set_apps_padding(config.apps_padding as i32);
    window.set_label_lines(config.label_lines as i32);
}

/// Settings change: saves it and shows the saved config.
fn save_setting(
    weak: &slint::Weak<SettingsWindow>,
    action: &str,
    change: impl FnOnce(&mut config::Config),
) {
    let result = config::update(change);
    if let (Ok(config), Some(window)) = (&result, weak.upgrade()) {
        show_settings(&window, config);
    }
    log_error(action, result.map(|_| ()));
}

fn run_settings(locale: &str) -> Result<()> {
    let window = SettingsWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    apply_accent(window.global::<Tokens>());
    show_settings(&window, &config::load()?);

    let weak = window.as_weak();
    window.on_launcher_screens_changed(move |value| {
        save_setting(&weak, "save launcher screens", |config| {
            config.launcher_screens = value.into()
        });
    });
    let weak = window.as_weak();
    window.on_apps_view_changed(move |value| {
        save_setting(&weak, "save apps view", |config| {
            config.apps_view = value.into()
        });
    });
    let weak = window.as_weak();
    window.on_swipe_down_action_changed(move |value| {
        save_setting(&weak, "save swipe down action", |config| {
            config.swipe_down = value.into()
        });
    });
    let weak = window.as_weak();
    window.on_icon_shape_changed(move |value| {
        save_setting(&weak, "save icon shape", |config| {
            config.icon_shape = value.into()
        });
    });
    let weak = window.as_weak();
    window.on_icon_corner_radius_changed(move |value| {
        save_setting(&weak, "save icon corner radius", |config| {
            config.icon_corner_radius = to_index(value).clamp(
                *config::ICON_CORNER_RADIUS_RANGE.start(),
                *config::ICON_CORNER_RADIUS_RANGE.end(),
            )
        });
    });
    let weak = window.as_weak();
    window.on_legacy_icons_changed(move |value| {
        save_setting(&weak, "save legacy icons", |config| {
            config.legacy_icons = value.into()
        });
    });
    let weak = window.as_weak();
    window.on_grid_columns_changed(move |columns| {
        save_setting(&weak, "save grid columns", |config| {
            config.home_columns = to_index(columns).clamp(
                *config::HOME_COLUMNS_RANGE.start(),
                *config::HOME_COLUMNS_RANGE.end(),
            );
            config.normalize();
        });
    });
    let weak = window.as_weak();
    window.on_item_size_changed(move |value| {
        save_setting(&weak, "save item size", |config| {
            config.set_item_size(config.apps_view, to_index(value))
        });
    });
    let weak = window.as_weak();
    window.on_apps_padding_changed(move |value| {
        save_setting(&weak, "save apps padding", |config| {
            config.apps_padding = to_index(value).clamp(
                *config::APPS_PADDING_RANGE.start(),
                *config::APPS_PADDING_RANGE.end(),
            )
        });
    });
    let weak = window.as_weak();
    window.on_label_lines_changed(move |value| {
        save_setting(&weak, "save label lines", |config| {
            config.label_lines = to_index(value).clamp(
                *config::LABEL_LINES_RANGE.start(),
                *config::LABEL_LINES_RANGE.end(),
            )
        });
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
    let weak = window.as_weak();
    launcher.on_wallpaper_dimmed_changed(move || {
        if let Some(window) = weak.upgrade() {
            apply_system_bars(window.global::<Tokens>());
        }
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

    let weak = window.as_weak();
    actions.on_remove_from_folder(move |app| {
        edit_home(&weak, "remove app from folder", |config| {
            config.remove_from_folder(&app.app_ref(), is_installed)
        });
    });

    let home = window.global::<HomeActions>();
    let weak = window.as_weak();
    home.on_move_to(move |item, dock, page, col, row| {
        edit_home(&weak, "move app on home", |config| {
            config.move_to(&item.key(), slot(dock, page, col, row), is_installed)
        });
    });
    let weak = window.as_weak();
    home.on_merge_into(move |item, dock, page, col, row| {
        edit_home(&weak, "make folder", |config| {
            config.merge_into(&item.key(), slot(dock, page, col, row), is_installed)
        });
    });
    let weak = window.as_weak();
    home.on_move_to_index(move |item, index| {
        edit_home(&weak, "reorder home", |config| {
            config.move_to_index(&item.key(), to_index(index) as usize, is_installed)
        });
    });
    let weak = window.as_weak();
    home.on_disband_folder(move |id| {
        edit_home(&weak, "disband folder", |config| {
            config.disband_folder(to_index(id), is_installed)
        });
    });
    home.on_folder_shown(|id| OPEN_FOLDER.set((id >= 0).then_some(id as u32)));
    home.on_can_move(|item, dock, page, col, row| {
        SHOWN_CONFIG.with_borrow(|config| config.can_move(&item.key(), slot(dock, page, col, row)))
    });
    home.on_add_picture(|page, col, row| {
        let cell = (page >= 0 && col >= 0 && row >= 0).then(|| slot(false, page, col, row));
        PICTURE_SLOT.set(cell);
        log_error("open the image picker", images::pick());
    });
    let weak = window.as_weak();
    home.on_remove_picture(move |id| {
        let mut file = None;
        let result = config::update(|config| file = config.remove_image(to_index(id)));
        // Only once it's no longer in the saved config.
        if let (Ok(_), Some(file)) = (&result, file) {
            log_error("delete picture image", images::delete(&file));
        }
        let result = result.and_then(|_| weak.upgrade().map_or(Ok(()), |window| refresh(&window)));
        log_error("remove picture", result);
    });
    home.on_can_resize_picture(|id, columns, rows| {
        SHOWN_CONFIG.with_borrow(|config| {
            config.can_resize_image(to_index(id), to_index(columns), to_index(rows))
        })
    });
    let weak = window.as_weak();
    home.on_resize_picture(move |id, columns, rows| {
        edit_home(&weak, "resize picture", |config| {
            config.resize_image(to_index(id), to_index(columns), to_index(rows))
        });
    });
    let weak = window.as_weak();
    home.on_rename_folder(move |id, title| {
        edit_home(&weak, "rename folder", |config| {
            config.rename_folder(to_index(id), &title)
        });
    });
    let weak = window.as_weak();
    home.on_rows_fit(move |rows| {
        let rows = to_index(rows).clamp(
            *config::HOME_ROWS_RANGE.start(),
            *config::HOME_ROWS_RANGE.end(),
        );
        // Also reported when the layout settles at the same size: only save changes.
        if config::load().is_ok_and(|config| config.home_rows == rows) {
            return;
        }
        // Apps in rows that no longer fit move to the first free cells.
        edit_home(&weak, "save home rows", |config| {
            config.home_rows = rows;
            config.normalize();
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
    images::init(app.internal_data_path())?;

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

/// Called from `HomeActivity` (a background thread) once an image picked for
/// Home (`HomeActions.add-picture`) was copied into the images directory as
/// `file_name`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_HomeActivity_nativeOnImagePicked(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    file_name: jni::objects::JString,
) {
    let file: String = match env.get_string(&file_name) {
        Ok(name) => name.into(),
        Err(err) => {
            log::error!("failed to read the picked image name: {err}");
            return;
        }
    };
    slint::invoke_from_event_loop(move || {
        let Some(weak) = MAIN_WINDOW.with_borrow(|main| main.clone()) else {
            return;
        };
        let cell = PICTURE_SLOT.take();
        edit_home(&weak, "add picture", |config| config.add_image(&file, cell));
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
