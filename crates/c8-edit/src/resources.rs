//! The asset package (a game data folder or a `.c8pak` bundle) as a list of resources,
//! grouped by kind. A bundle's files are unpacked into a temporary mirror folder, which the
//! documents open and save like those of a folder.
use std::path::{Path, PathBuf};

/// Resource kinds of the game's data files (spec 01). Each kind gets its own document type; the
/// ones without an editor yet open in the file viewer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Level,
    TerrainTextures,
    Sky,
    ColourTable,
    Palette,
    Font,
    Sprites,
    Image,
    Billboards,
    ModelTextures,
    Model,
    Sound,
    Strings,
    SearchRings,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Level => "Levels",
            Kind::TerrainTextures => "Terrain textures",
            Kind::Sky => "Skies",
            Kind::ColourTable => "Colour tables",
            Kind::Palette => "Palettes",
            Kind::Font => "Fonts",
            Kind::Sprites => "Sprite sheets",
            Kind::Image => "Screens and pictures",
            Kind::Billboards => "Billboards",
            Kind::ModelTextures => "Model textures",
            Kind::Model => "Models",
            Kind::Sound => "Sound and music",
            Kind::Strings => "Languages",
            Kind::SearchRings => "Explosion rings",
            Kind::Other => "Other",
        }
    }
    /// Classify a file by its name (relative to the data folder, any case).
    pub fn of(rel: &str) -> Kind {
        let r = rel.to_ascii_lowercase().replace('\\', "/");
        let name = r.rsplit('/').next().unwrap_or(&r);
        let dir = r.split('/').next().unwrap_or("");
        if dir == "lang" {
            return Kind::Strings;
        }
        if dir == "objects" {
            return if name.starts_with("tex") { Kind::ModelTextures } else { Kind::Model };
        }
        // a bank outside the data folder (opened by its file name), e.g. assets/custom/SOUND/MUSIC.DAT
        if r == name && (name == "sound.dat" || name == "music.dat") {
            return Kind::Sound;
        }
        if dir == "sound" {
            // the containers; the rest are the original's sound drivers and timbre banks
            return if name == "sound.dat" || name == "music.dat" { Kind::Sound } else { Kind::Other };
        }
        if name.starts_with("level") && name.ends_with(".dat") {
            return Kind::Level;
        }
        let stem = name.split('.').next().unwrap_or("");
        let base: String = stem.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        match base.as_str() {
            "textu" => Kind::TerrainTextures,
            "sky" => Kind::Sky,
            "table" => Kind::ColourTable,
            "palet" => Kind::Palette,
            "hfont" | "mfont" | "olfnt" | "osfnt" | "pfont" => Kind::Font,
            "hspr" | "mspr" | "logo" | "panel" | "point" | "mini" | "track" => Kind::Sprites,
            "oscr" | "onet" | "title" | "puzzle" => Kind::Image,
            "tmaps" => Kind::Billboards,
            "srch" => Kind::SearchRings,
            _ => Kind::Other,
        }
    }
}

pub struct Entry {
    pub kind: Kind,
    /// path relative to the data folder, as shown
    pub rel: String,
    pub path: PathBuf,
}

/// All files in the data folder's subfolders (up to three levels), sorted by kind and name. `.TAB`
/// index files are listed with their `.DAT`, not on their own.
pub fn scan(root: &Path) -> Vec<Entry> {
    fn visit(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<Entry>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if p.is_dir() {
                if depth < 3 && !(depth == 0 && name.eq_ignore_ascii_case("save")) {
                    visit(&p, &rel, depth + 1, out);
                }
            } else if depth > 0 && !name.to_ascii_lowercase().ends_with(".tab") {
                out.push(Entry { kind: Kind::of(&rel), rel, path: p });
            }
        }
    }
    let mut out = Vec::new();
    visit(root, "", 0, &mut out);
    out.sort_by(|a, b| (a.kind, a.rel.to_ascii_lowercase()).cmp(&(b.kind, b.rel.to_ascii_lowercase())));
    out
}

/// Does this look like an asset package (a directory or a `.c8pak` bundle with `MAPS/`,
/// `DATA/` or `OBJECTS/` in the original layout)? Makes it the current package when it does.
/// Packages may be partial: a missing palette and the like only print warnings later.
pub fn is_data_dir(p: &Path) -> bool {
    let Some(pkg) = probe(p) else { return false };
    c8_data::files::set_package(pkg);
    true
}

/// The package at `p` if it looks like one (see [`is_data_dir`]), without making it current.
pub fn probe(p: &Path) -> Option<c8_data::package::Package> {
    let pkg = c8_data::package::Package::open(p).ok()?;
    ["maps", "data", "objects"].iter().any(|d| pkg.has_dir(d)).then_some(pkg)
}

/// The folder a bundle is unpacked into for the editor's documents (they open and save local
/// files): `<temp>/c8h18-edit-bundles/<stem>-<hash of the bundle path>`.
pub fn mirror_dir(bundle: &Path) -> PathBuf {
    let abs = bundle.canonicalize().unwrap_or_else(|_| bundle.to_path_buf());
    let hash = abs.to_string_lossy().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let stem = bundle.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    std::env::temp_dir().join("c8h18-edit-bundles").join(format!("{stem}-{hash:016x}"))
}

const STAMP: &str = ".bundle-stamp";

fn bundle_stamp(bundle: &Path) -> String {
    let m = std::fs::metadata(bundle).ok();
    let t = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{} {t}", m.map(|m| m.len()).unwrap_or(0))
}

/// Unpack `bundle` into its [mirror](mirror_dir) unless the mirror already holds this version
/// of it (same size and modification time). Returns the mirror folder.
pub fn refresh_mirror(bundle: &Path) -> Result<PathBuf, String> {
    let dir = mirror_dir(bundle);
    let stamp = bundle_stamp(bundle);
    if std::fs::read_to_string(dir.join(STAMP)).is_ok_and(|s| s == stamp) {
        return Ok(dir);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    c8_data::package::unpack(bundle, &dir)?;
    std::fs::write(dir.join(STAMP), stamp).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// Write the mirror (with the documents saved into it) back over its bundle, atomically (a
/// temporary file renamed over the bundle), and mark the mirror as up to date. Returns the
/// number of files. The caller reopens the package (an open archive still reads the old file).
pub fn repack_mirror(bundle: &Path) -> Result<usize, String> {
    let dir = mirror_dir(bundle);
    let n = c8_data::package::pack(&dir, bundle)?;
    let _ = std::fs::write(dir.join(STAMP), bundle_stamp(bundle));
    Ok(n)
}

/// The local file of `rel` in the current package: in a directory package the file itself, in a
/// bundle its copy in the mirror.
pub fn local_file(rel: &str) -> Option<PathBuf> {
    let pkg = c8_data::files::package();
    if pkg.is_bundle() {
        let dir = refresh_mirror(pkg.path()).map_err(|e| log::warn!("{e}")).ok()?;
        c8_data::files::resolve_in(dir, rel).filter(|p| p.is_file())
    } else {
        c8_data::files::resolve(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_lists_and_opens_like_a_folder() {
        let base = std::env::temp_dir().join(format!("c8-edit-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("pkg");
        for f in ["DATA/PALET0-0.DAT", "MAPS/LEVEL0-1.DAT", "DATA/PANEL0-1.DAT", "DATA/PANEL0-1.TAB"] {
            std::fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
            std::fs::write(src.join(f), f).unwrap();
        }
        let bundle = base.join("test.c8pak");
        c8_data::package::pack(&src, &bundle).unwrap();
        assert!(probe(&bundle).is_some_and(|p| p.is_bundle()));
        assert!(probe(&base).is_none(), "not a package");
        let mirror = refresh_mirror(&bundle).unwrap();
        let names: Vec<String> = scan(&mirror).into_iter().map(|e| e.rel).collect();
        let dir_names: Vec<String> = scan(&src).into_iter().map(|e| e.rel).collect();
        assert_eq!(names, dir_names);
        assert_eq!(std::fs::read(mirror.join("MAPS/LEVEL0-1.DAT")).unwrap(), b"MAPS/LEVEL0-1.DAT");
        // save a changed file into the mirror and write the bundle back
        std::fs::write(mirror.join("MAPS/LEVEL0-1.DAT"), b"changed").unwrap();
        assert_eq!(repack_mirror(&bundle).unwrap(), 4);
        assert_eq!(c8_data::package::Package::open(&bundle).unwrap().read("maps/level0-1.dat").unwrap(), b"changed");
        assert_eq!(refresh_mirror(&bundle).unwrap(), mirror, "still up to date");
        assert!(mirror.join("MAPS/LEVEL0-1.DAT").is_file());
        let _ = std::fs::remove_dir_all(&mirror);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn classify() {
        assert_eq!(Kind::of("MAPS/LEVEL0-3.DAT"), Kind::Level);
        assert_eq!(Kind::of("DATA/TEXTU0-2.DAT"), Kind::TerrainTextures);
        assert_eq!(Kind::of("DATA/OLFNT0-1.DAT"), Kind::Font);
        assert_eq!(Kind::of("DATA/PALET0-0.DAT"), Kind::Palette);
        assert_eq!(Kind::of("lang/pl.txt"), Kind::Strings);
        assert_eq!(Kind::of("SOUND/SOUND.DAT"), Kind::Sound);
        assert_eq!(Kind::of("SOUND/MUSIC.DAT"), Kind::Sound);
        assert_eq!(Kind::of("MUSIC.DAT"), Kind::Sound);
        assert_eq!(Kind::of("SOUND/SB16.DIG"), Kind::Other);
        assert_eq!(Kind::of("DATA/TMAPS.DAT"), Kind::Billboards);
        assert_eq!(Kind::of("DATA/SRCH0-0.DAT"), Kind::SearchRings);
        assert_eq!(Kind::of("DATA/OSCR0-1.DAT"), Kind::Image);
        assert_eq!(Kind::of("OBJECTS/DATA/TEX0-0.DAT"), Kind::ModelTextures);
        assert_eq!(Kind::of("OBJECTS/DATA/CAR.DAT"), Kind::Model);
    }
}
