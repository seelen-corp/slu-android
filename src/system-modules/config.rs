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

/// Where the notification badge of an app is drawn on Home: a corner of its
/// icon. The text list (no icons) shows it left of the title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgePosition {
    /// No badges.
    Off,
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

impl BadgePosition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::TopLeft => "top_left",
            Self::TopRight => "top_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomRight => "bottom_right",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "top_left" => Some(Self::TopLeft),
            "top_right" => Some(Self::TopRight),
            "bottom_left" => Some(Self::BottomLeft),
            "bottom_right" => Some(Self::BottomRight),
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

/// Where an app (or folder) is placed on Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// A cell of a grid page. Ordered page by page, row by row.
    Grid { page: u32, row: u32, col: u32 },
    /// A slot of the dock (below the pages, the same on every page).
    Dock { col: u32 },
}

impl Slot {
    fn serialize(&self) -> String {
        match *self {
            Slot::Grid { page, row, col } => format!("grid,{page},{row},{col}"),
            Slot::Dock { col } => format!("dock,{col}"),
        }
    }
}

/// Several apps in one slot of Home, opened as their own view. Made by dropping
/// an app on another; a folder left with a single app becomes that app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Unique among the folders on Home.
    pub id: u32,
    /// Set by the user; empty shows a default title.
    pub title: String,
    pub apps: Vec<AppRef>,
    /// Enlarged: it covers 2x2 cells of the grid (from its slot, the top-left
    /// cell) instead of one. Never in the dock.
    pub large: bool,
}

/// Cells covered by a large folder (`Folder::large`).
pub const LARGE_FOLDER_SPAN: (u32, u32) = (2, 2);

/// An image picked by the user, on the Home grid only (never in the dock or the
/// list views). It covers `columns` x `rows` cells from its slot (the top-left
/// cell) and is drawn covering all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeImage {
    /// Unique among the images on Home.
    pub id: u32,
    /// File name inside the images directory (`images::path`).
    pub file: String,
    pub columns: u32,
    pub rows: u32,
}

/// An Android app widget (`widgets.rs`), on the Home grid only, like an image:
/// it covers `columns` x `rows` cells from its slot (the top-left cell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeWidget {
    /// The id the system gave it (`AppWidgetHost.allocateAppWidgetId`), unique.
    pub id: u32,
    /// Flattened component name of its provider (`package/class`).
    pub provider: String,
    pub columns: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeContent {
    App(AppRef),
    Folder(Folder),
    Image(HomeImage),
    Widget(HomeWidget),
}

/// Identifies an entry of Home (e.g. the one being dragged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKey {
    App(AppRef),
    Folder(u32),
    Image(u32),
    Widget(u32),
}

impl HomeContent {
    pub fn key(&self) -> EntryKey {
        match self {
            Self::App(app) => EntryKey::App(app.clone()),
            Self::Folder(folder) => EntryKey::Folder(folder.id),
            Self::Image(image) => EntryKey::Image(image.id),
            Self::Widget(widget) => EntryKey::Widget(widget.id),
        }
    }

    /// The app itself, or a folder with it.
    fn contains(&self, app: &AppRef) -> bool {
        match self {
            Self::App(a) => a == app,
            Self::Folder(folder) => folder.apps.contains(app),
            Self::Image(_) | Self::Widget(_) => false,
        }
    }

    /// Cells covered (columns, rows): only images, widgets and large folders
    /// cover more than one.
    pub fn span(&self) -> (u32, u32) {
        match self {
            Self::Image(image) => (image.columns, image.rows),
            Self::Widget(widget) => (widget.columns, widget.rows),
            Self::Folder(folder) if folder.large => LARGE_FOLDER_SPAN,
            _ => (1, 1),
        }
    }

    /// An image or a widget: only shown on the grid (not in the dock or the
    /// list views), and resizable.
    pub fn is_grid_only(&self) -> bool {
        matches!(self, Self::Image(_) | Self::Widget(_))
    }

    /// An image, a widget or a large folder: it never makes room for the others
    /// (see `move_to`), and it's never in the dock.
    fn keeps_its_cells(&self) -> bool {
        self.is_grid_only() || self.span() != (1, 1)
    }

    /// Makes an image or a widget cover `columns` x `rows` cells. False (and
    /// nothing changes) for anything else.
    fn set_span(&mut self, columns: u32, rows: u32) -> bool {
        match self {
            Self::Image(image) => (image.columns, image.rows) = (columns, rows),
            Self::Widget(widget) => (widget.columns, widget.rows) = (columns, rows),
            _ => return false,
        }
        true
    }
}

/// The area of `span` cells from `slot` (its top-left cell) and the one of
/// `other_span` from `other` share a cell. Dock slots are a single cell.
fn areas_overlap(slot: Slot, span: (u32, u32), other: Slot, other_span: (u32, u32)) -> bool {
    match (slot, other) {
        (
            Slot::Grid { page, row, col },
            Slot::Grid {
                page: other_page,
                row: other_row,
                col: other_col,
            },
        ) => {
            page == other_page
                && col < other_col + other_span.0
                && other_col < col + span.0
                && row < other_row + other_span.1
                && other_row < row + span.1
        }
        (Slot::Dock { col }, Slot::Dock { col: other_col }) => col == other_col,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeEntry {
    pub content: HomeContent,
    pub slot: Slot,
}

impl HomeEntry {
    fn app(app: AppRef, slot: Slot) -> Self {
        Self {
            content: HomeContent::App(app),
            slot,
        }
    }

    /// It covers a cell of the area of `span` cells from `slot`.
    fn overlaps(&self, slot: Slot, span: (u32, u32)) -> bool {
        areas_overlap(self.slot, self.content.span(), slot, span)
    }

    /// It covers `cell`.
    fn covers(&self, cell: Slot) -> bool {
        self.overlaps(cell, (1, 1))
    }

    /// Every cell it covers.
    pub fn cells(&self) -> Vec<Slot> {
        match self.slot {
            Slot::Grid { page, row, col } => {
                let (columns, rows) = self.content.span();
                (row..row + rows)
                    .flat_map(|row| {
                        (col..col + columns).map(move |col| Slot::Grid { page, row, col })
                    })
                    .collect()
            }
            Slot::Dock { .. } => vec![self.slot],
        }
    }
}

/// A file name of the images directory: no path, nothing that leaves it.
fn valid_image_file(file: &str) -> bool {
    !file.is_empty() && file != "." && file != ".." && !file.contains(['/', '\\', '\n', '\r'])
}

/// A folder title as saved: one line, without surrounding spaces.
fn clean_title(title: &str) -> String {
    title.replace(['\n', '\r'], " ").trim().to_owned()
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
    /// Notification badges of the apps on Home.
    pub badge_position: BadgePosition,
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
            badge_position: BadgePosition::default(),
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

    /// The app is on Home, by itself or in a folder.
    pub fn is_on_home(&self, app: &AppRef) -> bool {
        self.home.iter().any(|entry| entry.content.contains(app))
    }

    fn position(&self, key: &EntryKey) -> Option<usize> {
        self.home
            .iter()
            .position(|entry| entry.content.key() == *key)
    }

    /// `content` can be at `slot`: inside the grid (all of its cells) or the
    /// dock (single cells only, not images).
    fn fits(&self, content: &HomeContent, slot: Slot) -> bool {
        let (columns, rows) = content.span();
        match slot {
            Slot::Grid { row, col, .. } => {
                columns >= 1
                    && rows >= 1
                    && col + columns <= self.home_columns
                    && row + rows <= self.home_rows
            }
            Slot::Dock { col } => !content.keeps_its_cells() && col < self.home_columns,
        }
    }

    /// No entry covers the cell.
    fn is_free(&self, cell: Slot) -> bool {
        !self.home.iter().any(|entry| entry.covers(cell))
    }

    /// The first free grid cell, page by page.
    fn first_free_cell(&self) -> Slot {
        self.first_free_area((1, 1))
    }

    /// The first grid slot, page by page, where `span` cells are free (`span`
    /// reduced to the grid size, see `clamped_span`).
    fn first_free_area(&self, span: (u32, u32)) -> Slot {
        let (columns, rows) = self.clamped_span(span);
        (0..)
            .flat_map(|page| {
                (0..=self.home_rows - rows).flat_map(move |row| {
                    (0..=self.home_columns - columns).map(move |col| Slot::Grid { page, row, col })
                })
            })
            .find(|&slot| {
                !self
                    .home
                    .iter()
                    .any(|entry| entry.overlaps(slot, (columns, rows)))
            })
            .expect("there is always a free area")
    }

    /// `span` reduced to fit the grid, at least one cell.
    fn clamped_span(&self, (columns, rows): (u32, u32)) -> (u32, u32) {
        (
            columns.clamp(1, self.home_columns.max(1)),
            rows.clamp(1, self.home_rows.max(1)),
        )
    }

    /// An image or widget bigger than the grid (e.g. made smaller in Settings)
    /// shrinks to it; a large folder that doesn't fit becomes a single cell.
    fn clamp_span(&self, content: &mut HomeContent) {
        match content {
            HomeContent::Image(_) | HomeContent::Widget(_) => {
                let (columns, rows) = self.clamped_span(content.span());
                content.set_span(columns, rows);
            }
            HomeContent::Folder(folder) if folder.large => {
                folder.large = self.clamped_span(LARGE_FOLDER_SPAN) == LARGE_FOLDER_SPAN;
            }
            _ => {}
        }
    }

    /// Puts an app in the first free grid cell (no-op if it's already on Home).
    pub fn add_to_home(&mut self, app: AppRef, installed: impl Fn(&AppRef) -> bool) {
        if !installed(&app) {
            return;
        }
        self.drop_uninstalled(&installed);
        if !self.is_on_home(&app) {
            let slot = self.first_free_cell();
            self.home.push(HomeEntry::app(app, slot));
        }
        self.compact_pages();
    }

    /// Removes an app from Home, also from the folder it's in.
    pub fn remove_from_home(&mut self, app: &AppRef, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        self.home
            .retain(|entry| entry.content != HomeContent::App(app.clone()));
        for entry in &mut self.home {
            if let HomeContent::Folder(folder) = &mut entry.content {
                folder.apps.retain(|a| a != app);
            }
        }
        self.dissolve_small_folders();
        self.compact_pages();
    }

    /// The entry can be moved to `slot` (its top-left cell): all its cells fit,
    /// and no image or large folder is on them (single-cell entries make room,
    /// see `move_to`).
    pub fn can_move(&self, key: &EntryKey, slot: Slot) -> bool {
        let Some(from) = self.position(key) else {
            return false;
        };
        let entry = &self.home[from];
        let span = entry.content.span();
        self.fits(&entry.content, slot)
            && !self.home.iter().enumerate().any(|(i, other)| {
                i != from && other.content.keeps_its_cells() && other.overlaps(slot, span)
            })
    }

    /// Moves an entry of Home to `slot` (see `can_move`). The entries it lands
    /// on go to the cells it left (for a single cell, both swap places), or to
    /// the first free cells when those are taken.
    pub fn move_to(&mut self, key: &EntryKey, slot: Slot, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        if !self.can_move(key, slot) {
            return;
        }
        let Some(from) = self.position(key) else {
            return;
        };
        let span = self.home[from].content.span();
        let old_cells = self.home[from].cells();
        let displaced: Vec<usize> = (0..self.home.len())
            .filter(|&i| i != from && self.home[i].overlaps(slot, span))
            .collect();
        self.home[from].slot = slot;
        // The displaced entries (single cells) are inside the moved one's new
        // area, so they never take each other's new cell.
        for i in displaced {
            let target = old_cells
                .iter()
                .copied()
                .find(|&cell| self.is_free(cell))
                .unwrap_or_else(|| self.first_free_cell());
            self.home[i].slot = target;
        }
        self.compact_pages();
    }

    /// Drops an entry of Home on the one at `slot`: an app dropped on another
    /// app makes a folder with both (in that slot); dropped on a folder, it joins
    /// it. Anything else (a folder, an empty slot) is a `move_to`.
    pub fn merge_into(&mut self, key: &EntryKey, slot: Slot, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        let target = self.home.iter().position(|entry| entry.covers(slot));
        let (EntryKey::App(app), Some(from), Some(target)) = (key, self.position(key), target)
        else {
            return self.move_to(key, slot, installed);
        };
        if from == target {
            return;
        }
        let id = self.next_folder_id();
        let entry = &mut self.home[target];
        let pair = match &mut entry.content {
            HomeContent::Folder(folder) => {
                folder.apps.push(app.clone());
                None
            }
            HomeContent::App(other) => Some(vec![other.clone(), app.clone()]),
            // Nothing goes into an image or a widget (`move_to` refuses it too).
            HomeContent::Image(_) | HomeContent::Widget(_) => return,
        };
        if let Some(apps) = pair {
            entry.content = HomeContent::Folder(Folder {
                id,
                title: String::new(),
                apps,
                large: false,
            });
        }
        self.home.remove(from);
        self.compact_pages();
    }

    /// Takes an app out of its folder, to the first free grid cell.
    pub fn remove_from_folder(&mut self, app: &AppRef, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        let mut found = false;
        for entry in &mut self.home {
            if let HomeContent::Folder(folder) = &mut entry.content {
                let before = folder.apps.len();
                folder.apps.retain(|a| a != app);
                found |= folder.apps.len() != before;
            }
        }
        if !found {
            return;
        }
        self.dissolve_small_folders();
        let slot = self.first_free_cell();
        self.home.push(HomeEntry::app(app.clone(), slot));
        self.compact_pages();
    }

    /// Removes a folder, putting its apps back on Home: the first one in the
    /// folder's slot, the others in the first free grid cells.
    pub fn disband_folder(&mut self, id: u32, installed: impl Fn(&AppRef) -> bool) {
        self.drop_uninstalled(&installed);
        let Some(index) = self.position(&EntryKey::Folder(id)) else {
            return;
        };
        let HomeContent::Folder(folder) = &mut self.home[index].content else {
            return;
        };
        let mut apps = std::mem::take(&mut folder.apps).into_iter();
        match apps.next() {
            Some(first) => self.home[index].content = HomeContent::App(first),
            None => {
                self.home.remove(index);
            }
        }
        for app in apps {
            let slot = self.first_free_cell();
            self.home.push(HomeEntry::app(app, slot));
        }
        self.compact_pages();
    }

    pub fn rename_folder(&mut self, id: u32, title: &str) {
        for entry in &mut self.home {
            if let HomeContent::Folder(folder) = &mut entry.content
                && folder.id == id
            {
                folder.title = clean_title(title);
            }
        }
    }

    /// Where a folder goes when enlarged (`resize_folder`): a 2x2 area of the
    /// grid with its cell, preferably from it (to the right and down), without
    /// images or large folders. Areas where nothing has to make room come first.
    /// `None` in the dock, when it's already large, or when nothing fits.
    fn large_folder_slot(&self, index: usize) -> Option<Slot> {
        let entry = &self.home[index];
        let Slot::Grid { page, row, col } = entry.slot else {
            return None;
        };
        if entry.content.span() != (1, 1) {
            return None;
        }
        let (columns, rows) = LARGE_FOLDER_SPAN;
        let blocked = |slot: Slot, any_entry: bool| {
            self.home.iter().enumerate().any(|(i, other)| {
                i != index
                    && (any_entry || other.content.keeps_its_cells())
                    && other.overlaps(slot, LARGE_FOLDER_SPAN)
            })
        };
        let candidates: Vec<Slot> = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .filter(|&(dc, dr)| col >= dc && row >= dr)
            .map(|(dc, dr)| (col - dc, row - dr))
            .filter(|&(col, row)| {
                col + columns <= self.home_columns && row + rows <= self.home_rows
            })
            .map(|(col, row)| Slot::Grid { page, row, col })
            .filter(|&slot| !blocked(slot, false))
            .collect();
        candidates
            .iter()
            .copied()
            .find(|&slot| !blocked(slot, true))
            .or_else(|| candidates.first().copied())
    }

    /// A folder can be enlarged (`large`) or shrunk (not `large`).
    pub fn can_resize_folder(&self, id: u32, large: bool) -> bool {
        let Some(index) = self.position(&EntryKey::Folder(id)) else {
            return false;
        };
        match &self.home[index].content {
            HomeContent::Folder(folder) if folder.large == large => false,
            HomeContent::Folder(_) => !large || self.large_folder_slot(index).is_some(),
            _ => false,
        }
    }

    /// Enlarges a folder to 2x2 cells (see `large_folder_slot`; the single-cell
    /// entries there go to the first free cells) or shrinks it back to its
    /// top-left cell.
    pub fn resize_folder(&mut self, id: u32, large: bool) {
        if !self.can_resize_folder(id, large) {
            return;
        }
        let Some(index) = self.position(&EntryKey::Folder(id)) else {
            return;
        };
        if large {
            let Some(slot) = self.large_folder_slot(index) else {
                return;
            };
            let displaced: Vec<usize> = (0..self.home.len())
                .filter(|&i| i != index && self.home[i].overlaps(slot, LARGE_FOLDER_SPAN))
                .collect();
            self.home[index].slot = slot;
            if let HomeContent::Folder(folder) = &mut self.home[index].content {
                folder.large = true;
            }
            for i in displaced {
                self.home[i].slot = self.first_free_cell();
            }
        } else if let HomeContent::Folder(folder) = &mut self.home[index].content {
            folder.large = false;
        }
        self.compact_pages();
    }

    fn next_folder_id(&self) -> u32 {
        self.home
            .iter()
            .filter_map(|entry| match &entry.content {
                HomeContent::Folder(folder) => Some(folder.id + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    fn next_image_id(&self) -> u32 {
        self.home
            .iter()
            .filter_map(|entry| match &entry.content {
                HomeContent::Image(image) => Some(image.id + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// Puts an image file (of the images directory) on the grid, one cell big:
    /// at `preferred` if that cell is free, otherwise in the first free one.
    pub fn add_image(&mut self, file: &str, preferred: Option<Slot>) {
        if !valid_image_file(file) {
            return;
        }
        let content = HomeContent::Image(HomeImage {
            id: self.next_image_id(),
            file: file.to_owned(),
            columns: 1,
            rows: 1,
        });
        let slot = preferred
            .filter(|&slot| self.fits(&content, slot) && self.is_free(slot))
            .unwrap_or_else(|| self.first_free_cell());
        self.home.push(HomeEntry { content, slot });
        self.compact_pages();
    }

    /// Removes an image from Home. Returns its file, to delete it.
    pub fn remove_image(&mut self, id: u32) -> Option<String> {
        let index = self.position(&EntryKey::Image(id))?;
        let entry = self.home.remove(index);
        self.compact_pages();
        match entry.content {
            HomeContent::Image(image) => Some(image.file),
            _ => None,
        }
    }

    /// Puts a widget (already bound to its provider, see `widgets.rs`) on the
    /// grid, `span` cells big (reduced to the grid): at `preferred` if those
    /// cells are free, otherwise in the first free area. No-op if it's there.
    pub fn add_widget(
        &mut self,
        id: u32,
        provider: &str,
        span: (u32, u32),
        preferred: Option<Slot>,
    ) {
        if self.position(&EntryKey::Widget(id)).is_some() {
            return;
        }
        let (columns, rows) = self.clamped_span(span);
        let content = HomeContent::Widget(HomeWidget {
            id,
            provider: provider.replace(['\n', '\r'], ""),
            columns,
            rows,
        });
        let slot = preferred
            .filter(|&slot| {
                self.fits(&content, slot)
                    && !self
                        .home
                        .iter()
                        .any(|entry| entry.overlaps(slot, (columns, rows)))
            })
            .unwrap_or_else(|| self.first_free_area((columns, rows)));
        self.home.push(HomeEntry { content, slot });
        self.compact_pages();
    }

    /// Removes a widget from Home. Returns whether it was there.
    pub fn remove_widget(&mut self, id: u32) -> bool {
        let Some(index) = self.position(&EntryKey::Widget(id)) else {
            return false;
        };
        self.home.remove(index);
        self.compact_pages();
        true
    }

    /// Ids of the widgets on Home.
    pub fn widget_ids(&self) -> Vec<u32> {
        self.home
            .iter()
            .filter_map(|entry| match &entry.content {
                HomeContent::Widget(widget) => Some(widget.id),
                _ => None,
            })
            .collect()
    }

    /// An image or a widget can cover `columns` x `rows` cells from its slot:
    /// they fit in the grid and no other entry is on them.
    pub fn can_resize(&self, key: &EntryKey, columns: u32, rows: u32) -> bool {
        let Some(index) = self.position(key) else {
            return false;
        };
        let entry = &self.home[index];
        let mut resized = entry.content.clone();
        resized.set_span(columns, rows)
            && self.fits(&resized, entry.slot)
            && !self
                .home
                .iter()
                .enumerate()
                .any(|(i, other)| i != index && other.overlaps(entry.slot, (columns, rows)))
    }

    /// Makes an image or a widget cover `columns` x `rows` cells (see
    /// `can_resize`).
    pub fn resize(&mut self, key: &EntryKey, columns: u32, rows: u32) {
        if !self.can_resize(key, columns, rows) {
            return;
        }
        if let Some(index) = self.position(key) {
            self.home[index].content.set_span(columns, rows);
        }
    }

    /// A folder left with a single app becomes that app (in the folder's slot);
    /// an empty one is removed.
    fn dissolve_small_folders(&mut self) {
        self.home.retain_mut(|entry| {
            let single = match &mut entry.content {
                HomeContent::Folder(folder) if folder.apps.is_empty() => return false,
                HomeContent::Folder(folder) if folder.apps.len() == 1 => folder.apps.pop(),
                _ => None,
            };
            if let Some(app) = single {
                entry.content = HomeContent::App(app);
            }
            true
        });
    }

    /// List views: moves an entry so it's right before the one at `index` of the
    /// Home list (`list_order`; the length puts it last); the entries in between
    /// shift. The set of used slots doesn't change, only what is in each one, so
    /// the grid layout keeps its shape.
    pub fn move_to_index(
        &mut self,
        key: &EntryKey,
        index: usize,
        installed: impl Fn(&AppRef) -> bool,
    ) {
        self.drop_uninstalled(&installed);
        let order = self.list_order();
        // Not in the list: they keep their place.
        let images: Vec<HomeEntry> = self
            .home
            .iter()
            .filter(|entry| entry.content.is_grid_only())
            .cloned()
            .collect();
        let slots: Vec<Slot> = order.iter().map(|&i| self.home[i].slot).collect();
        let mut contents: Vec<HomeContent> = order
            .iter()
            .map(|&i| self.home[i].content.clone())
            .collect();
        let Some(from) = contents.iter().position(|c| c.key() == *key) else {
            return;
        };
        let moved = contents.remove(from);
        let index = if index > from { index - 1 } else { index };
        contents.insert(index.min(contents.len()), moved);
        // Images and widgets first: they keep their cells whatever lands next to them. A
        // large folder that lands where its cells don't fit or are taken moves
        // (or the entries in its way do, see `normalize`).
        self.home = images
            .into_iter()
            .chain(
                contents
                    .into_iter()
                    .zip(slots)
                    .map(|(content, slot)| HomeEntry { content, slot }),
            )
            .collect();
        self.normalize();
    }

    /// Indexes of `home` in list order: the grid page by page, then the dock.
    /// Images and widgets are only shown on the grid: they're not in the list.
    pub fn list_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.home.len())
            .filter(|&i| !self.home[i].content.is_grid_only())
            .collect();
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
    /// Settings) or that is otherwise invalid: duplicated apps, folders or slots,
    /// entries outside the grid (those go to the first free cells), folders with
    /// fewer than two apps. Returns whether anything changed.
    pub fn normalize(&mut self) -> bool {
        let before = self.home.clone();
        let mut seen_apps: Vec<AppRef> = Vec::new();
        let mut seen_folders: Vec<u32> = Vec::new();
        let mut seen_images: Vec<u32> = Vec::new();
        let mut seen_widgets: Vec<u32> = Vec::new();
        let mut kept: Vec<HomeEntry> = Vec::with_capacity(self.home.len());
        let mut misplaced: Vec<HomeContent> = Vec::new();
        for mut entry in std::mem::take(&mut self.home) {
            match &mut entry.content {
                HomeContent::App(app) => {
                    if seen_apps.contains(app) {
                        continue;
                    }
                    seen_apps.push(app.clone());
                }
                HomeContent::Folder(folder) => {
                    folder.apps.retain(|app| {
                        let new = !seen_apps.contains(app);
                        if new {
                            seen_apps.push(app.clone());
                        }
                        new
                    });
                    if seen_folders.contains(&folder.id) {
                        folder.id = seen_folders.iter().max().map_or(0, |id| id + 1);
                    }
                    seen_folders.push(folder.id);
                }
                HomeContent::Image(image) => {
                    if seen_images.contains(&image.id) {
                        image.id = seen_images.iter().max().map_or(0, |id| id + 1);
                    }
                    seen_images.push(image.id);
                }
                // The id is the system's: a duplicate can't get another one.
                HomeContent::Widget(widget) => {
                    if seen_widgets.contains(&widget.id) {
                        continue;
                    }
                    seen_widgets.push(widget.id);
                }
            }
            self.clamp_span(&mut entry.content);
            let span = entry.content.span();
            if self.fits(&entry.content, entry.slot)
                && !kept.iter().any(|k| k.overlaps(entry.slot, span))
            {
                kept.push(entry);
            } else {
                misplaced.push(entry.content);
            }
        }
        self.home = kept;
        for content in misplaced {
            let slot = self.first_free_area(content.span());
            self.home.push(HomeEntry { content, slot });
        }
        self.dissolve_small_folders();
        self.compact_pages();
        self.home != before
    }

    fn drop_uninstalled(&mut self, installed: impl Fn(&AppRef) -> bool) {
        self.home.retain_mut(|entry| match &mut entry.content {
            HomeContent::App(app) => installed(app),
            HomeContent::Folder(folder) => {
                folder.apps.retain(|app| installed(app));
                true
            }
            HomeContent::Image(_) | HomeContent::Widget(_) => true,
        });
        self.dissolve_small_folders();
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
        // `folder_app=` lines, attached to their folder once all are read.
        let mut folder_apps: Vec<(u32, AppRef)> = Vec::new();
        // `folder_large=<id>` lines: the large folders.
        let mut large_folders: Vec<u32> = Vec::new();
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
                "badge_position" => {
                    if let Some(position) = BadgePosition::parse(value) {
                        config.badge_position = position;
                    }
                }
                "home" => {
                    if let Some(entry) = parse_home_entry(value) {
                        config.home.push(entry);
                    }
                }
                "folder" => {
                    if let Some(entry) = parse_folder_entry(value) {
                        config.home.push(entry);
                    }
                }
                "image" => {
                    if let Some(entry) = parse_image_entry(value) {
                        config.home.push(entry);
                    }
                }
                "widget" => {
                    if let Some(entry) = parse_widget_entry(value) {
                        config.home.push(entry);
                    }
                }
                "folder_large" => {
                    if let Ok(id) = value.parse::<u32>() {
                        large_folders.push(id);
                    }
                }
                "folder_app" => {
                    if let Some((id, app)) = value.split_once(',')
                        && let (Ok(id), Some(app)) = (id.parse::<u32>(), AppRef::parse(app))
                    {
                        folder_apps.push((id, app));
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
        for (id, app) in folder_apps {
            let folder = config
                .home
                .iter_mut()
                .find_map(|entry| match &mut entry.content {
                    HomeContent::Folder(folder) if folder.id == id => Some(folder),
                    _ => None,
                });
            if let Some(folder) = folder {
                folder.apps.push(app);
            }
        }
        for entry in &mut config.home {
            if let HomeContent::Folder(folder) = &mut entry.content {
                folder.large = large_folders.contains(&folder.id);
            }
        }
        for app in legacy_pinned {
            if !config.is_on_home(&app) {
                let slot = config.first_free_cell();
                config.home.push(HomeEntry::app(app, slot));
            }
        }
        config
    }

    fn serialize(&self) -> String {
        let mut text = format!(
            "launcher_screens={}\napps_view={}\nswipe_down={}\nicon_shape={}\nicon_corner_radius={}\nlegacy_icons={}\nhome_columns={}\nhome_rows={}\nitem_size_grid={}\nitem_size_list={}\nitem_size_text_list={}\napps_padding={}\nlabel_lines={}\nbadge_position={}\n",
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
            self.badge_position.as_str(),
        );
        for entry in &self.home {
            let slot = entry.slot.serialize();
            match &entry.content {
                HomeContent::App(app) => {
                    text.push_str(&format!("home={slot},{}\n", app.serialize()));
                }
                HomeContent::Folder(folder) => {
                    let id = folder.id;
                    text.push_str(&format!(
                        "folder={slot},{id},{}\n",
                        clean_title(&folder.title)
                    ));
                    if folder.large {
                        text.push_str(&format!("folder_large={id}\n"));
                    }
                    for app in &folder.apps {
                        text.push_str(&format!("folder_app={id},{}\n", app.serialize()));
                    }
                }
                HomeContent::Image(image) => {
                    text.push_str(&format!(
                        "image={slot},{},{},{},{}\n",
                        image.id, image.columns, image.rows, image.file
                    ));
                }
                HomeContent::Widget(widget) => {
                    text.push_str(&format!(
                        "widget={slot},{},{},{},{}\n",
                        widget.id, widget.columns, widget.rows, widget.provider
                    ));
                }
            }
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
    let (slot, app) = parse_slot(value)?;
    Some(HomeEntry::app(AppRef::parse(app)?, slot))
}

/// `<slot>,<id>,<title>` (`<slot>` as in `parse_home_entry`); its apps are
/// `folder_app=<id>,<package>/<activity>` lines.
fn parse_folder_entry(value: &str) -> Option<HomeEntry> {
    let (slot, rest) = parse_slot(value)?;
    let (id, title) = rest.split_once(',').unwrap_or((rest, ""));
    Some(HomeEntry {
        content: HomeContent::Folder(Folder {
            id: id.trim().parse().ok()?,
            title: clean_title(title),
            apps: Vec::new(),
            large: false,
        }),
        slot,
    })
}

/// `<slot>,<id>,<columns>,<rows>,<file>` (`<slot>` as in `parse_home_entry`).
fn parse_image_entry(value: &str) -> Option<HomeEntry> {
    let (slot, rest) = parse_slot(value)?;
    let mut parts = rest.splitn(4, ',');
    let id = parts.next()?.trim().parse().ok()?;
    let columns = parts.next()?.trim().parse().ok()?;
    let rows = parts.next()?.trim().parse().ok()?;
    let file = parts.next()?.trim();
    valid_image_file(file).then(|| HomeEntry {
        content: HomeContent::Image(HomeImage {
            id,
            file: file.to_owned(),
            columns,
            rows,
        }),
        slot,
    })
}

/// `<slot>,<id>,<columns>,<rows>,<provider>` (`<slot>` as in `parse_home_entry`).
fn parse_widget_entry(value: &str) -> Option<HomeEntry> {
    let (slot, rest) = parse_slot(value)?;
    let mut parts = rest.splitn(4, ',');
    let id = parts.next()?.trim().parse().ok()?;
    let columns = parts.next()?.trim().parse().ok()?;
    let rows = parts.next()?.trim().parse().ok()?;
    let provider = parts.next()?.trim();
    (!provider.is_empty()).then(|| HomeEntry {
        content: HomeContent::Widget(HomeWidget {
            id,
            provider: provider.to_owned(),
            columns,
            rows,
        }),
        slot,
    })
}

/// `grid,<page>,<row>,<col>,<rest>` or `dock,<col>,<rest>`.
fn parse_slot(value: &str) -> Option<(Slot, &str)> {
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
    Some((slot, app))
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

    fn key(n: u32) -> EntryKey {
        EntryKey::App(app(n))
    }

    fn cell(page: u32, row: u32, col: u32) -> Slot {
        Slot::Grid { page, row, col }
    }

    fn folder(config: &Config, slot: Slot) -> Option<&Folder> {
        config
            .home
            .iter()
            .find(|entry| entry.slot == slot)
            .and_then(|entry| match &entry.content {
                HomeContent::Folder(folder) => Some(folder),
                _ => None,
            })
    }

    #[test]
    fn roundtrip() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.home.push(HomeEntry {
            content: HomeContent::App(app(2)),
            slot: Slot::Dock { col: 2 },
        });
        config.home.push(HomeEntry {
            content: HomeContent::Folder(Folder {
                id: 7,
                title: "Games, fun = yes".to_owned(),
                apps: vec![app(3), app(4)],
                large: false,
            }),
            slot: Slot::Dock { col: 0 },
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
        config.badge_position = BadgePosition::BottomLeft;
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
            &key(1),
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
        config.move_to(&key(1), Slot::Dock { col: 0 }, all);
        config.move_to(
            &key(2),
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
        config.move_to_index(&key(3), 0, all);
        let order: Vec<_> = config
            .list_order()
            .into_iter()
            .map(|i| config.home[i].content.key())
            .collect();
        assert_eq!(order, vec![key(3), key(1), key(2)]);
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
            content: HomeContent::App(app(1)),
            slot: Slot::Grid {
                page: 0,
                row: 6,
                col: 5,
            },
        });
        config.home.push(HomeEntry {
            content: HomeContent::App(app(2)),
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

    #[test]
    fn app_dropped_on_app_makes_a_folder() {
        let mut config = Config::default();
        for n in 1..=3 {
            config.add_to_home(app(n), all);
        }
        config.merge_into(&key(2), cell(0, 0, 0), all);
        let made = folder(&config, cell(0, 0, 0)).expect("a folder");
        assert_eq!(made.apps, vec![app(1), app(2)]);
        assert_eq!(made.title, "");
        assert!(config.is_on_home(&app(2)));
        // Dropped on the folder: joins it.
        config.merge_into(&key(3), cell(0, 0, 0), all);
        let id = folder(&config, cell(0, 0, 0)).unwrap().id;
        assert_eq!(folder(&config, cell(0, 0, 0)).unwrap().apps.len(), 3);
        assert_eq!(config.home.len(), 1);
        // A folder dropped on an app only swaps.
        config.add_to_home(app(4), all);
        config.merge_into(&EntryKey::Folder(id), cell(0, 0, 1), all);
        assert!(folder(&config, cell(0, 0, 1)).is_some());
        assert_eq!(config.home.len(), 2);
    }

    #[test]
    fn folders_dissolve_with_one_app() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.add_to_home(app(2), all);
        config.merge_into(&key(2), cell(0, 0, 0), all);
        config.remove_from_folder(&app(1), all);
        // The folder became app 2, in its cell; app 1 went to the next free one.
        assert_eq!(config.home[0], HomeEntry::app(app(2), cell(0, 0, 0)));
        assert_eq!(config.home[1], HomeEntry::app(app(1), cell(0, 0, 1)));

        config.merge_into(&key(1), cell(0, 0, 0), all);
        config.remove_from_home(&app(2), all);
        assert_eq!(config.home, vec![HomeEntry::app(app(1), cell(0, 0, 0))]);

        config.add_to_home(app(2), all);
        config.merge_into(&key(2), cell(0, 0, 0), all);
        config.remove_from_home(&app(1), |a| *a != app(2));
        assert!(config.home.is_empty());
    }

    #[test]
    fn disband_puts_the_apps_on_home() {
        let mut config = Config::default();
        for n in 1..=4 {
            config.add_to_home(app(n), all);
        }
        config.merge_into(&key(2), cell(0, 0, 0), all);
        config.merge_into(&key(3), cell(0, 0, 0), all);
        // Folder [1, 2, 3] in (0,0,0); app 4 in (0,0,3); (0,0,1) and (0,0,2) free.
        let id = folder(&config, cell(0, 0, 0)).unwrap().id;
        config.disband_folder(id, all);
        assert_eq!(
            config.home,
            vec![
                HomeEntry::app(app(1), cell(0, 0, 0)),
                HomeEntry::app(app(4), cell(0, 0, 3)),
                HomeEntry::app(app(2), cell(0, 0, 1)),
                HomeEntry::app(app(3), cell(0, 0, 2)),
            ]
        );
    }

    #[test]
    fn rename_folder_keeps_one_line() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.add_to_home(app(2), all);
        config.merge_into(&key(2), cell(0, 0, 0), all);
        let id = folder(&config, cell(0, 0, 0)).unwrap().id;
        config.rename_folder(id, "  Work\nstuff ");
        assert_eq!(folder(&config, cell(0, 0, 0)).unwrap().title, "Work stuff");
    }

    #[test]
    fn move_to_index_inserts_before() {
        let mut config = Config::default();
        for n in 1..=3 {
            config.add_to_home(app(n), all);
        }
        let order = |config: &Config| -> Vec<EntryKey> {
            config
                .list_order()
                .into_iter()
                .map(|i| config.home[i].content.key())
                .collect()
        };
        config.move_to_index(&key(1), 3, all);
        assert_eq!(order(&config), vec![key(2), key(3), key(1)]);
        config.move_to_index(&key(1), 1, all);
        assert_eq!(order(&config), vec![key(2), key(1), key(3)]);
        // Right before or after itself: no change.
        config.move_to_index(&key(1), 2, all);
        assert_eq!(order(&config), vec![key(2), key(1), key(3)]);
    }

    fn image_at(config: &Config, id: u32) -> (Slot, (u32, u32)) {
        let entry = &config.home[config.position(&EntryKey::Image(id)).unwrap()];
        (entry.slot, entry.content.span())
    }

    #[test]
    fn image_roundtrip_and_bad_files() {
        let mut config = Config::default();
        config.add_image("img-1.jpg", Some(cell(0, 1, 1)));
        config.resize(&EntryKey::Image(0), 2, 3);
        assert_eq!(image_at(&config, 0), (cell(0, 1, 1), (2, 3)));
        assert_eq!(Config::parse(&config.serialize()), config);
        let parsed = Config::parse("image=grid,0,0,0,0,1,1,../x\nimage=grid,0,0,0,1,1,1,\n");
        assert!(parsed.home.is_empty());
    }

    fn widget_at(config: &Config, id: u32) -> (Slot, (u32, u32)) {
        let entry = &config.home[config.position(&EntryKey::Widget(id)).unwrap()];
        (entry.slot, entry.content.span())
    }

    #[test]
    fn widget_roundtrip_resize_and_remove() {
        let mut config = Config::default();
        config.add_widget(42, "com.example/.ClockWidget", (2, 2), Some(cell(0, 1, 1)));
        assert_eq!(widget_at(&config, 42), (cell(0, 1, 1), (2, 2)));
        // Already there: nothing changes.
        config.add_widget(42, "com.example/.ClockWidget", (1, 1), None);
        assert_eq!(config.widget_ids(), vec![42]);
        assert!(config.can_resize(&EntryKey::Widget(42), 3, 1));
        config.resize(&EntryKey::Widget(42), 3, 1);
        assert_eq!(widget_at(&config, 42), (cell(0, 1, 1), (3, 1)));
        assert_eq!(Config::parse(&config.serialize()), config);
        // Never in the dock, nothing goes into it, not in the list views.
        assert!(!config.can_move(&EntryKey::Widget(42), Slot::Dock { col: 0 }));
        assert!(config.list_order().is_empty());
        assert!(config.remove_widget(42));
        assert!(config.home.is_empty());
        assert!(Config::parse("widget=grid,0,0,0,7,1,1,\n").home.is_empty());
    }

    #[test]
    fn widget_bigger_than_the_grid_or_on_taken_cells() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        // Its cells are taken: the first free area instead.
        config.add_widget(1, "a/.W", (2, 1), Some(cell(0, 0, 0)));
        assert_eq!(widget_at(&config, 1), (cell(0, 0, 1), (2, 1)));
        // Reduced to the grid.
        config.add_widget(2, "a/.W", (99, 99), None);
        assert_eq!(
            widget_at(&config, 2).1,
            (DEFAULT_HOME_COLUMNS, DEFAULT_HOME_ROWS)
        );
        // A duplicated id is dropped.
        let mut duplicated =
            Config::parse("widget=grid,0,0,0,5,1,1,a/.W\nwidget=grid,0,1,0,5,1,1,a/.W\n");
        assert!(duplicated.normalize());
        assert_eq!(duplicated.widget_ids(), vec![5]);
    }

    #[test]
    fn image_added_to_the_pressed_cell_or_the_first_free_one() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.add_image("a.png", Some(cell(0, 0, 0)));
        assert_eq!(image_at(&config, 0).0, cell(0, 0, 1));
        config.add_image("b.png", Some(cell(0, 3, 2)));
        assert_eq!(image_at(&config, 1).0, cell(0, 3, 2));
        // Never in the dock.
        config.add_image("c.png", Some(Slot::Dock { col: 0 }));
        assert_eq!(image_at(&config, 2).0, cell(0, 0, 2));
    }

    #[test]
    fn image_resize_needs_free_cells_inside_the_grid() {
        let mut config = Config::default();
        config.add_image("a.png", None);
        config.add_to_home(app(1), all);
        // Image at (0,0,0), app at (0,0,1).
        assert!(!config.can_resize(&EntryKey::Image(0), 2, 1));
        assert!(config.can_resize(&EntryKey::Image(0), 1, 2));
        assert!(!config.can_resize(&EntryKey::Image(0), 1, DEFAULT_HOME_ROWS + 1));
        assert!(!config.can_resize(&EntryKey::Image(0), 0, 1));
        config.resize(&EntryKey::Image(0), 2, 2);
        assert_eq!(image_at(&config, 0).1, (1, 1));
        config.resize(&EntryKey::Image(0), 1, 5);
        assert_eq!(image_at(&config, 0).1, (1, 5));
        // Its cells are taken: new apps go around it.
        config.add_to_home(app(2), all);
        assert_eq!(config.home[2].slot, cell(0, 0, 2));
    }

    #[test]
    fn apps_dont_move_onto_images() {
        let mut config = Config::default();
        config.add_image("a.png", None);
        config.resize(&EntryKey::Image(0), 2, 2);
        config.add_to_home(app(1), all);
        // App at (0,0,2).
        assert!(!config.can_move(&key(1), cell(0, 1, 1)));
        config.move_to(&key(1), cell(0, 1, 1), all);
        config.merge_into(&key(1), cell(0, 1, 1), all);
        assert_eq!(config.home[1], HomeEntry::app(app(1), cell(0, 0, 2)));
        assert!(!config.can_move(&EntryKey::Image(0), Slot::Dock { col: 0 }));
    }

    #[test]
    fn moved_image_displaces_apps_into_its_old_cells() {
        let mut config = Config::default();
        config.add_image("a.png", None);
        config.resize(&EntryKey::Image(0), 2, 1);
        config.add_to_home(app(1), all);
        config.add_to_home(app(2), all);
        // Image (0,0,0)-(0,0,1), apps at (0,0,2) and (0,0,3).
        assert!(config.can_move(&EntryKey::Image(0), cell(0, 0, 2)));
        assert!(!config.can_move(&EntryKey::Image(0), cell(0, 0, 3)));
        config.move_to(&EntryKey::Image(0), cell(0, 0, 2), all);
        assert_eq!(image_at(&config, 0).0, cell(0, 0, 2));
        assert_eq!(config.home[1].slot, cell(0, 0, 0));
        assert_eq!(config.home[2].slot, cell(0, 0, 1));
        // One cell to the left: only one old cell is left free.
        config.move_to(&EntryKey::Image(0), cell(0, 0, 1), all);
        assert_eq!(image_at(&config, 0).0, cell(0, 0, 1));
        assert_eq!(config.home[2].slot, cell(0, 0, 3));
    }

    #[test]
    fn images_stay_out_of_the_list() {
        let mut config = Config::default();
        config.add_to_home(app(1), all);
        config.add_image("a.png", None);
        config.add_to_home(app(2), all);
        assert_eq!(config.list_order().len(), 2);
        config.move_to_index(&key(2), 0, all);
        assert_eq!(image_at(&config, 0).0, cell(0, 0, 1));
        assert!(
            config
                .home
                .iter()
                .any(|e| e == &HomeEntry::app(app(2), cell(0, 0, 0)))
        );
        assert!(
            config
                .home
                .iter()
                .any(|e| e == &HomeEntry::app(app(1), cell(0, 0, 2)))
        );
    }

    #[test]
    fn normalize_shrinks_and_moves_images() {
        let mut config = Config::parse(
            "home_columns=4\nimage=grid,0,0,0,0,6,2,a.png\nhome=grid,0,0,0,p1/p1.Main\nimage=dock,0,1,1,1,b.png\n",
        );
        assert!(config.normalize());
        assert_eq!(image_at(&config, 0), (cell(0, 0, 0), (4, 2)));
        // The app was under the image; the image in the dock goes to the grid.
        assert_eq!(config.home[1], HomeEntry::app(app(1), cell(0, 2, 0)));
        assert_eq!(image_at(&config, 1).0, cell(0, 2, 1));
        assert!(!config.normalize());
    }

    /// Home with a folder of apps 1 and 2 at (0,0,0) (id 0) and apps 3.. `n`
    /// after it.
    fn with_folder(n: u32) -> Config {
        let mut config = Config::default();
        for i in 1..=n {
            config.add_to_home(app(i), all);
        }
        config.merge_into(&key(2), cell(0, 0, 0), all);
        config
    }

    fn folder_at(config: &Config, id: u32) -> (Slot, (u32, u32)) {
        let entry = &config.home[config.position(&EntryKey::Folder(id)).unwrap()];
        (entry.slot, entry.content.span())
    }

    #[test]
    fn large_folder_roundtrip_and_shrink() {
        let mut config = with_folder(2);
        assert!(!config.can_resize_folder(0, false));
        config.resize_folder(0, true);
        assert_eq!(folder_at(&config, 0), (cell(0, 0, 0), (2, 2)));
        assert!(!config.can_resize_folder(0, true));
        assert_eq!(Config::parse(&config.serialize()), config);
        config.resize_folder(0, false);
        assert_eq!(folder_at(&config, 0), (cell(0, 0, 0), (1, 1)));
    }

    #[test]
    fn enlarged_folder_moves_apps_in_its_way() {
        // Folder at (0,0,0), app 3 at (0,1,1), app 4 at (0,0,3).
        let mut config = with_folder(4);
        config.move_to(&key(3), cell(0, 1, 1), all);
        config.resize_folder(0, true);
        assert_eq!(folder_at(&config, 0), (cell(0, 0, 0), (2, 2)));
        let slot_of = |config: &Config, n| config.home[config.position(&key(n)).unwrap()].slot;
        assert_eq!(slot_of(&config, 3), cell(0, 0, 2));
        assert_eq!(slot_of(&config, 4), cell(0, 0, 3));
        // Nothing goes on it but into it.
        assert!(!config.can_move(&key(3), cell(0, 1, 1)));
        config.merge_into(&key(3), cell(0, 1, 1), all);
        assert_eq!(config.home.len(), 2);
    }

    #[test]
    fn enlarged_folder_grows_left_or_up_at_the_edges() {
        let mut config = with_folder(2);
        let last = cell(0, DEFAULT_HOME_ROWS - 1, DEFAULT_HOME_COLUMNS - 1);
        config.move_to(&EntryKey::Folder(0), last, all);
        config.resize_folder(0, true);
        assert_eq!(
            folder_at(&config, 0),
            (
                cell(0, DEFAULT_HOME_ROWS - 2, DEFAULT_HOME_COLUMNS - 2),
                (2, 2)
            )
        );
    }

    #[test]
    fn large_folders_stay_out_of_the_dock_and_images() {
        let mut config = with_folder(2);
        config.add_image("a.png", Some(cell(0, 0, 1)));
        // Only up or left of the image: the folder's cell is on the top row.
        assert!(!config.can_resize_folder(0, true));
        config.move_to(&EntryKey::Folder(0), Slot::Dock { col: 0 }, all);
        assert!(!config.can_resize_folder(0, true));
        config.move_to(&EntryKey::Folder(0), cell(0, 2, 0), all);
        config.resize_folder(0, true);
        assert!(!config.can_move(&EntryKey::Folder(0), Slot::Dock { col: 0 }));
        assert!(!config.can_move(&EntryKey::Folder(0), cell(0, 0, 0)));
        assert!(config.can_move(&EntryKey::Folder(0), cell(0, 1, 2)));
    }

    #[test]
    fn normalize_shrinks_large_folders_that_dont_fit() {
        let mut config = with_folder(2);
        config.resize_folder(0, true);
        config.home_columns = 1;
        assert!(config.normalize());
        assert_eq!(folder_at(&config, 0), (cell(0, 0, 0), (1, 1)));
    }

    #[test]
    fn list_reorder_keeps_large_folders_whole() {
        // Folder 2x2 at (0,0,0), apps 3 and 4 after it.
        let mut config = with_folder(4);
        config.resize_folder(0, true);
        config.move_to_index(&EntryKey::Folder(0), 2, all);
        let mut taken: Vec<Slot> = config.home.iter().flat_map(|e| e.cells()).collect();
        let count = taken.len();
        taken.sort();
        taken.dedup();
        assert_eq!(taken.len(), count);
        assert_eq!(folder_at(&config, 0).1, (2, 2));
    }

    #[test]
    fn normalize_fixes_folders() {
        let mut config = Config::parse(
            "home=grid,0,0,0,p1/p1.Main\nfolder=grid,0,0,1,3,A\nfolder_app=3,p1/p1.Main\nfolder_app=3,p2/p2.Main\nfolder=grid,0,0,2,3,B\nfolder_app=3,p4/p4.Main\n",
        );
        // The apps go to the first folder with their id.
        assert!(config.normalize());
        // p1 is already on Home: it leaves the folder; the second folder (a
        // duplicated id, no apps) is removed.
        assert_eq!(config.home.len(), 2);
        assert_eq!(
            folder(&config, cell(0, 0, 1)).unwrap().apps,
            vec![app(2), app(4)]
        );
        assert!(!config.normalize());
    }
}
