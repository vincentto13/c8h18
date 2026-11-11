//! "Test in game": start the game with edited files in an override folder, so the originals in
//! the asset package (a data folder or a `.c8pak` bundle) stay untouched. The game gets
//! `--asset <package> --override <folder>` plus the document's own arguments.
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
    let args = game_command_args(&c8_data::files::data_dir(), &dir, docs.get(active).map(|d| d.game_args()).unwrap_or_default());
    let exe = game_exe();
    std::process::Command::new(&exe)
        .args(&args)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e} (build it with `cargo build --release --workspace`)", exe.display()))?;
    Ok(if written.is_empty() { "started the game with the original files".into() } else { format!("started the game with {}", written.join(", ")) })
}

/// The game's arguments for "Test in game": the package `data` (folder or bundle, made
/// absolute), the override folder with the edited files, then the document's own arguments.
pub fn game_command_args(data: &std::path::Path, override_dir: &std::path::Path, doc_args: Vec<String>) -> Vec<std::ffi::OsString> {
    let data = data.canonicalize().unwrap_or_else(|_| data.to_path_buf());
    let mut v: Vec<std::ffi::OsString> = vec!["--asset".into(), data.into(), "--override".into(), override_dir.into()];
    v.extend(doc_args.into_iter().map(Into::into));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_game_arguments_name_the_bundle() {
        let base = std::env::temp_dir().join(format!("c8-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("pkg/MAPS")).unwrap();
        std::fs::write(base.join("pkg/MAPS/LEVEL0-1.DAT"), b"x").unwrap();
        let bundle = base.join("my.c8pak");
        c8_data::package::pack(&base.join("pkg"), &bundle).unwrap();
        let data = bundle.canonicalize().unwrap();
        let got = game_command_args(&bundle, &override_dir(), vec!["--race".into(), "1".into()]);
        let want: Vec<std::ffi::OsString> = vec!["--asset".into(), data.into(), "--override".into(), override_dir().into(), "--race".into(), "1".into()];
        assert_eq!(got, want);
        let _ = std::fs::remove_dir_all(&base);
    }
}
