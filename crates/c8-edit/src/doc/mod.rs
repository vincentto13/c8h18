//! Documents: one per opened file. Every resource kind implements [`Document`]; the app only
//! deals with this trait, so adding an editor for textures, fonts etc. means adding a type here.
pub mod history;
pub mod level;
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
}

/// Open a file as the document type for its kind.
pub fn open(path: PathBuf, kind: Kind, rel: String) -> Result<Box<dyn Document>, String> {
    Ok(match kind {
        Kind::Level => Box::new(level::LevelDoc::open(path, rel)?),
        _ => Box::new(viewer::Viewer::open(path, kind, rel)?),
    })
}
