//! The few files cb2026 keeps for network play, and how they are written.
//!
//! They live in one directory ([`default_dir`], or the one chosen with `--data-dir`) that only
//! its owner can enter, on systems with such permissions. Each write goes to a new file that is
//! then renamed over the old one, so a program reading the file, or a crash, never sees half of
//! it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Where cb2026 keeps its identity, approved clients and pairing window when no directory is
/// chosen: the per-user data directory of the system, in a folder named `cb2026`.
///
/// `None` if the system gives no home to put it in.
pub fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Application Support/cb2026"))
    } else if cfg!(windows) {
        var("APPDATA").map(|dir| dir.join("cb2026"))
    } else {
        var("XDG_DATA_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".local/share")))
            .map(|dir| dir.join("cb2026"))
    }
}

/// Writes `contents` to `path` so only the owner can read it: the directory made for it gets
/// mode 0700 and the file 0600 on Unix. (Windows keeps a user's application data from other
/// users already, and has no mode to set.) An existing file is replaced whole.
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir.filter(|d| !d.exists()) {
        fs::create_dir_all(dir)?;
        restrict(dir, 0o700)?;
    }
    let temporary = path.with_extension("tmp");
    // A file left by an earlier crash is not ours to append to.
    let _ = fs::remove_file(&temporary);
    let mut file = create_private(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn restrict(dir: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_dir: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// A new, empty directory for a test, removed by the operating system's clean-up of its
/// temporary directory.
#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "cb2026-net-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
