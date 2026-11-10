//! Documents: one per opened file. Every resource kind implements [`Document`]; the app only
//! deals with this trait, so adding an editor for textures, fonts etc. means adding a type here.
pub mod history;
pub mod image;
pub mod lang;
pub mod level;
pub mod palette;
pub mod pixels;
pub mod search;
pub mod sound;
pub mod table;
pub mod viewer;

use crate::resources::Kind;
use std::path::{Path, PathBuf};

pub trait Document {
    fn title(&self) -> String;
    fn kind(&self) -> Kind;
    fn path(&self) -> Option<&Path>;
    /// Draw the document (central area) and its side panel content.
    fn ui(&mut self, ui: &mut eframe::egui::Ui);
    /// Inspector / tool options, drawn in the right-hand panel.
    fn side_ui(&mut self, _ui: &mut eframe::egui::Ui) {}
    fn dirty(&self) -> bool {
        false
    }
    fn can_save(&self) -> bool {
        false
    }
    fn save_to(&mut self, _path: &Path) -> Result<(), String> {
        Err("this resource kind cannot be saved yet".into())
    }
    fn undo(&mut self) {}
    fn redo(&mut self) {}
    fn can_undo(&self) -> bool {
        false
    }
    fn can_redo(&self) -> bool {
        false
    }
    /// Files to put in the game's override folder for "Test in game": (path relative to the data
    /// folder, contents). The game loads them instead of the originals.
    fn game_files(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        Ok(Vec::new())
    }
    /// Game arguments that show this document's content (e.g. a race on its track).
    fn game_args(&self) -> Vec<String> {
        vec!["--no-intro".into()]
    }
    /// Polled each frame: did the user press the document's "Test in game" button?
    fn wants_test(&mut self) -> bool {
        false
    }
    /// The level document behind this one, for MCP.
    fn as_level(&mut self) -> Option<&mut level::LevelDoc> {
        None
    }
}

/// Open a file as the document type for its kind.
pub fn open(path: PathBuf, kind: Kind, rel: String) -> Result<Box<dyn Document>, String> {
    Ok(match kind {
        Kind::Level => Box::new(level::LevelDoc::open(path, rel)?),
        Kind::Palette => Box::new(palette::PaletteDoc::open(path, rel)?),
        Kind::Strings => Box::new(lang::LangDoc::open(path, rel)?),
        Kind::ColourTable => Box::new(table::TableDoc::open(path, rel)?),
        Kind::Sound => Box::new(sound::SoundDoc::open(path, rel)?),
        Kind::SearchRings => Box::new(search::SearchDoc::open(path, rel)?),
        Kind::TerrainTextures | Kind::Sky | Kind::Image | Kind::ModelTextures | Kind::Sprites | Kind::Font | Kind::Billboards => Box::new(image::ImageDoc::open(path, kind, rel)?),
        _ => Box::new(viewer::Viewer::open(path, kind, rel)?),
    })
}
