//! Asset packages (port addition): the game's files in the original layout (`DATA/`, `MAPS/`,
//! `OBJECTS/DATA/`, `SOUND/`, `lang/`), either as a directory or as a `.c8pak` bundle, a zip
//! file holding the same layout at its root.
//!
//! Lookups are case-insensitive (the original files are upper case, the code asks in lower case)
//! and take `/`-separated paths relative to the package root. A bundle is read-only: its central
//! directory is read once when it is opened, entries are decompressed on demand.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// File extension of asset bundles.
pub const BUNDLE_EXT: &str = "c8pak";

pub struct Package {
    path: PathBuf,
    src: Source,
}

enum Source {
    Dir,
    Bundle(Bundle),
}

struct Bundle {
    zip: Mutex<zip::ZipArchive<std::fs::File>>,
    /// lower-case path -> (path as stored, entry index)
    names: BTreeMap<String, (String, usize)>,
}

/// A safe, normalised relative path: `/` separators, no empty, `.` or `..` parts, not absolute.
fn clean_rel(rel: &str) -> Option<String> {
    let r = rel.replace('\\', "/");
    if r.starts_with('/') || r.contains(':') {
        return None;
    }
    let parts: Vec<&str> = r.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    Some(parts.join("/"))
}

impl Package {
    /// A directory package (not checked).
    pub fn dir(path: PathBuf) -> Package {
        Package { path, src: Source::Dir }
    }

    /// Open a directory or a bundle (any regular file is read as a zip).
    pub fn open(path: &Path) -> Result<Package, String> {
        if path.is_dir() {
            return Ok(Package::dir(path.to_path_buf()));
        }
        if !path.is_file() {
            return Err(format!("{}: no such directory or bundle", path.display()));
        }
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: not a .{BUNDLE_EXT} bundle (zip): {e}", path.display()))?;
        let mut names = BTreeMap::new();
        for i in 0..zip.len() {
            let Some(Ok(name)) = zip.name_for_index(i) else { continue };
            if name.ends_with('/') || name.ends_with('\\') {
                continue; // directory entry
            }
            match clean_rel(&name) {
                Some(rel) => {
                    names.entry(rel.to_ascii_lowercase()).or_insert((rel, i));
                }
                _ => println!("warning: {}: ignoring unsafe entry {name:?} (absolute path or ..)", path.display()),
            }
        }
        Ok(Package { path: path.to_path_buf(), src: Source::Bundle(Bundle { zip: Mutex::new(zip), names }) })
    }

    /// The directory or bundle file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_bundle(&self) -> bool {
        matches!(self.src, Source::Bundle(_))
    }

    /// The directory of a directory package.
    pub fn dir_path(&self) -> Option<&Path> {
        if self.is_bundle() { None } else { Some(&self.path) }
    }

    /// The path of `rel` as stored in the package (its real letter case), if it is a file there.
    pub fn find(&self, rel: &str) -> Option<String> {
        let rel = clean_rel(rel)?;
        match &self.src {
            Source::Dir => {
                let p = crate::files::resolve_in(self.path.clone(), &rel).filter(|p| p.is_file())?;
                let r = p.strip_prefix(&self.path).ok()?;
                Some(r.to_string_lossy().replace('\\', "/"))
            }
            Source::Bundle(b) => b.names.get(&rel.to_ascii_lowercase()).map(|(n, _)| n.clone()),
        }
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.find(rel).is_some()
    }

    /// Does the package have a folder `rel` (any case)?
    pub fn has_dir(&self, rel: &str) -> bool {
        let Some(rel) = clean_rel(rel) else { return false };
        match &self.src {
            Source::Dir => crate::files::resolve_in(self.path.clone(), &rel).is_some_and(|p| p.is_dir()),
            Source::Bundle(b) => {
                let pre = format!("{}/", rel.to_ascii_lowercase());
                b.names.range(pre.clone()..).next().is_some_and(|(k, _)| k.starts_with(&pre))
            }
        }
    }

    /// The local file of `rel` in a directory package.
    pub fn local_path(&self, rel: &str) -> Option<PathBuf> {
        match &self.src {
            Source::Dir => crate::files::resolve_in(self.path.clone(), &clean_rel(rel)?),
            Source::Bundle(_) => None,
        }
    }

    /// Read a file's bytes (as stored, not RNC unpacked).
    pub fn read(&self, rel: &str) -> Result<Vec<u8>, String> {
        let real = self.find(rel).ok_or_else(|| format!("missing data file {rel}"))?;
        match &self.src {
            Source::Dir => std::fs::read(self.path.join(&real)).map_err(|e| format!("{rel}: {e}")),
            Source::Bundle(b) => {
                let i = b.names[&real.to_ascii_lowercase()].1;
                let mut zip = b.zip.lock().unwrap_or_else(|e| e.into_inner());
                let mut f = zip.by_index(i).map_err(|e| format!("{rel} in {}: {e}", self.path.display()))?;
                let mut out = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut out).map_err(|e| format!("{rel} in {}: {e}", self.path.display()))?;
                Ok(out)
            }
        }
    }

    /// Every file of the package (paths as stored, `/`-separated), sorted. A directory package
    /// leaves out hidden files and its `save/` folder.
    pub fn list(&self) -> Vec<String> {
        match &self.src {
            Source::Dir => {
                let mut out = Vec::new();
                list_dir(&self.path, "", &mut out);
                out.sort();
                out
            }
            Source::Bundle(b) => {
                let mut v: Vec<String> = b.names.values().map(|(n, _)| n.clone()).collect();
                v.sort();
                v
            }
        }
    }

    /// The writable folder for `config.dat`, saves, key and video settings: `save/` inside a
    /// directory package; for a bundle `<user data>/c8h18/<bundle file stem>/save` (see
    /// [`user_data_dir`]). `C8H18_SAVE_DIR` replaces both. Never inside a bundle.
    pub fn save_dir(&self) -> PathBuf {
        if let Some(d) = std::env::var_os("C8H18_SAVE_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(d);
        }
        match &self.src {
            Source::Dir => self.path.join("save"),
            Source::Bundle(_) => {
                let stem = self.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "bundle".into());
                user_data_dir().join(stem).join("save")
            }
        }
    }
}

fn list_dir(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let p = e.path();
        if p.is_dir() {
            if !(prefix.is_empty() && name.eq_ignore_ascii_case("save")) {
                list_dir(&p, &rel, out);
            }
        } else if p.is_file() {
            out.push(rel);
        }
    }
}

/// The per-user folder for writable data of bundles: `~/Library/Application Support/c8h18`
/// on macOS, `%APPDATA%\c8h18` on Windows, `$XDG_DATA_HOME/c8h18` (default
/// `~/.local/share/c8h18`) elsewhere.
pub fn user_data_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/c8h18")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("c8h18")
    } else {
        std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("c8h18")
    }
}

/// Is `p` a bundle file name (`.c8pak`, any case)?
pub fn is_bundle_name(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(BUNDLE_EXT))
}

/// Pack a directory package into a bundle (deflate), leaving out hidden files and `save/`.
/// Written to a temporary file next to `out` and renamed over it, so an existing bundle is
/// replaced atomically. Returns the number of files.
pub fn pack(dir: &Path, out: &Path) -> Result<usize, String> {
    if !dir.is_dir() {
        return Err(format!("{}: not a directory", dir.display()));
    }
    let src = Package::dir(dir.to_path_buf());
    let files = src.list();
    write_bundle(out, files.iter().map(|rel| (rel.clone(), src.read(rel))))?;
    Ok(files.len())
}

/// Write a bundle from (path, bytes) pairs, atomically (temporary file, then rename).
pub fn write_bundle(out: &Path, files: impl Iterator<Item = (String, Result<Vec<u8>, String>)>) -> Result<(), String> {
    let name = out.file_name().ok_or_else(|| format!("{}: no file name", out.display()))?;
    let tmp = out.with_file_name(format!(".{}.tmp{}", name.to_string_lossy(), std::process::id()));
    let res = (|| -> Result<(), String> {
        let f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let mut w = zip::ZipWriter::new(std::io::BufWriter::new(f));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o644);
        for (rel, bytes) in files {
            let rel = clean_rel(&rel).ok_or_else(|| format!("unsafe path {rel:?}"))?;
            let bytes = bytes?;
            w.start_file(rel.as_str(), opts).map_err(|e| format!("{rel}: {e}"))?;
            w.write_all(&bytes).map_err(|e| format!("{rel}: {e}"))?;
        }
        let mut bw = w.finish().map_err(|e| format!("{}: {e}", tmp.display()))?;
        bw.flush().map_err(|e| format!("{}: {e}", tmp.display()))?;
        let f = bw.into_inner().map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.sync_all().map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, out).map_err(|e| format!("{}: {e}", out.display()))
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Unpack a bundle into a directory (created if needed; existing files are overwritten).
/// Returns the number of files.
pub fn unpack(bundle: &Path, dir: &Path) -> Result<usize, String> {
    let p = Package::open(bundle)?;
    if !p.is_bundle() {
        return Err(format!("{}: not a bundle file", bundle.display()));
    }
    let files = p.list();
    for rel in &files {
        let bytes = p.read(rel)?;
        let out = dir.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("c8-pkg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn pack_open_and_read() {
        let d = temp("src");
        for (f, b) in [("DATA/PALET0-0.DAT", &b"pal"[..]), ("MAPS/LEVEL0-1.DAT", b"lvl"), ("lang/en.txt", b"hello"), ("save/config.dat", b"cfg"), ("OBJECTS/DATA/CAR0-0.DAT", b"car")] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), b).unwrap();
        }
        let out = temp("out").with_extension(BUNDLE_EXT);
        assert_eq!(pack(&d, &out).unwrap(), 4, "save/ is left out");
        let p = Package::open(&out).unwrap();
        assert!(p.is_bundle());
        assert_eq!(p.list(), ["DATA/PALET0-0.DAT", "MAPS/LEVEL0-1.DAT", "OBJECTS/DATA/CAR0-0.DAT", "lang/en.txt"]);
        assert_eq!(p.read("data/palet0-0.dat").unwrap(), b"pal");
        assert_eq!(p.read("objects/data/car0-0.dat").unwrap(), b"car");
        assert_eq!(p.find("maps/level0-1.dat").as_deref(), Some("MAPS/LEVEL0-1.DAT"));
        assert!(p.has_dir("maps") && p.has_dir("OBJECTS/data") && !p.has_dir("sound"));
        assert!(p.read("save/config.dat").is_err());
        assert!(p.read("../DATA/PALET0-0.DAT").is_err());
        assert!(!p.save_dir().starts_with(&out), "saves never go into the bundle");
        // the directory reads the same
        let dp = Package::open(&d).unwrap();
        assert_eq!(dp.list(), p.list());
        assert_eq!(dp.read("DATA/palet0-0.DAT").unwrap(), b"pal");
        // and unpacks to the same files
        let back = temp("back");
        assert_eq!(unpack(&out, &back).unwrap(), 4);
        assert_eq!(std::fs::read(back.join("lang/en.txt")).unwrap(), b"hello");
        // repacking replaces the bundle
        std::fs::write(d.join("DATA/PALET0-0.DAT"), b"new").unwrap();
        pack(&d, &out).unwrap();
        assert_eq!(Package::open(&out).unwrap().read("DATA/PALET0-0.DAT").unwrap(), b"new");
        for x in [&d, &back] {
            let _ = std::fs::remove_dir_all(x);
        }
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn unsafe_entries_are_ignored() {
        let out = temp("evil").with_extension(BUNDLE_EXT);
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for n in ["../escape.dat", "/abs.dat", "DATA/../../x.dat", "DATA/OK.DAT"] {
                w.start_file(n, o).unwrap();
                w.write_all(b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let p = Package::open(&out).unwrap();
        assert_eq!(p.list(), ["DATA/OK.DAT"]);
        let back = temp("evil-out");
        unpack(&out, &back).unwrap();
        assert!(back.join("DATA/OK.DAT").is_file());
        let _ = std::fs::remove_dir_all(&back);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn clean_paths() {
        assert_eq!(clean_rel("data//x.dat").as_deref(), Some("data/x.dat"));
        assert_eq!(clean_rel("./data\\x.dat").as_deref(), Some("data/x.dat"));
        assert!(clean_rel("../x").is_none() && clean_rel("/x").is_none() && clean_rel("C:/x").is_none() && clean_rel("").is_none());
    }
}
