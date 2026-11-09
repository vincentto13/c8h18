//! c8h18-edit: level and resource editor for the game.
//!
//!   c8h18-edit [--data <game data folder>] [<file to open>]
//!
//! The editor reads the original data files from the game data folder (DATA/, MAPS/, OBJECTS/,
//! SOUND/), chosen on the first start and remembered. Each opened file is a document: levels,
//! textures, fonts, sprites, pictures and the palette have editors, other kinds open in a viewer
//! (see `docs/editor-plan.md`).
mod app;
mod audio;
mod doc;
mod gpu;
mod launch;
mod resources;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut data: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("usage: c8h18-edit [--data <game data folder>] [<file to open>]");
                return Ok(());
            }
            _ => open = Some(PathBuf::from(a)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native("c8h18-edit", options, Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, data, open)))))
}
