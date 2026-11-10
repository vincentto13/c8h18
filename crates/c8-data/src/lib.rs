//! c8-data: the game's data file formats (spec chapter 01), shared by the game and the editor.
//!
//! Each module reads one family of files; writers sit next to the readers as they are added
//! (levels first), so that anything the editor saves loads in the game.
pub mod rnc;
pub mod files;
pub mod sprites;
pub mod images;
pub mod model;
pub mod level;
pub mod tmaps;
pub mod config;
pub mod strings;
pub mod search;
