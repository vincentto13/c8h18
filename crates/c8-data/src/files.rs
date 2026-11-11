//! The game's files: the current asset package (a directory or a `.c8pak` bundle, see
//! [`crate::package`]), looked up case-insensitively, plus an optional override folder.
use crate::package::Package;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

static PACKAGE: RwLock<Option<Arc<Package>>> = RwLock::new(None);
static OVERRIDE_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Use `p` (a directory or a bundle) as the asset package.
pub fn set_asset(p: &Path) -> Result<(), String> {
    set_package(Package::open(p)?);
    Ok(())
}

pub fn set_package(p: Package) {
    *PACKAGE.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(p));
}

/// Like [`set_asset`], but a path that cannot be opened still becomes the (empty) package, so
/// every later lookup fails with a warning instead of the program stopping.
pub fn set_data_dir(p: PathBuf) {
    set_package(Package::open(&p).unwrap_or_else(|_| Package::dir(p)));
}

/// The current package (default: the directory `resources`).
pub fn package() -> Arc<Package> {
    if let Some(p) = PACKAGE.read().unwrap_or_else(|e| e.into_inner()).clone() {
        return p;
    }
    let mut w = PACKAGE.write().unwrap_or_else(|e| e.into_inner());
    w.get_or_insert_with(|| Arc::new(Package::dir(PathBuf::from("resources")))).clone()
}

/// The package's path: its directory or bundle file.
pub fn data_dir() -> PathBuf {
    package().path().to_path_buf()
}

/// Port addition: a folder whose files take precedence over the package's (same layout,
/// e.g. `<dir>/DATA/TEXTU0-0.DAT`). The editor uses it to try edited files in the game.
pub fn set_override_dir(p: Option<PathBuf>) {
    *OVERRIDE_DIR.write().unwrap_or_else(|e| e.into_inner()) = p;
}

fn override_dir() -> Option<PathBuf> {
    OVERRIDE_DIR.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The local file of `rel` (e.g. "data/palet0-0.dat"), case-insensitively, in the override
/// folder first and then in a directory package. `None` for files inside a bundle: use
/// [`exists`] and [`read_raw`] to work with any package.
pub fn resolve(rel: &str) -> Option<PathBuf> {
    override_dir().and_then(|o| resolve_in(o, rel)).or_else(|| package().local_path(rel))
}

/// Is `rel` a file in the override folder or the package?
pub fn exists(rel: &str) -> bool {
    override_dir().and_then(|o| resolve_in(o, rel)).is_some_and(|p| p.is_file()) || package().exists(rel)
}

/// Resolve `rel` case-insensitively under `root` only (no override folder).
pub fn resolve_in(root: PathBuf, rel: &str) -> Option<PathBuf> {
    let mut cur = root;
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        if part == ".." {
            return None;
        }
        let direct = cur.join(part);
        if direct.exists() {
            cur = direct;
            continue;
        }
        let lower = part.to_ascii_lowercase();
        let mut found = None;
        for e in std::fs::read_dir(&cur).ok()?.flatten() {
            if e.file_name().to_string_lossy().to_ascii_lowercase() == lower {
                found = Some(e.path());
                break;
            }
        }
        cur = found?;
    }
    Some(cur)
}

/// Read a file raw (override folder first, then the package).
pub fn read_raw(rel: &str) -> Result<Vec<u8>, String> {
    if let Some(p) = override_dir().and_then(|o| resolve_in(o, rel)).filter(|p| p.is_file()) {
        return std::fs::read(&p).map_err(|e| format!("{rel}: {e}"));
    }
    package().read(rel)
}

/// Read a file and unpack it if RNC compressed.
pub fn load(rel: &str) -> Result<Vec<u8>, String> {
    let d = read_raw(rel)?;
    super::rnc::maybe_unpack(d).map_err(|e| format!("ERROR decompressing {rel}: {e}"))
}

/// How a file is named in warnings: as the original set names it (upper case; `lang/` files
/// lower case).
pub fn asset_name(rel: &str) -> String {
    let r = rel.replace('\\', "/");
    if r.to_ascii_lowercase().starts_with("lang/") { r.to_ascii_lowercase() } else { r.to_ascii_uppercase() }
}

/// Print a warning about asset `rel` on stdout, once per file and run:
/// `warning: missing asset DATA/SKY0-0.DAT: drawing a plain sky`. A file that exists but cannot
/// be used gets `unusable asset NAME (error)` instead. Returns whether it was printed.
pub fn warn_asset(rel: &str, error: Option<&str>, fallback: &str) -> bool {
    let name = asset_name(rel);
    let mut w = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if !w.get_or_insert_with(HashSet::new).insert(name.clone()) {
        return false;
    }
    let msg = match error {
        Some(e) if exists(rel) => format!("warning: unusable asset {name} ({e}): {fallback}"),
        _ => format!("warning: missing asset {name}: {fallback}"),
    };
    println!("{msg}");
    log::debug!("{msg}");
    true
}

/// The files warned about so far in this run (names as in [`asset_name`]).
pub fn warned() -> Vec<String> {
    let mut v: Vec<String> = WARNED.lock().unwrap_or_else(|e| e.into_inner()).iter().flatten().cloned().collect();
    v.sort();
    v
}

/// [`load`], or `None` after a warning naming the fallback (see [`warn_asset`]).
pub fn load_or_warn(rel: &str, fallback: &str) -> Option<Vec<u8>> {
    load(rel).map_err(|e| warn_asset(rel, Some(&e), fallback)).ok()
}

/// [`read_raw`], or `None` after a warning naming the fallback.
pub fn read_or_warn(rel: &str, fallback: &str) -> Option<Vec<u8>> {
    read_raw(rel).map_err(|e| warn_asset(rel, Some(&e), fallback)).ok()
}

/// Writable directory for config, saves and settings (created if needed); see
/// [`Package::save_dir`]. Never inside a bundle.
pub fn save_dir() -> PathBuf {
    let p = package().save_dir();
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
pub fn i16le(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}
pub fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
pub fn i32le(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_dir_wins() {
        let d = std::env::temp_dir().join(format!("c8-override-{}", std::process::id()));
        std::fs::create_dir_all(d.join("DATA")).unwrap();
        std::fs::write(d.join("DATA/ZZTEST.DAT"), b"over").unwrap();
        set_override_dir(Some(d.clone()));
        // only files present in the override folder come from it
        assert_eq!(read_raw("data/zztest.dat").unwrap(), b"over");
        set_override_dir(None);
        assert!(read_raw("data/zztest.dat").is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn warnings_are_printed_once_per_file() {
        assert!(warn_asset("data/zzmissing0-0.dat", None, "testing"));
        assert!(!warn_asset("DATA/ZZMISSING0-0.DAT", Some("again"), "testing"));
        assert!(warned().contains(&"DATA/ZZMISSING0-0.DAT".to_string()));
        assert!(load_or_warn("data/zzmissing0-1.dat", "testing").is_none());
        assert!(warned().contains(&"DATA/ZZMISSING0-1.DAT".to_string()));
        assert_eq!(asset_name("lang/EN.txt"), "lang/en.txt");
    }
}
