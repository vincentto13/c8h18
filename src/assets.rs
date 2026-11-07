//! Assets loaded at start (640x480 sets only; low resolution dropped).
use crate::data::images::{load_search_rings, ColourTable, Image, Palette, TexAtlas};
use crate::data::model::{load_all_models, Model};
use crate::data::sprites::{Font, SpriteSheet};
use crate::data::tmaps::{load_tmaps, TmapImage};
use std::rc::Rc;

pub struct Assets {
    pub palette: Palette,
    /// font slots 0..4: HFONT0-0, MFONT0-0, OLFNT0-1, OSFNT0-1, PFONT0-1
    pub fonts: Vec<Font>,
    pub panel: SpriteSheet,
    pub hspr: SpriteSheet,
    pub logo: SpriteSheet,
    pub point: SpriteSheet,
    pub track: SpriteSheet,
    pub oscr: Image,
    pub onet: Image,
    pub title: Image,
    pub puzzle: Image,
    pub models: Vec<Model>,
    pub atlas: TexAtlas,
    pub tmaps: Vec<TmapImage>,
    pub rings: Vec<Vec<(i32, i32)>>,
    /// front-end colour table (port: always table0-0)
    pub fe_table: Rc<ColourTable>,
    pub cube: Vec<u8>,
}

impl Assets {
    pub fn load() -> Result<Assets, String> {
        let palette = Palette::load()?;
        let fonts = vec![
            Font::load("hfont0-0", false)?,
            Font::load("mfont0-0", false)?,
            Font::load("olfnt0-1", true)?,
            Font::load("osfnt0-1", true)?,
            Font::load("pfont0-1", false)?,
        ];
        let cube = palette.colour_cube();
        Ok(Assets {
            fonts,
            panel: SpriteSheet::load("panel0-1")?,
            hspr: SpriteSheet::load("hspr0-0")?,
            logo: SpriteSheet::load("logo0-1")?,
            point: SpriteSheet::load("point0-0")?,
            track: SpriteSheet::load("track0-1")?,
            oscr: Image::load_raw("data/oscr0-1.dat", 640, 480)?,
            onet: Image::load_raw("data/onet0-1.dat", 640, 480)?,
            title: Image::load_raw("data/title.dat", 320, 200)?,
            puzzle: Image::load_raw("data/puzzle.dat", 112, 96)?,
            models: load_all_models()?,
            atlas: TexAtlas::load()?,
            tmaps: load_tmaps()?,
            rings: load_search_rings()?,
            fe_table: Rc::new(ColourTable::load(0)?),
            palette,
            cube,
        })
    }
}
