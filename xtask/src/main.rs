//! Release tooling for the game (not the editor).
//!
//! ```text
//! cargo xtask dist [--platform macos|linux|windows] [--sign <identity>] [--skip-build] [--no-dmg]
//!     build the game for release, pack assets/custom into c8h18.c8pak and lay out the release
//!     in dist/ (macOS: C8H18.app, signed ad hoc unless --sign, and C8H18-<version>-macos-arm64.dmg)
//! cargo xtask notices [--target <triple>] [--out <file>]
//!     THIRD-PARTY-NOTICES.txt for the game binary's dependencies (default: stdout)
//! cargo xtask pack <package dir> <out.c8pak>
//!     pack an asset folder the way the release does (no save/ folders, no hidden files)
//! ```
mod dist;
mod notices;

use std::path::{Path, PathBuf};
use std::process::Command;

/// The game's name, bundle id and file names.
pub const APP_NAME: &str = "C8H18";
pub const BUNDLE_ID: &str = "xyz.c8h18.game";
pub const BIN_NAME: &str = "c8h18";
pub const GAME_PACKAGE: &str = "c8-game";
/// The package shipped with the game (see `c8_data::locate`).
pub const PACKAGE_FILE: &str = c8_data::locate::SHIPPED_PACKAGE;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("dist") => dist::Options::parse(&args[1..]).and_then(|o| dist::run_dist(&o)),
        Some("notices") => notices_cmd(&args[1..]),
        Some("pack") if args.len() == 3 => dist::pack(Path::new(&args[1]), Path::new(&args[2])).map(|n| println!("packed {n} files into {}", args[2])),
        _ => Err(USAGE.trim().to_string()),
    };
    if let Err(e) = r {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

const USAGE: &str = "
usage: cargo xtask dist [--platform macos|linux|windows] [--sign <identity>] [--skip-build] [--no-dmg]
       cargo xtask notices [--target <triple>] [--out <file>]
       cargo xtask pack <package dir> <out.c8pak>
";

fn notices_cmd(args: &[String]) -> Result<(), String> {
    let mut target = dist::Platform::MacOs.triple().to_string();
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => target = args.get(i + 1).cloned().ok_or("--target needs a triple")?,
            "--out" => out = Some(args.get(i + 1).ok_or("--out needs a file")?.into()),
            a => return Err(format!("unknown option {a}\n{}", USAGE.trim())),
        }
        i += 2;
    }
    let n = notices::generate(&workspace_root(), &target)?;
    match out {
        Some(p) => {
            std::fs::write(&p, &n.text).map_err(|e| format!("{}: {e}", p.display()))?;
            eprintln!("wrote {} ({} crates)", p.display(), n.crates);
        }
        None => print!("{}", n.text),
    }
    for w in &n.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

/// The workspace root (the parent of this crate).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

/// The `cargo` running us, or `cargo` on the PATH.
pub fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Run a command, failing with its name when it does not succeed.
pub fn run(cmd: &mut Command) -> Result<(), String> {
    let name = format!("{cmd:?}");
    let st = cmd.status().map_err(|e| format!("cannot run {name}: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("{name} failed ({st})")) }
}

/// Run a command and return its stdout.
pub fn output(cmd: &mut Command) -> Result<String, String> {
    let name = format!("{cmd:?}");
    let o = cmd.output().map_err(|e| format!("cannot run {name}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{name} failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).trim()));
    }
    String::from_utf8(o.stdout).map_err(|e| format!("{name}: {e}"))
}
