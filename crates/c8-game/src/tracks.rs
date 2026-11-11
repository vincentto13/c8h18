//! Which of the six tracks are present (port addition, spec 02 §17).
//!
//! A track counts as available when all its own files can be found through the normal lookup
//! (`--override` folder first, then the data folder); see [`required_files`]. The original set
//! has all six, so everything below is the identity there. Track numbers here are 0-based
//! (track t of the spec is index t - 1), as the config's race number is.
use std::sync::OnceLock;

/// Number of track slots the game knows (names, lap tables, records, models 50..55).
pub const SLOTS: usize = 6;

/// The files track `t` (0-based) must have to be offered: its level and the terrain textures,
/// sky and colour table its race loads. Its preview model (OBJECTS/DATA/TRACK0-t.DAT) and its
/// minimap pictures in DATA/TRACK0-1 are optional: without them the carousel and HUD draw nothing.
pub fn required_files(t: usize) -> [String; 4] {
    [
        format!("MAPS/LEVEL0-{}.DAT", t + 1),
        format!("DATA/TEXTU0-{t}.DAT"),
        format!("DATA/SKY0-{t}.DAT"),
        format!("DATA/TABLE0-{t}.DAT"),
    ]
}

/// The tracks whose required files all exist according to `exists`, in track order.
pub fn discover(exists: impl Fn(&str) -> bool) -> Vec<usize> {
    (0..SLOTS).filter(|&t| required_files(t).iter().all(|f| exists(f))).collect()
}

static AVAILABLE: OnceLock<Vec<usize>> = OnceLock::new();

/// The available tracks (looked up once, on first use).
pub fn available() -> &'static [usize] {
    AVAILABLE.get_or_init(|| {
        let v = discover(|f| crate::data::files::resolve(f).is_some_and(|p| p.is_file()));
        if v.len() < SLOTS {
            for t in (0..SLOTS).filter(|t| !v.contains(t)) {
                let missing: Vec<String> = required_files(t).into_iter().filter(|f| crate::data::files::resolve(f).is_none()).collect();
                log::info!("track {} not available (missing {})", t + 1, missing.join(", "));
            }
        }
        if v.is_empty() {
            log::error!("no track is available: races cannot be started");
        }
        v
    })
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
    use crate::data::files::resolve_in;

    #[test]
    fn discovery_in_a_partial_data_folder() {
        let d = std::env::temp_dir().join(format!("c8-tracks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["MAPS", "DATA"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        // track 2 (index 1) complete, in mixed case like a hand-made folder
        for f in ["MAPS/level0-2.dat", "DATA/TEXTU0-1.DAT", "DATA/sky0-1.dat", "DATA/TABLE0-1.DAT"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        // track 4 (index 3) without its sky: not available
        for f in ["MAPS/LEVEL0-4.DAT", "DATA/TEXTU0-3.DAT", "DATA/TABLE0-3.DAT"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        // the front-end colour table alone does not make track 1 available
        std::fs::write(d.join("DATA/TABLE0-0.DAT"), b"x").unwrap();
        let found = discover(|f| resolve_in(d.clone(), f).is_some());
        assert_eq!(found, vec![1]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn full_original_set_is_found() {
        let res = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources");
        if !res.join("MAPS").is_dir() {
            return;
        }
        assert_eq!(discover(|f| resolve_in(res.clone(), f).is_some()), vec![0, 1, 2, 3, 4, 5]);
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
