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
static FORCED: OnceLock<usize> = OnceLock::new();

/// Use this language regardless of the config (game option `--lang <code>`); not saved.
pub fn force_language(code: &str) -> Result<(), String> {
    let n = CODES.iter().position(|c| c.eq_ignore_ascii_case(code)).ok_or_else(|| format!("unknown language `{code}` (known: {})", CODES.join(", ")))?;
    let _ = FORCED.set(n);
    Ok(())
}

pub fn forced_language() -> Option<usize> {
    FORCED.get().copied()
}

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

/// A language file kept line by line, so that editing an entry keeps the comments, order and
/// layout of everything else.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct LangFile {
    pub lines: Vec<String>,
}

/// The key as the game reads it: numeric keys as three digits (`7` and `007` are the same).
pub fn normal_key(k: &str) -> String {
    let k = k.trim();
    match k.parse::<usize>() {
        Ok(n) if k.bytes().all(|b| b.is_ascii_digit()) => format!("{n:03}"),
        _ => k.to_string(),
    }
}

/// The table index of a numeric key.
pub fn table_index(key: &str) -> Option<usize> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) { None } else { key.parse().ok() }
}

impl LangFile {
    pub fn parse(text: &str) -> LangFile {
        LangFile { lines: text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect() }
    }
    pub fn encode(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
    /// (key, text) of a line, if it is an entry.
    fn entry(line: &str) -> Option<(String, &str)> {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            return None;
        }
        let (k, v) = t.split_once('=')?;
        Some((normal_key(k), v.trim()))
    }
    /// Lines that are neither entries, comments nor blank (the game refuses the file).
    pub fn bad_lines(&self) -> Vec<usize> {
        self.lines.iter().enumerate().filter(|(_, l)| { let t = l.trim(); !t.is_empty() && !t.starts_with('#') && !t.contains('=') }).map(|(i, _)| i).collect()
    }
    /// The keys in file order (later duplicates included).
    pub fn keys(&self) -> Vec<String> {
        self.lines.iter().filter_map(|l| Self::entry(l)).map(|(k, _)| k).collect()
    }
    fn line_of(&self, key: &str) -> Option<usize> {
        let key = normal_key(key);
        // the game keeps the last of duplicate entries
        self.lines.iter().rposition(|l| Self::entry(l).is_some_and(|(k, _)| k == key))
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.line_of(key).and_then(|i| Self::entry(&self.lines[i])).map(|(_, v)| v)
    }
    /// Set an entry. A new entry goes after the nearest key that precedes it in `order` (the
    /// English file's keys), else at the end.
    pub fn set(&mut self, key: &str, text: &str, order: &[String]) {
        let key = normal_key(key);
        let text = text.replace(['\n', '\r'], " ");
        if let Some(i) = self.line_of(&key) {
            let old = &self.lines[i];
            let k = old.split_once('=').map(|(k, _)| k.trim_end()).unwrap_or(&key);
            self.lines[i] = format!("{k} = {}", text.trim());
            return;
        }
        let line = format!("{key} = {}", text.trim());
        let pos = order.iter().position(|k| *k == key);
        let after = pos.and_then(|p| order[..p].iter().rev().find_map(|k| self.line_of(k)));
        match after {
            Some(i) => self.lines.insert(i + 1, line),
            None => self.lines.push(line),
        }
    }
    /// Remove every entry of a key.
    pub fn remove(&mut self, key: &str) {
        let key = normal_key(key);
        self.lines.retain(|l| Self::entry(l).is_none_or(|(k, _)| k != key));
    }
}

/// The `$` markup codes in a string (`$` + letter + optional number, spec 02 §2.3), in order.
pub fn markup_codes(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let mut end = i + 1;
            if end < b.len() && b[end].is_ascii_alphabetic() {
                end += 1;
                while end < b.len() && b[end].is_ascii_digit() {
                    end += 1;
                }
            }
            out.push(s[i..end].to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Characters of a string the original fonts have no glyph for.
pub fn missing_glyphs(s: &str) -> Vec<char> {
    let mut v: Vec<char> = s.chars().filter(|&c| c != '?' && to_cp437(&c.to_string()) == b"?").collect();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language file loads, has the whole table and every named key, and every entry has
    /// glyphs in the fonts. (The game crate checks that the entries fit the screen.)
    #[test]
    fn language_files_complete() {
        crate::files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        load().expect("load");
        let en = &langs()[0];
        let mut problems = Vec::new();
        for (i, l) in langs().iter().enumerate() {
            let code = CODES[i];
            for n in 0..COUNT {
                match &l.table[n] {
                    None => problems.push(format!("{code}: {n:03} missing")),
                    Some(s) => {
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

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn lang_file_edits_keep_layout() {
        let mut f = LangFile::parse("# head\n\nlanguage = X\n000 = NULL\n002 = TWO\n");
        let order: Vec<String> = ["language", "000", "001", "002"].map(String::from).to_vec();
        f.set("1", "ONE", &order);
        f.set("002", "DEUX", &order);
        assert_eq!(f.encode(), "# head\n\nlanguage = X\n000 = NULL\n001 = ONE\n002 = DEUX\n");
        assert_eq!(f.get("001"), Some("ONE"));
        f.remove("000");
        assert_eq!(f.keys(), ["language", "001", "002"]);
        assert_eq!(markup_codes("$S1HELLO $G WORLD$"), ["$S1", "$G", "$"]);
        assert_eq!(missing_glyphs("ŁÓDŹ €"), vec!['€']);
    }
}
