//! Headless mode: `c8h18-edit mcp [--data <dir>] [--allow <dir>]... [level]` serves MCP over
//! stdio without a window. Levels are read from the data folder or the allowed folders, and
//! written only into the allowed folders (default: the current directory).
use super::Workspace;
use crate::doc::level::LevelDoc;
use crate::doc::Document;
use std::path::{Path, PathBuf};

pub struct Headless {
    data: Option<PathBuf>,
    allowed: Vec<PathBuf>,
    docs: Vec<LevelDoc>,
    current: usize,
}

fn absolute(p: &Path) -> PathBuf {
    let p = if p.is_relative() { std::env::current_dir().map(|d| d.join(p)).unwrap_or(p.to_path_buf()) } else { p.to_path_buf() };
    // canonicalize what exists (the file may not exist yet: then its folder)
    p.canonicalize().unwrap_or_else(|_| match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
        (Some(d), Some(n)) => d.join(n),
        _ => p,
    })
}

impl Headless {
    pub fn new(data: Option<PathBuf>, allowed: Vec<PathBuf>) -> Headless {
        let allowed = if allowed.is_empty() { vec![std::env::current_dir().unwrap_or_default()] } else { allowed };
        Headless { data: data.map(|d| absolute(&d)), allowed: allowed.iter().map(|d| absolute(d)).collect(), docs: Vec::new(), current: 0 }
    }
}

impl Workspace for Headless {
    fn data_dir(&self) -> Option<PathBuf> {
        self.data.clone()
    }
    fn levels(&mut self) -> Vec<(String, PathBuf, bool, bool)> {
        self.docs.iter().enumerate().map(|(i, d)| (d.title(), d.path().map(Path::to_path_buf).unwrap_or_default(), d.dirty(), i == self.current)).collect()
    }
    fn level(&mut self, name: Option<&str>) -> Result<&mut LevelDoc, String> {
        let i = match name {
            None => (!self.docs.is_empty()).then_some(self.current).ok_or("no level is open: use level_open")?,
            Some(n) => self
                .docs
                .iter()
                .position(|d| d.title().eq_ignore_ascii_case(n) || d.path().and_then(|p| p.file_name()).is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(n)))
                .ok_or(format!("no open level named {n}"))?,
        };
        Ok(&mut self.docs[i])
    }
    fn open_level(&mut self, path: PathBuf) -> Result<String, String> {
        let path = absolute(&path);
        if let Some(i) = self.docs.iter().position(|d| d.path() == Some(path.as_path())) {
            self.current = i;
            return Ok(self.docs[i].title());
        }
        let rel = match &self.data {
            Some(d) if path.starts_with(d) => path.strip_prefix(d).unwrap().to_string_lossy().replace('\\', "/"),
            _ => path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        };
        let doc = LevelDoc::open(path, rel)?;
        let name = doc.title();
        self.docs.push(doc);
        self.current = self.docs.len() - 1;
        Ok(name)
    }
    fn may_write(&self, p: &Path) -> Result<(), String> {
        let p = absolute(p);
        if self.data.as_ref().is_some_and(|d| p.starts_with(d)) {
            return Err(format!("{} is in the game data folder, which is never written", p.display()));
        }
        if self.allowed.iter().any(|a| p.starts_with(a)) {
            Ok(())
        } else {
            Err(format!("{} is outside the allowed folders ({}); start the server with --allow <dir>", p.display(), self.allowed.iter().map(|a| a.display().to_string()).collect::<Vec<_>>().join(", ")))
        }
    }
    fn may_read(&self, p: &Path) -> Result<(), String> {
        let p = absolute(p);
        if self.data.as_ref().is_some_and(|d| p.starts_with(d)) || self.allowed.iter().any(|a| p.starts_with(a)) {
            Ok(())
        } else {
            Err(format!("{} is outside the data folder and the allowed folders", p.display()))
        }
    }
}
