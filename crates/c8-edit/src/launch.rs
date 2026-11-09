//! "Test in game": start the game with edited files in an override folder, so the originals in
//! the data folder stay untouched.
use crate::doc::Document;
use std::path::PathBuf;

fn override_dir() -> PathBuf {
    std::env::temp_dir().join("c8h18-edit-override")
}

pub(crate) fn game_exe() -> PathBuf {
    let name = if cfg!(windows) { "c8h18.exe" } else { "c8h18" };
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from(name))
}

/// Write every open document that has changes (and the active one) to the override folder and
/// start the game with the active document's arguments. Returns a status line.
pub fn test_in_game(docs: &[Box<dyn Document>], active: usize) -> Result<String, String> {
    let dir = override_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut written = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        if i != active && !d.dirty() {
            continue;
        }
        for (rel, bytes) in d.game_files()? {
            let p = dir.join(&rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
            written.push(rel);
        }
    }
    let args = docs.get(active).map(|d| d.game_args()).unwrap_or_default();
    let data = c8_data::files::data_dir();
    let data = data.canonicalize().unwrap_or(data);
    let exe = game_exe();
    std::process::Command::new(&exe)
        .arg(&data)
        .arg("--override")
        .arg(&dir)
        .args(&args)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e} (build it with `cargo build --release --workspace`)", exe.display()))?;
    Ok(if written.is_empty() { "started the game with the original files".into() } else { format!("started the game with {}", written.join(", ")) })
}
