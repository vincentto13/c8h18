//! The game data folder as a list of resources, grouped by kind.
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
        if dir == "sound" {
            return Kind::Sound;
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

/// Does this look like a game data folder?
pub fn is_data_dir(p: &Path) -> bool {
    c8_data::files::set_data_dir(p.to_path_buf());
    c8_data::files::resolve("data/palet0-0.dat").is_some() && c8_data::files::resolve("maps").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(Kind::of("MAPS/LEVEL0-3.DAT"), Kind::Level);
        assert_eq!(Kind::of("DATA/TEXTU0-2.DAT"), Kind::TerrainTextures);
        assert_eq!(Kind::of("DATA/OLFNT0-1.DAT"), Kind::Font);
        assert_eq!(Kind::of("DATA/PALET0-0.DAT"), Kind::Palette);
        assert_eq!(Kind::of("lang/pl.txt"), Kind::Strings);
        assert_eq!(Kind::of("SOUND/SOUND.BNK"), Kind::Sound);
        assert_eq!(Kind::of("DATA/TMAPS.DAT"), Kind::Billboards);
        assert_eq!(Kind::of("DATA/OSCR0-1.DAT"), Kind::Image);
        assert_eq!(Kind::of("OBJECTS/DATA/TEX0-0.DAT"), Kind::ModelTextures);
        assert_eq!(Kind::of("OBJECTS/DATA/CAR.DAT"), Kind::Model);
    }
}
