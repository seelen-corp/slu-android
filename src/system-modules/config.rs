//! Persistent launcher configuration, stored as a small `key=value` text file
//! in the app's internal data directory.
//!
//! Home and Settings run in different processes (see the manifest), so there
//! is no shared in-memory state: every read goes to disk, and every write is a
//! read-modify-write of the latest file contents (`update`), so one process
//! never clobbers a change made by the other.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::Result;

const FILE_NAME: &str = "config.txt";

static PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LauncherScreens {
    /// A single screen listing every app.
    #[default]
    AppsMenu,
    /// A Home screen with the apps placed by the user, plus the Apps Menu.
    HomeAndAppsMenu,
}

impl LauncherScreens {
    fn as_str(self) -> &'static str {
        match self {
            Self::AppsMenu => "apps_menu",
            Self::HomeAndAppsMenu => "home_and_apps_menu",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "apps_menu" => Some(Self::AppsMenu),
            "home_and_apps_menu" => Some(Self::HomeAndAppsMenu),
            _ => None,
        }
    }
}

/// What a swipe down on Home does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwipeDownAction {
    /// The Apps Menu with its search focused.
    #[default]
    Search,
    /// The system notifications panel.
    Notifications,
}

impl SwipeDownAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Notifications => "notifications",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "search" => Some(Self::Search),
            "notifications" => Some(Self::Notifications),
            _ => None,
        }
    }
}

/// How apps are shown, on Home and in the Apps Menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppsView {
    /// Rows with icon and title.
    #[default]
    List,
    /// Rows with the title only.
    TextList,
    /// Icons with the title below.
    Grid,
}

impl AppsView {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::TextList => "text_list",
            Self::Grid => "grid",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "list" => Some(Self::List),
            "text_list" => Some(Self::TextList),
            "grid" => Some(Self::Grid),
            _ => None,
        }
    }
}

/// Shape of the app icons (see `icons.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IconShape {
    /// The device's own icon mask.
    #[default]
    System,
    Circle,
    Squircle,
    /// Round, with a less rounded bottom-right corner.
    Teardrop,
    /// Corners of `Config::icon_corner_radius` (0 = square).
    RoundedSquare,
}

impl IconShape {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Circle => "circle",
            Self::Squircle => "squircle",
            Self::Teardrop => "teardrop",
            Self::RoundedSquare => "rounded_square",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "circle" => Some(Self::Circle),
            "squircle" => Some(Self::Squircle),
            "teardrop" => Some(Self::Teardrop),
            "rounded_square" => Some(Self::RoundedSquare),
            _ => None,
        }
    }
}

/// Icons made of a single image (not adaptive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LegacyIcons {
    /// Wrapped in the icon shape, like adaptive icons.
    #[default]
    Wrap,
    /// Drawn as they are.
    Original,
}

impl LegacyIcons {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wrap => "wrap",
            Self::Original => "original",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "wrap" => Some(Self::Wrap),
            "original" => Some(Self::Original),
            _ => None,
        }
    }
}

/// Everything that decides how an icon is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconStyle {
    pub shape: IconShape,
    /// Percent of the icon side; only used by `IconShape::RoundedSquare`.
    pub corner_radius: u32,
    pub legacy: LegacyIcons,
}

/// Corner radius of `IconShape::RoundedSquare`, in percent of the icon side
/// (50 = circle).
pub const DEFAULT_ICON_CORNER_RADIUS: u32 = 25;
pub const ICON_CORNER_RADIUS_RANGE: std::ops::RangeInclusive<u32> = 0..=50;

/// A launcher entry, identified the same way as `apps::AppInfo` (a package can
/// expose several launcher activities).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRef {
    pub package_name: String,
    pub activity_name: String,
}

impl AppRef {
    pub fn new(package_name: &str, activity_name: &str) -> Self {
        Self {
            package_name: package_name.to_owned(),
            activity_name: activity_name.to_owned(),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let (package_name, activity_name) = value.trim().split_once('/')?;
        (!package_name.is_empty() && !activity_name.is_empty())
            .then(|| Self::new(package_name, activity_name))
    }

    fn serialize(&self) -> String {
        format!("{}/{}", self.package_name, self.activity_name)
    }
}

/// Where an app is placed on Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// A cell of a grid page. Ordered page by page, row by row.
    Grid { page: u32, row: u32, col: u32 },
    /// A slot of the dock (below the pages, the same on every page).
    Dock { col: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeEntry {
    pub app: AppRef,
    pub slot: Slot,
}

pub const DEFAULT_HOME_COLUMNS: u32 = 4;
pub const DEFAULT_HOME_ROWS: u32 = 5;
pub const HOME_COLUMNS_RANGE: std::ops::RangeInclusive<u32> = 3..=6;
/// Not a setting: the rows that fit on screen, reported by the UI.
pub const HOME_ROWS_RANGE: std::ops::RangeInclusive<u32> = 1..=30;
/// Padding around the apps (logical px), in every view.
pub const DEFAULT_APPS_PADDING: u32 = 8;
pub const APPS_PADDING_RANGE: std::ops::RangeInclusive<u32> = 0..=60;
/// Lines of the app titles, in every view (always reserved, see `AppsLayout`).
pub const DEFAULT_LABEL_LINES: u32 = 1;
pub const LABEL_LINES_RANGE: std::ops::RangeInclusive<u32> = 1..=2;

impl AppsView {
    /// Item size (logical px): icon size in the grid (the label adds its own
    /// space below it), or height of the list row. Keep in sync with
    /// `item-size-min/max` in `ui/pages/config.slint`.
    pub fn item_size_range(self) -> std::ops::RangeInclusive<u32> {
        match self {
            Self::Grid => 32..=96,
            Self::List => 40..=120,
            Self::TextList => 32..=96,
        }
    }

    pub fn default_item_size(self) -> u32 {
        match self {
            Self::Grid => 48,
            Self::List => 64,
            Self::TextList => 48,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub launcher_screens: LauncherScreens,
    pub apps_view: AppsView,
    pub swipe_down: SwipeDownAction,
    pub icon_shape: IconShape,
    /// See `IconStyle::corner_radius`.
    pub icon_corner_radius: u32,
    pub legacy_icons: LegacyIcons,
    /// Grid columns, on Home (cells per page, slots of the dock) and in the Apps
    /// Menu.
    pub home_columns: u32,
    /// Home grid rows: as many as fit on screen (portrait), reported by the UI.
    pub home_rows: u32,
    /// Item size of each view (see `AppsView::item_size_range`).
    pub item_size_grid: u32,
    pub item_size_list: u32,
    pub item_size_text_list: u32,
    /// Space around the apps, on Home and in the Apps Menu.
    pub apps_padding: u32,
    /// Lines of the app titles (see `LABEL_LINES_RANGE`).
    pub label_lines: u32,
    /// Apps on Home. Entries of apps that are not installed are kept (they come
    /// back if the app is reinstalled, or mid-update when it's briefly gone),
    /// until the user edits Home.
    pub home: Vec<HomeEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            launcher_screens: LauncherScreens::default(),
            apps_view: AppsView::default(),
            swipe_down: SwipeDownAction::default(),
            icon_shape: IconShape::default(),
            icon_corner_radius: DEFAULT_ICON_CORNER_RADIUS,
            legacy_icons: LegacyIcons::default(),
            home_columns: DEFAULT_HOME_COLUMNS,
            home_rows: DEFAULT_HOME_ROWS,
            item_size_grid: AppsView::Grid.default_item_size(),
            item_size_list: AppsView::List.default_item_size(),
            item_size_text_list: AppsView::TextList.default_item_size(),
            apps_padding: DEFAULT_APPS_PADDING,
            label_lines: DEFAULT_LABEL_LINES,
            home: Vec::new(),
        }
    }
}

impl Config {
    pub fn icon_style(&self) -> IconStyle {
        IconStyle {
            shape: self.icon_shape,
            // Ignored by the other shapes: keep it out of the icon cache key.
            corner_radius: match self.icon_shape {
                IconShape::RoundedSquare => self.icon_corner_radius,
                _ => 0,
            },
            legacy: self.legacy_icons,
        }
    }

    pub fn item_size(&self, view: AppsView) -> u32 {
        match view {
            AppsView::Grid => self.item_size_grid,
            AppsView::List => self.item_size_list,
            AppsView::TextList => self.item_size_text_list,
        }
    }

    /// Sets the item size of a view, clamped to its range.
    pub fn set_item_size(&mut self, view: AppsView, size: u32) {
        let range = view.item_size_range();
        let size = size.clamp(*range.start(), *range.end());
        match view {
            AppsView::Grid => self.item_size_grid = size,
            AppsView::List => self.item_size_list = size,
            AppsView::TextList => self.item_size_text_list = size,
        }
    }

    pub fn is_on_home(&self, app: &AppRef) -> bool {
        self.home.iter().any(|entry| entry.app == *app)
    }

    fn fits(&self, slot: Slot) -> bool {
        match slot {
            Slot::Grid { row, col, .. } => col < self.home_columns && row < self.home_rows,
            Slot::Dock { col } => col < self.home_columns,
        }
    }

    fn is_free(&self, slot: Slot) -> bool {
        !self.home.iter().any(|entry| entry.slot == slot)
    }

    /// The first free grid cell, page by page.
    fn first_free_cell(&self) -> Slot {
        (0..)
            .flat_map(|page| {
                (0..self.home_rows).flat_map(move |row| {
                    (0..self.home_columns).map(move |col| Slot::Grid { page, row, col })
                })
            })
            .find(|slot| self.is_free(*slot))
            .expect("there is always a free cell")
    }

    /// Puts an app in the first free grid cell (no-op if it's already on Home).
    pub fn add_to_home(&mut self, app: AppRef, installed: impl Fn(&AppRef) -> bool) {
        if !installed(&app) {
            return;
        }
        self.drop_uninstalled(&installed);
        if !self.is_on_home(&app) {
            let slot = self.first_free_cell();
            self.home.push(HomeEntry { app, slot });
        }
        self.compact_pages();
    }

    pub fn remove_from_home(&mut self, app: &AppRef, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        self.home.retain(|entry| entry.app != *app);
        self.compact_pages();
    }

    /// Moves an app on Home to `slot`; if another app is there, they swap.
    pub fn move_to(&mut self, app: &AppRef, slot: Slot, installed: impl Fn(&AppRef) -> bool) {
        if !self.fits(slot) {
            return;
        }
        self.drop_uninstalled(&installed);
        let Some(from) = self.home.iter().position(|entry| entry.app == *app) else {
            return;
        };
        let old_slot = self.home[from].slot;
        if let Some(other) = self.home.iter_mut().find(|entry| entry.slot == slot) {
            other.slot = old_slot;
        }
        self.home[from].slot = slot;
        self.compact_pages();
    }

    /// List views: moves an app to `index` of the Home list (`list_order`); the
    /// apps in between shift. The set of used slots doesn't change, only which
    /// app is in each one, so the grid layout keeps its shape.
    pub fn move_to_index(
        &mut self,
        app: &AppRef,
        index: usize,
        installed: impl Fn(&AppRef) -> bool,
    ) {
        self.drop_uninstalled(&installed);
        let order = self.list_order();
        let slots: Vec<Slot> = order.iter().map(|&i| self.home[i].slot).collect();
        let mut apps: Vec<AppRef> = order.iter().map(|&i| self.home[i].app.clone()).collect();
        let Some(from) = apps.iter().position(|a| a == app) else {
            return;
        };
        let moved = apps.remove(from);
        apps.insert(index.min(apps.len()), moved);
        self.home = apps
            .into_iter()
            .zip(slots)
            .map(|(app, slot)| HomeEntry { app, slot })
            .collect();
    }

    /// Indexes of `home` in list order: the grid page by page, then the dock.
    pub fn list_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.home.len()).collect();
        order.sort_by_key(|&i| self.home[i].slot);
        order
    }

    /// Number of grid pages (at least one).
    pub fn home_pages(&self) -> u32 {
        self.home
            .iter()
            .filter_map(|entry| match entry.slot {
                Slot::Grid { page, .. } => Some(page + 1),
                Slot::Dock { .. } => None,
            })
            .max()
            .unwrap_or(1)
    }

    /// Fixes a layout that doesn't fit the grid size (e.g. it was made smaller in
    /// Settings) or that is otherwise invalid: duplicated apps or slots, apps
    /// outside the grid. Those apps go to the first free cells. Returns whether
    /// anything changed.
    pub fn normalize(&mut self) -> bool {
        let before = self.home.clone();
        let mut kept: Vec<HomeEntry> = Vec::with_capacity(self.home.len());
        let mut misplaced: Vec<AppRef> = Vec::new();
        for entry in std::mem::take(&mut self.home) {
            if kept.iter().any(|k| k.app == entry.app) {
                continue;
            }
            if self.fits(entry.slot) && !kept.iter().any(|k| k.slot == entry.slot) {
                kept.push(entry);
            } else {
                misplaced.push(entry.app);
            }
        }
        self.home = kept;
        for app in misplaced {
            let slot = self.first_free_cell();
            self.home.push(HomeEntry { app, slot });
        }
        self.compact_pages();
        self.home != before
    }

    fn drop_uninstalled(&mut self, installed: impl Fn(&AppRef) -> bool) {
        self.home.retain(|entry| installed(&entry.app));
    }

    /// Removes empty pages between used ones (renumbers the pages after them).
    fn compact_pages(&mut self) {
        let mut pages: Vec<u32> = self
            .home
            .iter()
            .filter_map(|entry| match entry.slot {
                Slot::Grid { page, .. } => Some(page),
                Slot::Dock { .. } => None,
            })
            .collect();
        pages.sort_unstable();
        pages.dedup();
        for entry in &mut self.home {
            if let Slot::Grid { page, .. } = &mut entry.slot {
                *page = pages.iter().position(|p| p == page).unwrap_or(0) as u32;
            }
        }
    }

    fn parse(text: &str) -> Self {
        let mut config = Config::default();
        // `pinned=` (older versions): apps in Home order, without a cell.
        let mut legacy_pinned: Vec<AppRef> = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "launcher_screens" => {
                    if let Some(screens) = LauncherScreens::parse(value) {
                        config.launcher_screens = screens;
                    }
                }
                "swipe_down" => {
                    if let Some(action) = SwipeDownAction::parse(value) {
                        config.swipe_down = action;
                    }
                }
                "icon_shape" => {
                    if let Some(shape) = IconShape::parse(value) {
                        config.icon_shape = shape;
                    }
                }
                "icon_corner_radius" => {
                    if let Some(n) = parse_in(value, &ICON_CORNER_RADIUS_RANGE) {
                        config.icon_corner_radius = n;
                    }
                }
                "legacy_icons" => {
                    if let Some(mode) = LegacyIcons::parse(value) {
                        config.legacy_icons = mode;
                    }
                }
                "apps_view" => {
                    if let Some(view) = AppsView::parse(value) {
                        config.apps_view = view;
                    }
                }
                "home_columns" => {
                    if let Some(n) = value
                        .parse()
                        .ok()
                        .filter(|n| HOME_COLUMNS_RANGE.contains(n))
                    {
                        config.home_columns = n;
                    }
                }
                "home_rows" => {
                    if let Some(n) = value.parse().ok().filter(|n| HOME_ROWS_RANGE.contains(n)) {
                        config.home_rows = n;
                    }
                }
                "item_size_grid" => {
                    if let Some(n) = parse_in(value, &AppsView::Grid.item_size_range()) {
                        config.item_size_grid = n;
                    }
                }
                "item_size_list" => {
                    if let Some(n) = parse_in(value, &AppsView::List.item_size_range()) {
                        config.item_size_list = n;
                    }
                }
                "item_size_text_list" => {
                    if let Some(n) = parse_in(value, &AppsView::TextList.item_size_range()) {
                        config.item_size_text_list = n;
                    }
                }
                "apps_padding" => {
                    if let Some(n) = parse_in(value, &APPS_PADDING_RANGE) {
                        config.apps_padding = n;
                    }
                }
                "label_lines" => {
                    if let Some(n) = parse_in(value, &LABEL_LINES_RANGE) {
                        config.label_lines = n;
                    }
                }
                "home" => {
                    if let Some(entry) = parse_home_entry(value) {
                        config.home.push(entry);
                    }
                }
                "pinned" => {
                    if let Some(app) = AppRef::parse(value) {
                        legacy_pinned.push(app);
                    }
                }
                _ => {}
            }
        }
        for app in legacy_pinned {
            if !config.is_on_home(&app) {
                let slot = config.first_free_cell();
                config.home.push(HomeEntry { app, slot });
            }
        }
        config
    }

    fn serialize(&self) -> String {
        let mut text = format!(
            "launcher_screens={}\napps_view={}\nswipe_down={}\nicon_shape={}\nicon_corner_radius={}\nlegacy_icons={}\nhome_columns={}\nhome_rows={}\nitem_size_grid={}\nitem_size_list={}\nitem_size_text_list={}\napps_padding={}\nlabel_lines={}\n",
            self.launcher_screens.as_str(),
            self.apps_view.as_str(),
            self.swipe_down.as_str(),
            self.icon_shape.as_str(),
            self.icon_corner_radius,
            self.legacy_icons.as_str(),
            self.home_columns,
            self.home_rows,
            self.item_size_grid,
            self.item_size_list,
            self.item_size_text_list,
            self.apps_padding,
            self.label_lines,
        );
        for entry in &self.home {
            let slot = match entry.slot {
                Slot::Grid { page, row, col } => format!("grid,{page},{row},{col}"),
                Slot::Dock { col } => format!("dock,{col}"),
            };
            text.push_str(&format!("home={slot},{}\n", entry.app.serialize()));
        }
        text
    }
}

/// A number inside `range` (anything else is ignored: the default stays).
fn parse_in(value: &str, range: &std::ops::RangeInclusive<u32>) -> Option<u32> {
    value.parse().ok().filter(|n| range.contains(n))
}

/// `grid,<page>,<row>,<col>,<package>/<activity>` or `dock,<col>,<package>/<activity>`.
fn parse_home_entry(value: &str) -> Option<HomeEntry> {
    let (kind, rest) = value.split_once(',')?;
    let (slot, app) = match kind {
        "grid" => {
            let mut parts = rest.splitn(4, ',');
            let page = parts.next()?.parse().ok()?;
            let row = parts.next()?.parse().ok()?;
            let col = parts.next()?.parse().ok()?;
            (Slot::Grid { page, row, col }, parts.next()?)
        }
        "dock" => {
            let (col, app) = rest.split_once(',')?;
            (
                Slot::Dock {
                    col: col.parse().ok()?,
                },
                app,
            )
        }
        _ => return None,
    };
    Some(HomeEntry {
        app: AppRef::parse(app)?,
        slot,
    })
}

/// Must be called once from `android_main` with the app's internal data
/// directory (`AndroidApp::internal_data_path`).
pub fn init(data_dir: Option<PathBuf>) -> Result<()> {
    let dir = data_dir.ok_or("the app has no internal data path")?;
    std::fs::create_dir_all(&dir)?;
    PATH.set(dir.join(FILE_NAME)).ok();
    Ok(())
}

fn path() -> Result<&'static PathBuf> {
    PATH.get()
        .ok_or_else(|| "config::init must be called before use".into())
}

/// Reads the current configuration. A missing file means defaults.
pub fn load() -> Result<Config> {
    match std::fs::read_to_string(path()?) {
        Ok(text) => Ok(Config::parse(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(err) => Err(err.into()),
    }
}

/// Applies `change` to the latest configuration on disk and saves it
/// atomically (write to a temp file, then rename). Returns the saved config.
pub fn update(change: impl FnOnce(&mut Config)) -> Result<Config> {
    let mut config = load()?;
    change(&mut config);
    save(&config)?;
    Ok(config)
}

fn save(config: &Config) -> Result<()> {
    let path = path()?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, config.serialize())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(n: u32) -> AppRef {
        AppRef::new(&format!("p{n}"), &format!("p{n}.Main"))
    }

    fn all(_: &AppRef) -> bool {
        true
    }

    #[test]
    fn roundtrip() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.home.push(HomeEntry {
            app: app(2),
            slot: Slot::Dock { col: 2 },
        });
        config.home_rows = 9;
        config.swipe_down = SwipeDownAction::Notifications;
        config.icon_shape = IconShape::Teardrop;
        config.icon_corner_radius = 0;
        config.legacy_icons = LegacyIcons::Original;
        config.item_size_grid = 96;
        config.item_size_list = 40;
        config.item_size_text_list = 96;
        config.apps_padding = 60;
        config.label_lines = 2;
        assert_eq!(Config::parse(&config.serialize()), config);
    }

    #[test]
    fn out_of_range_keeps_default() {
        let config = Config::parse(
            "item_size_grid=10\nitem_size_list=999\napps_padding=-1\nhome_rows=0\nlabel_lines=3\n",
        );
        assert_eq!(config, Config::default());
    }

    #[test]
    fn set_item_size_clamps() {
        let mut config = Config::default();
        config.set_item_size(AppsView::Grid, 1000);
        assert_eq!(config.item_size(AppsView::Grid), 96);
        config.set_item_size(AppsView::TextList, 0);
        assert_eq!(config.item_size(AppsView::TextList), 32);
    }

    #[test]
    fn legacy_pinned_fill_cells() {
        let config = Config::parse("pinned=p1/p1.Main\npinned=p2/p2.Main\n");
        assert_eq!(
            config.home[0].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 0
            }
        );
        assert_eq!(
            config.home[1].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 1
            }
        );
    }

    #[test]
    fn move_swaps_and_compacts() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.add_to_home(app(2), all);
        config.move_to(
            &app(1),
            Slot::Grid {
                page: 0,
                row: 0,
                col: 1,
            },
            all,
        );
        assert_eq!(
            config.home[0].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 1
            }
        );
        assert_eq!(
            config.home[1].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 0
            }
        );
        // Alone on a new page 1 after page 0 is left empty: it becomes page 0.
        config.move_to(&app(1), Slot::Dock { col: 0 }, all);
        config.move_to(
            &app(2),
            Slot::Grid {
                page: 1,
                row: 2,
                col: 2,
            },
            all,
        );
        assert_eq!(
            config.home[1].slot,
            Slot::Grid {
                page: 0,
                row: 2,
                col: 2
            }
        );
        assert_eq!(config.home_pages(), 1);
    }

    #[test]
    fn move_to_index_keeps_slots() {
        let mut config = Config::default();
        for n in 1..=3 {
            config.add_to_home(app(n), all);
        }
        config.move_to_index(&app(3), 0, all);
        let order: Vec<_> = config
            .list_order()
            .into_iter()
            .map(|i| config.home[i].app.clone())
            .collect();
        assert_eq!(order, vec![app(3), app(1), app(2)]);
        assert_eq!(
            config.home[0].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 0
            }
        );
    }

    #[test]
    fn normalize_moves_out_of_grid() {
        let mut config = Config::default();
        config.home.push(HomeEntry {
            app: app(1),
            slot: Slot::Grid {
                page: 0,
                row: 6,
                col: 5,
            },
        });
        config.home.push(HomeEntry {
            app: app(2),
            slot: Slot::Grid {
                page: 0,
                row: 0,
                col: 0,
            },
        });
        assert!(config.normalize());
        assert_eq!(
            config.home[1].slot,
            Slot::Grid {
                page: 0,
                row: 0,
                col: 1
            }
        );
        assert!(!config.normalize());
    }
}
