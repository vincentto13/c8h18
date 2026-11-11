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
mod model_io;
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
    let all: Vec<String> = std::env::args().skip(1).collect();
    if all.iter().any(|a| a == "--export-model" || a == "--import-model") {
        match run_model_cli(all) {
            Ok(msg) => {
                println!("{msg}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("c8h18-edit: {e}");
                std::process::exit(1);
            }
        }
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("usage: c8h18-edit [--data <game data folder>] [<file to open>]");
                println!("       c8h18-edit mcp [--data <game data folder>] [--allow <folder>]... [<level to open>]");
                println!("         MCP server on stdio, no window; levels are written only into the --allow folders");
                println!("         (default: the current directory)");
                println!("       c8h18-edit [--data <dir>] [--override <dir>] [--table <0..5>] --export-model <model.dat> <out.glb|out.obj>");
                println!("         export a model for Blender (.obj also writes .mtl and _tex.png next to it)");
                println!("       c8h18-edit [--data <dir>] [--override <dir>] [--like <original.dat>] --import-model <in.glb|in.obj> <out.dat>");
                println!("         convert a .glb/.gltf/.obj back to the game's model format");
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

/// `--export-model` / `--import-model`: model conversion without a window. Returns a report.
fn run_model_cli(args: Vec<String>) -> Result<String, String> {
    let mut data = None;
    let mut over = None;
    let mut like = None;
    let mut table = 0usize;
    let mut job = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().map(PathBuf::from),
            "--override" => over = Some(it.next().map(PathBuf::from).ok_or("--override takes a folder")?),
            "--like" => like = it.next().map(PathBuf::from),
            "--table" => table = it.next().and_then(|v| v.parse().ok()).filter(|&t| t < 6).ok_or("--table takes 0..5")?,
            "--export-model" | "--import-model" => {
                let (i, o) = (it.next().ok_or("missing input file")?, it.next().ok_or("missing output file")?);
                job = Some((a == "--export-model", PathBuf::from(i), PathBuf::from(o)));
            }
            _ => return Err(format!("unexpected argument {a}")),
        }
    }
    let data = data.unwrap_or_else(|| PathBuf::from("resources"));
    if !resources::is_data_dir(&data) {
        return Err(format!("{}: not a game data folder (give --data <dir>; the palette and model textures are needed)", data.display()));
    }
    // files in the override folder (same layout) win, e.g. a new OBJECTS/DATA/TEX0-0 atlas
    c8_data::files::set_override_dir(over);
    let look = model_io::Look::load(table)?;
    let (export, input, output) = job.ok_or("nothing to do")?;
    let read_model = |p: &std::path::Path| -> Result<c8_data::model::Model, String> {
        let d = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        c8_data::model::Model::parse(&d).map_err(|e| format!("{}: {e}", p.display()))
    };
    if export {
        let m = read_model(&input)?;
        let name = input.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_else(|| "MODEL".into());
        let files = model_io::export_file(&m, &look, &name, &output)?;
        Ok(format!("wrote {}", files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")))
    } else {
        let like = like.map(|p| read_model(&p)).transpose()?;
        let r = model_io::import_file(&input, &look, like.as_ref())?;
        let bytes = r.model.to_bytes()?;
        std::fs::write(&output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;
        let mut msg = format!("wrote {} ({} vertices, {} triangles, {} bytes{})", output.display(), r.model.verts.len(), r.model.tris.len(), bytes.len(), if r.exact { ", identical to the original" } else { "" });
        for n in r.notes {
            msg.push_str("\n  ");
            msg.push_str(&n);
        }
        Ok(msg)
    }
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
