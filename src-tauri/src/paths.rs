// One place for the app's data folder. The folder carries the product name, so it is easy to find
// in %APPDATA%; settings and logs live outside the install directory and survive upgrades.
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The visible folder name in `%APPDATA%`.
pub const NAME: &str = "Windows Dynamic Island";
/// The old identifier-shaped folder written by 0.1.0 and earlier.
const OLD: &str = "dev.yassine808.islandwin";

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

pub fn data_dir() -> PathBuf {
    appdata().join(NAME)
}

fn copy_once(from: &Path, to: &Path, log: &mut Vec<String>) {
    if !from.is_file() || to.exists() {
        return;
    }
    if let Some(dir) = to.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Err(e) = fs::copy(from, to) {
        log.push(format!("could not copy {} -> {}: {e}", from.display(), to.display()));
    } else {
        log.push(format!("migrated {} -> {}", from.display(), to.display()));
    }
}

/// Carry the old settings and log over once. The old folder is left in place: a downgrade or a
/// second look at the history never loses anything. Returns the lines to log after the logger is
/// open (the migration runs before logger::init, so logging here would only reach stdout).
pub fn migrate() -> Vec<String> {
    let mut log = Vec::new();
    let new = data_dir();
    let old = appdata().join(OLD);
    if old.is_dir() {
        copy_once(&old.join("settings.json"), &new.join("settings.json"), &mut log);
        copy_once(&old.join("logs").join("islandwin.log"), &new.join("logs").join("islandwin.log"), &mut log);
    }
    // 0.1.0's predecessor kept the log inside the per-user install folder.
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        copy_once(&local.join("islandwin").join("islandwin.log"), &new.join("logs").join("islandwin.log"), &mut log);
    }
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_the_product_name() {
        assert!(data_dir().ends_with(NAME));
        assert_eq!(NAME, "Windows Dynamic Island");
    }
}
