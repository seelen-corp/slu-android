mod error;
#[path = "system-modules/mod.rs"]
mod system_modules;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use error::Result;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use system_modules::config::{AppRef, EntryKey, HomeContent, Slot};
use system_modules::{
    activity, android_ctx, apps, badges, config, icons, images, locale, system, theme, widget_gpu,
    widgets,
};

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

impl From<config::BadgePosition> for BadgePosition {
    fn from(value: config::BadgePosition) -> Self {
        match value {
            config::BadgePosition::Off => Self::Off,
            config::BadgePosition::TopLeft => Self::TopLeft,
            config::BadgePosition::TopRight => Self::TopRight,
            config::BadgePosition::BottomLeft => Self::BottomLeft,
            config::BadgePosition::BottomRight => Self::BottomRight,
        }
    }
}

impl From<BadgePosition> for config::BadgePosition {
    fn from(value: BadgePosition) -> Self {
        match value {
            BadgePosition::Off => Self::Off,
            BadgePosition::TopLeft => Self::TopLeft,
            BadgePosition::TopRight => Self::TopRight,
            BadgePosition::BottomLeft => Self::BottomLeft,
            BadgePosition::BottomRight => Self::BottomRight,
        }
    }
}

impl AppItem {
    fn app_ref(&self) -> AppRef {
        AppRef::new(&self.package_name, &self.activity_name)
    }

    /// Sets its notification count (`badges`). Returns whether it changed.
    fn update_badge(&mut self) -> bool {
        let count = badges::count(&self.package_name) as i32;
        let changed = self.notifications != count;
        self.notifications = count;
        changed
    }
}

/// Notifications of all the apps of a folder (its badge).
fn folder_notifications(apps: &ModelRc<AppItem>) -> i32 {
    apps.iter().map(|app| app.notifications).sum()
}

impl HomeItem {
    fn key(&self) -> EntryKey {
        if self.is_folder {
            EntryKey::Folder(to_index(self.folder.id))
        } else if self.is_picture {
            self.picture.key()
        } else {
            EntryKey::App(self.app.app_ref())
        }
    }
}

impl PictureItem {
    fn key(&self) -> EntryKey {
        if self.widget {
            EntryKey::Widget(to_index(self.id))
        } else {
            EntryKey::Image(to_index(self.id))
        }
    }
}

/// Logical px (`length` callback arguments) to physical px of the main window.
fn to_physical(window: &MainWindow, length: f32) -> f32 {
    length * window.window().scale_factor()
}

/// A new frame of a widget (`widgets::take_frame` or `widget_gpu::take`):
/// shown on Home in place.
fn apply_widget_frame(window: &MainWindow, id: u32) {
    let frame = match widgets::take_frame(id) {
        Some(buffer) => slint::Image::from_rgba8_premultiplied(buffer),
        None => match widget_gpu::take(id) {
            Some(frame) => frame,
            None => return,
        },
    };
    set_widget_frame(window, id, frame);
}

/// Shows `frame` as the image of widget `id` on Home.
fn set_widget_frame(window: &MainWindow, id: u32, frame: slint::Image) {
    WIDGET_FRAMES.with_borrow_mut(|frames| frames.insert(id, frame.clone()));
    let pictures = window.get_home_pictures();
    for i in 0..pictures.row_count() {
        if let Some(mut item) = pictures.row_data(i)
            && item.picture.widget
            && item.picture.id == id as i32
        {
            item.picture.source = frame;
            pictures.set_row_data(i, item);
            return;
        }
    }
}

/// Loads the widget providers (on another thread: rendering their previews is
/// slow) into the widget picker.
fn load_widget_providers(window: &MainWindow) {
    window.set_widget_providers(ModelRc::default());
    window.set_widget_providers_loading(true);
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let providers = widgets::providers().unwrap_or_else(|err| {
            log::error!("failed to load the widget providers: {err}");
            Vec::new()
        });
        weak.upgrade_in_event_loop(move |window| show_widget_providers(&window, providers))
            .ok();
    });
}

fn show_widget_providers(window: &MainWindow, mut providers: Vec<widgets::WidgetProvider>) {
    let locale = window.global::<I18n>().get_locale();
    let (cell, gap, inset) = GRID_METRICS.get();
    let (columns, rows) =
        SHOWN_CONFIG.with_borrow(|config| (config.home_columns, config.home_rows));
    providers.sort_by_cached_key(|provider| {
        (
            fold(&provider.app_label),
            provider.app_label.clone(),
            fold(&provider.label),
        )
    });
    let mut previous_app: Option<String> = None;
    let items: Vec<WidgetProviderItem> = providers
        .into_iter()
        .map(|provider| {
            let (span_columns, span_rows) = provider.default_span(cell, gap, inset);
            let (span_columns, span_rows) = (
                span_columns.clamp(1, columns.max(1)),
                span_rows.clamp(1, rows.max(1)),
            );
            let first_of_app = previous_app.as_deref() != Some(provider.app_label.as_str());
            previous_app = Some(provider.app_label.clone());
            WidgetProviderItem {
                size: rust_i18n::t!(
                    "widgets.size",
                    locale = locale.as_str(),
                    columns = span_columns,
                    rows = span_rows
                )
                .as_ref()
                .into(),
                component: provider.component.into(),
                label: provider.label.into(),
                app_label: provider.app_label.into(),
                first_of_app,
                preview: provider.preview.map_or_else(
                    slint::Image::default,
                    slint::Image::from_rgba8_premultiplied,
                ),
                columns: span_columns as i32,
                rows: span_rows as i32,
            }
        })
        .collect();
    window.set_widget_providers(ModelRc::new(VecModel::from(items)));
    window.set_widget_providers_loading(false);
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

/// A widget being added: the Home page it goes on, its grid cell (if any) and
/// its size in cells.
type PendingWidget = (u32, Option<Slot>, (u32, u32));

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
    /// Page and grid cell where the image being picked goes
    /// (`HomeActions.add-picture`).
    static PICTURE_SLOT: Cell<(u32, Option<Slot>)> = const { Cell::new((0, None)) };
    /// Page and grid cell where the widget picker was opened
    /// (`HomeActions.open-widget-picker`).
    static WIDGET_PICKER_SLOT: Cell<(u32, Option<Slot>)> = const { Cell::new((0, None)) };
    /// The widget being added (`HomeActions.add-widget`, until the system
    /// reports it bound): where it goes, and its cells.
    static PENDING_WIDGET: Cell<Option<PendingWidget>> = const { Cell::new(None) };
    /// Latest frame of each widget on Home (`widgets::take_frame`).
    static WIDGET_FRAMES: RefCell<HashMap<u32, slint::Image>> = Default::default();
    /// Grid cell size (width, height), space between rows and inset of what's
    /// drawn in a cell, logical px (`HomeActions.grid-metrics`).
    static GRID_METRICS: Cell<((f32, f32), f32, f32)> = const { Cell::new(((80.0, 100.0), 0.0, 4.0)) };
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
            notifications: badges::count(&app.package_name) as i32,
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
                let apps = ModelRc::new(VecModel::from(apps));
                item.folder = FolderItem {
                    id: folder.id as i32,
                    title: folder.title.as_str().into(),
                    notifications: folder_notifications(&apps),
                    apps,
                    large: folder.large,
                };
            }
            HomeContent::Image(image) => {
                item.is_picture = true;
                item.picture = PictureItem {
                    id: image.id as i32,
                    widget: false,
                    source: picture_image(&image.file),
                    columns: image.columns as i32,
                    rows: image.rows as i32,
                };
            }
            // Shown like a picture, with its latest frame.
            HomeContent::Widget(widget) => {
                item.is_picture = true;
                item.picture = PictureItem {
                    id: widget.id as i32,
                    widget: true,
                    source: WIDGET_FRAMES
                        .with_borrow(|frames| frames.get(&widget.id).cloned())
                        .unwrap_or_default(),
                    columns: widget.columns as i32,
                    rows: widget.rows as i32,
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
    // 0 free, 1 app or folder, 2 picture, 3 + 2 * dy + dx a cell of a large
    // folder, `dx`/`dy` cells from its top-left one (`HomeGrid.home-cells`).
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
        for cell in entry.cells() {
            if let Slot::Grid { page, row, col } = cell {
                let kind = if item.is_picture {
                    2
                } else if item.is_folder && item.folder.large {
                    3 + 2 * (row as i32 - item.row) + (col as i32 - item.col)
                } else {
                    1
                };
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
    // Frames of widgets no longer on Home.
    let widget_ids = config.widget_ids();
    WIDGET_FRAMES.with_borrow_mut(|frames| frames.retain(|id, _| widget_ids.contains(id)));
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
    // Twice the icon size: large folders (2x2 cells) draw it about that big.
    layout.set_icon_shape(
        icon_size.map_or_else(slint::Image::default, |size| icon_shape(size * 2, style)),
    );
    layout.set_item_size(config.item_size(config.apps_view) as f32);
    layout.set_padding(config.apps_padding as f32);
    layout.set_label_lines(config.label_lines as i32);
    layout.set_badge_position(config.badge_position.into());

    ALL_APPS.with_borrow_mut(|apps| *apps = items);
    SHOWN_CONFIG.with_borrow_mut(|shown| *shown = config);
    apply_search(window);
    Ok(())
}

/// The notification counts changed (`badges`): updates the entries of Home in
/// place (the rest of the UI, e.g. a drag in progress, is left as it is).
fn apply_badges(window: &MainWindow) {
    fn update_apps(apps: &ModelRc<AppItem>) {
        for i in 0..apps.row_count() {
            if let Some(mut app) = apps.row_data(i)
                && app.update_badge()
            {
                apps.set_row_data(i, app);
            }
        }
    }
    fn update_items(items: ModelRc<HomeItem>) {
        for i in 0..items.row_count() {
            let Some(mut item) = items.row_data(i) else {
                continue;
            };
            let changed = if item.is_folder {
                // Shared with the open folder view, if it's this one.
                update_apps(&item.folder.apps);
                let count = folder_notifications(&item.folder.apps);
                let changed = item.folder.notifications != count;
                item.folder.notifications = count;
                changed
            } else {
                !item.is_picture && item.app.update_badge()
            };
            if changed {
                items.set_row_data(i, item);
            }
        }
    }
    update_items(window.get_home_items());
    update_items(window.get_dock_items());
    update_items(window.get_home_list());
    // Usually the same model as its entry's, already updated. The folder view
    // doesn't show the folder's own badge: it's left as it is (a title being
    // edited too).
    update_apps(&window.get_open_folder().apps);
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
    window.set_badge_position(config.badge_position.into());
}

/// Whether the launcher may read notifications, in the Settings window (it's
/// changed in the system settings: read again on resume).
fn show_notification_access(window: &SettingsWindow) {
    let granted = badges::access_granted().unwrap_or_else(|err| {
        log::error!("failed to read the notification access: {err}");
        false
    });
    window.set_notification_access(granted);
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

fn run_settings(locale: &str, on_resume: &ResumeHook) -> Result<()> {
    let window = SettingsWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    apply_accent(window.global::<Tokens>());
    show_settings(&window, &config::load()?);
    show_notification_access(&window);
    let weak = window.as_weak();
    *on_resume.borrow_mut() = Some(Box::new(move || {
        weak.upgrade_in_event_loop(|window| show_notification_access(&window))
            .ok();
    }));

    let weak = window.as_weak();
    window.on_badge_position_changed(move |value| {
        save_setting(&weak, "save badge position", |config| {
            config.badge_position = value.into()
        });
    });
    window.on_open_notification_access(|| {
        log_error(
            "open notification access settings",
            system::open_notification_access_settings(),
        )
    });

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
    window.on_open_url(|url| log_error("open link", system::open_url(&url)));
    window.run()?;
    Ok(())
}

fn run_home(locale: &str, on_resume: &ResumeHook) -> Result<()> {
    let window = MainWindow::new()?;
    setup_i18n(window.global::<I18n>(), locale);
    refresh(&window)?;
    // Back on screen (after opening an app, Settings, the wallpaper picker...):
    // Settings runs in another process, so reload the config, and start again
    // from Home.
    let weak = window.as_weak();
    *on_resume.borrow_mut() = Some(Box::new(move || {
        weak.upgrade_in_event_loop(|window| {
            log_error("refresh on resume", refresh(&window));
            window.invoke_show_home();
        })
        .ok();
    }));
    MAIN_WINDOW.with_borrow_mut(|main| *main = Some(window.as_weak()));

    // Reported by `NotificationBadgeService` (Java thread).
    let weak = window.as_weak();
    badges::subscribe(move || {
        weak.upgrade_in_event_loop(|window| apply_badges(&window))
            .ok();
    });

    // Widgets (`WidgetHost`, Android UI thread). Ids left by an add that never
    // finished, or by entries dropped from the config, are deleted.
    log_error(
        "prune widgets",
        widgets::prune(&SHOWN_CONFIG.with_borrow(|config| config.widget_ids())),
    );
    let weak = window.as_weak();
    widgets::subscribe_frames(move |id| {
        weak.upgrade_in_event_loop(move |window| apply_widget_frame(&window, id))
            .ok();
    });
    // GPU frames of the widgets live in Slint's OpenGL context (`widget_gpu`).
    let weak = window.as_weak();
    let notifier = window
        .window()
        .set_rendering_notifier(move |state, _| match state {
            slint::RenderingState::RenderingSetup => {
                widget_gpu::setup();
                log_error("redraw widgets", widgets::redraw());
            }
            slint::RenderingState::BeforeRendering => widget_gpu::before_rendering(),
            slint::RenderingState::AfterRendering => widget_gpu::after_rendering(),
            slint::RenderingState::RenderingTeardown => {
                widget_gpu::teardown();
                // Their textures are gone: no frame until they're drawn again.
                weak.upgrade_in_event_loop(|window| {
                    let ids: Vec<u32> =
                        WIDGET_FRAMES.with_borrow(|frames| frames.keys().copied().collect());
                    for id in ids {
                        set_widget_frame(&window, id, slint::Image::default());
                    }
                    WIDGET_FRAMES.with_borrow_mut(|frames| frames.clear());
                    // Frames shown before this ran were just dropped too.
                    log_error("redraw widgets", widgets::redraw());
                })
                .ok();
            }
            _ => {}
        });
    if let Err(err) = notifier {
        log::error!("widget GPU frames unavailable: {err}");
        log_error("use widget bitmaps", widgets::use_bitmaps());
    }
    let weak = window.as_weak();
    widgets::subscribe_added(move |id, provider| {
        let weak = weak.clone();
        slint::invoke_from_event_loop(move || {
            let (page, cell, span) = PENDING_WIDGET.take().unwrap_or((0, None, (1, 1)));
            edit_home(&weak, "add widget", |config| {
                config.add_widget(id, &provider, span, page, cell)
            });
        })
        .ok();
    });

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
        PICTURE_SLOT.set((to_index(page), cell));
        log_error("open the image picker", images::pick());
    });
    let weak = window.as_weak();
    home.on_remove_picture(move |picture| {
        let id = to_index(picture.id);
        let mut file = None;
        let mut widget = false;
        let result = config::update(|config| {
            if picture.widget {
                widget = config.remove_widget(id);
            } else {
                file = config.remove_image(id);
            }
        });
        // Only once it's no longer in the saved config.
        if let (Ok(_), Some(file)) = (&result, file) {
            log_error("delete picture image", images::delete(&file));
        }
        if result.is_ok() && widget {
            log_error("delete widget", widgets::remove(id));
        }
        let result = result.and_then(|_| weak.upgrade().map_or(Ok(()), |window| refresh(&window)));
        log_error("remove picture", result);
    });
    home.on_can_resize_picture(|picture, columns, rows| {
        SHOWN_CONFIG.with_borrow(|config| {
            config.can_resize(&picture.key(), to_index(columns), to_index(rows))
        })
    });
    let weak = window.as_weak();
    home.on_resize_picture(move |picture, columns, rows| {
        edit_home(&weak, "resize picture", |config| {
            config.resize(&picture.key(), to_index(columns), to_index(rows))
        });
    });

    home.on_grid_metrics(|cell_width, cell_height, row_gap, inset| {
        GRID_METRICS.set(((cell_width, cell_height), row_gap, inset));
    });
    let weak = window.as_weak();
    home.on_open_widget_picker(move |page, col, row| {
        let cell = (page >= 0 && col >= 0 && row >= 0).then(|| slot(false, page, col, row));
        WIDGET_PICKER_SLOT.set((to_index(page), cell));
        if let Some(window) = weak.upgrade() {
            window.set_widget_picker_open(true);
            load_widget_providers(&window);
        }
    });
    home.on_add_widget(|provider| {
        let (page, cell) = WIDGET_PICKER_SLOT.get();
        PENDING_WIDGET.set(Some((
            page,
            cell,
            (to_index(provider.columns), to_index(provider.rows)),
        )));
        log_error("add widget", widgets::add(&provider.component));
    });
    let weak = window.as_weak();
    home.on_widget_size(move |id, width, height| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let width = to_physical(&window, width).round() as i32;
        let height = to_physical(&window, height).round() as i32;
        log_error("lay out widget", widgets::show(to_index(id), width, height));
    });
    let weak = window.as_weak();
    home.on_widget_touch(move |id, action, x, y| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        log_error(
            "forward widget touch",
            widgets::touch(
                to_index(id),
                action,
                to_physical(&window, x),
                to_physical(&window, y),
            ),
        );
    });
    home.on_can_resize_folder(|id, large| {
        SHOWN_CONFIG.with_borrow(|config| config.can_resize_folder(to_index(id), large))
    });
    let weak = window.as_weak();
    home.on_resize_folder(move |id, large| {
        edit_home(&weak, "resize folder", |config| {
            config.resize_folder(to_index(id), large)
        });
    });
    let weak = window.as_weak();
    home.on_move_in_folder(move |id, app, index| {
        edit_home(&weak, "reorder folder", |config| {
            config.move_in_folder(
                to_index(id),
                &app.app_ref(),
                to_index(index) as usize,
                is_installed,
            )
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

/// What the window does when the activity is back on screen, set once it's
/// created. Run by the lifecycle listener, which is installed before any
/// window exists.
type ResumeHook = std::rc::Rc<RefCell<Option<Box<dyn Fn()>>>>;

fn run(app: slint::android::AndroidApp) -> Result<()> {
    use slint::android::android_activity::{MainEvent, PollEvent};

    android_ctx::init(&app)?;
    apps::init()?;
    config::init(app.internal_data_path())?;
    images::init(app.internal_data_path())?;

    let on_resume = ResumeHook::default();
    let listener_hook = on_resume.clone();
    slint::android::init_with_event_listener(app, move |event| {
        if let PollEvent::Main(MainEvent::Resume { .. }) = event
            && let Some(hook) = listener_hook.borrow().as_ref()
        {
            hook();
        }
    })?;

    let locale = current_locale();
    if activity::current_class_name()? == "SettingsActivity" {
        run_settings(&locale, &on_resume)
    } else {
        run_home(&locale, &on_resume)
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
        let (page, cell) = PICTURE_SLOT.take();
        edit_home(&weak, "add picture", |config| config.add_image(&file, page, cell));
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
