//! Case-insensitive data directory access.
use std::path::PathBuf;
use std::sync::RwLock;

static DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
static OVERRIDE_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set the data directory (default `resources`). Can be changed later, e.g. when the editor opens
/// another game folder.
pub fn set_data_dir(p: PathBuf) {
    *DATA_DIR.write().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

pub fn data_dir() -> PathBuf {
    DATA_DIR.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| PathBuf::from("resources"))
}

/// Port addition: a folder whose files take precedence over the data directory's (same layout,
/// e.g. `<dir>/DATA/TEXTU0-0.DAT`). The editor uses it to try edited files in the game.
pub fn set_override_dir(p: Option<PathBuf>) {
    *OVERRIDE_DIR.write().unwrap_or_else(|e| e.into_inner()) = p;
}

/// Resolve a relative path like "data/palet0-0.dat" case-insensitively, in the override folder
/// first and then in the data directory.
pub fn resolve(rel: &str) -> Option<PathBuf> {
    let over = OVERRIDE_DIR.read().unwrap_or_else(|e| e.into_inner()).clone();
    over.and_then(|o| resolve_in(o, rel)).or_else(|| resolve_in(data_dir(), rel))
}

/// Resolve `rel` case-insensitively under `root` only (no override folder).
pub fn resolve_in(root: PathBuf, rel: &str) -> Option<PathBuf> {
    let mut cur = root;
    for part in rel.split('/') {
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

/// Read a file raw.
pub fn read_raw(rel: &str) -> Result<Vec<u8>, String> {
    let p = resolve(rel).ok_or_else(|| format!("missing data file {rel}"))?;
    std::fs::read(&p).map_err(|e| format!("{rel}: {e}"))
}

/// Read a file and unpack it if RNC compressed.
pub fn load(rel: &str) -> Result<Vec<u8>, String> {
    let d = read_raw(rel)?;
    super::rnc::maybe_unpack(d).map_err(|e| format!("ERROR decompressing {rel}: {e}"))
}

/// Writable directory (save/) under the data directory.
pub fn save_dir() -> PathBuf {
    let p = data_dir().join("save");
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
}
