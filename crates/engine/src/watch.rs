//! Reload on change: a background thread watches the folders of the
//! loaded third-party addons and raises a flag when a `.lua`, `.xml` or
//! `.toc` file changes, appears or goes away. The engine reloads the UI
//! on its next tick, as an addon author would type `/reload` in the game.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// How often the folders are scanned.
const INTERVAL: Duration = Duration::from_millis(500);

pub struct Watcher {
    changed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Watcher {
    pub fn start(dirs: Vec<PathBuf>) -> Watcher {
        let changed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (c, s) = (changed.clone(), stop.clone());
        // The first scan happens now, so an edit right after starting
        // counts as a change.
        let mut last = signature(&dirs);
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                std::thread::sleep(INTERVAL);
                let now = signature(&dirs);
                if now != last {
                    last = now;
                    c.store(true, Ordering::Relaxed);
                }
            }
        });
        Watcher { changed, stop }
    }

    /// Whether a file changed since the last call.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The watched files and their modification times, sorted.
pub fn signature(dirs: &[PathBuf]) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for d in dirs {
        collect(d, &mut out, 0);
    }
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Hidden folders (.git) and packager output (.release) are not
        // part of the addon.
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect(&path, out, depth + 1);
        } else {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".lua") || lower.ends_with(".xml") || lower.ends_with(".toc") {
                out.push((path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_a_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("A.lua"), "x = 1").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git").join("B.lua"), "").unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        let before = signature(&dirs);
        assert_eq!(before.len(), 1, "hidden folders are skipped");
        std::fs::write(dir.path().join("New.xml"), "<Ui/>").unwrap();
        assert_ne!(signature(&dirs), before);
    }
}
