//! The level operations behind the MCP tools (see `crate::mcp`). Every editing call is one undo
//! step in the document's history, labelled "MCP: ...", so the user can undo an agent's edits in
//! the open editor like their own.
use super::check::{check, AiState};
use super::edit::{entity_name, field_use, type_name};
use super::objects::region_name;
use super::preview::Cam;
use super::{LevelDoc, PX};
use crate::mcp::Part;
use c8_data::level::{Block, Column, Entity, Level, Region, MAP_H, MAP_W};
use serde_json::{json, Value};

type R = Result<Vec<Part>, String>;

/// Largest area `cells_read` returns at once.
const MAX_READ: usize = 64 * 64;

// ---- arguments -------------------------------------------------------------------------------

fn opt_i(a: &Value, k: &str) -> Result<Option<i64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)).map(Some).ok_or_else(|| format!("`{k}` must be an integer")),
    }
}
fn req_i(a: &Value, k: &str) -> Result<i64, String> {
    opt_i(a, k)?.ok_or_else(|| format!("`{k}` is required"))
}
fn opt_f(a: &Value, k: &str) -> Result<Option<f64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().map(Some).ok_or_else(|| format!("`{k}` must be a number")),
    }
}
fn req_f(a: &Value, k: &str) -> Result<f64, String> {
    opt_f(a, k)?.ok_or_else(|| format!("`{k}` is required"))
}
fn opt_b(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(Value::as_bool)
}
fn in_range(k: &str, v: i64, lo: i64, hi: i64) -> Result<i64, String> {
    if (lo..=hi).contains(&v) { Ok(v) } else { Err(format!("`{k}` = {v} is out of range {lo}..{hi}")) }
}

/// A rectangle of cells (x, y, w, h), inclusive bounds (x0, y0, x1, y1), clipped to the map.
/// Absent: the whole map, when `whole` allows it.
fn rect(a: &Value, whole: bool) -> Result<(i32, i32, i32, i32), String> {
    let x = opt_i(a, "x")?;
    let y = opt_i(a, "y")?;
    let (w, h) = (opt_i(a, "w")?, opt_i(a, "h")?);
    if x.is_none() && y.is_none() && w.is_none() && h.is_none() {
        if whole {
            return Ok((0, 0, MAP_W as i32 - 1, MAP_H as i32 - 1));
        }
        return Err("give the area as x, y, w, h (cells)".into());
    }
    let (x, y) = (x.ok_or("`x` is required")?, y.ok_or("`y` is required")?);
    let (w, h) = (w.unwrap_or(1).max(1), h.unwrap_or(1).max(1));
    let x0 = x.clamp(0, MAP_W as i64 - 1) as i32;
    let y0 = y.clamp(0, MAP_H as i64 - 1) as i32;
    let x1 = (x + w - 1).clamp(0, MAP_W as i64 - 1) as i32;
    let y1 = (y + h - 1).clamp(0, MAP_H as i64 - 1) as i32;
    if x + w <= 0 || y + h <= 0 || x >= MAP_W as i64 || y >= MAP_H as i64 {
        return Err(format!("the area lies outside the map (0..{} × 0..{})", MAP_W - 1, MAP_H - 1));
    }
    Ok((x0, y0, x1, y1))
}

fn text(s: impl Into<String>) -> R {
    Ok(vec![Part::Text(s.into())])
}
fn json_out(v: Value) -> R {
    Ok(vec![Part::Text(serde_json::to_string(&v).unwrap_or_default())])
}

fn region_kind(v: &Value) -> Result<u8, String> {
    match v {
        Value::Number(n) => n.as_u64().filter(|k| (1..=4).contains(k)).map(|k| k as u8).ok_or_else(|| "region kind is 1..4".into()),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "shield" => Ok(1),
            "fuel" => Ok(2),
            "ammo" => Ok(3),
            "start" => Ok(4),
            _ => Err(format!("unknown region kind `{s}` (shield, fuel, ammo, start)")),
        },
        _ => Err("`kind` is required (shield, fuel, ammo or start)".into()),
    }
}

// ---- JSON views ------------------------------------------------------------------------------

pub(crate) fn entity_json(e: &Entity) -> Value {
    let u = field_use(e);
    let mut v = json!({
        "index": e.index, "type": e.kind, "sub": e.sub, "name": entity_name(e), "type_name": type_name(e.kind),
        "x": e.x, "y": e.y, "group": e.group,
    });
    let m = v.as_object_mut().unwrap();
    // only the fields this type uses, plus any non-zero ones
    for (k, val, used) in [("link", e.link, u.link), ("value", e.value, u.value), ("w", e.w, u.w), ("h", e.h, u.h), ("target_group", e.target_group, u.target)] {
        if used.is_some() || val != 0 {
            m.insert(k.into(), json!(val));
        }
    }
    v
}

fn region_json(r: &Region) -> Value {
    json!({ "slot": r.slot, "kind": r.kind, "name": region_name(r.kind), "x0": r.cx - r.hw, "y0": r.cy - r.hh, "x1": r.cx + r.hw, "y1": r.cy + r.hh })
}

fn column_json(l: &Level, id: usize, users: usize) -> Value {
    let c = &l.columns[id];
    json!({ "id": id, "floor_texture": c.floor_tex, "blocks": c.blocks, "solid_levels": (0..8).filter(|k| c.mask & (1 << k) != 0).collect::<Vec<_>>(), "cells": users })
}

fn block_json(l: &Level, id: usize) -> Value {
    let b = &l.blocks[id];
    json!({ "id": id, "north": b.tex[0], "east": b.tex[1], "south": b.tex[2], "west": b.tex[3], "top": b.tex[4], "bottom": b.tex[5], "orient": b.orient })
}

fn png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut wr| wr.write_image_data(rgba)).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

// ---- a tiny rasterizer for the top view overlays ----------------------------------------------

struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn put(&mut self, x: i32, y: i32, c: [u8; 3]) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let o = ((y * self.w + x) * 4) as usize;
            self.px[o..o + 3].copy_from_slice(&c);
        }
    }
    fn line(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), c: [u8; 3]) {
        let n = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32).max(1);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.put((x0 + (x1 - x0) * t) as i32, (y0 + (y1 - y0) * t) as i32, c);
        }
    }
    fn rect(&mut self, a: (f32, f32), b: (f32, f32), c: [u8; 3]) {
        self.line(a, (b.0, a.1), c);
        self.line((b.0, a.1), b, c);
        self.line(b, (a.0, b.1), c);
        self.line((a.0, b.1), a, c);
    }
    fn dot(&mut self, (x, y): (f32, f32), r: i32, c: [u8; 3]) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.put(x as i32 + dx, y as i32 + dy, c);
                }
            }
        }
    }
    fn shade(&mut self, x: i32, y: i32, c: [u8; 3], a: f32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let o = ((y * self.w + x) * 4) as usize;
            for k in 0..3 {
                self.px[o + k] = (self.px[o + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
            }
        }
    }
}

impl LevelDoc {
    /// Run one MCP level tool. Editing tools become one undo step.
    pub fn agent_call(&mut self, name: &str, a: &Value) -> R {
        self.finish_edit();
        self.end_stroke("edit");
        match name {
            "level_info" => self.info(),
            "cells_read" => self.cells_read(a),
            "validate" => self.validate(),
            "entities_list" => self.entities_list(a),
            "columns_list" => self.columns_list(),
            "render_top" => self.render_top(a),
            "render_3d" => self.render_3d(a),
            "ai_test" => self.ai_test(a),
            "history" => json_out(json!({ "undo": self.history.labels().collect::<Vec<_>>(), "can_redo": self.history.can_redo(), "unsaved": self.history.dirty() })),
            "undo" => {
                if !self.history.can_undo() {
                    return Err("nothing to undo".into());
                }
                let label = self.history.labels().last().unwrap_or_default();
                self.history.undo(&mut self.level);
                self.after_change();
                text(format!("undid: {label}"))
            }
            "redo" => {
                if !self.history.can_redo() {
                    return Err("nothing to redo".into());
                }
                self.history.redo(&mut self.level);
                self.after_change();
                text(format!("redid: {}", self.history.labels().last().unwrap_or_default()))
            }
            _ => {
                // editing tools
                self.begin_edit(&format!("MCP: {name}"));
                let r = self.edit_tool(name, a);
                match &r {
                    Ok(_) => self.finish_edit(),
                    // nothing of a failed call stays
                    Err(_) => self.cancel_edit(),
                }
                self.after_change();
                r
            }
        }
    }

    fn after_change(&mut self) {
        self.level.update_extent();
        self.touch();
        // drop selections that may point at removed things
        if let Some(super::objects::Sel::Entity(i)) = self.obj_sel {
            if self.entity(i).is_none() {
                self.obj_sel = None;
            }
        }
    }

    fn edit_tool(&mut self, name: &str, a: &Value) -> R {
        match name {
            "cells_write" => self.cells_write(a),
            "heights_fill" => self.heights_fill(a),
            "heights_ramp" => self.heights_ramp(a),
            "heights_smooth" => self.heights_smooth(a),
            "textures_fill" => self.textures_fill(a),
            "light_fill" => self.light_fill(a),
            "light_bake" => self.light_bake(a),
            "texture_attr_set" => {
                let t = in_range("texture", req_i(a, "texture")?, 0, 255)? as usize;
                let v = in_range("attr", req_i(a, "attr")?, 0, 65535)? as u16;
                self.level.tex_attr[t] = v;
                text(format!("texture {t} attributes = {v}"))
            }
            "column_define" => self.column_define(a),
            "block_define" => self.block_define(a),
            "column_place" => {
                let (x0, y0, x1, y1) = rect(a, false)?;
                let id = req_i(a, "column")?;
                if id < 1 || id as usize >= self.level.columns.len() || self.level.columns[id as usize] == Column::default() {
                    return Err(format!("column {id} is not defined (column_define makes one)"));
                }
                let n = self.cells_in(x0, y0, x1, y1, |c| c.tex = -(id as i16));
                text(format!("column {id} on {n} cells"))
            }
            "column_clear" => {
                let (x0, y0, x1, y1) = rect(a, false)?;
                let t = in_range("texture", req_i(a, "texture")?, 0, 255)? as i16;
                let o = in_range("orient", opt_i(a, "orient")?.unwrap_or(0), 0, 7)? as u8;
                let n = self.cells_in(x0, y0, x1, y1, |c| {
                    if c.tex < 0 {
                        c.tex = t;
                        c.orient = o;
                    }
                });
                text(format!("cleared columns in the area ({n} cells looked at)"))
            }
            "entity_add" => self.entity_add(a),
            "entity_update" => self.entity_update(a),
            "entity_delete" => {
                let i = req_i(a, "index")? as usize;
                if self.entity(i).is_none() {
                    return Err(format!("no entity {i}"));
                }
                self.level.entities.retain(|e| e.index != i);
                let mut cleared = 0;
                for e in self.level.entities.iter_mut().filter(|e| e.link as usize == i) {
                    e.link = 0;
                    cleared += 1;
                }
                text(format!("deleted entity {i}; cleared {cleared} links to it"))
            }
            "chain_add" => self.chain_add(a),
            "region_add" => {
                let kind = region_kind(a.get("kind").unwrap_or(&Value::Null))?;
                let (x0, y0, x1, y1) = (req_f(a, "x0")? as f32, req_f(a, "y0")? as f32, req_f(a, "x1")? as f32, req_f(a, "y1")? as f32);
                let slot = self.level.free_region_slot().ok_or("all 10 region records are in use")?;
                let r = Region { slot, kind, cx: (x0 + x1) / 2.0, cy: (y0 + y1) / 2.0, hw: (x1 - x0).abs() / 2.0, hh: (y1 - y0).abs() / 2.0 };
                if !(0.0..=MAP_W as f32).contains(&r.cx) || !(0.0..=MAP_H as f32).contains(&r.cy) {
                    return Err("the region lies outside the map".into());
                }
                self.level.regions.push(r);
                json_out(region_json(&r))
            }
            "region_update" => {
                let slot = req_i(a, "slot")? as usize;
                let mut r = *self.level.regions.iter().find(|r| r.slot == slot).ok_or(format!("no region {slot}"))?;
                if let Some(k) = a.get("kind") {
                    r.kind = region_kind(k)?;
                }
                let (mut x0, mut y0, mut x1, mut y1) = (r.cx - r.hw, r.cy - r.hh, r.cx + r.hw, r.cy + r.hh);
                x0 = opt_f(a, "x0")?.map_or(x0, |v| v as f32);
                y0 = opt_f(a, "y0")?.map_or(y0, |v| v as f32);
                x1 = opt_f(a, "x1")?.map_or(x1, |v| v as f32);
                y1 = opt_f(a, "y1")?.map_or(y1, |v| v as f32);
                (r.cx, r.cy, r.hw, r.hh) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
                *self.level.regions.iter_mut().find(|r| r.slot == slot).unwrap() = r;
                json_out(region_json(&r))
            }
            "region_delete" => {
                let slot = req_i(a, "slot")? as usize;
                let n = self.level.regions.len();
                self.level.regions.retain(|r| r.slot != slot);
                if n == self.level.regions.len() {
                    return Err(format!("no region {slot}"));
                }
                text(format!("deleted region {slot}"))
            }
            _ => Err(format!("unknown tool `{name}`")),
        }
    }

    /// Apply f to every cell in the rectangle; returns the number of cells.
    fn cells_in(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(&mut c8_data::level::Cell)) -> usize {
        for y in y0..=y1 {
            for x in x0..=x1 {
                f(self.level.cell_mut(x, y));
            }
        }
        ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize
    }

    // ---- reading ---------------------------------------------------------------------------

    fn info(&self) -> R {
        let l = &self.level;
        let mut kinds = std::collections::BTreeMap::new();
        for e in &l.entities {
            *kinds.entry(type_name(e.kind)).or_insert(0) += 1;
        }
        let (lo, hi) = l.cells.iter().fold((u16::MAX, 0), |(lo, hi), c| (lo.min(c.height), hi.max(c.height)));
        let used_cols: std::collections::HashSet<i16> = l.cells.iter().filter(|c| c.tex < 0).map(|c| c.tex).collect();
        let issues = check(l);
        json_out(json!({
            "file": self.rel,
            "path": self.path.display().to_string(),
            "unsaved_changes": self.history.dirty(),
            "map": { "width": MAP_W, "height": MAP_H, "track_extent": [l.max_x, l.max_y] },
            "texture_set": self.look.set,
            "track": self.look.set + 1,
            "heights": { "min": lo as f64 / 256.0, "max": hi as f64 / 256.0 },
            "entities": l.entities.len(),
            "entities_by_type": kinds,
            "regions": l.regions.iter().map(region_json).collect::<Vec<_>>(),
            "columns_used": used_cols.len(),
            "columns_defined": l.columns.iter().skip(1).filter(|c| **c != Column::default()).count(),
            "blocks_defined": l.blocks.iter().skip(1).filter(|b| **b != Block::default()).count(),
            "track_textures": (0..256).filter(|&t| l.tex_attr[t] & 4 != 0).collect::<Vec<_>>(),
            "errors": issues.iter().filter(|i| i.error).count(),
            "warnings": issues.iter().filter(|i| !i.error).count(),
            "undo_steps": self.history.labels().count(),
        }))
    }

    fn cells_read(&self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        if w * h > MAX_READ {
            return Err(format!("{w} × {h} cells is too many at once (at most {MAX_READ}); read in parts"));
        }
        let layers: Vec<String> = match a.get("layers").and_then(Value::as_array) {
            Some(v) => v.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
            None => ["height", "texture", "orient", "light", "column"].map(String::from).to_vec(),
        };
        let mut out = serde_json::Map::new();
        out.insert("x".into(), json!(x0));
        out.insert("y".into(), json!(y0));
        out.insert("w".into(), json!(w));
        out.insert("h".into(), json!(h));
        let l = &self.level;
        for layer in &layers {
            let rows: Vec<Value> = (y0..=y1)
                .map(|y| {
                    Value::Array(
                        (x0..=x1)
                            .map(|x| {
                                let c = l.cell(x, y);
                                match layer.as_str() {
                                    "height" => json!((c.height as f64 / 256.0 * 100.0).round() / 100.0),
                                    "texture" => json!(l.floor_tex(x, y)),
                                    "orient" => json!(c.orient),
                                    "light" => json!(c.light.max(0) >> 8),
                                    "column" => json!(if c.tex < 0 { -(c.tex as i32) } else { 0 }),
                                    "attr" => json!(l.attr(x, y)),
                                    _ => Value::Null,
                                }
                            })
                            .collect(),
                    )
                })
                .collect();
            if rows.first().and_then(|r| r.as_array()).and_then(|r| r.first()).is_some_and(Value::is_null) {
                return Err(format!("unknown layer `{layer}` (height, texture, orient, light, column, attr)"));
            }
            out.insert(layer.clone(), Value::Array(rows));
        }
        json_out(Value::Object(out))
    }

    fn validate(&self) -> R {
        let issues = check(&self.level);
        let list: Vec<Value> = issues.iter().map(|i| json!({ "error": i.error, "text": i.text, "at": i.at.map(|(x, y)| [x, y]) })).collect();
        json_out(json!({ "errors": issues.iter().filter(|i| i.error).count(), "warnings": issues.iter().filter(|i| !i.error).count(), "issues": list }))
    }

    fn entities_list(&self, a: &Value) -> R {
        let kind = opt_i(a, "type")?;
        let area = if a.get("x").is_some() { Some(rect(a, false)?) } else { None };
        let limit = opt_i(a, "limit")?.unwrap_or(500).clamp(1, 4000) as usize;
        let mut list: Vec<&Entity> = self
            .level
            .entities
            .iter()
            .filter(|e| kind.is_none_or(|k| e.kind as i64 == k))
            .filter(|e| area.is_none_or(|(x0, y0, x1, y1)| (x0..=x1).contains(&e.x) && (y0..=y1).contains(&e.y)))
            .collect();
        list.sort_by_key(|e| e.index);
        let total = list.len();
        json_out(json!({ "total": total, "shown": total.min(limit), "entities": list.iter().take(limit).map(|e| entity_json(e)).collect::<Vec<_>>() }))
    }

    fn columns_list(&self) -> R {
        let l = &self.level;
        let mut users = vec![0usize; l.columns.len()];
        for c in l.cells.iter().filter(|c| c.tex < 0) {
            if let Some(u) = users.get_mut(-(c.tex as i32) as usize) {
                *u += 1;
            }
        }
        let cols: Vec<Value> = (1..l.columns.len()).filter(|&i| l.columns[i] != Column::default() || users[i] > 0).map(|i| column_json(l, i, users[i])).collect();
        let blocks: Vec<Value> = (1..l.blocks.len()).filter(|&i| l.blocks[i] != Block::default()).map(|i| block_json(l, i)).collect();
        json_out(json!({ "columns": cols, "blocks": blocks }))
    }

    fn render_top(&self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, true)?;
        let scale = in_range("scale", opt_i(a, "scale")?.unwrap_or(if (x1 - x0) > 100 { 3 } else { 6 }), 1, 12)? as usize;
        let base = self.render();
        let (cw, ch) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let (w, h) = (cw * scale, ch * scale);
        if w * h > 4096 * 4096 {
            return Err("the image would be too large; use a smaller scale or area".into());
        }
        let mut cv = Canvas { w: w as i32, h: h as i32, px: vec![255; w * h * 4] };
        let bw = base.size[0];
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (x0 as usize * PX + x * PX / scale, y0 as usize * PX + y * PX / scale);
                let c = base.pixels[sy * bw + sx];
                let o = (y * w + x) * 4;
                cv.px[o..o + 3].copy_from_slice(&[c.r(), c.g(), c.b()]);
            }
        }
        let s = scale as f32;
        let to = |x: f32, y: f32| ((x - x0 as f32) * s, (y - y0 as f32) * s);
        let l = &self.level;
        // columns: tinted by their top height
        if opt_b(a, "columns").unwrap_or(true) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(col) = l.column(x, y) {
                        let top = 16 - col.mask.leading_zeros() as i32;
                        let tint = [60, 60 + (top * 24).min(190) as u8, 255];
                        for py in 0..scale as i32 {
                            for px in 0..scale as i32 {
                                cv.shade((x - x0) * scale as i32 + px, (y - y0) * scale as i32 + py, tint, 0.35);
                            }
                        }
                    }
                }
            }
        }
        let grid = in_range("grid", opt_i(a, "grid")?.unwrap_or(16), 0, 256)? as i32;
        if grid > 0 {
            for gx in (x0..=x1 + 1).filter(|v| v % grid == 0) {
                for py in 0..h as i32 {
                    cv.shade(((gx - x0) * scale as i32).min(w as i32 - 1), py, [255, 255, 255], 0.35);
                }
            }
            for gy in (y0..=y1 + 1).filter(|v| v % grid == 0) {
                for px in 0..w as i32 {
                    cv.shade(px, ((gy - y0) * scale as i32).min(h as i32 - 1), [255, 255, 255], 0.35);
                }
            }
        }
        if opt_b(a, "overlays").unwrap_or(true) {
            let by_index: std::collections::HashMap<usize, &Entity> = l.entities.iter().map(|e| (e.index, e)).collect();
            for r in &l.regions {
                let c = match r.kind {
                    1 => [80, 230, 80],
                    2 => [90, 150, 255],
                    3 => [255, 220, 60],
                    _ => [255, 255, 255],
                };
                cv.rect(to(r.cx - r.hw, r.cy - r.hh), to(r.cx + r.hw, r.cy + r.hh), c);
            }
            for e in &l.entities {
                let p = to(e.x as f32 + 0.5, e.y as f32 + 0.5);
                match e.kind {
                    9 => {
                        if let Some(n) = by_index.get(&(e.link as usize)) {
                            let q = to(n.x as f32 + 0.5, n.y as f32 + 0.5);
                            let c = match e.sub {
                                0 => [255, 60, 60],
                                6 => [255, 0, 255],
                                _ => [0, 230, 230],
                            };
                            cv.line(p, q, c);
                            if e.sub != 0 {
                                // arrow head: a dot near the next point shows the direction
                                cv.dot((q.0 + (p.0 - q.0) * 0.25, q.1 + (p.1 - q.1) * 0.25), 1, c);
                            }
                        }
                    }
                    1 => cv.rect(to(e.x as f32 + 0.5, e.y as f32 + 0.5), to(e.x as f32 + e.w as f32 + 1.5, e.y as f32 + e.h as f32 + 1.5), [255, 255, 0]),
                    5 => cv.dot(p, (s / 3.0) as i32 + 1, [60, 255, 60]),
                    10 => cv.dot(p, (s / 3.0) as i32 + 1, [255, 150, 0]),
                    8 => cv.dot(p, (s / 4.0) as i32 + 1, [255, 80, 200]),
                    3 if (6..=10).contains(&e.sub) => cv.dot(p, 1, [255, 255, 255]),
                    _ => {}
                }
            }
        }
        let png = png(w as u32, h as u32, &cv.px)?;
        Ok(vec![
            Part::Text(format!(
                "Top view of cells x {x0}..{x1}, y {y0}..{y1} at {scale} px per cell (north up; pixel (px, py) = cell ({x0} + px/{scale}, {y0} + py/{scale})). Grid lines every {grid} cells. Red: walls; cyan: AI route (the dot sits near the end a link points to); magenta: shortcuts; yellow boxes: checkpoints; rectangles: chargers (green shield, blue fuel, yellow ammo) and the white start area; green dots: pickups; orange: recovery trucks; pink: triggers; blue tint: block columns (lighter = taller)."
            )),
            Part::Png(png),
        ])
    }

    fn render_3d(&mut self, a: &Value) -> R {
        let x = req_f(a, "x")? as f32;
        let y = req_f(a, "y")? as f32;
        let w = in_range("width", opt_i(a, "width")?.unwrap_or(960), 64, 2048)? as u32;
        let h = in_range("height", opt_i(a, "height")?.unwrap_or(540), 64, 2048)? as u32;
        // above the highest surface around the spot, so the camera is not inside a building
        let mut ground: f32 = 0.0;
        for dy in -2..=2 {
            for dx in -2..=2 {
                ground = ground.max(self.level.floor_below(x + dx as f32, y + dy as f32, 255.0));
            }
        }
        let z = ground + opt_f(a, "above")?.unwrap_or(6.0) as f32;
        let (yaw, pitch) = match (opt_f(a, "look_x")?, opt_f(a, "look_y")?) {
            (Some(lx), Some(ly)) => {
                let (dx, dy) = (lx as f32 - x, ly as f32 - y);
                let target = self.level.floor_below(lx as f32, ly as f32, 255.0);
                let dist = (dx * dx + dy * dy).sqrt().max(0.01);
                (dx.atan2(-dy), ((target - z) / dist).atan())
            }
            _ => (opt_f(a, "yaw")?.unwrap_or(0.0).to_radians() as f32, opt_f(a, "pitch")?.unwrap_or(-25.0).to_radians() as f32),
        };
        let cam = Cam { pos: [x, z, y], yaw, pitch: pitch.clamp(-1.55, 1.55) };
        let rgba = self.snapshot_3d(cam, w, h, opt_b(a, "lines").unwrap_or(true), opt_b(a, "haze").unwrap_or(false))?;
        Ok(vec![
            Part::Text(format!("3D view from cell ({x:.1}, {y:.1}) at height {z:.1} (highest surface around: {ground:.1}), heading {:.0}° (0 = north, clockwise), pitch {:.0}°. Lines: red walls, cyan AI route, magenta shortcuts, yellow checkpoint frames, green pickups, orange trucks, region outlines.", yaw.to_degrees().rem_euclid(360.0), cam.pitch.to_degrees())),
            Part::Png(png(w, h, &rgba)?),
        ])
    }

    fn ai_test(&mut self, a: &Value) -> R {
        self.ai_ticks = in_range("ticks", opt_i(a, "ticks")?.unwrap_or(4000), 100, 40000)? as u32;
        self.start_ai_test();
        let report = match self.ai.take() {
            Some(AiState::Running(rx)) => rx.recv().map_err(|e| e.to_string())?,
            Some(AiState::Done(r)) => r,
            None => return Err("the AI test did not start".into()),
        };
        let out = match &report {
            Ok(r) => json!({
                "ticks": self.ai_ticks,
                "seconds": self.ai_ticks as f32 / 20.0,
                "summary": r.summary,
                "laps": r.laps.iter().map(|(n, l)| json!({ "craft": n, "laps": l })).collect::<Vec<_>>(),
                "stuck": r.stuck.iter().map(|(n, x, y, t)| json!({ "craft": n, "x": x, "y": y, "tick": t })).collect::<Vec<_>>(),
            }),
            Err(e) => json!({ "error": e }),
        };
        // show the result in the editor's check panel too
        self.ai = Some(AiState::Done(report));
        json_out(out)
    }

    // ---- editing ---------------------------------------------------------------------------

    fn cells_write(&mut self, a: &Value) -> R {
        let x0 = req_i(a, "x")? as i32;
        let y0 = req_i(a, "y")? as i32;
        let mut n = 0;
        for (layer, apply) in [("height", 0), ("texture", 1), ("orient", 2), ("light", 3)] {
            let Some(rows) = a.get(layer) else { continue };
            let rows = rows.as_array().ok_or(format!("`{layer}` must be an array of rows"))?;
            for (dy, row) in rows.iter().enumerate() {
                let row = row.as_array().ok_or(format!("`{layer}` rows must be arrays"))?;
                for (dx, v) in row.iter().enumerate() {
                    if v.is_null() {
                        continue;
                    }
                    let (x, y) = (x0 + dx as i32, y0 + dy as i32);
                    if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 {
                        return Err(format!("cell ({x}, {y}) is outside the map"));
                    }
                    let f = v.as_f64().ok_or(format!("`{layer}` values must be numbers or null"))?;
                    let c = self.level.cell_mut(x, y);
                    match apply {
                        0 => c.height = (f.clamp(0.0, 255.99) * 256.0).round() as u16,
                        1 => {
                            if c.tex >= 0 {
                                c.tex = in_range("texture", f as i64, 0, 255)? as i16;
                            }
                        }
                        2 => c.orient = in_range("orient", f as i64, 0, 7)? as u8,
                        _ => c.light = ((in_range("light", f as i64, 0, 63)? as i16) << 8) | 0xff,
                    }
                    n += 1;
                }
            }
        }
        text(format!("wrote {n} values"))
    }

    fn heights_fill(&mut self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let add = opt_f(a, "add")?;
        let set = opt_f(a, "height")?;
        if add.is_none() == set.is_none() {
            return Err("give either `height` (set) or `add` (raise/lower by)".into());
        }
        let n = self.cells_in(x0, y0, x1, y1, |c| {
            let v = match (set, add) {
                (Some(h), _) => h,
                (_, Some(d)) => c.height as f64 / 256.0 + d,
                _ => unreachable!(),
            };
            c.height = (v.clamp(0.0, 255.99) * 256.0).round() as u16;
        });
        text(format!("{n} corner heights changed"))
    }

    fn heights_ramp(&mut self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let (from, to) = (req_f(a, "from")?, req_f(a, "to")?);
        let dir = a.get("direction").and_then(Value::as_str).unwrap_or("east");
        let n = (match dir {
            "east" | "west" => x1 - x0,
            "north" | "south" => y1 - y0,
            _ => return Err("`direction` is east, west, north or south (the direction the ramp rises from `from` to `to`)".into()),
        })
        .max(1) as f64;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let t = match dir {
                    "east" => (x - x0) as f64,
                    "west" => (x1 - x) as f64,
                    "south" => (y - y0) as f64,
                    _ => (y1 - y) as f64,
                } / n;
                self.level.cell_mut(x, y).height = ((from + (to - from) * t).clamp(0.0, 255.99) * 256.0).round() as u16;
            }
        }
        text(format!("ramp {from} → {to} towards the {dir} over {} × {} corners", x1 - x0 + 1, y1 - y0 + 1))
    }

    fn heights_smooth(&mut self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let passes = in_range("passes", opt_i(a, "passes")?.unwrap_or(1), 1, 20)?;
        for _ in 0..passes {
            let snap: Vec<f32> = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).map(|(x, y)| {
                let mut s = 0.0;
                for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    s += self.level.cell(x + dx, y + dy).height as f32;
                }
                s / 5.0
            }).collect();
            let w = (x1 - x0 + 1) as usize;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    self.level.cell_mut(x, y).height = snap[(y - y0) as usize * w + (x - x0) as usize].round() as u16;
                }
            }
        }
        text(format!("smoothed {} × {} corners ({passes} passes)", x1 - x0 + 1, y1 - y0 + 1))
    }

    fn textures_fill(&mut self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let t = in_range("texture", req_i(a, "texture")?, 0, 255)? as i16;
        let o = in_range("orient", opt_i(a, "orient")?.unwrap_or(0), 0, 7)? as u8;
        let mut skipped = 0;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = self.level.cell_mut(x, y);
                if c.tex < 0 {
                    skipped += 1;
                } else {
                    c.tex = t;
                    c.orient = o;
                }
            }
        }
        let note = if skipped > 0 { format!("; {skipped} cells with block columns kept (their floor comes from the column)") } else { String::new() };
        text(format!("texture {t} (orientation {o}) painted{note}"))
    }

    fn light_fill(&mut self, a: &Value) -> R {
        let (x0, y0, x1, y1) = rect(a, false)?;
        let row = in_range("light", req_i(a, "light")?, 0, 63)? as i16;
        let n = self.cells_in(x0, y0, x1, y1, |c| c.light = (row << 8) | 0xff);
        text(format!("light {row} on {n} corners"))
    }

    fn light_bake(&mut self, a: &Value) -> R {
        let area = rect(a, true)?;
        let saved = (self.bake_azimuth, self.bake_strength, self.bake_ambient);
        self.bake_azimuth = opt_f(a, "azimuth")?.unwrap_or(315.0) as f32;
        self.bake_strength = opt_f(a, "strength")?.unwrap_or(12.0) as f32;
        self.bake_ambient = opt_f(a, "ambient")?.unwrap_or(31.0) as f32;
        let values = self.baked_light(area);
        (self.bake_azimuth, self.bake_strength, self.bake_ambient) = saved;
        for (i, v) in values {
            self.level.cells[i].light = v;
        }
        text(format!("baked light into x {}..{}, y {}..{}", area.0, area.2, area.1, area.3))
    }

    fn column_define(&mut self, a: &Value) -> R {
        let id = match opt_i(a, "id")? {
            Some(id) => {
                if id < 1 || id as usize >= self.level.columns.len() {
                    return Err(format!("column id {id} is out of range 1..{}", self.level.columns.len() - 1));
                }
                id as usize
            }
            None => self.free_column().ok_or("all columns are in use")?,
        };
        let blocks: Vec<i64> = a.get("blocks").and_then(Value::as_array).ok_or("`blocks` is required: block ids for levels 0..7 from the bottom, 0 = empty")?.iter().map(|v| v.as_i64().unwrap_or(0)).collect();
        if blocks.len() > 8 {
            return Err("a column has 8 levels".into());
        }
        let mut col = Column { mask: 0, floor_tex: in_range("floor_texture", opt_i(a, "floor_texture")?.unwrap_or(0), 0, 255)? as i16, blocks: [0; 8] };
        for (k, &b) in blocks.iter().enumerate() {
            if b != 0 {
                if b < 1 || b as usize >= self.level.blocks.len() || self.level.blocks[b as usize] == Block::default() {
                    return Err(format!("block {b} (level {k}) is not defined (block_define makes one)"));
                }
                col.blocks[k] = b as i16;
                col.mask |= 1 << k;
            }
        }
        let users = self.column_users(id);
        self.level.columns[id] = col;
        json_out(json!({ "column": column_json(&self.level, id, users), "note": if users > 0 { format!("{users} cells already use this column and change with it") } else { "place it with column_place".into() } }))
    }

    fn block_define(&mut self, a: &Value) -> R {
        let id = match opt_i(a, "id")? {
            Some(id) => {
                if id < 1 || id as usize >= self.level.blocks.len() {
                    return Err(format!("block id {id} is out of range 1..{}", self.level.blocks.len() - 1));
                }
                id as usize
            }
            None => self.free_block().ok_or("all blocks are in use")?,
        };
        let mut b = self.level.blocks[id];
        let all = opt_i(a, "texture")?;
        for (k, face) in ["north", "east", "south", "west", "top", "bottom"].iter().enumerate() {
            if let Some(t) = opt_i(a, face)?.or(all) {
                b.tex[k] = in_range(face, t, 0, 255)? as u8;
            }
        }
        if let Some(o) = a.get("orient").and_then(Value::as_array) {
            for (k, v) in o.iter().take(6).enumerate() {
                b.orient[k] = in_range("orient", v.as_i64().unwrap_or(0), 0, 7)? as u8;
            }
        }
        if b == Block::default() {
            return Err("give the face textures (`texture` for all faces, or north/east/south/west/top/bottom)".into());
        }
        self.level.blocks[id] = b;
        json_out(block_json(&self.level, id))
    }

    fn set_entity_fields(e: &mut Entity, a: &Value) -> Result<(), String> {
        if let Some(v) = opt_i(a, "x")? {
            e.x = in_range("x", v, 0, MAP_W as i64 - 1)? as i32;
        }
        if let Some(v) = opt_i(a, "y")? {
            e.y = in_range("y", v, 0, MAP_H as i64 - 1)? as i32;
        }
        for (k, f) in [("group", &mut e.group), ("target_group", &mut e.target_group), ("link", &mut e.link), ("value", &mut e.value), ("w", &mut e.w), ("h", &mut e.h)] {
            if let Some(v) = opt_i(a, k)? {
                *f = in_range(k, v, 0, 65535)? as u16;
            }
        }
        if let Some(v) = opt_i(a, "sub")? {
            e.sub = in_range("sub", v, 0, 255)? as u8;
        }
        Ok(())
    }

    fn entity_add(&mut self, a: &Value) -> R {
        let kind = in_range("type", req_i(a, "type")?, 1, 10)? as u8;
        let sub = in_range("sub", req_i(a, "sub")?, 0, 255)? as u8;
        let (x, y) = (in_range("x", req_i(a, "x")?, 0, MAP_W as i64 - 1)? as i32, in_range("y", req_i(a, "y")?, 0, MAP_H as i64 - 1)? as i32);
        let i = self.add_entity(kind, sub, x, y).ok_or("all 3999 entity records are in use")?;
        let e = self.entity_mut(i).unwrap();
        Self::set_entity_fields(e, a)?;
        let e = *e;
        if e.link != 0 && self.entity(e.link as usize).is_none() {
            return Err(format!("link {} is not an entity", e.link));
        }
        json_out(entity_json(&e))
    }

    fn entity_update(&mut self, a: &Value) -> R {
        let i = req_i(a, "index")? as usize;
        let e = self.entity_mut(i).ok_or(format!("no entity {i}"))?;
        if let Some(k) = opt_i(a, "type")? {
            e.kind = in_range("type", k, 1, 10)? as u8;
        }
        Self::set_entity_fields(e, a)?;
        let e = *e;
        if e.link != 0 && self.entity(e.link as usize).is_none() {
            return Err(format!("link {} is not an entity", e.link));
        }
        json_out(entity_json(&e))
    }

    fn chain_add(&mut self, a: &Value) -> R {
        let kind = a.get("kind").and_then(Value::as_str).unwrap_or("waypoint");
        let sub = match kind {
            "wall" => 0,
            "waypoint" => in_range("sub", opt_i(a, "sub")?.unwrap_or(1), 1, 10)? as u8,
            _ => return Err("`kind` is wall or waypoint".into()),
        };
        let pts: Vec<(i32, i32)> = a
            .get("points")
            .and_then(Value::as_array)
            .ok_or("`points` is required: [[x, y], ...] in cells")?
            .iter()
            .map(|p| {
                let p = p.as_array().filter(|p| p.len() == 2).ok_or("each point is [x, y]")?;
                let (x, y) = (p[0].as_i64().ok_or("point coordinates are integers")?, p[1].as_i64().ok_or("point coordinates are integers")?);
                Ok((in_range("x", x, 0, MAP_W as i64 - 1)? as i32, in_range("y", y, 0, MAP_H as i64 - 1)? as i32))
            })
            .collect::<Result<_, String>>()?;
        if pts.len() < 2 && opt_i(a, "join_to")?.is_none() {
            return Err("a chain needs at least two points (or one point and `join_to`)".into());
        }
        let closed = opt_b(a, "closed").unwrap_or(false);
        let group = opt_i(a, "group")?.unwrap_or(1) as u16;
        let mut idx = Vec::new();
        for &(x, y) in &pts {
            let i = self.add_entity(9, sub, x, y).ok_or("all 3999 entity records are in use")?;
            self.entity_mut(i).unwrap().group = group;
            idx.push(i);
        }
        for k in 0..idx.len().saturating_sub(1) {
            self.entity_mut(idx[k]).unwrap().link = idx[k + 1] as u16;
        }
        let last = *idx.last().unwrap();
        if closed && idx.len() > 2 {
            self.entity_mut(last).unwrap().link = idx[0] as u16;
        } else if let Some(t) = opt_i(a, "join_to")? {
            if self.entity(t as usize).is_none() {
                return Err(format!("join_to: no entity {t}"));
            }
            self.entity_mut(last).unwrap().link = t as u16;
        }
        if let Some(f) = opt_i(a, "join_from")? {
            let e = self.entity_mut(f as usize).ok_or(format!("join_from: no entity {f}"))?;
            e.link = idx[0] as u16;
        }
        json_out(json!({ "kind": kind, "sub": sub, "indices": idx, "closed": closed }))
    }
}
