//! UI strings (spec 02 appendix A), loaded at start-up from `lang/<code>.txt` in the data directory.
//!
//! File format: UTF-8, one `<key> = <text>` per line, `#` starts a comment line. Keys `000`..`193`
//! are the original string table; named keys (`language`, the controls-menu labels) are port
//! additions. Missing entries fall back to English.
use std::collections::HashMap;
use std::sync::OnceLock;

/// Language files in config order: the original five (0 EN, 1 DE, 2 FR, 3 ES, 4 IT), then the
/// port's additions (5 PL).
pub const CODES: [&str; 6] = ["en", "de", "fr", "es", "it", "pl"];
/// Number of strings in the original table.
pub const COUNT: usize = 194;

#[derive(Default)]
struct Lang {
    table: Vec<Option<String>>,
    named: HashMap<String, String>,
}

static LANGS: OnceLock<Vec<Lang>> = OnceLock::new();

fn parse(text: &str, file: &str) -> Result<Lang, String> {
    let mut l = Lang { table: vec![None; COUNT], named: HashMap::new() };
    for (no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("{file}:{}: expected `key = text`", no + 1))?;
        let (k, v) = (k.trim(), v.trim().to_string());
        if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) {
            let n: usize = k.parse().map_err(|_| format!("{file}:{}: bad key", no + 1))?;
            *l.table.get_mut(n).ok_or_else(|| format!("{file}:{}: string {n} out of range", no + 1))? = Some(v);
        } else {
            l.named.insert(k.to_string(), v);
        }
    }
    Ok(l)
}

/// Load every language file. English is required; a missing other language falls back to English.
pub fn load() -> Result<(), String> {
    let mut langs = Vec::new();
    for code in CODES {
        let rel = format!("lang/{code}.txt");
        let lang = match super::files::read_raw(&rel) {
            Ok(d) => parse(&String::from_utf8_lossy(&d), &rel)?,
            Err(e) if code == "en" => return Err(e),
            Err(_) => {
                log::warn!("{rel} not found, using English");
                Lang::default()
            }
        };
        langs.push(lang);
    }
    let _ = LANGS.set(langs);
    Ok(())
}

fn langs() -> &'static [Lang] {
    LANGS.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// Number of selectable languages.
pub fn count() -> usize {
    CODES.len()
}

/// String n (0..193) in language lang, UTF-8; English when the language lacks it.
pub fn get(lang: usize, n: usize) -> &'static str {
    let l = langs();
    let pick = |i: usize| l.get(i).and_then(|x| x.table.get(n)).and_then(|s| s.as_deref());
    pick(lang).or_else(|| pick(0)).unwrap_or("")
}

/// A named entry (port additions) in language lang; English when the language lacks it.
pub fn named(lang: usize, key: &str) -> &'static str {
    let l = langs();
    let pick = |i: usize| l.get(i).and_then(|x| x.named.get(key)).map(|s| s.as_str());
    pick(lang).or_else(|| pick(0)).unwrap_or("")
}

/// The language's own name for the language menu.
pub fn language_name(lang: usize) -> &'static str {
    match langs().get(lang).and_then(|x| x.named.get("language")) {
        Some(s) => s,
        None => CODES.get(lang).copied().unwrap_or(""),
    }
}

/// Convert UTF-8 text to code page 437 bytes for the fonts (Ä Ö Ü Ñ etc.). Polish letters have
/// no glyphs in the original fonts and are drawn as their base letters for now.
pub fn to_cp437(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            'Ä' => 0x8E,
            'Ö' => 0x99,
            'Ü' => 0x9A,
            'Ñ' => 0xA5,
            'ä' => 0x84,
            'ö' => 0x94,
            'ü' => 0x81,
            'ñ' => 0xA4,
            'Ą' => b'A',
            'Ć' => b'C',
            'Ę' => b'E',
            'Ł' => b'L',
            'Ń' => b'N',
            'Ó' => b'O',
            'Ś' => b'S',
            'Ź' | 'Ż' => b'Z',
            'ą' => b'a',
            'ć' => b'c',
            'ę' => b'e',
            'ł' => b'l',
            'ń' => b'n',
            'ó' => b'o',
            'ś' => b's',
            'ź' | 'ż' => b'z',
            c if (c as u32) < 128 => c as u8,
            _ => b'?',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language file loads, has the whole table and every named key, and every entry fits
    /// the 640-wide screen in the large menu font.
    #[test]
    fn language_files_complete() {
        super::super::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        load().expect("load");
        let font = super::super::sprites::Font::load("olfnt0-1", true).expect("font");
        let en = &langs()[0];
        let mut problems = Vec::new();
        for (i, l) in langs().iter().enumerate() {
            let code = CODES[i];
            for n in 0..COUNT {
                match &l.table[n] {
                    None => problems.push(format!("{code}: {n:03} missing")),
                    Some(s) => {
                        let w = crate::gfx::canvas::Canvas::text_width(&font, &to_cp437(s), 0);
                        if w > 640 {
                            println!("{code} {n:03} {w}px: {s}");
                        }
                        if to_cp437(s).contains(&b'?') && !s.contains('?') {
                            problems.push(format!("{code}: {n:03} has characters without glyphs: {s}"));
                        }
                    }
                }
            }
            for k in en.named.keys() {
                if !l.named.contains_key(k) {
                    problems.push(format!("{code}: `{k}` missing"));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
