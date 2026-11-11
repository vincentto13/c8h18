//! Port addition: true-colour skies (spec 01 §4.1). Track N + 1 may have `DATA/SKY0-N.PNG`, a
//! cylindrical panorama in true colour, and `DATA/SKY0-N.TXT`, a small text file with the
//! panorama's placement and the sun. When the PNG is present it replaces the palette sky
//! `DATA/SKY0-N.DAT`; without it nothing changes.
//!
//! Sidecar format (UTF-8, one `key = value` per line, `#` starts a comment, keys in any case,
//! numbers separated by spaces or commas; every key is optional):
//!
//! ```text
//! repeat = 1            # copies of the image around a full turn (1..16)
//! azimuth = 0           # azimuth of the image's left edge, degrees clockwise from map north
//! top = 90              # elevation of the top edge, degrees (-90..90)
//! bottom = 0            # elevation of the bottom edge, degrees (below top)
//! horizon_blend = 4     # degrees above the horizon over which the sky fades into the haze colour
//! sun = 248 13          # sun azimuth and elevation in degrees; no `sun` line = no sun
//! sun_size = 1.2        # angular radius of the sun disk, degrees (0.1..10)
//! sun_colour = 255 236 200   # 8-bit RGB
//! glare = 1             # strength of bloom, lens flare and veil (0..4, 0 = none)
//! ```
//!
//! Map north is the direction of decreasing map row (camera yaw 0), east (azimuth 90) is
//! increasing x.
use crate::files;

/// The sun of a true-colour sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// degrees clockwise from map north
    pub azimuth: f32,
    /// degrees above the horizon
    pub elevation: f32,
    /// angular radius of the disk in degrees
    pub size: f32,
    pub colour: [u8; 3],
    /// bloom, flare and veil strength (0 = none)
    pub glare: f32,
}

impl Sun {
    /// Unit direction towards the sun in world axes (x, height, row).
    pub fn direction(&self) -> [f32; 3] {
        let (az, el) = (self.azimuth.to_radians(), self.elevation.to_radians());
        [az.sin() * el.cos(), el.sin(), -az.cos() * el.cos()]
    }
}

/// The placement of the panorama and its sun (the sidecar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyParams {
    pub repeat: u32,
    /// azimuth of the left edge, degrees
    pub azimuth: f32,
    /// elevations of the top and bottom edges, degrees
    pub top: f32,
    pub bottom: f32,
    /// fade into the haze colour over this many degrees above the horizon (0 = none)
    pub horizon_blend: f32,
    pub sun: Option<Sun>,
}

impl Default for SkyParams {
    fn default() -> Self {
        SkyParams { repeat: 1, azimuth: 0.0, top: 90.0, bottom: 0.0, horizon_blend: 4.0, sun: None }
    }
}

const SUN_DEFAULT: Sun = Sun { azimuth: 0.0, elevation: 30.0, size: 1.0, colour: [255, 240, 215], glare: 1.0 };

/// Parse a sidecar. Unknown keys, malformed or out-of-range values are errors.
pub fn parse_params(text: &str) -> Result<SkyParams, String> {
    let mut p = SkyParams::default();
    let mut sun_pos: Option<(f32, f32)> = None;
    let mut sun = SUN_DEFAULT;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |m: &str| format!("line {}: {m}", n + 1);
        let (k, v) = line.split_once('=').ok_or_else(|| err("expected `key = value`"))?;
        let key = k.trim().to_ascii_lowercase();
        let nums: Vec<f32> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f32>().ok().filter(|x| x.is_finite()))
            .collect::<Option<Vec<f32>>>()
            .ok_or_else(|| err(&format!("bad number in `{}`", v.trim())))?;
        let want = |count: usize| -> Result<(), String> {
            if nums.len() == count { Ok(()) } else { Err(err(&format!("`{key}` takes {count} number(s)"))) }
        };
        let range = |x: f32, lo: f32, hi: f32| -> Result<f32, String> {
            if (lo..=hi).contains(&x) { Ok(x) } else { Err(err(&format!("`{key}` must be within {lo}..{hi}"))) }
        };
        match key.as_str() {
            "repeat" => {
                want(1)?;
                let r = range(nums[0], 1.0, 16.0)?;
                if r.fract() != 0.0 {
                    return Err(err("`repeat` must be a whole number"));
                }
                p.repeat = r as u32;
            }
            "azimuth" => {
                want(1)?;
                p.azimuth = nums[0].rem_euclid(360.0);
            }
            "top" => {
                want(1)?;
                p.top = range(nums[0], -90.0, 90.0)?;
            }
            "bottom" => {
                want(1)?;
                p.bottom = range(nums[0], -90.0, 90.0)?;
            }
            "horizon_blend" => {
                want(1)?;
                p.horizon_blend = range(nums[0], 0.0, 45.0)?;
            }
            "sun" => {
                want(2)?;
                sun_pos = Some((nums[0].rem_euclid(360.0), range(nums[1], -10.0, 90.0)?));
            }
            "sun_size" => {
                want(1)?;
                sun.size = range(nums[0], 0.1, 10.0)?;
            }
            "sun_colour" | "sun_color" => {
                want(3)?;
                for (i, &c) in nums.iter().enumerate() {
                    sun.colour[i] = range(c, 0.0, 255.0)?.round() as u8;
                }
            }
            "glare" => {
                want(1)?;
                sun.glare = range(nums[0], 0.0, 4.0)?;
            }
            _ => return Err(err(&format!("unknown key `{key}`"))),
        }
    }
    if p.bottom >= p.top {
        return Err("`bottom` must be below `top`".into());
    }
    p.sun = sun_pos.map(|(azimuth, elevation)| Sun { azimuth, elevation, ..sun });
    Ok(p)
}

/// Largest accepted panorama side (a common GPU texture limit).
pub const MAX_SIDE: u32 = 8192;

/// Decode a PNG (any colour type and depth) to (width, height, RGBA8).
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut rd = dec.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (rd.info().width, rd.info().height);
    if w < 2 || h < 2 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}x{h} is outside 2x2..{MAX_SIDE}x{MAX_SIDE}"));
    }
    let mut buf = vec![0; rd.output_buffer_size().ok_or("PNG too large")?];
    let info = rd.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let n = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..n * 4].to_vec(),
        png::ColorType::Rgb => buf[..n * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..n * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..n].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok((w, h, rgba))
}

/// A loaded true-colour sky.
#[derive(Clone, Debug)]
pub struct TrueSky {
    pub w: u32,
    pub h: u32,
    /// RGBA8, row-major, alpha ignored
    pub rgba: Vec<u8>,
    pub params: SkyParams,
}

/// Build a sky from the PNG bytes and the sidecar text (if any).
pub fn from_bytes(png_bytes: &[u8], sidecar: Option<&str>) -> Result<TrueSky, String> {
    let params = match sidecar {
        Some(t) => parse_params(t).map_err(|e| format!("sidecar: {e}"))?,
        None => SkyParams::default(),
    };
    let (w, h, rgba) = decode_png(png_bytes)?;
    Ok(TrueSky { w, h, rgba, params })
}

/// The true-colour sky of track set `n` (0-based) from the package: `None` without
/// `DATA/SKY0-n.PNG` (silently: the palette sky is the normal case), or after one warning when
/// the PNG or its sidecar `DATA/SKY0-n.TXT` cannot be used.
pub fn load(n: usize) -> Option<TrueSky> {
    load_with(n, &files::exists, &files::read_raw)
}

/// [`load`] with the file lookups given (for tests).
fn load_with(n: usize, exists: &dyn Fn(&str) -> bool, read: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Option<TrueSky> {
    let png_rel = format!("data/sky0-{n}.png");
    let txt_rel = format!("data/sky0-{n}.txt");
    if !exists(&png_rel) {
        return None;
    }
    let fallback = format!("using the palette sky DATA/SKY0-{n}.DAT");
    let sidecar = if exists(&txt_rel) {
        match read(&txt_rel).and_then(|b| String::from_utf8(b).map_err(|e| e.to_string())) {
            Ok(t) => Some(t),
            Err(e) => {
                files::warn_asset(&txt_rel, Some(&e), &fallback);
                return None;
            }
        }
    } else {
        None
    };
    let params = match sidecar.as_deref().map(parse_params).transpose() {
        Ok(p) => p.unwrap_or_default(),
        Err(e) => {
            files::warn_asset(&txt_rel, Some(&e), &fallback);
            return None;
        }
    };
    match read(&png_rel).and_then(|b| decode_png(&b)) {
        Ok((w, h, rgba)) => Some(TrueSky { w, h, rgba, params }),
        Err(e) => {
            files::warn_asset(&png_rel, Some(&e), &fallback);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let mut wr = e.write_header().unwrap();
            let data: Vec<u8> = (0..w * h * 3).map(|i| (i % 251) as u8).collect();
            wr.write_image_data(&data).unwrap();
        }
        out
    }

    #[test]
    fn sidecar_parsing() {
        assert_eq!(parse_params("").unwrap(), SkyParams::default());
        let p = parse_params(
            "# comment\nRepeat = 2\nazimuth = -90\ntop = 80 # trailing\nbottom=-5\nhorizon_blend = 6\n\
             sun = 250, 14\nsun_size = 1.5\nsun_colour = 255 230 190\nglare = 0.5\n",
        )
        .unwrap();
        assert_eq!(p.repeat, 2);
        assert_eq!(p.azimuth, 270.0);
        assert_eq!((p.top, p.bottom, p.horizon_blend), (80.0, -5.0, 6.0));
        let s = p.sun.unwrap();
        assert_eq!((s.azimuth, s.elevation, s.size, s.colour, s.glare), (250.0, 14.0, 1.5, [255, 230, 190], 0.5));
        // sun settings without a `sun` line: no sun
        assert!(parse_params("sun_size = 2\n").unwrap().sun.is_none());
        // the direction: azimuth 90 = +x, 0 = -row
        let d = Sun { azimuth: 90.0, elevation: 0.0, ..SUN_DEFAULT }.direction();
        assert!((d[0] - 1.0).abs() < 1e-6 && d[1].abs() < 1e-6 && d[2].abs() < 1e-6);
        let d = Sun { azimuth: 0.0, elevation: 90.0, ..SUN_DEFAULT }.direction();
        assert!((d[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sidecar_errors() {
        for bad in [
            "colour = 1",
            "repeat = 0",
            "repeat = 1.5",
            "sun = 10",
            "sun = 10 x",
            "top = 10\nbottom = 20",
            "glare = 9",
            "sun_colour = 1 2",
            "just text",
            "top = nan",
        ] {
            assert!(parse_params(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn png_and_fallback() {
        let sky = from_bytes(&png_bytes(8, 4), Some("sun = 1 2")).unwrap();
        assert_eq!((sky.w, sky.h, sky.rgba.len()), (8, 4, 8 * 4 * 4));
        assert_eq!(&sky.rgba[..8], &[0, 1, 2, 255, 3, 4, 5, 255]);
        assert!(from_bytes(b"not a png", None).is_err());
        assert!(from_bytes(&png_bytes(8, 4), Some("bogus = 1")).is_err());
        assert!(from_bytes(&png_bytes(1, 1), None).is_err());

        // through the package lookups: missing PNG = no sky (no warning), bad files = warning, None
        let mut fs = std::collections::HashMap::new();
        fs.insert("data/sky0-3.png".to_string(), png_bytes(16, 8));
        fs.insert("data/sky0-3.txt".to_string(), b"sun = 180 20\nglare = 2\n".to_vec());
        fs.insert("data/sky0-2.png".to_string(), png_bytes(4, 4));
        fs.insert("data/sky0-4.png".to_string(), b"garbage".to_vec());
        fs.insert("data/sky0-5.png".to_string(), png_bytes(16, 8));
        fs.insert("data/sky0-5.txt".to_string(), b"sun = 180\n".to_vec());
        let exists = |r: &str| fs.contains_key(r);
        let read = |r: &str| fs.get(r).cloned().ok_or_else(|| format!("{r}: missing"));
        let ok = load_with(3, &exists, &read).expect("sky 3");
        assert_eq!((ok.w, ok.h), (16, 8));
        assert_eq!(ok.params.sun.unwrap().glare, 2.0);
        assert!(load_with(2, &exists, &read).unwrap().params.sun.is_none(), "PNG without sidecar: defaults");
        assert!(load_with(1, &exists, &read).is_none());
        assert!(!files::warned().contains(&"DATA/SKY0-1.PNG".to_string()));
        assert!(load_with(4, &exists, &read).is_none());
        assert!(files::warned().contains(&"DATA/SKY0-4.PNG".to_string()));
        assert!(load_with(5, &exists, &read).is_none());
        assert!(files::warned().contains(&"DATA/SKY0-5.TXT".to_string()));
    }
}
