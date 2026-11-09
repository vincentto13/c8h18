# c8h18-edit: plan

A standalone level and resource editor for the game, in this repository as the `c8-edit` crate
(binary `c8h18-edit`). It edits the game's level files (`LEVEL0-N.DAT`, spec 01 §9), later also
textures, fonts and the other resources, and has a built-in MCP server, so an AI agent (Claude Code
or any other MCP client) can inspect and edit a level, alone or alongside the user.

Targets: macOS first, Linux and Windows from the same code.

## 1. Goals

- Open an original level or start a new one from a template, edit it, save it, and test it in the
  game with one click.
- Edit everything the game reads from a level:
  - terrain heights and lighting;
  - textures and their orientation;
  - block columns (buildings, tunnels, bridges);
  - entities: checkpoints, AI waypoints, invisible walls, start grid, pickups, recovery trucks,
    triggers and terrain morphs, cameras;
  - charger regions;
  - texture attributes (which textures count as track, and their friction).
- Catch broken tracks before the game does: checkpoint order, waypoint graph, start grid, walls,
  and craft-traps such as gaps that are too low.
- Optionally run a headless AI race inside the editor to see whether the computer can lap the track.
- Expose the same operations over MCP, with undo, so an agent's edits are visible and reversible
  in the open editor.

Out of scope for v1: editing textures or models themselves (the pixels and meshes), sound, and the
front-end carousel model.

## 2. Architecture

One repository, one Cargo workspace:

```
Cargo.toml                 workspace root (default member: the game)
crates/
  c8-data/                library: the data file formats (spec 01), read AND write;
                           levels first, then textures, fonts, sprites, models...
  c8-game/                the game, binary `c8h18`
  c8-edit/                the editor, binary `c8h18-edit`
    src/resources.rs       the data folder as resources grouped by kind
    src/doc/               one Document type per resource kind (level editor, viewer for the
                           kinds without an editor yet), undo/redo history of Commands
    src/app.rs             window: browser, tabs, inspector
  c8-sim/      (later)    track build + flight/AI sim, extracted from the game, for the
                           editor's AI test races and the network server
```

- **Shared format code.** The editor reads and writes with exactly the code the game uses, so a
  file that loads in the editor loads in the game.
- **Unknown bytes kept.** The level is a fixed 896140-byte snapshot with unknown areas. `Level`
  keeps the bytes it was read from and writes only the fields it understands, so a round trip of
  an unedited level is byte-identical (tested for all six levels).
- **Levels are saved unpacked**; the game reads packed and unpacked files.
- **Original data stays out of the repo.** The editor asks once for the game data folder,
  remembers it, and never writes into it: saving an original asks for a new file name.
- **Other resources.** Every resource kind is a `Document` implementation. Kinds without an editor
  open in a viewer (file facts plus a preview for palettes, terrain textures, skies, language
  files); an editor replaces the viewer kind by kind, with the writer added to `c8-data`.
- **Edits are Commands** in a per-document history (undo/redo, dirty state). The MCP server will
  issue the same Commands.

## 3. User interface (egui)

- **Main view, 2D top-down map**
  - Overlays that can be switched on and off:
    - height (shaded relief or contours);
    - textures as rendered;
    - lighting;
    - columns (coloured by height, tunnels hatched);
    - entities (icons), with waypoint graph arrows and checkpoint gates numbered in order;
    - charger rectangles;
    - the "track" attribute mask.
  - Zoom, pan, grid, and a cursor readout (cell, height, texture, column).
- **3D view (dock or split)**
  - A port of the game's terrain/block/model shader, through an egui-wgpu paint callback, so it
    looks like the game.
  - Fly camera, plus a "drive" camera that follows the AI line.
  - Clicking in 3D picks the cell.
- **Tools**
  - Height: raise, lower, smooth, flatten to value, ramp between two points, noise-free plateau.
    Each with a brush size and falloff, working on corners.
  - Texture: paint, fill, rotate/flip orientation, pick, replace across the map.
  - Light: paint, smooth, and "bake" from a light direction (slope shading), with manual touch-up.
  - Columns: a stack editor (16 levels, each block with six face textures). Paint a column onto
    cells, extrude or lower it, cut tunnels (clear the lower levels), copy and paste columns.
  - Entities: place, move, delete and edit properties.
    - Waypoint chains are drawn as a polyline, with branches for alternative routes.
    - Walls are drawn as a polyline.
    - Checkpoints are placed as gates.
    - Start grid: 8 slots with a heading.
  - Regions: draw charger rectangles (shield, fuel, ammo).
  - Selection: select a rectangle, then copy, paste, rotate or mirror (cells, columns and entities
    together).
- **Panels**
  - Properties of the selection.
  - Texture palette (the level's set) with attributes.
  - Validation list: each problem clickable, jumping to the spot.
  - History, with undo/redo.
  - MCP activity log, showing what the agent did, with an "undo agent session" action.
- **File**
  - Open, new from template, save, save as, and "Test in game", which saves to a temporary file
    and starts `c8h18 --race <file>`.
- **Platform**
  - Native menus and file dialogs (`rfd`).
  - macOS keyboard shortcuts (⌘Z, ⌘S, …) mapped to Ctrl on the other systems.

## 4. MCP support

The editor contains an MCP server built on `rmcp`, the official Rust MCP SDK. It has two modes:

1. **Live, inside the running editor.** A Streamable-HTTP endpoint on `127.0.0.1:<port>`, on by
   default, with the port shown in the status bar. The agent then edits the document the user has
   open:
   - every MCP call becomes a normal Command on the UI thread, sent through a channel;
   - edits show up immediately and can be undone;
   - a lock icon pauses agent access.
2. **Headless.** `c8h18-edit mcp [--data <dir>] [level]` runs over stdio with no window. This
   is for scripted or batch work and for clients that only launch stdio servers, e.g.
   `claude mcp add c8h18-edit -- c8h18-edit mcp --data …`.

Tools. All coordinates are cells; large grids come back as compact run-length or base64 arrays.

| Tool | Purpose |
|---|---|
| `level_open`, `level_new`, `level_save`, `level_info` | file handling and a summary (size, texture set, counts) |
| `cells_read(rect, layers)` | heights, textures, orientation, light, columns for a rectangle |
| `heights_set`, `heights_brush`, `heights_ramp`, `heights_smooth` | terrain editing |
| `textures_paint`, `textures_fill`, `light_bake`, `light_paint` | surface editing |
| `column_define`, `column_place`, `column_clear`, `tunnel_cut` | blocks |
| `entities_list(filter)`, `entity_add`, `entity_update`, `entity_delete` | entities |
| `waypoints_set_chain`, `walls_set_chain`, `checkpoints_set`, `start_grid_set` | track structure, at a higher level than raw entities |
| `region_add`, `region_delete` | chargers |
| `validate` | the problem list, as structured data |
| `render_top(rect, layers)`, `render_3d(camera)` | PNG images, returned as MCP image content, so the agent can see the result |
| `ai_sim(ticks, difficulty)` (phase 5) | lap times, stuck points, shortcut use |
| `undo`, `redo`, `history` | reversibility |

Resources:
- `level://summary`;
- `level://validation`;
- `docs://format`, a short format and design guide for agents covering units, texture attributes,
  entity kinds, and what makes a good track.

Prompts: `design-track`, `fix-validation`.

Safety:
- the live endpoint binds to localhost only and needs a per-session token, which the editor shows
  and copies into the client config command;
- headless mode can touch only files under the folders given on the command line.

## 5. Changes needed in the game (`c8h18`)

1. **Split into a workspace and extract `c8-data`** with level writing (done).
2. **`--race <path to level file>`**, so the editor can test a level directly, with a texture set
   and lap count taken from a small sidecar file.
3. **Custom tracks folder (optional, phase 6).** `custom/<name>/level.dat` plus `track.txt` (name,
   texture set, laps, music), offered in the game after the six originals. Needed only if edited
   tracks should be playable from the menus, not just from the editor.
4. **`c8-sim` extraction (phase 5)** for the in-editor AI test. This overlaps with phase 0–1 of the
   network plan (a clean `World::step`), so it should be done once, for both.

## 6. Technology

- **Language and GUI:** Rust 2024; `eframe`/`egui` 0.37 with the wgpu backend for the UI. egui is
  immediate-mode, quick to build tool panels with, and runs the same on all three systems.
- **Rendering:** the 3D preview reuses the game's WGSL shader. The editor's wgpu version has to
  match the one egui-wgpu pins, so the shared code (`c8-data`) stays free of wgpu, and only the
  preview renderer depends on it.
- **MCP:** `rmcp` 3.x (features `server`, `transport-io`, Streamable-HTTP server) on a small
  `tokio` runtime in a background thread; `serde`/`serde_json`/`schemars` for the tool schemas.
- **Other crates:** `rfd` for file dialogs, `png`, `directories` for the preferences location.
- **Packaging:**
  - macOS: `.app` bundle (cargo-bundle), signing and notarization later;
  - Linux: tarball/AppImage;
  - Windows: zip with the `.exe`.
- **CI:** GitHub Actions for the three systems once a remote exists.

## 7. Phases

| Phase | Content | Done when |
|---|---|---|
| 0 | Workspace split, `c8-data` with level writing; byte-identical round-trip test | **done**: game builds and runs unchanged; 6/6 levels round-trip |
| 1 | Editor skeleton: open/save, 2D map with overlays, pan/zoom, cell inspector, undo/redo framework, preferences (data folder), resource browser and viewers | **done** (first tool: height brush) |
| 2 | Terrain tools: heights, textures and orientation, light paint/bake; selection copy/paste; texture attributes; "Test in game" (game option `--level`) | **done** |
| 2b | Resource editors: pixel editor for terrain textures, skies, screens, model texture atlases, billboards, sprite sheets and fonts (PNG import/export, glyph resize, font preview); palette editor; "Test in game" for any edited file via the game option `--override <dir>` | **done** |
| 3 | Columns and blocks; entities (checkpoints, waypoint chains, walls, start grid, pickups, trucks, cameras, triggers/morphs); regions; validation panel; AI test through the game's headless sim | **done** (AI test via `c8h18 --override <dir> --sim`, so the `c8-sim` extraction of phase 5 is no longer needed for it) |
| 4 | MCP: headless stdio and live HTTP modes, the tool set above, image renders, activity log | Claude Code can build a small track loop through MCP alone |
| 5 | 3D preview (shared mesh code in `c8-render`, exact light-table colours, fly camera, entity lines) | **done**; the in-editor AI test moved to phase 3 (via `--sim`). Models and billboards in the preview are still open |
| 6 | Custom tracks in the game menus; minimap generation; packaging for macOS/Linux/Windows | an edited track is selectable in the game on all three systems |

Phases 1–4 give a usable editor with MCP. Phase 5 is the most work, because of the sim extraction.

## 8. Risks

- **Unknown level areas.** Keeping the original bytes covers them. A brand-new level starts from a
  copy of an original as a template, so the unknown bytes stay plausible.
- **Level limits.**
  - 1023 columns and 1023 blocks.
  - 4000 entities.
  - 256×160 cells.
  The editor shows the counts and refuses to exceed them.
- **Waypoint and wall entity semantics** (spec 01 §9.4) are only as well understood as the spec.
  Phase 3 validates by building the game's `Track` from the edited level and running a short AI sim
  where possible.
- **wgpu versions.** egui-wgpu pins its own wgpu, which may differ from the game's wgpu 30.
  Keeping wgpu out of the shared crates avoids any conflict.
- **Copyright.** The editor ships no original data; it reads it from the user's game folder. The
  same publishing notes as for the game apply.
