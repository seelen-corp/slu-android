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
    /// A Home screen with the apps pinned by the user, plus the Apps Menu.
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

/// A launcher entry pinned to Home, identified the same way as
/// `apps::AppInfo` (a package can expose several launcher activities).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedApp {
    pub package_name: String,
    pub activity_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub launcher_screens: LauncherScreens,
    pub apps_view: AppsView,
    /// In display order.
    pub pinned: Vec<PinnedApp>,
}

impl Config {
    pub fn is_pinned(&self, package_name: &str, activity_name: &str) -> bool {
        self.pinned
            .iter()
            .any(|p| p.package_name == package_name && p.activity_name == activity_name)
    }

    fn parse(text: &str) -> Self {
        let mut config = Config::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "launcher_screens" => {
                    if let Some(screens) = LauncherScreens::parse(value.trim()) {
                        config.launcher_screens = screens;
                    }
                }
                "apps_view" => {
                    if let Some(view) = AppsView::parse(value.trim()) {
                        config.apps_view = view;
                    }
                }
                "pinned" => {
                    if let Some((package_name, activity_name)) = value.trim().split_once('/') {
                        config.pinned.push(PinnedApp {
                            package_name: package_name.to_owned(),
                            activity_name: activity_name.to_owned(),
                        });
                    }
                }
                _ => {}
            }
        }
        config
    }

    fn serialize(&self) -> String {
        let mut text = format!(
            "launcher_screens={}\napps_view={}\n",
            self.launcher_screens.as_str(),
            self.apps_view.as_str()
        );
        for pinned in &self.pinned {
            text.push_str(&format!(
                "pinned={}/{}\n",
                pinned.package_name, pinned.activity_name
            ));
        }
        text
    }
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

    let path = path()?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, config.serialize())?;
    std::fs::rename(&tmp, path)?;
    Ok(config)
}
