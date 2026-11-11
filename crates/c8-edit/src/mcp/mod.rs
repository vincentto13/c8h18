//! MCP support (docs/editor-plan.md §4): the tools, resources and prompts, and the dispatch of a
//! call onto the open documents. The calls run synchronously on whichever thread owns the
//! documents (the window's UI thread in live mode, the executor thread in headless mode); the
//! MCP transports live in [`server`].
pub mod headless;
pub mod server;

use crate::doc::level::LevelDoc;
use crate::doc::Document;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// One piece of a tool result.
#[derive(Debug)]
pub enum Part {
    Text(String),
    Png(Vec<u8>),
}

pub type Out = Result<Vec<Part>, String>;

/// The documents an MCP session works on.
pub trait Workspace {
    fn data_dir(&self) -> Option<PathBuf>;
    /// Open levels: (name, path, unsaved changes, is the default target).
    fn levels(&mut self) -> Vec<(String, PathBuf, bool, bool)>;
    /// The level a call works on: by name (file name or shown title), else the default one.
    fn level(&mut self, name: Option<&str>) -> Result<&mut LevelDoc, String>;
    /// Open a level file (or bring it to the front) and make it the default target.
    fn open_level(&mut self, path: PathBuf) -> Result<String, String>;
    /// May the agent write this file? (Never into the game data folder.)
    fn may_write(&self, p: &Path) -> Result<(), String>;
    fn may_read(&self, p: &Path) -> Result<(), String>;
}

/// A tool definition: name, description, input schema properties, required properties.
struct ToolDef {
    name: &'static str,
    description: &'static str,
    props: Value,
    required: &'static [&'static str],
    read_only: bool,
}

fn area_props(extra: Value) -> Value {
    let mut m = json!({
        "x": { "type": "integer", "description": "left column of the area (0..255)" },
        "y": { "type": "integer", "description": "top row of the area (0..159)" },
        "w": { "type": "integer", "description": "width in cells (default 1)" },
        "h": { "type": "integer", "description": "height in cells (default 1)" },
    });
    if let (Some(m), Some(e)) = (m.as_object_mut(), extra.as_object()) {
        m.extend(e.clone());
    }
    m
}

fn defs() -> Vec<ToolDef> {
    let level = json!({ "type": "string", "description": "which open level (file name, e.g. LEVEL0-1.DAT); default: the active one" });
    let mut v = vec![
        ToolDef { name: "editor_status", description: "The data folder and the open levels (which one tools work on by default). Start here.", props: json!({}), required: &[], read_only: true },
        ToolDef { name: "level_open", description: "Open a level: an original track (1..6, as a template; save it under a new name) or a level file. It becomes the default target.", props: json!({ "track": { "type": "integer", "description": "original track 1..6" }, "path": { "type": "string", "description": "path of a level file" } }), required: &[], read_only: false },
        ToolDef { name: "level_save", description: "Save the level. Originals in the game data folder are never overwritten: give a new path for them.", props: json!({ "path": { "type": "string", "description": "file to write (default: where it was opened from, unless that is an original)" } }), required: &[], read_only: false },
        ToolDef { name: "level_info", description: "Summary of the level: size, track extent, height range, entity counts by type, regions, columns, track textures, number of check errors and warnings, unsaved changes.", props: json!({}), required: &[], read_only: true },
        ToolDef { name: "cells_read", description: "Read layers of an area (at most 4096 cells) as rows of values: height (corner height in cells, float), texture (floor texture id 0..255), orient (texture orientation 0..7), light (colour-table row: 32 unchanged, lower brighter, higher darker), column (block column id, 0 = none), attr (texture attribute bits).", props: area_props(json!({ "layers": { "type": "array", "items": { "type": "string", "enum": ["height", "texture", "orient", "light", "column", "attr"] }, "description": "default: height, texture, orient, light, column" } })), required: &["x", "y", "w", "h"], read_only: true },
        ToolDef { name: "cells_write", description: "Write layers starting at (x, y): each layer is rows of values (null keeps a cell). Layers: height (float, cells), texture (0..255; cells with block columns keep theirs), orient (0..7), light (row 0..63).", props: json!({ "x": { "type": "integer" }, "y": { "type": "integer" }, "height": { "type": "array", "items": { "type": "array" } }, "texture": { "type": "array", "items": { "type": "array" } }, "orient": { "type": "array", "items": { "type": "array" } }, "light": { "type": "array", "items": { "type": "array" } } }), required: &["x", "y"], read_only: false },
        ToolDef { name: "heights_fill", description: "Set the corner heights of an area to `height`, or raise/lower them by `add` (cells; 1 = one block level).", props: area_props(json!({ "height": { "type": "number" }, "add": { "type": "number" } })), required: &["x", "y", "w", "h"], read_only: false },
        ToolDef { name: "heights_ramp", description: "A straight ramp over an area: heights go from `from` to `to` towards `direction`.", props: area_props(json!({ "from": { "type": "number" }, "to": { "type": "number" }, "direction": { "type": "string", "enum": ["east", "west", "north", "south"] } })), required: &["x", "y", "w", "h", "from", "to"], read_only: false },
        ToolDef { name: "heights_smooth", description: "Smooth the heights of an area (average with the 4 neighbours).", props: area_props(json!({ "passes": { "type": "integer", "description": "1..20, default 1" } })), required: &["x", "y", "w", "h"], read_only: false },
        ToolDef { name: "textures_fill", description: "Paint a floor texture on an area (cells with block columns keep the column's floor). Make a track with texture ids whose `track` attribute bit (4) is set: see level_info.track_textures.", props: area_props(json!({ "texture": { "type": "integer" }, "orient": { "type": "integer", "description": "0..7, default 0" } })), required: &["x", "y", "w", "h", "texture"], read_only: false },
        ToolDef { name: "light_fill", description: "Set the light of an area to a colour-table row (32 unchanged, lower brighter, higher darker; the originals use about 31).", props: area_props(json!({ "light": { "type": "integer" } })), required: &["x", "y", "w", "h", "light"], read_only: false },
        ToolDef { name: "light_bake", description: "Light the terrain from its slopes (area, or the whole map): light from `azimuth` degrees (0 = north, clockwise, default 315), `ambient` row on flat ground (default 31), ± `strength` rows on slopes (default 12).", props: area_props(json!({ "azimuth": { "type": "number" }, "ambient": { "type": "number" }, "strength": { "type": "number" } })), required: &[], read_only: false },
        ToolDef { name: "texture_attr_set", description: "Set a texture's attribute bits: 1 sand particles, 2 water particles, 4 track surface (counts for the track extent and lets crafts climb steep slopes), bits 10..13 friction class.", props: json!({ "texture": { "type": "integer" }, "attr": { "type": "integer" } }), required: &["texture", "attr"], read_only: false },
        ToolDef { name: "columns_list", description: "The block columns (floor texture, block per level 0..7, solid levels, cells using it) and the blocks (texture of each face).", props: json!({}), required: &[], read_only: true },
        ToolDef { name: "block_define", description: "Define a block (or change block `id`): face textures `texture` (all faces) or north/east/south/west/top/bottom, optional `orient` (6 values 0..7).", props: json!({ "id": { "type": "integer" }, "texture": { "type": "integer" }, "north": { "type": "integer" }, "east": { "type": "integer" }, "south": { "type": "integer" }, "west": { "type": "integer" }, "top": { "type": "integer" }, "bottom": { "type": "integer" }, "orient": { "type": "array", "items": { "type": "integer" } } }), required: &[], read_only: false },
        ToolDef { name: "column_define", description: "Define a block column (or change column `id`, which changes every cell using it): `blocks` = block id per level from the bottom (8 levels, 0 = empty: an empty level 0 under solid ones makes a tunnel or bridge), `floor_texture` drawn when level 0 is empty.", props: json!({ "id": { "type": "integer" }, "blocks": { "type": "array", "items": { "type": "integer" } }, "floor_texture": { "type": "integer" } }), required: &["blocks"], read_only: false },
        ToolDef { name: "column_place", description: "Put a block column on every cell of an area (buildings, walls, tunnels, bridges).", props: area_props(json!({ "column": { "type": "integer" } })), required: &["x", "y", "w", "h", "column"], read_only: false },
        ToolDef { name: "column_clear", description: "Remove block columns from an area, giving the cells a floor texture.", props: area_props(json!({ "texture": { "type": "integer" }, "orient": { "type": "integer" } })), required: &["x", "y", "w", "h", "texture"], read_only: false },
        ToolDef { name: "entities_list", description: "List entities (optionally of one `type`, inside an area) with the fields their type uses. See the resource docs://format for types and sub-types.", props: area_props(json!({ "type": { "type": "integer" }, "limit": { "type": "integer", "description": "default 500" } })), required: &[], read_only: true },
        ToolDef { name: "entity_add", description: "Add an entity: type and sub-type (docs://format), cell x, y, and optional group (default 1 = present from the start), target_group, link, value, w, h. Checkpoints get the next order value by default.", props: json!({ "type": { "type": "integer" }, "sub": { "type": "integer" }, "x": { "type": "integer" }, "y": { "type": "integer" }, "group": { "type": "integer" }, "target_group": { "type": "integer" }, "link": { "type": "integer" }, "value": { "type": "integer" }, "w": { "type": "integer" }, "h": { "type": "integer" } }), required: &["type", "sub", "x", "y"], read_only: false },
        ToolDef { name: "entity_update", description: "Change fields of entity `index` (type, sub, x, y, group, target_group, link, value, w, h).", props: json!({ "index": { "type": "integer" }, "type": { "type": "integer" }, "sub": { "type": "integer" }, "x": { "type": "integer" }, "y": { "type": "integer" }, "group": { "type": "integer" }, "target_group": { "type": "integer" }, "link": { "type": "integer" }, "value": { "type": "integer" }, "w": { "type": "integer" }, "h": { "type": "integer" } }), required: &["index"], read_only: false },
        ToolDef { name: "entity_delete", description: "Delete entity `index`; links to it are cleared.", props: json!({ "index": { "type": "integer" } }), required: &["index"], read_only: false },
        ToolDef { name: "chain_add", description: "Add a chain of linked points: `kind` wall (invisible wall segments between the points) or waypoint (the computer players' route, in driving order; `sub` 1 normal, 2 fuel, 3 ammo, 4 shield charger branch, 6 shortcut, 7 brake, 8 hold fire, 9 fire). `closed` links the last point back to the first (a lap); `join_to` links the last point to an existing entity, `join_from` makes an existing entity link to the first point (branches).", props: json!({ "kind": { "type": "string", "enum": ["wall", "waypoint"] }, "sub": { "type": "integer" }, "points": { "type": "array", "items": { "type": "array", "items": { "type": "integer" } }, "description": "[[x, y], ...] cells" }, "closed": { "type": "boolean" }, "join_to": { "type": "integer" }, "join_from": { "type": "integer" }, "group": { "type": "integer" } }), required: &["kind", "points"], read_only: false },
        ToolDef { name: "region_add", description: "Add a region: charger pad (shield, fuel, ammo) or the start area (`start`: crafts start on its cells with texture 122). Corners x0, y0, x1, y1 in cells (fractions allowed; cell edges are whole numbers).", props: json!({ "kind": { "type": "string", "enum": ["shield", "fuel", "ammo", "start"] }, "x0": { "type": "number" }, "y0": { "type": "number" }, "x1": { "type": "number" }, "y1": { "type": "number" } }), required: &["kind", "x0", "y0", "x1", "y1"], read_only: false },
        ToolDef { name: "region_update", description: "Change region `slot` (kind, x0, y0, x1, y1).", props: json!({ "slot": { "type": "integer" }, "kind": { "type": "string" }, "x0": { "type": "number" }, "y0": { "type": "number" }, "x1": { "type": "number" }, "y1": { "type": "number" } }), required: &["slot"], read_only: false },
        ToolDef { name: "region_delete", description: "Delete region `slot`.", props: json!({ "slot": { "type": "integer" } }), required: &["slot"], read_only: false },
        ToolDef { name: "validate", description: "Check the level the way the game builds a track: checkpoints, start area, chargers, links, waypoint graph, groups and triggers, columns. Errors make a broken race.", props: json!({}), required: &[], read_only: true },
        ToolDef { name: "ai_test", description: "Race the 8 computer players on the level in the game's headless simulation (20 ticks per second) and report the laps each completed and where any got stuck for 15 s. Needs the game (c8h18) next to the editor or on the PATH.", props: json!({ "ticks": { "type": "integer", "description": "100..40000, default 4000 (200 s)" } }), required: &[], read_only: true },
        ToolDef { name: "render_top", description: "A top-down picture (PNG) of an area or the whole map with the textures as lit in the game and overlays (walls, AI route, checkpoints, regions, pickups, trucks, block columns, a grid).", props: area_props(json!({ "scale": { "type": "integer", "description": "pixels per cell 1..12 (default 3 for large areas, 6 for small)" }, "overlays": { "type": "boolean" }, "columns": { "type": "boolean" }, "grid": { "type": "integer", "description": "grid spacing in cells, 0 = none (default 16)" } })), required: &[], read_only: true },
        ToolDef { name: "render_3d", description: "A 3D picture (PNG) as the game draws the level, from a camera above cell (x, y): `above` cells over the ground (default 6), looking at (look_x, look_y) or with `yaw` degrees (0 = north, clockwise) and `pitch` (default -25).", props: json!({ "x": { "type": "number" }, "y": { "type": "number" }, "above": { "type": "number" }, "look_x": { "type": "number" }, "look_y": { "type": "number" }, "yaw": { "type": "number" }, "pitch": { "type": "number" }, "width": { "type": "integer" }, "height": { "type": "integer" }, "lines": { "type": "boolean", "description": "draw walls, route, checkpoints (default true)" }, "haze": { "type": "boolean" } }), required: &["x", "y"], read_only: true },
        ToolDef { name: "history", description: "The level's undo history (MCP edits are labelled \"MCP: ...\").", props: json!({}), required: &[], read_only: true },
        ToolDef { name: "undo", description: "Undo the last edit (the user's or an agent's).", props: json!({}), required: &[], read_only: false },
        ToolDef { name: "redo", description: "Redo the last undone edit.", props: json!({}), required: &[], read_only: false },
    ];
    for d in v.iter_mut().filter(|d| !matches!(d.name, "editor_status" | "level_open")) {
        d.props.as_object_mut().unwrap().insert("level".into(), level.clone());
    }
    v
}

/// The tools as rmcp definitions.
pub fn tools() -> Vec<rmcp::model::Tool> {
    defs()
        .into_iter()
        .map(|d| {
            let schema: Map<String, Value> = json!({ "type": "object", "properties": d.props, "required": d.required }).as_object().cloned().unwrap();
            rmcp::model::Tool::new(d.name, d.description, std::sync::Arc::new(schema)).annotate(rmcp::model::ToolAnnotations::new().read_only(d.read_only).destructive(false))
        })
        .collect()
}

/// Is this a tool that does not change anything? (Allowed while the user has paused editing.)
pub fn read_only(name: &str) -> bool {
    defs().iter().any(|d| d.name == name && d.read_only)
}

fn text(s: impl Into<String>) -> Out {
    Ok(vec![Part::Text(s.into())])
}

/// Run one tool.
pub fn call(ws: &mut dyn Workspace, name: &str, args: &Value) -> Out {
    let level_name = args.get("level").and_then(Value::as_str);
    match name {
        "editor_status" => {
            let levels: Vec<Value> = ws.levels().into_iter().map(|(n, p, dirty, cur)| json!({ "name": n, "path": p.display().to_string(), "unsaved_changes": dirty, "default_target": cur })).collect();
            let hint = if levels.is_empty() { "no level open: use level_open (track 1..6 opens an original as a template)" } else { "tools work on the default target unless `level` names another" };
            text(serde_json::to_string(&json!({ "data_folder": ws.data_dir().map(|d| d.display().to_string()), "levels": levels, "hint": hint })).unwrap_or_default())
        }
        "level_open" => {
            let path = match (args.get("track").and_then(Value::as_i64), args.get("path").and_then(Value::as_str)) {
                (Some(t), _) => {
                    if !(1..=6).contains(&t) {
                        return Err("track is 1..6".into());
                    }
                    crate::resources::local_file(&format!("maps/level0-{t}.dat")).ok_or(format!("track {t} not found in the asset package"))?
                }
                (None, Some(p)) => {
                    let p = PathBuf::from(p);
                    ws.may_read(&p)?;
                    p
                }
                _ => return Err("give `track` (1..6) or `path`".into()),
            };
            let name = ws.open_level(path)?;
            text(format!("opened {name}; it is now the default target"))
        }
        "level_save" => {
            let data = ws.data_dir();
            let target = match args.get("path").and_then(Value::as_str) {
                Some(p) => PathBuf::from(p),
                None => ws.level(level_name)?.path().map(Path::to_path_buf).ok_or("give a path")?,
            };
            let target = if target.is_relative() { std::env::current_dir().map(|d| d.join(&target)).unwrap_or(target) } else { target };
            if data.as_ref().is_some_and(|d| target.starts_with(d) || d.canonicalize().is_ok_and(|c| target.starts_with(c))) {
                return Err(format!("{} is in the game data folder; originals are never overwritten: give another `path`", target.display()));
            }
            ws.may_write(&target)?;
            ws.level(level_name)?.save_to(&target)?;
            text(format!("saved {}", target.display()))
        }
        _ => {
            if !defs().iter().any(|d| d.name == name) {
                return Err(format!("unknown tool `{name}`"));
            }
            ws.level(level_name)?.agent_call(name, args)
        }
    }
}

// ---- resources and prompts ------------------------------------------------------------------

pub const RESOURCES: [(&str, &str, &str); 3] = [
    ("level://summary", "Level summary", "level_info of the default level"),
    ("level://validation", "Level check", "validate of the default level"),
    ("docs://format", "Level format and design guide", "units, layers, entity types, and what makes a working track"),
];

pub fn read_resource(ws: &mut dyn Workspace, uri: &str) -> Result<String, String> {
    let first = |o: Out| -> Result<String, String> {
        o?.into_iter().find_map(|p| match p {
            Part::Text(t) => Some(t),
            Part::Png(_) => None,
        }).ok_or_else(|| "no text".into())
    };
    match uri {
        "level://summary" => first(ws.level(None)?.agent_call("level_info", &json!({}))),
        "level://validation" => first(ws.level(None)?.agent_call("validate", &json!({}))),
        "docs://format" => Ok(guide()),
        _ => Err(format!("unknown resource {uri}")),
    }
}

pub fn guide() -> String {
    format!("{GUIDE}\n## Entity types and sub-types\n\n{}", crate::doc::level::entity_reference())
}

const GUIDE: &str = r#"# Level format and design guide (c8h18-edit)

## The map
- 256 columns (x, 0..255, west to east) × 160 rows (y, 0..159, north to south) of **cells**.
- Each cell stores the values of its **north-west corner**: a cell's surface spans the corners
  (x, y), (x+1, y), (x+1, y+1), (x, y+1).
  - height in cells (one block level = 1.0);
  - light;
  - floor texture and orientation.
- The original tracks use only part of the map. `level_info.track_extent` gives the right and
  bottom edge of the track textures.
- **Heights**: smooth slopes drive well. Steps of more than about 0.5 between neighbouring corners
  stop crafts, except on textures with the track attribute.
  - Valleys between walls of blocks keep crafts on the course.
  - Crafts fly low over the surface, so tunnels need a free level 0 (see columns).
- **Textures**: 256 per track set (TEXTU0-N, N = track − 1). Orientation 0..7 rotates and mirrors
  a texture.
  - The attribute bit 4 ("track") marks the drivable road; `level_info.track_textures` lists the
    textures with it.
  - Texture 122 marks the start grid cells inside the start area.
- **Light**: a row of the colour table.
  - 32 is the texture unchanged; lower rows are brighter and fade into haze, higher rows are darker.
  - The originals use about 31 on flat ground. `light_bake` shades the slopes.
- **Block columns**: a cell can carry a column instead of a floor texture: 8 levels from the cell's
  corner heights upwards, each empty or holding a block.
  - A block has a texture per face.
  - Cells that use the same column share it: changing the column changes all of them.
  - Use them for buildings, walls, tunnels (empty level 0, solid above) and bridges.
  - Define blocks first (`block_define`), then columns (`column_define`), then place them
    (`column_place`).

## What a race needs (checked by `validate`)
1. **Checkpoints** (type 1, sub 5) in driving order: value 0 is the start/finish line, then 1, 2,
   … around the lap.
   - The box starts at the entity's cell and extends w+1 × h+1 cells: make it span the road.
   - Crafts must pass them in order to count a lap.
2. **A start area** (`region_add` kind start), placed on the road just before checkpoint 0 and
   facing north.
   - It should cover at least 8 cells of texture 122; the crafts start there in a grid.
3. **The AI route**: a closed chain of waypoints (`chain_add` kind waypoint, `closed`: true)
   along the middle of the road in driving order.
   - Points every 4–10 cells, more in bends.
   - Branches (sub 2/3/4) lead through charger pads and rejoin. Shortcuts use sub 6. At most
     200 positions.
4. **Walls** (`chain_add` kind wall): invisible segments that keep crafts on the course, usually
   along both road edges where the terrain does not.
5. **Chargers**: shield, fuel and ammo pads (`region_add`) on or beside the road, ideally with a
   waypoint branch through them.
6. Recovery trucks (type 10, sub 9) and pickups (type 5) along the road make it complete.
7. Entities of group 1 exist from the start. Other groups appear when a trigger (type 8) with
   that target_group fires.

## Working method
- Start from an original track (`level_open` track N) as a template: its texture set fits it, and
  its unknown bytes stay valid. Save under a new path.
- Look before and after editing: `render_top` for the layout, `render_3d` for the driving view,
  `cells_read` for exact numbers.
- Every editing call is one undo step (`undo`). The user sees the edits live and can undo them too.
- Run `validate`, then `ai_test`. All 8 crafts should complete laps without getting stuck; a
  stuck point usually means a step, a wall gap or a waypoint that leads into a wall.
"#;

pub const PROMPTS: [(&str, &str); 2] = [
    ("design-track", "Design or rework a race track in the open level, step by step, checking it with validate and ai_test"),
    ("fix-validation", "Fix the problems validate reports in the open level"),
];

pub fn prompt(name: &str, args: &Map<String, Value>) -> Result<String, String> {
    match name {
        "design-track" => {
            let idea = args.get("idea").and_then(Value::as_str).unwrap_or("a fun, varied lap that the computer players can drive");
            Ok(format!("Read the resource docs://format first. Then design a race track in the open level (or open an original track with level_open as a template): {idea}.\n\nWork in small steps and look at the result after each (render_top, render_3d): shape the terrain (heights_*), paint the road with track textures (textures_fill), add the start area, checkpoints in order, the AI waypoint loop, walls where needed, chargers and pickups. Then run validate and fix every error, and run ai_test until all crafts complete laps without getting stuck. Save under a new path when done and summarise what you built."))
        }
        "fix-validation" => Ok("Run validate on the open level. For each error and warning, look at the place (render_top around its `at` coordinates, entities_list in that area) and fix it with the smallest change. Run validate again until no errors remain, then run ai_test to confirm the computer players can lap. Explain each fix.".into()),
        _ => Err(format!("unknown prompt {name}")),
    }
}

/// Server instructions sent to the client at the start.
pub const INSTRUCTIONS: &str = "c8h18-edit edits race track levels of the 1995 hover-craft racer remake. Call editor_status first; read docs://format before designing. Every editing call is one undo step; validate and ai_test check the track; render_top and render_3d show it.";

#[cfg(test)]
mod tests {
    use super::*;
    use headless::Headless;

    fn res() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources"))
    }

    fn ok(ws: &mut Headless, name: &str, args: Value) -> String {
        match call(ws, name, &args) {
            Ok(parts) => parts.into_iter().map(|p| if let Part::Text(t) = p { t } else { "<png>".into() }).collect::<Vec<_>>().join("\n"),
            Err(e) => panic!("{name}: {e}"),
        }
    }

    #[test]
    fn tools_edit_undo_and_guard_files() {
        if !crate::resources::is_data_dir(&res()) {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("hiedit-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut ws = Headless::new(Some(res()), vec![tmp.clone()]);
        assert!(call(&mut ws, "level_info", &json!({})).is_err(), "no level open yet");
        ok(&mut ws, "level_open", json!({ "track": 2 }));
        let before: Value = serde_json::from_str(&ok(&mut ws, "cells_read", json!({ "x": 50, "y": 50, "w": 3, "h": 2 }))).unwrap();

        // an editing call is one undo step
        ok(&mut ws, "heights_fill", json!({ "x": 50, "y": 50, "w": 3, "h": 2, "height": 20.5 }));
        let cells: Value = serde_json::from_str(&ok(&mut ws, "cells_read", json!({ "x": 50, "y": 50, "w": 3, "h": 2, "layers": ["height"] }))).unwrap();
        assert_eq!(cells["height"][1][2], json!(20.5));
        ok(&mut ws, "undo", json!({}));
        let after: Value = serde_json::from_str(&ok(&mut ws, "cells_read", json!({ "x": 50, "y": 50, "w": 3, "h": 2 }))).unwrap();
        assert_eq!(before, after);

        // a failing call leaves nothing behind
        let n = ws.level(None).unwrap().agent_call("history", &json!({})).ok().map(|p| p.len());
        assert!(call(&mut ws, "column_define", &json!({ "blocks": [9999] })).is_err());
        assert_eq!(ws.level(None).unwrap().agent_call("history", &json!({})).ok().map(|p| p.len()), n);
        let h: Value = serde_json::from_str(&ok(&mut ws, "history", json!({}))).unwrap();
        assert_eq!(h["undo"].as_array().unwrap().len(), 0);

        // structure: a block, a column on cells, a closed waypoint loop, a region
        let b: Value = serde_json::from_str(&ok(&mut ws, "block_define", json!({ "texture": 7 }))).unwrap();
        let c: Value = serde_json::from_str(&ok(&mut ws, "column_define", json!({ "blocks": [0, b["id"]], "floor_texture": 3 }))).unwrap();
        ok(&mut ws, "column_place", json!({ "x": 200, "y": 10, "w": 2, "h": 2, "column": c["column"]["id"] }));
        let cells: Value = serde_json::from_str(&ok(&mut ws, "cells_read", json!({ "x": 200, "y": 10, "w": 1, "h": 1, "layers": ["column", "texture"] }))).unwrap();
        assert_eq!(cells["column"][0][0], c["column"]["id"]);
        assert_eq!(cells["texture"][0][0], json!(3));
        let chain: Value = serde_json::from_str(&ok(&mut ws, "chain_add", json!({ "kind": "waypoint", "points": [[200, 20], [210, 20], [210, 30]], "closed": true }))).unwrap();
        let first = chain["indices"][0].as_u64().unwrap();
        let last: Value = serde_json::from_str(&ok(&mut ws, "entities_list", json!({ "type": 9, "x": 210, "y": 30, "w": 1, "h": 1 }))).unwrap();
        assert_eq!(last["entities"][0]["link"].as_u64(), Some(first));
        ok(&mut ws, "region_add", json!({ "kind": "fuel", "x0": 200, "y0": 40, "x1": 204, "y1": 42 }));
        assert!(ok(&mut ws, "validate", json!({})).contains("issues"));
        let png = call(&mut ws, "render_top", &json!({ "x": 190, "y": 0, "w": 30, "h": 50, "scale": 2 })).unwrap();
        assert!(matches!(&png[1], Part::Png(d) if d.starts_with(b"\x89PNG")));

        // files: originals and folders outside --allow are refused
        assert!(call(&mut ws, "level_save", &json!({})).unwrap_err().contains("data folder"));
        assert!(call(&mut ws, "level_save", &json!({ "path": "/elsewhere/x.dat" })).is_err());
        let out = tmp.join("LEVEL0-2.DAT");
        ok(&mut ws, "level_save", json!({ "path": out.to_string_lossy() }));
        let saved = c8_data::level::Level::read_file(&out).unwrap();
        assert!(saved.regions.iter().any(|r| r.kind == 2 && (r.cx - 202.0).abs() < 0.01));
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(guide().contains("| 9 Wall / waypoint | 6 | Waypoint shortcut |"));
    }
}
