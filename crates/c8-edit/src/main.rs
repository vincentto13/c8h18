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
mod mcp;
mod resources;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut data: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("mcp") {
        args.next();
        return run_mcp(args.collect());
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("usage: c8h18-edit [--data <game data folder>] [<file to open>]");
                println!("       c8h18-edit mcp [--data <game data folder>] [--allow <folder>]... [<level to open>]");
                println!("         MCP server on stdio, no window; levels are written only into the --allow folders");
                println!("         (default: the current directory)");
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

/// `c8h18-edit mcp ...`: the headless MCP server on stdio.
fn run_mcp(args: Vec<String>) -> eframe::Result {
    let mut data = None;
    let mut allow = Vec::new();
    let mut open = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().map(PathBuf::from),
            "--allow" => allow.extend(it.next().map(PathBuf::from)),
            _ => open = Some(PathBuf::from(a)),
        }
    }
    let data = data.or_else(|| Some(PathBuf::from("resources"))).filter(|d| resources::is_data_dir(d));
    if data.is_none() {
        eprintln!("c8h18-edit mcp: no game data folder (give --data <dir>)");
        std::process::exit(2);
    }
    let mut ws = mcp::headless::Headless::new(data, allow);
    if let Some(p) = open {
        if let Err(e) = mcp::Workspace::open_level(&mut ws, p) {
            eprintln!("c8h18-edit mcp: {e}");
            std::process::exit(2);
        }
    }
    if let Err(e) = mcp::server::run_stdio(ws) {
        eprintln!("c8h18-edit mcp: {e}");
        std::process::exit(1);
    }
    Ok(())
}
