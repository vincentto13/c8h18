//! THIRD-PARTY-NOTICES.txt: every crate compiled into the game binary, from `cargo metadata`.
//!
//! The crates are the game package's normal and build dependencies, transitively, on the target
//! platform (`--filter-platform`), without dev dependencies and without the editor's. `cargo
//! metadata` resolves features for the whole workspace, so the set is checked against `cargo tree
//! -p c8-game`, which resolves them for the game alone; crates only the editor's features pull in
//! are left out. For each crate: name, version, licence expression, repository and the licence
//! files found in its source folder (`LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*`, `UNLICENSE*`,
//! `COPYRIGHT*`, the manifest's `license-file`, files in `LICENSES/`). Apache-2.0 `NOTICE` files
//! are included like the rest. Identical texts are printed once and referred to by number.
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

pub struct Notices {
    pub text: String,
    /// Number of third-party crates listed.
    pub crates: usize,
    pub warnings: Vec<String>,
    /// The Rust standard library's full copyright file, to ship next to the notices.
    pub std_copyright: Option<PathBuf>,
}

struct Crate {
    name: String,
    version: String,
    license: String,
    repository: String,
    authors: Vec<String>,
    /// (file name relative to the crate folder, text number)
    files: Vec<(String, usize)>,
    /// for a crate without licence files: the standard text used (SPDX id, text number)
    standard: Option<(String, usize)>,
}

fn is_licence_name(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"].iter().any(|p| u.starts_with(p))
}

/// The licence files of a crate folder (relative names, sorted).
pub fn licence_files(dir: &Path, license_file: Option<&str>) -> Vec<String> {
    let mut out = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if p.is_file() && is_licence_name(&name) {
                out.insert(name);
            } else if p.is_dir() && name.eq_ignore_ascii_case("LICENSES") {
                for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                    if f.path().is_file() {
                        out.insert(format!("{name}/{}", f.file_name().to_string_lossy()));
                    }
                }
            }
        }
    }
    if let Some(lf) = license_file
        && dir.join(lf).is_file()
    {
        out.insert(lf.replace('\\', "/"));
    }
    out.into_iter().collect()
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// `(name, version)` of every package in `cargo tree -p c8-game` (normal and build edges).
fn tree_set(root: &Path, target: &str) -> Result<BTreeSet<(String, String)>, String> {
    let out = crate::output(crate::cargo().current_dir(root).args(["tree", "--quiet", "-p", crate::GAME_PACKAGE, "-e", "normal,build", "--target", target, "--prefix", "none", "--format", "{p}"]))?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            let ver = it.next()?.strip_prefix('v')?.to_string();
            Some((name, ver))
        })
        .collect())
}

fn normalise(t: &str) -> String {
    t.replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string()
}

/// The standard texts the Rust toolchain ships (`<sysroot>/share/doc/rust/licenses/<SPDX>.txt`),
/// used for crates that ship no licence file.
fn standard_text(doc: Option<&Path>, expr: &str) -> Option<(String, String)> {
    let doc = doc?;
    let ids: Vec<&str> = expr.split(|c: char| c == '/' || c == '(' || c == ')' || c.is_whitespace()).filter(|t| !t.is_empty() && *t != "OR" && *t != "AND").collect();
    // prefer MIT, then Apache-2.0, then whatever the toolchain has
    let mut order: Vec<&str> = ["MIT", "Apache-2.0"].into_iter().filter(|p| ids.contains(p)).collect();
    order.extend(ids.iter().copied());
    order.into_iter().find_map(|id| std::fs::read_to_string(doc.join("licenses").join(format!("{id}.txt"))).ok().map(|t| (id.to_string(), normalise(&t))))
}

pub fn generate(root: &Path, target: &str) -> Result<Notices, String> {
    let doc = crate::output(std::process::Command::new("rustc").args(["--print", "sysroot"])).ok().map(|s| PathBuf::from(s.trim()).join("share/doc/rust")).filter(|d| d.is_dir());
    let meta = crate::output(crate::cargo().current_dir(root).args(["metadata", "--quiet", "--format-version", "1", "--filter-platform", target]))?;
    let meta: Value = serde_json::from_str(&meta).map_err(|e| format!("cargo metadata: {e}"))?;
    let packages: HashMap<String, &Value> = meta["packages"].as_array().ok_or("cargo metadata: no packages")?.iter().map(|p| (s(p, "id"), p)).collect();
    let members: BTreeSet<String> = meta["workspace_members"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    let game = members.iter().find(|id| packages.get(*id).is_some_and(|p| s(p, "name") == crate::GAME_PACKAGE)).ok_or("the game package is not in the workspace")?.clone();
    let nodes: HashMap<String, &Value> = meta["resolve"]["nodes"].as_array().ok_or("cargo metadata: no resolve graph")?.iter().map(|n| (s(n, "id"), n)).collect();
    // walk normal and build edges from the game
    let mut seen = BTreeSet::from([game.clone()]);
    let mut queue = VecDeque::from([game.clone()]);
    while let Some(id) = queue.pop_front() {
        let Some(n) = nodes.get(&id) else { continue };
        for d in n["deps"].as_array().into_iter().flatten() {
            let wanted = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null() || k["kind"].as_str() == Some("build"));
            let pkg = s(d, "pkg");
            if wanted && seen.insert(pkg.clone()) {
                queue.push_back(pkg);
            }
        }
    }
    let tree = tree_set(root, target)?;
    let mut warnings = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut text_ids: HashMap<String, usize> = HashMap::new();
    let mut crates = Vec::new();
    let mut in_meta = BTreeSet::new();
    for id in &seen {
        if members.contains(id) {
            continue;
        }
        let p = packages[id];
        let (name, version) = (s(p, "name"), s(p, "version"));
        in_meta.insert((name.clone(), version.clone()));
        if !tree.contains(&(name.clone(), version.clone())) {
            // only pulled in by features another workspace member (the editor) turns on
            continue;
        }
        let dir = PathBuf::from(s(p, "manifest_path")).parent().map(Path::to_path_buf).unwrap_or_default();
        let lf = p.get("license_file").and_then(Value::as_str);
        let mut files = Vec::new();
        for f in licence_files(&dir, lf) {
            let raw = std::fs::read(dir.join(&f)).map_err(|e| format!("{}: {e}", dir.join(&f).display()))?;
            let t = normalise(&String::from_utf8_lossy(&raw));
            let n = *text_ids.entry(t.clone()).or_insert_with(|| {
                texts.push(t);
                texts.len()
            });
            files.push((f, n));
        }
        let mut license = s(p, "license");
        if license.is_empty() {
            license = lf.map(|f| format!("see {f}")).unwrap_or_else(|| "(not stated)".into());
        }
        let mut standard = None;
        if files.is_empty() {
            match standard_text(doc.as_deref(), &license) {
                Some((id, t)) => {
                    let n = *text_ids.entry(t.clone()).or_insert_with(|| {
                        texts.push(t);
                        texts.len()
                    });
                    standard = Some((id, n));
                }
                None => warnings.push(format!("{name} {version} ({license}): no licence file in {} and no standard text for it", dir.display())),
            }
        }
        let authors = p["authors"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        crates.push(Crate { name, version, license, repository: s(p, "repository"), authors, files, standard });
    }
    for (n, v) in &tree {
        if !in_meta.contains(&(n.clone(), v.clone())) && !members.iter().any(|m| packages.get(m).is_some_and(|p| s(p, "name") == *n)) {
            warnings.push(format!("{n} {v} is in `cargo tree` but not in the metadata graph"));
        }
    }
    crates.sort_by(|a, b| (a.name.as_str(), a.version.as_str()).cmp(&(b.name.as_str(), b.version.as_str())));

    let version = s(packages[&game], "version");
    let mut o = String::new();
    let rule = "=".repeat(78);
    let thin = "-".repeat(78);
    o += &format!("{} {version}: third-party notices\n{rule}\n\n", crate::APP_NAME);
    o += &format!(
        "{} is free software, licensed under the GNU General Public License version 3 or (at your\n\
         option) any later version (GPL-3.0-or-later); the licence text is in COPYING. The licence of\n\
         the game's assets (c8h18.c8pak) is in ASSETS-LICENSE.\n\n\
         The game program is built with the Rust toolchain from the open-source crates listed below:\n\
         every crate compiled into the game binary ({target}; normal and build dependencies, from\n\
         `cargo metadata`). This file is generated by `cargo xtask notices`. Each crate is followed by\n\
         the licence files found in its source; identical texts are printed once, under \"Licence\n\
         texts\" at the end, and referred to by number.\n\n",
        crate::APP_NAME
    );
    o += &format!("{} crates\n{thin}\n", crates.len());
    let w = crates.iter().map(|c| c.name.len() + c.version.len() + 1).max().unwrap_or(10);
    for c in &crates {
        o += &format!("{:<w$}  {}\n", format!("{} {}", c.name, c.version), c.license);
    }
    o += &format!("\n\nCrates\n{rule}\n");
    for c in &crates {
        o += &format!("\n{} {}\n{thin}\nLicence:    {}\n", c.name, c.version, c.license);
        if !c.repository.is_empty() {
            o += &format!("Repository: {}\n", c.repository);
        }
        if !c.authors.is_empty() {
            o += &format!("Authors:    {}\n", c.authors.join(", "));
        }
        if let Some((id, n)) = &c.standard {
            o += &format!("File:       (none in the crate's source) standard {id} text: text [{n}], copyright the authors above\n");
        }
        for (f, n) in &c.files {
            o += &format!("File:       {f}: text [{n}]\n");
        }
    }
    // the standard library, linked into every Rust program
    let mut std_copyright = None;
    let mut std_texts: Vec<usize> = Vec::new();
    o += &format!("\n\nThe Rust standard library\n{rule}\n\n");
    o += "The Rust standard library, linked into the game, is licensed under the Apache License,\n\
          Version 2.0 or the MIT license, at your option, with the exceptions and the out-of-tree\n\
          crates listed in its copyright file, shipped as RUST-STD-COPYRIGHT.html next to this file.\n\
          Copyright: The Rust Project Developers (see https://thanks.rust-lang.org).\n";
    match &doc {
        Some(doc) => {
            if doc.join("COPYRIGHT-library.html").is_file() {
                std_copyright = Some(doc.join("COPYRIGHT-library.html"));
            } else {
                warnings.push(format!("no {}", doc.join("COPYRIGHT-library.html").display()));
            }
            for f in ["MIT.txt", "Apache-2.0.txt"] {
                match std::fs::read_to_string(doc.join("licenses").join(f)) {
                    Ok(t) => {
                        let t = normalise(&t);
                        let n = *text_ids.entry(t.clone()).or_insert_with(|| {
                            texts.push(t);
                            texts.len()
                        });
                        o += &format!("File:       licenses/{f}: text [{n}]\n");
                        std_texts.push(n);
                    }
                    Err(e) => warnings.push(format!("{}: {e}", doc.join("licenses").join(f).display())),
                }
            }
        }
        None => warnings.push("no Rust toolchain documentation (rustc --print sysroot)/share/doc/rust".into()),
    }
    o += &format!("\n\nLicence texts\n{rule}\n");
    let mut users: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for c in &crates {
        for n in c.files.iter().map(|f| f.1).chain(c.standard.iter().map(|s| s.1)) {
            let v = users.entry(n).or_default();
            let who = format!("{} {}", c.name, c.version);
            if !v.contains(&who) {
                v.push(who);
            }
        }
    }
    for (i, t) in texts.iter().enumerate() {
        let n = i + 1;
        let used = users.get(&n).map(|v| v.join(", ")).unwrap_or_default();
        let used = if std_texts.contains(&n) { if used.is_empty() { "the Rust standard library".into() } else { format!("{used}, the Rust standard library") } } else { used };
        o += &format!("\n[{n}] used by: {used}\n{thin}\n{t}\n");
    }
    Ok(Notices { text: o, crates: crates.len(), warnings, std_copyright })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_file_names() {
        for n in ["LICENSE", "LICENSE-MIT", "license-apache", "LICENCE.txt", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"] {
            assert!(is_licence_name(n), "{n}");
        }
        for n in ["README.md", "Cargo.toml", "src"] {
            assert!(!is_licence_name(n), "{n}");
        }
        let d = std::env::temp_dir().join(format!("xtask-lic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("LICENSES")).unwrap();
        for f in ["LICENSE-MIT", "NOTICE", "README.md", "LICENSES/Apache-2.0.txt", "legal.txt"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        assert_eq!(licence_files(&d, Some("legal.txt")), ["LICENSE-MIT", "LICENSES/Apache-2.0.txt", "NOTICE", "legal.txt"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
