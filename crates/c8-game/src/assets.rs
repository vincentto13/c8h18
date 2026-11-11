//! Assets loaded at start (640x480 sets only; low resolution dropped).
use crate::data::files;
use crate::data::images::{load_search_rings_or_generate, ColourTable, Image, Nearest, Palette, TexAtlas, FALLBACK_HAZE};
use crate::data::tables;
use crate::data::model::{model_files, Model};
use crate::data::sprites::{Font, SpriteSheet};
use crate::data::tmaps::{load_tmaps, TmapImage};
use std::rc::Rc;

pub struct Assets {
    pub palette: Palette,
    /// font slots 0..4: HFONT0-0, MFONT0-0, OLFNT0-1, OSFNT0-1, PFONT0-1
    pub fonts: Vec<Font>,
    pub panel: SpriteSheet,
    pub logo: SpriteSheet,
    pub track: SpriteSheet,
    pub oscr: Image,
    pub onet: Image,
    pub title: Image,
    /// the sliding puzzle's picture; without it the puzzle is disabled
    pub puzzle: Option<Image>,
    pub models: Vec<Model>,
    pub atlas: TexAtlas,
    pub tmaps: Vec<TmapImage>,
    pub rings: Vec<Vec<(i32, i32)>>,
    /// front-end colour table (port: always table0-0)
    pub fe_table: Rc<ColourTable>,
    pub cube: Vec<u8>,
}

impl Assets {
    /// Load everything the front end and races share. Nothing here is fatal: a missing or
    /// unusable file prints one warning on stdout and is replaced by a fallback (port addition,
    /// so partial asset packages run).
    pub fn load() -> Assets {
        let palette = Palette::load_or_builtin();
        let font = |name: &str, mask| Font { sheet: SpriteSheet::load_or_warn(name, "text in this font is not drawn"), mask_glyphs: mask };
        let fonts = vec![font("hfont0-0", false), font("mfont0-0", false), font("olfnt0-1", true), font("osfnt0-1", true), font("pfont0-1", false)];
        let cube = palette.colour_cube();
        let fe_table = match ColourTable::load(0) {
            Ok(t) => t,
            Err(e) => {
                files::warn_asset("data/table0-0.dat", Some(&e), "computing the colour table from the palette");
                ColourTable::from_bytes(&tables::build(&palette, FALLBACK_HAZE))
            }
        };
        let backdrop = |rel: &str, w, h| {
            Image::load_raw(rel, w, h).unwrap_or_else(|e| {
                files::warn_asset(rel, Some(&e), "drawing a plain background");
                plain_image(&palette, w, h)
            })
        };
        Assets {
            fonts,
            panel: SpriteSheet::load_or_warn("panel0-1", "no HUD or panels"),
            logo: SpriteSheet::load_or_warn("logo0-1", "no logo"),
            track: SpriteSheet::load_or_warn("track0-1", "no track maps"),
            oscr: backdrop("data/oscr0-1.dat", 640, 480),
            onet: backdrop("data/onet0-1.dat", 640, 480),
            title: backdrop("data/title.dat", 320, 200),
            puzzle: Image::load_raw("data/puzzle.dat", 112, 96).map_err(|e| files::warn_asset("data/puzzle.dat", Some(&e), "the sliding puzzle is disabled")).ok(),
            models: load_models(),
            atlas: TexAtlas::load().unwrap_or_else(|e| {
                let rel = if files::exists("objects/data/tex0-0.dat") { "objects/data/tex0-0.tab" } else { "objects/data/tex0-0.dat" };
                files::warn_asset(rel, Some(&e), "models are drawn with a plain texture");
                TexAtlas::plain(&palette)
            }),
            tmaps: load_tmaps().unwrap_or_else(|e| {
                let rel = if files::exists("data/tmaps.dat") { "data/tmaps.tab" } else { "data/tmaps.dat" };
                files::warn_asset(rel, Some(&e), "no billboards");
                Vec::new()
            }),
            rings: load_search_rings_or_generate(),
            fe_table: Rc::new(fe_table),
            palette,
            cube,
        }
    }
}

/// A plain picture in a dark blue-grey: the stand-in for a missing backdrop.
fn plain_image(pal: &Palette, w: u32, h: u32) -> Image {
    let c = Nearest::new(pal).get([10, 12, 18]);
    Image { w, h, pix: vec![c; (w * h) as usize] }
}

/// The 62 models in their fixed slots. A missing or unreadable model becomes an empty one (drawn as
/// nothing), so a package may leave out e.g. the preview models of tracks it does not have.
fn load_models() -> Vec<Model> {
    model_files()
        .iter()
        .map(|n| {
            let rel = format!("objects/data/{n}.dat");
            Model::load(&rel).unwrap_or_else(|e| {
                files::warn_asset(&rel, Some(&e), "drawn as nothing");
                Model::default()
            })
        })
        .collect()
}
