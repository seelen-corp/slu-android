//! Images placed on Home (`config::HomeImage`). The user picks them with the
//! system picker (`HomeActivity.pickImage`), which copies each one, already
//! rotated and scaled down, into the images directory and reports its file
//! name (`nativeOnImagePicked`, see `lib.rs`).

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::Result;
use crate::system_modules::android_ctx;

/// Inside the app's internal data directory, the one Java's `getFilesDir()`
/// returns too.
const DIR_NAME: &str = "images";

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Must be called once from `android_main` with the app's internal data
/// directory (`AndroidApp::internal_data_path`).
pub fn init(data_dir: Option<PathBuf>) -> Result<()> {
    let dir = data_dir
        .ok_or("the app has no internal data path")?
        .join(DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    DIR.set(dir).ok();
    Ok(())
}

fn path(file: &str) -> Result<PathBuf> {
    let dir = DIR.get().ok_or("images::init must be called before use")?;
    Ok(dir.join(file))
}

/// Decodes an image of the images directory.
pub fn load(file: &str) -> Result<slint::Image> {
    let path = path(file)?;
    slint::Image::load_from_path(&path)
        .map_err(|_| format!("can't load the image {}", path.display()).into())
}

/// Deletes an image of the images directory (it was removed from Home).
pub fn delete(file: &str) -> Result<()> {
    match std::fs::remove_file(path(file)?) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// Opens the system image picker (`HomeActivity.pickImage`). The picked image
/// arrives later, through `nativeOnImagePicked`.
pub fn pick() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "pickImage", "()V", &[])?;
        Ok(())
    })
}
