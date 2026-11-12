//! Which of the six tracks are present (port addition, spec 02 §17).
//!
//! A track is offered when its level `MAPS/LEVEL0-t.DAT` and all of its own artifacts (terrain
//! textures, a sky, colour table, carousel model; see [`artifacts`]) are found through the normal
//! lookup (the package, any letter case). A track without a level is skipped silently (debug log
//! only); a track with a level but missing artifacts is discarded with one warn-level log line.
//! The check only asks whether the files exist, so it never prints the stdout
//! `warning: missing asset` lines. The original set has all six, so everything below is the
//! identity there. Track numbers here are 0-based (track t of the spec is index t - 1), as the
//! config's race number is. The debug paths (`--race N`, `--level`, `--sim`) do not go through
//! this and may still load an incomplete track, using the race loader's fallbacks.
use std::sync::OnceLock;

/// Number of track slots the game knows (names, lap tables, records, models 50..55).
pub const SLOTS: usize = 6;

/// The level of track `t` (0-based). Without it the track is not offered.
pub fn level_file(t: usize) -> String {
    format!("MAPS/LEVEL0-{}.DAT", t + 1)
}

/// The per-track artifacts track `t` (0-based) needs besides its level, each as a list of
/// alternatives (any one of them counts): terrain textures, the sky (palette `.DAT` or
/// true-colour `.PNG`), the colour table and the carousel model. The minimap pictures in the
/// shared `DATA/TRACK0-1` sheet are optional.
pub fn artifacts(t: usize) -> [Vec<String>; 4] {
    [
        vec![format!("DATA/TEXTU0-{t}.DAT")],
        vec![format!("DATA/SKY0-{t}.DAT"), format!("DATA/SKY0-{t}.PNG")],
        vec![format!("DATA/TABLE0-{t}.DAT")],
        vec![format!("OBJECTS/DATA/TRACK0-{t}.DAT")],
    ]
}

/// What the files say about one track slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// No level: not offered, silently.
    NoLevel,
    /// Level present, these artifacts missing (alternatives joined by " or "): discarded.
    Incomplete(Vec<String>),
    /// Level and all artifacts present: offered.
    Complete,
}

/// The status of track `t` according to `exists`.
pub fn status(t: usize, exists: &impl Fn(&str) -> bool) -> Status {
    if !exists(&level_file(t)) {
        return Status::NoLevel;
    }
    let missing: Vec<String> = artifacts(t)
        .iter()
        .filter(|alts| !alts.iter().any(|f| exists(f)))
        .map(|alts| alts.join(" or "))
        .collect();
    if missing.is_empty() { Status::Complete } else { Status::Incomplete(missing) }
}

/// The complete tracks according to `exists`, in track order. Logs (never prints) the others:
/// a debug line per track without a level, a warn line per discarded track, an error line
/// when none is left.
pub fn discover(exists: impl Fn(&str) -> bool) -> Vec<usize> {
    let mut v = Vec::new();
    for t in 0..SLOTS {
        match status(t, &exists) {
            Status::Complete => v.push(t),
            Status::NoLevel => log::debug!("track {}: no {}, not offered", t + 1, level_file(t)),
            Status::Incomplete(m) => log::warn!("track {} discarded: missing {}", t + 1, m.join(", ")),
        }
    }
    if v.is_empty() {
        log::error!("no complete track in the asset package: track selection and races are disabled");
    }
    v
}

static AVAILABLE: OnceLock<Vec<usize>> = OnceLock::new();

/// The available tracks (looked up once, on first use).
pub fn available() -> &'static [usize] {
    AVAILABLE.get_or_init(|| discover(crate::data::files::exists))
}

/// The first available track (the championship's first race).
pub fn first_in(avail: &[usize]) -> Option<usize> {
    avail.first().copied()
}

/// The next available track after `t`, if any.
pub fn next_in(avail: &[usize], t: usize) -> Option<usize> {
    avail.iter().copied().find(|&x| x > t)
}

/// The championship race ordinal of track `t` (0 for the first available track).
pub fn ordinal_in(avail: &[usize], t: usize) -> usize {
    avail.iter().position(|&x| x == t).unwrap_or(t)
}

/// `t` if it is available, else the next available track after it, else the first one
/// (`t` itself when none is available).
pub fn snap_in(avail: &[usize], t: usize) -> usize {
    if avail.contains(&t) {
        t
    } else {
        next_in(avail, t).or(first_in(avail)).unwrap_or(t)
    }
}

pub fn first() -> usize {
    first_in(available()).unwrap_or(0)
}
pub fn snap(t: usize) -> usize {
    snap_in(available(), t)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::files::{self, resolve_in};
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("c8-tracks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["MAPS", "DATA", "OBJECTS/DATA"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        d
    }

    fn put(d: &Path, files: &[&str]) {
        for f in files {
            std::fs::write(d.join(f), b"x").unwrap();
        }
    }

    /// Everything track index `t` needs, palette sky.
    fn full(t: usize) -> Vec<String> {
        vec![
            format!("MAPS/LEVEL0-{}.DAT", t + 1),
            format!("DATA/TEXTU0-{t}.DAT"),
            format!("DATA/SKY0-{t}.DAT"),
            format!("DATA/TABLE0-{t}.DAT"),
            format!("OBJECTS/DATA/TRACK0-{t}.DAT"),
        ]
    }

    fn found(d: &Path) -> Vec<usize> {
        discover(|f| resolve_in(d.to_path_buf(), f).is_some())
    }

    #[test]
    fn missing_levels_are_skipped() {
        let d = scratch("levels");
        // track 2 complete, in mixed case like a hand-made folder
        put(&d, &["MAPS/level0-2.dat", "DATA/textu0-1.dat", "DATA/Sky0-1.dat", "DATA/TABLE0-1.DAT", "objects/data/track0-1.dat"]);
        // track 1's artifacts without its level do not make it available
        put(&d, &full(0)[1..].iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(found(&d), vec![1]);
        assert_eq!(status(0, &|f| resolve_in(d.clone(), f).is_some()), Status::NoLevel);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_sky_discards_the_track() {
        let d = scratch("nosky");
        let f: Vec<String> = full(2).into_iter().filter(|f| !f.contains("SKY")).collect();
        put(&d, &f.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(found(&d), Vec::<usize>::new());
        assert_eq!(
            status(2, &|f| resolve_in(d.clone(), f).is_some()),
            Status::Incomplete(vec!["DATA/SKY0-2.DAT or DATA/SKY0-2.PNG".into()])
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn png_only_sky_counts() {
        let d = scratch("pngsky");
        let mut f: Vec<String> = full(3).into_iter().filter(|f| !f.contains("SKY")).collect();
        f.push("DATA/SKY0-3.PNG".into());
        put(&d, &f.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(found(&d), vec![3]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_carousel_model_discards_the_track() {
        let d = scratch("nomodel");
        for t in [0, 4] {
            put(&d, &full(t).iter().map(String::as_str).collect::<Vec<_>>());
        }
        std::fs::remove_file(d.join("OBJECTS/DATA/TRACK0-4.DAT")).unwrap();
        // also drop track 1's textures and table: both are named
        std::fs::remove_file(d.join("DATA/TEXTU0-0.DAT")).unwrap();
        std::fs::remove_file(d.join("DATA/TABLE0-0.DAT")).unwrap();
        assert_eq!(found(&d), Vec::<usize>::new());
        let ex = |f: &str| resolve_in(d.clone(), f).is_some();
        assert_eq!(status(4, &ex), Status::Incomplete(vec!["OBJECTS/DATA/TRACK0-4.DAT".into()]));
        assert_eq!(status(0, &ex), Status::Incomplete(vec!["DATA/TEXTU0-0.DAT".into(), "DATA/TABLE0-0.DAT".into()]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn discovery_prints_no_asset_warnings() {
        // an empty package: every track lacks its level; plus a level-only track
        let d = scratch("quiet");
        put(&d, &["MAPS/LEVEL0-6.DAT"]);
        let names: Vec<String> = (0..SLOTS).flat_map(|t| std::iter::once(level_file(t)).chain(artifacts(t).into_iter().flatten())).collect();
        let before: Vec<String> = files::warned().into_iter().filter(|w| names.contains(w)).collect();
        assert_eq!(found(&d), Vec::<usize>::new());
        assert_eq!(discover(|_| false), Vec::<usize>::new());
        let after: Vec<String> = files::warned().into_iter().filter(|w| names.contains(w)).collect();
        assert_eq!(before, after, "discovery must not go through warn_asset");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn repo(dir: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir)
    }

    #[test]
    fn full_original_set_is_found() {
        let res = repo("resources");
        if !res.join("MAPS").is_dir() {
            return;
        }
        assert_eq!(found(&res), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn custom_set_offers_tracks_one_and_two() {
        let c = repo("assets/custom");
        if !c.join("MAPS").is_dir() {
            return;
        }
        assert_eq!(found(&c), vec![0, 1]);
    }

    #[test]
    fn helpers() {
        let all = [0, 1, 2, 3, 4, 5];
        assert_eq!(first_in(&all), Some(0));
        assert_eq!(next_in(&all, 4), Some(5));
        assert_eq!(next_in(&all, 5), None);
        assert_eq!(snap_in(&all, 3), 3);
        assert_eq!(ordinal_in(&all, 3), 3);
        let some = [1, 4];
        assert_eq!(first_in(&some), Some(1));
        assert_eq!(next_in(&some, 1), Some(4));
        assert_eq!(snap_in(&some, 0), 1);
        assert_eq!(snap_in(&some, 2), 4);
        assert_eq!(snap_in(&some, 5), 1);
        assert_eq!(ordinal_in(&some, 4), 1);
        assert_eq!(snap_in(&[], 3), 3);
    }
}
