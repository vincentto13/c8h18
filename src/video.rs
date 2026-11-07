//! Port addition: display settings beyond the original (DETAIL OPTIONS), saved in `video.txt` in the
//! save directory. None of them affects the game itself.
use std::sync::Mutex;

/// Fog end per VIEW DISTANCE level, in cells: the original 16, then 32 and 48. The haze keeps the
/// original proportions (fog start = 3/4 of the end).
pub const VIEW_DISTANCES: [f32; 3] = [16.0, 32.0, 48.0];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Video {
    /// VIEW DISTANCE level, index into [`VIEW_DISTANCES`]
    pub view: usize,
    /// ANTI-ALIASING: 4× multisampling of the 3D view
    pub antialias: bool,
    /// SMOOTHING: filtered sky, sharp-bilinear scaling of the 640×480 menus and HUD
    pub smoothing: bool,
}

const DEFAULT: Video = Video { view: 2, antialias: true, smoothing: true };

static VIDEO: Mutex<Video> = Mutex::new(DEFAULT);

pub fn get() -> Video {
    *VIDEO.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn update(f: impl FnOnce(&mut Video)) {
    let mut v = VIDEO.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut v);
    v.view = v.view.min(VIEW_DISTANCES.len() - 1);
    save(&v);
}

impl Video {
    pub fn fog_end(&self) -> f32 {
        VIEW_DISTANCES[self.view.min(VIEW_DISTANCES.len() - 1)]
    }
    pub fn fog_start(&self) -> f32 {
        self.fog_end() * 0.75
    }
    /// Things (models, billboards) are drawn up to this distance (2D), like the terrain.
    pub fn draw_radius(&self) -> f32 {
        self.fog_end() + 1.5
    }
    /// Beyond the original distance models fade into the haze like the terrain (the original draws
    /// them unfogged, which only works with its short view).
    pub fn fog_models(&self) -> bool {
        self.view > 0
    }
}

fn path() -> std::path::PathBuf {
    crate::data::files::save_dir().join("video.txt")
}

fn save(v: &Video) {
    if cfg!(test) {
        return;
    }
    let onoff = |b: bool| if b { "on" } else { "off" };
    let t = format!("view_distance = {}\nantialias = {}\nsmoothing = {}\n", v.view, onoff(v.antialias), onoff(v.smoothing));
    let _ = std::fs::create_dir_all(crate::data::files::save_dir());
    if let Err(e) = std::fs::write(path(), t) {
        log::warn!("cannot write {}: {e}", path().display());
    }
}

fn parse(t: &str) -> Video {
    let mut v = DEFAULT;
    for line in t.lines() {
        let Some((k, val)) = line.split_once('=') else { continue };
        let (k, val) = (k.trim(), val.trim());
        match k {
            "view_distance" => v.view = val.parse::<usize>().unwrap_or(DEFAULT.view).min(VIEW_DISTANCES.len() - 1),
            "antialias" => v.antialias = val == "on",
            "smoothing" => v.smoothing = val == "on",
            _ => {}
        }
    }
    v
}

/// Read `video.txt` (defaults when missing).
pub fn load() {
    let v = std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or(DEFAULT);
    *VIDEO.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file() {
        assert_eq!(parse(""), DEFAULT);
        let v = parse("view_distance = 0\nantialias = off\nsmoothing = off\n");
        assert_eq!(v, Video { view: 0, antialias: false, smoothing: false });
        assert_eq!(parse("view_distance = 9").view, VIEW_DISTANCES.len() - 1);
        assert_eq!(v.fog_end(), 16.0);
        assert_eq!(v.fog_start(), 12.0);
        assert!(!v.fog_models());
    }
}
