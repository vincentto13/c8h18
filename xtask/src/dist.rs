//! `cargo xtask dist`: the release layout per platform.
//!
//! Every platform ships the same three things: the game binary `c8h18`, the asset package
//! `c8h18.c8pak` (packed from `assets/custom`, found by the game relative to its executable,
//! see `c8_data::locate`) and the licence files (COPYING, ASSETS-LICENSE,
//! THIRD-PARTY-NOTICES.txt, RUST-STD-COPYRIGHT.html). Only the container differs:
//!
//! | Platform | Output | Layout |
//! |---|---|---|
//! | macOS (arm64) | `dist/C8H18.app`, `dist/C8H18-<v>-macos-arm64.dmg` | `Contents/MacOS/c8h18`, `Contents/Resources/c8h18.c8pak` + icon + licences |
//! | Linux (planned) | `dist/c8h18-<v>-linux-x86_64.tar.gz` | `bin/c8h18`, `share/c8h18/c8h18.c8pak`, `share/applications/c8h18.desktop`, icon |
//! | Windows (planned) | `dist/C8H18-<v>-windows-x86_64.zip` | `c8h18.exe` with `c8h18.c8pak` next to it |
use crate::{APP_NAME, BIN_NAME, BUNDLE_ID, GAME_PACKAGE, PACKAGE_FILE, output, run, workspace_root};
use std::path::{Path, PathBuf};
use std::process::Command;

/// macOS version the release needs (Apple silicon starts at 11).
pub const MACOS_MIN: &str = "11.0";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    pub fn triple(self) -> &'static str {
        match self {
            Platform::MacOs => "aarch64-apple-darwin",
            Platform::Linux => "x86_64-unknown-linux-gnu",
            Platform::Windows => "x86_64-pc-windows-msvc",
        }
    }
    fn parse(s: &str) -> Result<Platform, String> {
        match s {
            "macos" | "mac" | "darwin" => Ok(Platform::MacOs),
            "linux" => Ok(Platform::Linux),
            "windows" | "win" => Ok(Platform::Windows),
            _ => Err(format!("unknown platform {s:?} (macos, linux, windows)")),
        }
    }
}

pub struct Options {
    pub platform: Platform,
    /// codesign identity; `-` (ad hoc) by default
    pub sign: String,
    pub skip_build: bool,
    pub dmg: bool,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options { platform: Platform::MacOs, sign: "-".into(), skip_build: false, dmg: true };
        let mut i = 0;
        while i < args.len() {
            let val = || args.get(i + 1).cloned().ok_or_else(|| format!("{} needs a value", args[i]));
            match args[i].as_str() {
                "--platform" => {
                    o.platform = Platform::parse(&val()?)?;
                    i += 1;
                }
                "--sign" => {
                    o.sign = val()?;
                    i += 1;
                }
                "--skip-build" => o.skip_build = true,
                "--no-dmg" => o.dmg = false,
                a => return Err(format!("unknown option {a}")),
            }
            i += 1;
        }
        Ok(o)
    }
}

/// What every platform's layout is made from.
struct Inputs {
    version: String,
    binary: PathBuf,
    package: PathBuf,
    notices: PathBuf,
    std_copyright: Option<PathBuf>,
    copying: PathBuf,
    assets_license: PathBuf,
}

pub fn run_dist(o: &Options) -> Result<(), String> {
    match o.platform {
        Platform::MacOs => {}
        Platform::Linux => return Err("linux release: not yet (planned: tar.gz with bin/c8h18, share/c8h18/c8h18.c8pak, a .desktop file and the icon)".into()),
        Platform::Windows => return Err("windows release: not yet (planned: zip with c8h18.exe and c8h18.c8pak next to it)".into()),
    }
    let root = workspace_root();
    let dist = root.join("dist");
    let work = dist.join("work");
    let _ = std::fs::remove_dir_all(&work);
    mkdir(&work)?;
    let inputs = prepare(&root, &work, o)?;
    match o.platform {
        Platform::MacOs => macos(&root, &dist, &inputs, o),
        _ => unreachable!(),
    }
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

fn size(p: &Path) -> String {
    let n = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    format!("{:.1} MB", n as f64 / 1e6)
}

/// The workspace version (the game package's).
fn version(root: &Path) -> Result<String, String> {
    let meta = output(crate::cargo().current_dir(root).args(["metadata", "--quiet", "--no-deps", "--format-version", "1"]))?;
    let v: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
    v["packages"].as_array().into_iter().flatten().find(|p| p["name"] == GAME_PACKAGE).and_then(|p| p["version"].as_str()).map(String::from).ok_or_else(|| "no game package".into())
}

/// Pack an asset folder for release: every file but hidden ones and anything in a `save/`
/// folder (at any depth). Returns the number of files.
pub fn pack(dir: &Path, out: &Path) -> Result<usize, String> {
    if !dir.is_dir() {
        return Err(format!("{}: not a directory", dir.display()));
    }
    let src = c8_data::package::Package::dir(dir.to_path_buf());
    let files: Vec<String> = src.list().into_iter().filter(|rel| !rel.split('/').any(|p| p.eq_ignore_ascii_case("save") || p.starts_with('.'))).collect();
    c8_data::package::write_bundle(out, files.iter().map(|rel| (rel.clone(), src.read(rel))))?;
    Ok(files.len())
}

/// Build, pack and write the licence files into `work`.
fn prepare(root: &Path, work: &Path, o: &Options) -> Result<Inputs, String> {
    let version = version(root)?;
    let triple = o.platform.triple();
    if !o.skip_build {
        println!("building {GAME_PACKAGE} {version} for {triple} (release)");
        let mut c = crate::cargo();
        c.current_dir(root).args(["build", "--release", "--locked", "-p", GAME_PACKAGE, "--target", triple]);
        if o.platform == Platform::MacOs {
            c.env("MACOSX_DEPLOYMENT_TARGET", MACOS_MIN);
        }
        run(&mut c)?;
    }
    let exe = if o.platform == Platform::Windows { format!("{BIN_NAME}.exe") } else { BIN_NAME.to_string() };
    let binary = root.join("target").join(triple).join("release").join(exe);
    if !binary.is_file() {
        return Err(format!("{}: not built", binary.display()));
    }
    let package = work.join(PACKAGE_FILE);
    let n = pack(&root.join("assets/custom"), &package)?;
    println!("packed {n} files of assets/custom into {} ({})", PACKAGE_FILE, size(&package));
    let notices = work.join("THIRD-PARTY-NOTICES.txt");
    let nt = crate::notices::generate(root, triple)?;
    for w in &nt.warnings {
        println!("warning: {w}");
    }
    std::fs::write(&notices, &nt.text).map_err(|e| format!("{}: {e}", notices.display()))?;
    println!("THIRD-PARTY-NOTICES.txt: {} crates", nt.crates);
    let copying = root.join("COPYING");
    let assets_license = root.join("assets/custom/LICENSE.txt");
    for f in [&copying, &assets_license] {
        if !f.is_file() {
            return Err(format!("{}: missing", f.display()));
        }
    }
    Ok(Inputs { version, binary, package, notices, std_copyright: nt.std_copyright, copying, assets_license })
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{APP_NAME}</string>
	<key>CFBundleExecutable</key>
	<string>{BIN_NAME}</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>{APP_NAME}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.racing-games</string>
	<key>LSArchitecturePriority</key>
	<array>
		<string>arm64</string>
	</array>
	<key>LSMinimumSystemVersion</key>
	<string>{MACOS_MIN}</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>Free software under the GNU GPL version 3 or later.</string>
</dict>
</plist>
"#
    )
}

fn macos(root: &Path, dist: &Path, i: &Inputs, o: &Options) -> Result<(), String> {
    let app = dist.join(format!("{APP_NAME}.app"));
    let _ = std::fs::remove_dir_all(&app);
    let contents = app.join("Contents");
    let (macos, res) = (contents.join("MacOS"), contents.join("Resources"));
    mkdir(&macos)?;
    mkdir(&res)?;
    std::fs::write(contents.join("Info.plist"), info_plist(&i.version)).map_err(|e| e.to_string())?;
    std::fs::write(contents.join("PkgInfo"), "APPL????").map_err(|e| e.to_string())?;
    copy(&i.binary, &macos.join(BIN_NAME))?;
    copy(&i.package, &res.join(PACKAGE_FILE))?;
    copy(&root.join("packaging/macos/AppIcon.icns"), &res.join("AppIcon.icns"))?;
    copy(&i.copying, &res.join("COPYING"))?;
    copy(&i.assets_license, &res.join("ASSETS-LICENSE"))?;
    copy(&i.notices, &res.join("THIRD-PARTY-NOTICES.txt"))?;
    if let Some(s) = &i.std_copyright {
        copy(s, &res.join("RUST-STD-COPYRIGHT.html"))?;
    }
    run(Command::new("plutil").arg("-lint").arg(contents.join("Info.plist")))?;
    // ad hoc ("-") unless an identity is given; a real identity also gets the hardened runtime
    let mut cs = Command::new("codesign");
    cs.args(["--force", "--deep", "--sign", &o.sign]);
    if o.sign != "-" {
        cs.args(["--options", "runtime", "--timestamp"]);
    }
    run(cs.arg(&app))?;
    run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&app))?;
    println!("wrote {} (signed {})", app.display(), if o.sign == "-" { "ad hoc".to_string() } else { o.sign.clone() });
    if o.dmg {
        let dmg = dist.join(format!("{APP_NAME}-{}-macos-arm64.dmg", i.version));
        let stage = dist.join("work/dmg");
        let _ = std::fs::remove_dir_all(&stage);
        mkdir(&stage)?;
        run(Command::new("ditto").arg(&app).arg(stage.join(format!("{APP_NAME}.app"))))?;
        std::os::unix::fs::symlink("/Applications", stage.join("Applications")).map_err(|e| format!("Applications link: {e}"))?;
        let _ = std::fs::remove_file(&dmg);
        run(Command::new("hdiutil").args(["create", "-quiet", "-volname", APP_NAME, "-fs", "HFS+", "-format", "UDZO", "-ov", "-srcfolder"]).arg(&stage).arg(&dmg))?;
        if o.sign != "-" {
            run(Command::new("codesign").args(["--force", "--sign", &o.sign, "--timestamp"]).arg(&dmg))?;
        }
        println!("wrote {} ({})", dmg.display(), size(&dmg));
    }
    let _ = std::fs::remove_dir_all(dist.join("work"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = Options::parse(&a(&[])).unwrap();
        assert_eq!((o.platform, o.sign.as_str(), o.dmg), (Platform::MacOs, "-", true));
        let o = Options::parse(&a(&["--platform", "linux", "--sign", "Developer ID Application: X", "--no-dmg"])).unwrap();
        assert_eq!((o.platform, o.sign.as_str(), o.dmg), (Platform::Linux, "Developer ID Application: X", false));
        assert!(Options::parse(&a(&["--bogus"])).is_err());
        assert!(run_dist(&Options::parse(&a(&["--platform", "windows"])).unwrap()).unwrap_err().contains("not yet"));
    }

    #[test]
    fn pack_leaves_out_saves_and_hidden_files() {
        let d = std::env::temp_dir().join(format!("xtask-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["DATA/A.DAT", "save/config.dat", "SOUND/save/x", "DATA/.DS_Store", "lang/en.txt"] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let out = d.with_extension("c8pak");
        assert_eq!(pack(&d, &out).unwrap(), 2);
        assert_eq!(c8_data::package::Package::open(&out).unwrap().list(), ["DATA/A.DAT", "lang/en.txt"]);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn plist_names_the_app() {
        let p = info_plist("1.2.3");
        for s in ["<string>xyz.c8h18.game</string>", "<string>C8H18</string>", "<string>c8h18</string>", "<string>1.2.3</string>", "<string>11.0</string>", "NSHighResolutionCapable", "<string>AppIcon</string>"] {
            assert!(p.contains(s), "{s}");
        }
    }
}
