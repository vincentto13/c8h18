//! Port addition: where the game finds its asset package and where it writes its settings and
//! saves. Everything that depends on the computer (operating system, environment variables, the
//! executable's place, the current directory) comes in through [`Env`], so the rules can be
//! tested without touching the real environment.
//!
//! The package is the first of:
//! 1. `--asset <dir|file.c8pak>` (or a bare path) on the command line;
//! 2. the environment variable `C8H18_ASSET` or `GAME_DATA` (in that order);
//! 3. `package = <name>` in the user's settings file `<user dir>/settings.txt`, naming a
//!    `<name>.c8pak` bundle or a `<name>` folder in `<user dir>/packages/`;
//! 4. the shipped package `c8h18.c8pak`, next to the executable, in `../Resources/` (macOS
//!    app bundle) or in `../share/c8h18/` (Linux install);
//! 5. for development (`cargo run` in the repository): `resources/` in the current directory
//!    when it holds a `DATA/` folder (the original files), else `assets/custom/`;
//! 6. otherwise `resources/` as it is (every file then falls back with a warning).
//!
//! The user dir is `~/Library/Application Support/C8H18` on macOS, `%APPDATA%\C8H18` on Windows
//! and `$XDG_DATA_HOME/c8h18` (default `~/.local/share/c8h18`) elsewhere. Saves and settings of
//! a package go to `<user dir>/save/<package name>/`, except for a folder given with `--asset`,
//! which keeps its own `save/` (handy while developing a package). `C8H18_SAVE_DIR` replaces
//! either.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The game's name, used for the per-user folder on macOS and Windows.
pub const APP_NAME: &str = "C8H18";
/// The per-user folder's name on Linux and other Unix systems.
pub const APP_NAME_UNIX: &str = "c8h18";
/// The file name of the package that ships with the game.
pub const SHIPPED_PACKAGE: &str = "c8h18.c8pak";
/// The user's settings file in the user dir.
pub const SETTINGS_FILE: &str = "settings.txt";
/// Environment variables naming the asset package, first match wins.
pub const ASSET_VARS: [&str; 2] = ["C8H18_ASSET", "GAME_DATA"];
/// The environment variable naming the save folder.
pub const SAVE_VARS: [&str; 1] = ["C8H18_SAVE_DIR"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    MacOs,
    Windows,
    /// Linux and other Unix systems (XDG folders).
    Unix,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Unix
        }
    }
}

/// The parts of the computer the lookups depend on.
pub struct Env {
    pub os: Os,
    /// Environment variable lookup.
    pub vars: Box<dyn Fn(&str) -> Option<OsString>>,
    /// The running executable, if known.
    pub exe: Option<PathBuf>,
    /// The current directory.
    pub cwd: PathBuf,
}

impl Env {
    /// This computer: the real environment, executable and current directory.
    pub fn system() -> Env {
        Env {
            os: Os::current(),
            vars: Box::new(|k| std::env::var_os(k)),
            exe: std::env::current_exe().ok().map(|p| p.canonicalize().unwrap_or(p)),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// A non-empty environment variable.
    pub fn var(&self, k: &str) -> Option<OsString> {
        (self.vars)(k).filter(|v| !v.is_empty())
    }

    fn home(&self) -> PathBuf {
        self.var("HOME").or_else(|| if self.os == Os::Windows { self.var("USERPROFILE") } else { None }).map(PathBuf::from).unwrap_or_else(|| self.cwd.clone())
    }

    fn base_dir(&self) -> PathBuf {
        match self.os {
            Os::MacOs => self.home().join("Library").join("Application Support"),
            Os::Windows => self.var("APPDATA").map(PathBuf::from).unwrap_or_else(|| self.home().join("AppData").join("Roaming")),
            Os::Unix => self.var("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| self.home().join(".local").join("share")),
        }
    }

    /// The per-user folder: `~/Library/Application Support/C8H18`, `%APPDATA%\C8H18` or
    /// `$XDG_DATA_HOME/c8h18`.
    pub fn user_dir(&self) -> PathBuf {
        self.base_dir().join(if self.os == Os::Unix { APP_NAME_UNIX } else { APP_NAME })
    }

    /// Where the user puts extra packages (`.c8pak` files or folders).
    pub fn packages_dir(&self) -> PathBuf {
        self.user_dir().join("packages")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.user_dir().join(SETTINGS_FILE)
    }

    /// The save folder of the package called `name` in the user dir.
    pub fn package_save_dir(&self, name: &str) -> PathBuf {
        self.user_dir().join("save").join(name)
    }

    /// The save folder from `C8H18_SAVE_DIR`, if set.
    pub fn save_dir_override(&self) -> Option<PathBuf> {
        SAVE_VARS.iter().find_map(|k| self.var(k)).map(PathBuf::from)
    }

    /// Where the shipped package may be, in order: next to the executable, in the macOS app
    /// bundle's `Resources/`, in a Linux install's `share/c8h18/`.
    pub fn shipped_candidates(&self) -> Vec<PathBuf> {
        let Some(dir) = self.exe.as_ref().and_then(|e| e.parent()) else { return Vec::new() };
        vec![dir.join(SHIPPED_PACKAGE), dir.join("..").join("Resources").join(SHIPPED_PACKAGE), dir.join("..").join("share").join(APP_NAME_UNIX).join(SHIPPED_PACKAGE)]
    }
}

/// How the package was chosen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Origin {
    /// `--asset` or a bare path on the command line.
    Arg,
    /// This environment variable.
    Env(&'static str),
    /// `package = ...` in the settings file.
    Setting,
    /// The package shipped with the game.
    Shipped,
    /// A development folder in the current directory (`resources/`, `assets/custom/`).
    Dev,
    /// Nothing found: `resources/` as it is.
    Default,
}

/// The chosen package.
#[derive(Clone, Debug)]
pub struct Resolved {
    /// The folder or bundle file.
    pub path: PathBuf,
    pub origin: Origin,
    /// The package's name: the bundle's file stem or the folder's name (or the setting).
    pub name: String,
    /// Where config, saves and settings go.
    pub save_dir: PathBuf,
    /// Old save folders whose files are copied to `save_dir` when it does not exist yet,
    /// first existing one wins.
    pub legacy_saves: Vec<PathBuf>,
    /// Problems met on the way (e.g. a setting naming a missing package).
    pub warnings: Vec<String>,
}

/// A package name safe for a folder name.
pub fn sanitize_name(s: &str) -> String {
    let n: String = s.trim().chars().map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' }).collect();
    let n = n.trim_matches(|c| c == '.' || c == ' ').to_string();
    if n.is_empty() { "package".into() } else { n }
}

/// The name of the package at `path`: a bundle's file stem, a folder's name.
pub fn package_name(path: &Path) -> String {
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let n = if crate::package::is_bundle_name(&p) { p.file_stem() } else { p.file_name() };
    sanitize_name(&n.map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
}

/// `key = value` lines of a settings file (`#` starts a comment); the last value of `key`.
pub fn setting(text: &str, key: &str) -> Option<String> {
    let mut out = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        if let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            let v = v.trim();
            out = if v.is_empty() { None } else { Some(v.to_string()) };
        }
    }
    out
}

/// The package called `name` in `dir`: a `<name>.c8pak` file or a `<name>` folder (any letter
/// case; `name` may already end in `.c8pak`). Exact spelling wins over another letter case.
pub fn find_user_package(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    let bundle = format!("{name}.{}", crate::package::BUNDLE_EXT);
    let ok = |p: &Path| p.is_dir() || (p.is_file() && crate::package::is_bundle_name(p));
    // the names as stored (a case-insensitive file system would accept any spelling)
    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dir).ok()?.flatten().map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).filter(|(_, p)| ok(p)).collect();
    entries.sort();
    let exact = entries.iter().find(|(f, _)| *f == bundle || f == name);
    exact.or_else(|| entries.iter().find(|(f, _)| f.eq_ignore_ascii_case(&bundle) || f.eq_ignore_ascii_case(name))).map(|(_, p)| p.clone())
}

/// Does `dir` look like a package with data (a `DATA/` folder in any case)?
fn has_data(dir: &Path) -> bool {
    dir.is_dir() && crate::files::resolve_in(dir.to_path_buf(), "data").is_some_and(|p| p.is_dir())
}

/// Choose the package (see the module documentation). `arg` is the command line's `--asset`.
pub fn resolve(arg: Option<&Path>, env: &Env) -> Resolved {
    let mut warnings = Vec::new();
    let abs = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { env.cwd.join(p) };
    let mut chosen: Option<(PathBuf, Origin, Option<String>)> = arg.map(|p| (abs(p), Origin::Arg, None));
    if chosen.is_none() {
        chosen = ASSET_VARS.iter().find_map(|k| env.var(k).map(|v| (abs(Path::new(&v)), Origin::Env(k), None)));
    }
    if chosen.is_none() {
        let sp = env.settings_path();
        if let Ok(text) = std::fs::read_to_string(&sp)
            && let Some(name) = setting(&text, "package")
        {
            match find_user_package(&env.packages_dir(), &name) {
                Some(p) => {
                    let n = name.strip_suffix(&format!(".{}", crate::package::BUNDLE_EXT)).unwrap_or(&name).to_string();
                    chosen = Some((p, Origin::Setting, Some(sanitize_name(&n))));
                }
                None => warnings.push(format!("{}: package {name:?} not found in {}, using the shipped package", sp.display(), env.packages_dir().display())),
            }
        }
    }
    if chosen.is_none() {
        chosen = env.shipped_candidates().into_iter().find(|p| p.is_file()).map(|p| (p.canonicalize().unwrap_or(p), Origin::Shipped, None));
    }
    if chosen.is_none() {
        let res = env.cwd.join("resources");
        let custom = env.cwd.join("assets").join("custom");
        chosen = [res, custom].into_iter().find(|p| has_data(p)).map(|p| (p, Origin::Dev, None));
    }
    let (path, origin, name) = chosen.unwrap_or_else(|| {
        warnings.push(format!("no asset package found (no {SHIPPED_PACKAGE} next to the game, no --asset): using {}", env.cwd.join("resources").display()));
        (env.cwd.join("resources"), Origin::Default, None)
    });
    let name = name.unwrap_or_else(|| package_name(&path));
    let mut legacy_saves = Vec::new();
    let save_dir = if let Some(d) = env.save_dir_override() {
        d
    } else if origin == Origin::Arg && path.is_dir() {
        path.join("save")
    } else {
        // saves of earlier versions: a folder package's own save/
        if path.is_dir() {
            legacy_saves.push(path.join("save"));
        }
        env.package_save_dir(&name)
    };
    Resolved { path, origin, name, save_dir, legacy_saves, warnings }
}

/// Copy the first existing old save folder to `r.save_dir` when that does not exist yet (the
/// old files are left in place). Returns the folder copied from.
pub fn migrate_saves(r: &Resolved) -> Result<Option<PathBuf>, String> {
    if r.save_dir.exists() {
        return Ok(None);
    }
    let Some(old) = r.legacy_saves.iter().find(|d| d.is_dir() && std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_some())) else { return Ok(None) };
    copy_dir(old, &r.save_dir)?;
    Ok(Some(old.clone()))
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for e in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?.flatten() {
        let (from, to) = (e.path(), dst.join(e.file_name()));
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else if from.is_file() {
            std::fs::copy(&from, &to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let d = std::env::temp_dir().join(format!("c8-locate-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Tmp(d.canonicalize().unwrap())
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn env(os: Os, vars: &[(&str, &Path)], exe: Option<PathBuf>, cwd: &Path) -> Env {
        let m: HashMap<String, OsString> = vars.iter().map(|(k, v)| (k.to_string(), v.as_os_str().to_owned())).collect();
        Env { os, vars: Box::new(move |k| m.get(k).cloned()), exe, cwd: cwd.to_path_buf() }
    }

    fn touch(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn user_dirs_per_platform() {
        let h = Path::new("/home/u");
        let mac = env(Os::MacOs, &[("HOME", h)], None, h);
        assert_eq!(mac.user_dir(), Path::new("/home/u/Library/Application Support/C8H18"));
        assert_eq!(mac.packages_dir(), Path::new("/home/u/Library/Application Support/C8H18/packages"));
        assert_eq!(mac.settings_path(), Path::new("/home/u/Library/Application Support/C8H18/settings.txt"));
        assert_eq!(mac.package_save_dir("c8h18"), Path::new("/home/u/Library/Application Support/C8H18/save/c8h18"));
        let linux = env(Os::Unix, &[("HOME", h)], None, h);
        assert_eq!(linux.user_dir(), Path::new("/home/u/.local/share/c8h18"));
        let xdg = env(Os::Unix, &[("HOME", h), ("XDG_DATA_HOME", Path::new("/xdg"))], None, h);
        assert_eq!(xdg.packages_dir(), Path::new("/xdg/c8h18/packages"));
        let rel_xdg = env(Os::Unix, &[("HOME", h), ("XDG_DATA_HOME", Path::new("rel"))], None, h);
        assert_eq!(rel_xdg.user_dir(), Path::new("/home/u/.local/share/c8h18"), "a relative XDG_DATA_HOME is ignored");
        let win = env(Os::Windows, &[("APPDATA", Path::new("/appdata"))], None, h);
        assert_eq!(win.user_dir(), Path::new("/appdata").join("C8H18"));
        assert_eq!(win.packages_dir(), Path::new("/appdata").join("C8H18").join("packages"));
    }

    #[test]
    fn shipped_package_next_to_the_executable() {
        let t = Tmp::new("shipped");
        let home = t.0.join("home");
        // macOS app bundle
        let app = t.0.join("C8H18.app/Contents");
        touch(&app.join("MacOS/c8h18"), b"");
        touch(&app.join("Resources").join(SHIPPED_PACKAGE), b"pk");
        let e = env(Os::MacOs, &[("HOME", &home)], Some(app.join("MacOS/c8h18")), &t.0);
        let r = resolve(None, &e);
        assert_eq!(r.origin, Origin::Shipped);
        assert_eq!(r.path, app.join("Resources").join(SHIPPED_PACKAGE).canonicalize().unwrap());
        assert_eq!(r.name, "c8h18");
        assert_eq!(r.save_dir, home.join("Library/Application Support/C8H18/save/c8h18"));
        assert!(r.legacy_saves.is_empty());
        // Linux install
        let usr = t.0.join("usr");
        touch(&usr.join("bin/c8h18"), b"");
        touch(&usr.join("share/c8h18").join(SHIPPED_PACKAGE), b"pk");
        let e = env(Os::Unix, &[("HOME", &home)], Some(usr.join("bin/c8h18")), &t.0);
        let r = resolve(None, &e);
        assert_eq!((r.origin, r.path), (Origin::Shipped, usr.join("share/c8h18").join(SHIPPED_PACKAGE).canonicalize().unwrap()));
        assert_eq!(r.save_dir, home.join(".local/share/c8h18/save/c8h18"));
        // next to the executable (Windows zip) wins over the others
        touch(&usr.join("bin").join(SHIPPED_PACKAGE), b"pk");
        let r = resolve(None, &env(Os::Windows, &[("APPDATA", &home)], Some(usr.join("bin/c8h18.exe")), &t.0));
        assert_eq!(r.path, usr.join("bin").join(SHIPPED_PACKAGE).canonicalize().unwrap());
        assert_eq!(r.save_dir, home.join("C8H18").join("save").join("c8h18"));
    }

    #[test]
    fn resolution_order() {
        let t = Tmp::new("order");
        let home = t.0.join("home");
        let exe = t.0.join("bin/c8h18");
        touch(&exe, b"");
        touch(&t.0.join("bin").join(SHIPPED_PACKAGE), b"pk");
        let user = home.join("Library/Application Support/C8H18");
        touch(&user.join("packages/mine.c8pak"), b"pk");
        std::fs::create_dir_all(user.join("packages/Folder Pack/DATA")).unwrap();
        let argdir = t.0.join("work/argpack");
        std::fs::create_dir_all(argdir.join("DATA")).unwrap();
        let envpak = t.0.join("env.c8pak");
        touch(&envpak, b"pk");

        // 4: shipped
        let e = env(Os::MacOs, &[("HOME", &home)], Some(exe.clone()), &t.0);
        assert_eq!(resolve(None, &e).origin, Origin::Shipped);
        // 3: the setting
        touch(&user.join(SETTINGS_FILE), b"# my settings\npackage = mine\n");
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.path.clone(), r.name.as_str()), (Origin::Setting, user.join("packages/mine.c8pak"), "mine"));
        assert_eq!(r.save_dir, user.join("save/mine"));
        // a folder in packages/, in another letter case; its own save/ is a legacy source
        touch(&user.join(SETTINGS_FILE), b"package = folder pack\n");
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.name.as_str()), (Origin::Setting, "folder pack"));
        assert_eq!(r.path, user.join("packages/Folder Pack"));
        assert_eq!(r.save_dir, user.join("save/folder pack"));
        assert!(r.legacy_saves.contains(&user.join("packages/Folder Pack/save")));
        // a missing one warns and falls through to the shipped package
        touch(&user.join(SETTINGS_FILE), b"package = gone\n");
        let r = resolve(None, &e);
        assert_eq!(r.origin, Origin::Shipped);
        assert_eq!(r.warnings.len(), 1);
        touch(&user.join(SETTINGS_FILE), b"package = mine.c8pak\n");
        assert_eq!(resolve(None, &e).name, "mine");
        // 2: the environment, in its order
        let e = env(Os::MacOs, &[("HOME", &home), ("GAME_DATA", &argdir), ("C8H18_ASSET", &envpak)], Some(exe.clone()), &t.0);
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.path.clone(), r.name.as_str()), (Origin::Env("C8H18_ASSET"), envpak.clone(), "env"));
        let e = env(Os::MacOs, &[("HOME", &home), ("C8H18_ASSET", &argdir), ("GAME_DATA", &envpak)], Some(exe.clone()), &t.0);
        let r = resolve(None, &e);
        assert_eq!(r.origin, Origin::Env("C8H18_ASSET"));
        assert_eq!(r.save_dir, user.join("save/argpack"), "only --asset folders keep their own save/");
        // 1: the command line, relative to the current directory; a folder keeps its save/
        let r = resolve(Some(Path::new("work/argpack")), &e);
        assert_eq!((r.origin.clone(), r.path.clone()), (Origin::Arg, argdir.clone()));
        assert_eq!(r.save_dir, argdir.join("save"));
        assert!(r.legacy_saves.is_empty());
        // a bundle on the command line saves in the user dir
        let r = resolve(Some(&envpak), &e);
        assert_eq!(r.save_dir, user.join("save/env"));
        // the save folder variables replace all of it
        let sd = t.0.join("saves");
        let e = env(Os::MacOs, &[("HOME", &home), ("C8H18_SAVE_DIR", &sd)], Some(exe.clone()), &t.0);
        assert_eq!(resolve(Some(&argdir), &e).save_dir, sd);
        assert_eq!(resolve(None, &e).save_dir, sd);
    }

    #[test]
    fn development_fallbacks() {
        let t = Tmp::new("dev");
        let home = t.0.join("home");
        let exe = Some(t.0.join("target/release/c8h18"));
        let e = env(Os::MacOs, &[("HOME", &home)], exe.clone(), &t.0);
        // nothing at all
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.path.clone()), (Origin::Default, t.0.join("resources")));
        assert_eq!(r.warnings.len(), 1);
        // resources/ with only lang/ (as in a fresh clone) does not count; assets/custom does
        std::fs::create_dir_all(t.0.join("resources/lang")).unwrap();
        std::fs::create_dir_all(t.0.join("assets/custom/DATA")).unwrap();
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.path.clone(), r.name.as_str()), (Origin::Dev, t.0.join("assets/custom"), "custom"));
        assert_eq!(r.save_dir, home.join("Library/Application Support/C8H18/save/custom"));
        // the original files win
        std::fs::create_dir_all(t.0.join("resources/DATA")).unwrap();
        let r = resolve(None, &e);
        assert_eq!((r.origin.clone(), r.path.clone(), r.name.as_str()), (Origin::Dev, t.0.join("resources"), "resources"));
        assert!(r.legacy_saves.contains(&t.0.join("resources/save")));
    }

    #[test]
    fn saves_are_copied_from_the_old_folder_once() {
        let t = Tmp::new("migrate");
        let home = t.0.join("home");
        let exe = t.0.join("bin/c8h18");
        touch(&t.0.join("bin").join(SHIPPED_PACKAGE), b"pk");
        let e = env(Os::MacOs, &[("HOME", &home)], Some(exe), &t.0);
        // a folder package in packages/ that used to save into its own save/
        let user = home.join("Library/Application Support/C8H18");
        std::fs::create_dir_all(user.join("packages/Folder/DATA")).unwrap();
        touch(&user.join(SETTINGS_FILE), b"package = Folder\n");
        let old = user.join("packages/Folder/save");
        touch(&old.join("config.dat"), b"cfg");
        touch(&old.join("sub/x.txt"), b"x");
        let r = resolve(None, &e);
        assert_eq!(migrate_saves(&r).unwrap(), Some(old.clone()));
        assert_eq!(std::fs::read(r.save_dir.join("config.dat")).unwrap(), b"cfg");
        assert_eq!(std::fs::read(r.save_dir.join("sub/x.txt")).unwrap(), b"x");
        assert!(old.join("config.dat").is_file(), "the old files stay");
        // not again once the new folder exists
        std::fs::write(old.join("config.dat"), b"newer").unwrap();
        assert_eq!(migrate_saves(&r).unwrap(), None);
        assert_eq!(std::fs::read(r.save_dir.join("config.dat")).unwrap(), b"cfg");
    }

    #[test]
    fn names_and_settings() {
        assert_eq!(sanitize_name(" a/b:c "), "a_b_c");
        assert_eq!(sanitize_name(".."), "package");
        assert_eq!(package_name(Path::new("/x/My Pack.C8PAK")), "My Pack");
        assert_eq!(package_name(Path::new("/x/assets/custom")), "custom");
        assert_eq!(setting("a = 1\n package = x # comment\n", "package").as_deref(), Some("x"));
        assert_eq!(setting("package = x\npackage =\n", "package"), None);
        assert_eq!(find_user_package(Path::new("/nonexistent"), "../x"), None);
    }
}
