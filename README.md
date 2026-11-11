# C8H18

A recreation of the 1995 hover-craft combat racer from the specification in `docs/spec/`, running on the
original data files in `resources/` or on our own assets made from scratch in `assets/custom/` (see
[Asset packages](#asset-packages)).

- Rendering: `wgpu` (Metal / Vulkan / DX12). Textures stay 8-bit palette indices and the shaders go
  through the per-track light table, so lighting and haze match the original. The 3D view uses the
  whole window, widescreen included. The 2D layer (menus, HUD) is drawn at 640×480 in palette space.
- Audio: `cpal` with a software mixer. Music is XMIDI played on an emulated OPL3 using `SAMPLE.OPL`, or OGG
  Vorbis files that replace tunes (see [Music](#music)).
- Simulation: fixed 20 Hz tick, drawing interpolated between ticks; the front end runs at 70 frames/s.
- The 320×200 mode and the network game are not included. Where the spec lists "port deviations",
  this version follows them.

## Repository layout

A Cargo workspace with three crates:

| Crate | What | Binary |
|---|---|---|
| `crates/c8-data` | the data file formats (spec 01), read and write; shared by the game and the editor | |
| `crates/c8-game` | the game | `c8h18` |
| `crates/c8-audio` | sound and music banks (read and write), WAV samples, XMIDI tunes (read and write), Standard MIDI conversion, the GTL timbre bank (read and write), the OPL3 music player and OGG music, shared by the game and the editor | |
| `crates/c8-render` | the level mesh (floor quads and block faces), the model triangles and the vertex format, shared by the game's renderer and the editor's 3D views | |
| `crates/c8-edit` | level and resource editor (see `docs/editor-plan.md`) | `c8h18-edit` |

## Build and run

```
cargo build --release                # the game (the workspace's default member)
./target/release/c8h18            # assets default to ./resources
./target/release/c8h18 --asset assets/custom    # our own assets (partial: track 1, fallbacks for the rest)
cargo build --release --workspace    # game and editor
cargo test --workspace               # all tests
```

Options:

| Option | Effect |
|---|---|
| `-s`, `/s`, `--silent` | no sound or music |
| `--no-intro` | skip the intro movie (also skipped, with a warning, when `DATA/INTRO.DAT` is missing) |
| `--speed N` | game speed in percent of the original 20 ticks/s (default 90; e.g. 100 = original, 80 = slower) |
| `--asset <dir\|file.c8pak>` | the asset package: a folder in the original layout or a `.c8pak` bundle (see [Asset packages](#asset-packages)); default `resources`. `<path>`, `--data <path>` and `GAME_DATA=<path>` do the same |
| `--race N` / `--demo N` | debug: go straight to a race / demo race on track N |
| `--level <file>` | with `--race N` / `--demo N` / `--sim N`: race on this level file (e.g. saved by the editor) instead of track N's; textures, sky and music still come from track N |
| `--sim N TICKS` | debug: run a headless computer-only race and print the crafts' progress |
| `--lang <code>` | use this language (`en`, `de`, `fr`, `es`, `it`, `pl`) instead of the saved one, without saving it |

### Asset packages

The game and the editor read their files from one **asset package**, given with `--asset`:

- a **folder** with the original layout: `DATA/`, `MAPS/`, `OBJECTS/DATA/`, `SOUND/` (with
  `SOUND/MUSIC/` for OGG tunes) and `lang/`, e.g. `resources/` (the original files) or
  `assets/custom/` (our own);
- or a **bundle**: a zip file with the extension `.c8pak` holding the same layout at its root
  (deflate or stored; entries with absolute paths or `..` are ignored). Bundles are read in place,
  each file decompressed when it is loaded.

Names are matched in any letter case, so packages can be swapped freely. Build and unpack bundles
with the editor:

```
c8h18-edit --pack assets/custom custom.c8pak     # leaves out save/ and hidden files
c8h18-edit --unpack custom.c8pak some/folder
./target/release/c8h18 --asset custom.c8pak
```

**Saves.** Config, save games, key, pad and video settings (`config.dat`, `save00.dat`..,
`keys.txt`, `pad.txt`, `video.txt`) go to `save/` inside a folder package, as before. A bundle is
never written: its saves go to a per-user folder named after the bundle file,
`~/Library/Application Support/c8h18/<bundle name>/save/` on macOS
(`%APPDATA%\c8h18\<bundle name>\save\` on Windows, `$XDG_DATA_HOME/c8h18/<bundle name>/save/`,
default `~/.local/share/...`, elsewhere). `C8H18_SAVE_DIR=<dir>` puts them into `<dir>` instead,
for any package (handy for test runs that must not touch the package or the user folder). Running
`--asset assets/custom` writes `assets/custom/save/`, which `.gitignore` excludes (as it does
`*.c8pak`); `--pack` leaves `save/` out.

### Partial data sets

A package may hold only some of the files, e.g. our own assets only (port addition, spec 02 §17).
Every missing or unusable file prints one warning on stdout, once per run, and the game carries on
without it:

```
warning: missing asset DATA/SKY0-0.DAT: drawing a plain sky
```

| Missing | Fallback (warning text after the file name) |
|---|---|
| `DATA/PALET0-0.DAT` | a built-in neutral palette (`using a built-in neutral palette`) |
| fonts `HFONT0-0`, `MFONT0-0`, `OLFNT0-1`, `OSFNT0-1`, `PFONT0-1` (`.DAT`/`.TAB`) | that font's text is not drawn (`text in this font is not drawn`) |
| `DATA/PANEL0-1` | no HUD, no menu box frames (`no HUD or panels`) |
| `DATA/LOGO0-1` | no logo (`no logo`) |
| `DATA/TRACK0-1` | no minimaps (`no track maps`) |
| `DATA/OSCR0-1.DAT`, `ONET0-1.DAT`, `TITLE.DAT` | a plain dark background (`drawing a plain background`) |
| `DATA/PUZZLE.DAT` | the sliding puzzle is disabled (`the sliding puzzle is disabled`) |
| `DATA/TMAPS.DAT/.TAB` | no billboards (`no billboards`) |
| `DATA/SRCH0-0.DAT` | search rings generated by the same rule as the editor's (`using generated search rings`) |
| `DATA/TABLE0-N.DAT` | a colour table computed from the palette, flat colours from the track's textures (`computing the colour table from the palette`) |
| `DATA/TEXTU0-N.DAT` | placeholder textures: grey tiles with a grid, a little lighter or darker per texture id (`using generated placeholder textures`) |
| `DATA/SKY0-N.DAT` | a vertical gradient from blue to the haze colour (`drawing a plain sky`) |
| `DATA/SKY0-N.PNG` / `.TXT` (optional, [true-colour sky](#true-colour-skies)) | no warning when absent; an unreadable PNG or a bad sidecar warns and the palette sky `SKY0-N.DAT` is drawn (`using the palette sky DATA/SKY0-N.DAT`) |
| `DATA/INTRO.DAT` | the intro is skipped, as with `--no-intro` (`skipping the intro movie`) |
| `OBJECTS/DATA/<model>.DAT` | that model is drawn as nothing (`drawn as nothing`) |
| `OBJECTS/DATA/TEX0-0.DAT/.TAB` | models get one plain grey texture (`models are drawn with a plain texture`) |
| `SOUND/SOUND.DAT` | no sound effects (`sound effects are silent`) |
| `SOUND/SAMPLE.OPL`, `SOUND/MUSIC.DAT` | no FM music; OGG tunes still play (`FM music is silent`) |
| `lang/en.txt` / another `lang/*.txt` | empty texts / English (`texts are left empty` / `using English`) |
| `MAPS/LEVEL0-N.DAT` | track N is not offered (`track N is not available`) |

A file that exists but cannot be read or decoded gets `warning: unusable asset NAME (error): ...`
with the same fallback. A truncated `INTRO.DAT` plays the frames it has.

**Tracks.** A track is offered when its level `MAPS/LEVEL0-N.DAT` is found; its textures, sky and
colour table fall back as above. The track carousel shows just those tracks, a championship is
raced over them in track order (one track is a championship of one race), and demo races use them.
With all six present nothing changes. Optional: `OBJECTS/DATA/TRACK0-(N-1).DAT` (its carousel
model) and its minimap pictures in the shared `DATA/TRACK0-1.DAT/.TAB` (entries 1–2, 3–5, 6, 7–8,
9–10, 11 for tracks 1..6; nothing is drawn for missing entries). Track names, lap counts and the
music tune are built in by track number. `assets/custom/` has `LEVEL0-1` only, so it offers track 1.

The original DOS sound drivers in `SOUND/` (`*.DIG`, `*.MDI`, `*.INI`, `*.SBK`) are never
read.

## Display options

Port additions in OPTIONS → DETAIL OPTIONS (also in the pause menu), saved in `save/video.txt`;
none of them changes the game itself:

| Option | Effect | Default |
|---|---|---|
| VIEW DISTANCE | how far the world is drawn: 16 cells (the original), 32 or 48; the haze keeps the original proportions, and beyond the original distance crafts and objects fade into it like the terrain | 48 |
| ANTI-ALIASING | 4× multisampling of the 3D view | on |
| SMOOTHING | filtered sky, sharp-bilinear scaling of the 640×480 menus and HUD (even pixel sizes at any window size) | on |
| TEXTURE FILTERING | smooth textures on the ground, blocks, crafts and billboards, mipmapped and anisotropically filtered in the distance (no shimmer), and smooth light gradients instead of the 64 light steps | on |
| SUN GLARE | with a [true-colour sky](#true-colour-skies) that has a sun: bloom around the sun, a warm veil and a faint lens-flare chain while the sun is in view and not hidden by terrain; no effect on the original skies | on |

Texture filtering keeps the original colours: close up, the four nearest texels are each lit through
the track's light table and then blended; in the distance, where texels are smaller than a pixel,
the mipmapped colour copy is used with the light table applied as a fitted per-row colour transform
(about 1 % mean difference). With VIEW DISTANCE at the first step and the other options off the
picture matches the original.

## True-colour skies

Port addition (spec 01 §4.1, spec 08 §6.1): besides its 256-colour sky `DATA/SKY0-N.DAT`, track
N + 1 may have a sky in true colour, `DATA/SKY0-N.PNG`, with a small text sidecar
`DATA/SKY0-N.TXT`. When the PNG is there it replaces the palette sky; without it the track looks
exactly as before. Both are read through the asset package, so they work in bundles too.

The PNG is a **cylindrical panorama** (8-bit RGB or RGBA, or any PNG the decoder can expand; up to
8192 × 8192): x runs around the horizon, y from the top elevation down to the bottom one, both
linearly. It is seen in true perspective, so a 360° image (e.g. 4096 × 1024 for 0..90°) never
repeats. Near the horizon it fades into the haze colour of the track's colour table (light row 0),
the colour the terrain fades into, so sky and fog meet without a seam; below the horizon it is
that haze colour.

The sidecar holds `key = value` lines (`#` comments; all keys optional):

| Key | Meaning | Default |
|---|---|---|
| `repeat` | copies of the image around a full turn (1..16) | 1 |
| `azimuth` | azimuth of the image's left edge, degrees clockwise from map north (map north = decreasing row = camera yaw 0; 90 = east, increasing x) | 0 |
| `top` / `bottom` | elevation (degrees) of the top and bottom edges | 90 / 0 |
| `horizon_blend` | degrees above the horizon over which the sky fades into the haze colour (0 = a hard edge) | 4 |
| `sun` | sun azimuth and elevation in degrees; without this line there is no sun | none |
| `sun_size` | angular radius of the sun disk, degrees (0.1..10) | 1 |
| `sun_colour` | sun colour, 8-bit RGB | 255 240 215 |
| `glare` | strength of the sun glare (0..4, 0 = none) | 1 |

```
# DATA/SKY0-0.TXT of assets/custom
repeat = 1
azimuth = 0
top = 90
bottom = 0
horizon_blend = 5
sun = 248 13
sun_size = 1.3
sun_colour = 255 232 196
glare = 1
```

The sun is drawn by the game as a disk with a soft corona on top of the panorama (so the image
itself should not contain a sun disk). With SUN GLARE on (display options) and the sun in view, it
adds a bloom, a warm veil towards the sun and a few faint lens-flare ghosts along the line from the
sun through the screen centre, fading out as the sun nears the screen edge. A small disc around the
sun is tested against the depth buffer each frame, so the glare fades smoothly as the sun slips
behind a canyon bank, a block or a craft and is gone while it is hidden. The glare is drawn on the
3D view only, below the HUD, and costs one tiny extra pass; it changes nothing in the game itself.

## Languages

The UI text is loaded at start-up from `lang/<code>.txt` in the data directory: `en`, `de`, `fr`,
`es`, `it` (the original five) and `pl` (Polish, added by this port). These files are part of the
repository; the rest of `resources/` is not. Each line is `key = text` in UTF-8; keys `000`..`193`
are the original string table, the named keys (`language`, controls-menu labels) are port additions.
Missing entries fall back to English; `en.txt` is required.

The language menu appears on the first run (no `config.dat`) and under OPTIONS → LANGUAGE (port
addition); in the menus Alt+1..4 switch to English / German / French / Spanish, as in the original.
The original fonts have no Polish letters, so Ą Ć Ę Ł Ń Ó Ś Ź Ż are drawn as A C E L N O S Z for
now. `cargo test` checks that every language file is complete.

## Controls (race)

| Action | Keys |
|---|---|
| accelerate / brake | Up / Down (or W / S) |
| steer | Left / Right (or A / D) |
| minigun | Z, comma, V |
| missile | X, period, B |
| booster (hold to charge, release to fire) | C, slash, Space |
| pause menu | P (Escape or P to resume) |
| end race | Escape |
| quit game | right Shift + Q |
| camera views | F1..F9, F10 orbit; 1..8 follow a craft; - / = zoom |
| sliding puzzle | Ctrl + Tab |
| cheats (after solving the puzzle) | Alt + F1..F6 |

## Gamepad

Port addition, replacing the original's joystick (the REINITIALISE JOYSTICK menu item is gone).
Any controller known to `gilrs` works (Xbox, PlayStation, Switch Pro and most generic pads); it can be
plugged in at any time, and the pad last used drives the game. Names below are Xbox (PlayStation).

| Action | Pad (default) |
|---|---|
| accelerate / brake | left stick or D-pad up / down, RT / LT (R2 / L2) |
| steer | left stick or D-pad left / right |
| minigun / missile / booster | A (✕) / B (○) / X (□) |
| pause menu | Start (Options); Start or B resumes |
| camera view | Y (△) cycles cockpit and the three chase views (unless Y is bound to an action) |
| zoom out / in | LB / RB (L1 / R1) |
| end race | hold Back / Select for 1 second |
| menus | D-pad or stick to move, A to select, B to go back |

**Remapping**: in OPTIONS → CONTROLS each action shows its keys and, in brackets, its pad button
(e.g. `MINIGUN: Z , V [A]`). Select an action, then press up to 4 keys (they replace its old keys)
and Enter to confirm, or a pad button, which becomes its pad button and confirms at once. A key or
button taken by another action moves over. Escape or Back cancels and keeps the old binding. RESET
TO DEFAULTS restores keys and pad buttons. Fixed on the pad: the stick and D-pad directions, Back (end
race) and the menu buttons (A select, B back, Start).

OPTIONS → CONTROLS → ANALOG STEERING (off by default) makes the left stick steer proportionally
instead of fully left or right; the D-pad still steers fully. Pad settings are saved in
`save/pad.txt` (`analog_steering = on|off` and `<action> = <button>` lines). The name
entry needs the keyboard. Rumble is not supported (the controller library has none on macOS).

## Editor

```
cargo run --release -p c8-edit -- [--asset <dir|file.c8pak>] [<file>]
```

`c8h18-edit` asks for the asset package on the first start (File > Game data folder… or Asset
bundle…, or `--asset`; `--data` also works) and lists its files by kind; a bundle is listed like a
folder. `<file>` may also be a path inside the package, e.g. `MAPS/LEVEL0-1.DAT`. Levels open in a top-down map with switchable views (textures with lighting,
height, light), overlays (block columns, entities with walls, waypoint links and checkpoints,
chargers and start area, grid) and a cell inspector. Tools (keys in brackets):

| Tool | Use |
|---|---|
| Inspect (I) | cell details under the pointer |
| Height (H) | drag to raise, Shift+drag to lower; radius and step |
| Texture (T) | paint or flood-fill with a texture from the level's set, in any orientation (R rotates, F mirrors); Alt+click picks from the map; edit the texture's attributes (e.g. the "track" bit) |
| Light (L) | paint brighter / darker (Shift), smooth, or bake from a light direction and the slopes into the selection or the whole map |
| Select (S) | drag a rectangle; ⌘C copies, ⌘V pastes (click to place) heights, textures and light |
| Objects (O) | entities and regions: click to select, drag to move (regions: drag a corner to resize); the inspector names every type and sub-type and labels the fields by their meaning (link, order, group, target group, extent); Add places pickups, trucks, checkpoints, triggers, cameras, effects, or draws wall and waypoint chains point by point (click an existing point to join it); Delete removes (links to it are cleared), Cmd+D duplicates; layers hide kinds of entities; a searchable list of all entities |
| Blocks (B) | select a block column on the map, paint it on cells or erase columns; edit the column (floor texture, which of the 8 levels are solid, the block on each) and the block (texture and orientation of its six faces, picked from the texture set); copy a column or block before changing it when other cells share it |

**Check** lists what would break the track in the game, each item linked to its place on the map:
missing start/finish checkpoint or gaps in the checkpoint order, no start area or too few start
tiles, missing chargers, links to missing entities, waypoints that lead nowhere or more than the
game's 200 waypoint positions, groups no trigger activates, columns with solid levels but no
block. **Run AI test** races the 8 computer players on the edited level in the game's headless
simulation (`c8h18 --sim`) and reports the laps each completed and where any got stuck for 15
seconds or more (marked on the map).

**3D preview**: View → Map / Map + 3D / 3D shows the level as the game builds it (same mesh code,
colours lit exactly through the track's colour table), updated while you edit, with walls, the
computer players' route, checkpoints, pickups, trucks and regions as lines. Drag to look, WASD or
the arrow keys to fly, Q/E down/up, Shift faster, wheel forward; "Camera to map view" jumps above
the map's centre, and in Map + 3D a double-click on the map moves the camera there (the camera is
the white triangle on the map). Haze can be switched on.

Every stroke is one undo step (⌘Z / ⇧⌘Z). Pan with the scroll wheel or a right/middle drag, zoom
with pinch or Ctrl/⌘ + wheel.

Pictures and fonts open in a pixel editor: terrain textures (256 tiles of 64×64), skies, screens
(title, backdrops, loading picture, puzzle), model texture atlases (with their texture rectangles
shown), billboards (TMAPS), sprite sheets and fonts. Frames (textures, glyphs, sprites) are listed
on the left; fonts label each glyph with its code page 437 character and show a preview text.
Tools: Pencil (B), Eraser (E, sheets and fonts only: makes pixels transparent), Fill (G), Pick (I,
or right-click / Alt+click); brush size; flip, rotate, clear, copy/paste frames (⌘C / ⌘V, also
between files); [ and ] or the arrow keys step through frames. Glyphs and sprites can be resized
(a glyph's width is its advance) and cropped to their content. Colours come from the game palette;
OLFNT/OSFNT glyphs are masks (0 = outline, any other value = text colour). **PNG**: export / import
one frame, or all frames as one grid image to edit in any paint program; imported colours are
matched to the nearest palette colour. The palette (PALET0-0) has its own editor.

**Colour tables** (TABLE0-N) show the 64 light rows (with a terrain texture at several light
levels), the blend table and the flat colours, and regenerate them: light rows from the palette and
a haze colour, the blend table from the palette, flat colours from the track's textures (after a
palette edit: save the palette, then regenerate each table). The rebuilt light rows match 96 % of
the shipped entries and the blend table 95 %.

**Language files** (`lang/<code>.txt`) list every key with the English text beside the
translation, edited in place; filters for problems, missing entries and entries still in English;
checks for missing keys, keys out of range, `$` markup that differs from English, characters the
fonts cannot draw and text wider than the screen in the large menu font, with a preview of the
entry in that font. "Add missing entries" fills them with the English text. Comments and order of
the file are kept.

**Sound and music** (SOUND.DAT, MUSIC.DAT) list the banks, formats and items with their length and
what the game uses them for; play samples and tunes (the FM tunes on the emulated OPL3, as in the
game), show the waveform, export a sample as WAV, a tune as XMIDI or rendered to WAV, replace a
sample from any PCM WAV (converted to 8-bit mono, in all three 8-bit formats at once) and a tune
from an XMIDI file. For music see also [Music](#music): each tune shows whether the game plays its
XMIDI or an OGG replacement, and the side panel imports and removes OGGs, edits their loop points,
exports `.mid` and imports a Standard MIDI file.

**Explosion rings** (SRCH0-0) show the 32 × 32 grid of rings the big and massive explosions spread
over, with each ring's walk order, start cell and unreached cells; paint cells into rings, move
start cells, regenerate the circles (identical to the shipped grid except 16 hand-placed cells) and
play the spread of either explosion. Only rings 0–5 are used by the game.

Saving writes unpacked files (the game reads both; sheets also get their `.TAB`); originals in the
package are never overwritten (Save asks for a new name, e.g. a file in another package folder).
A bundle's files are unpacked into a temporary mirror folder (`<temp>/c8h18-edit-bundles/`) that
the documents open; besides Save as, **File > Save into bundle** writes the active file into the
mirror and repacks the mirror into a temporary file that is then renamed over the bundle, so the
bundle is replaced in one step or not at all. **Test in game** (button or ⌘R)
writes the active file and every other open file with changes into a temporary folder and starts
`c8h18 --asset <package> --override <folder>` (the game next to the editor, or on the PATH; the
package may be a bundle; `--override` is an internal option: files in that folder win over the
package's): textures and
skies on their track's race, levels on the race of the track set they are shown with, colour
tables and race sounds on their track's race, tunes where they play, a language with `--lang`, the
rest in the menus. The plan is in `docs/editor-plan.md`.

### Models

Model files (`OBJECTS/DATA/*.DAT`; one model per file) open in a 3D view drawn the way the game
draws them (the game's triangle builder from `c8-render`, palette and atlas through the chosen
colour table). Drag to orbit, right/middle or Shift drag to pan, wheel or pinch to zoom; toggles
for faces, wireframe, shading (to read the shape; the game has none) and axes (red right, green
up, blue forward). Hovering a triangle shows its type, atlas entry or colour, vertices, shade bytes
and UVs; clicking selects it to change its type, entry/colour and UVs, flip or delete it (undoable).
The side panel lists vertices, triangles per type, texture entries, colours, bounds, size, flags
and the format limits.

**Blender**: *Export .glb* writes glTF 2.0 with the texture sheet (TEX0-0 with the palette applied)
embedded as a PNG and unlit materials named after the triangle type and atlas entry or colour
(`c8_t5_e12`, `c8_f4_c37`, also in the materials' `extras`). It imports upright and facing
Blender's front (−Y); 1 m = 256 file units (a car is about 4 m). Import into Blender with the
default options; edit; assign existing `c8_` materials to new faces and keep their UVs on the
sheet; export as glTF Binary with *Include → Custom Properties* on. *Import…* reads .glb, .gltf
(embedded buffers) or .obj back, rounds positions to file units, maps materials and UVs back to
types and atlas entries (foreign materials: textured → type 5 with the entry found from the UVs,
others → the nearest palette colour, type 0), and checks the limits (at most 256 vertices, as
triangles address them with a byte; coordinates within ±128 m; UVs within 255 texels of the
entry's corner). Unchanged geometry comes back byte-identical (the export carries the original
file, and the open model is the template); changed geometry keeps the original vertex order,
triangle order and shade bytes for what is still there. *Export .obj* writes OBJ + MTL + PNG
with the same conventions. *Test in game* puts the model in a race on track 1 (TRACK0-* in the
track-selection screen).

Without a window:

```
c8h18-edit [--asset <package>] [--table 0..5] --export-model OBJECTS/DATA/CAR0-0.DAT car.glb   # or car.obj
c8h18-edit [--asset <package>] [--like OBJECTS/DATA/CAR0-0.DAT] --import-model car.glb CAR0-0.DAT
```

The palette, colour table and texture atlas come from the package: e.g. `--asset assets/custom`
exports with the new texture atlas and maps imported UVs to its entries. (Advanced:
`--override <dir>` takes files from `<dir>` first.)

`cargo test -p c8-edit blender -- --ignored` runs every shipped model through Blender (needs
`blender` on the PATH): glTF (Blender's default import) and OBJ come back byte-identical. With
"Merge Vertices" on, Blender deletes zero-area triangles and the back of two-sided faces (MARSH0-0,
SIGN0-0); everything else survives.

### MCP (AI agents)

The editor is also an MCP server, so an agent such as Claude Code can inspect and edit levels:
- read cells, terrain, textures and light;
- edit block columns, entities, wall and waypoint chains, and regions;
- run the checks and the AI test;
- see the level through top-down and 3D pictures.

Every editing call is one undo step, labelled "MCP: …".

- **Live, in the running editor.** The editor listens on `http://127.0.0.1:47811/mcp` (another
  free port if that one is taken). It accepts this computer only and needs a bearer token.
  - Click "MCP" in the status bar for the endpoint, the token, a "Copy Claude Code command" button
    (`claude mcp add --transport http c8h18-edit <url> --header "Authorization: Bearer <token>"`),
    a pause switch and the activity log.
  - The agent works on the open level tabs, and you see its edits as they happen. Cmd+Z undoes them.
  - The port and token are remembered; "New token" replaces the token.
- **Headless.** `c8h18-edit mcp --asset <package> [--allow <folder>]... [level]` serves MCP over
  stdio without a window, e.g.
  `claude mcp add c8h18-edit -- c8h18-edit mcp --asset /path/to/game --allow /path/to/tracks`.
  - Levels are read from the data folder and the allowed folders, and written only into the
    allowed folders (default: the current directory).
  - The game data folder is never written in either mode.

Tools:
- `editor_status`, `level_open` (an original track as a template, or a file), `level_save`,
  `level_info`;
- `cells_read`, `cells_write`, `heights_fill`, `heights_ramp`, `heights_smooth`, `textures_fill`,
  `light_fill`, `light_bake`, `texture_attr_set`;
- `columns_list`, `block_define`, `column_define`, `column_place`, `column_clear`;
- `entities_list`, `entity_add`, `entity_update`, `entity_delete`, `chain_add`;
- `region_add`, `region_update`, `region_delete`;
- `validate`, `ai_test`, `render_top`, `render_3d` (PNG images);
- `history`, `undo`, `redo`.

Resources: `docs://format` (units, layers, entity types, what a race needs), `level://summary`,
`level://validation`. Prompts: `design-track`, `fix-validation`.

## Our own assets

`assets/custom/` is an asset package made from scratch, in the original layout, for
`c8h18 --asset assets/custom` (or a bundle packed from it). What it lacks falls back as described
in [Partial data sets](#partial-data-sets): at the time of writing the terrain textures, skies and
colour tables of tracks 2–6 (`TEXTU0-*`, `SKY0-*`, `TABLE0-*`; track 1 has its own terrain textures
and colour table, `SKY0-0` is being made), the screens (`LOGO0-*`, `OSCR0-*`, `ONET0-*`, `TITLE`, `PUZZLE`), the billboards (`TMAPS`),
`SRCH0-0`, `INTRO` and tracks 2–6. `lang/` holds copies of the language files.

### Fonts

`assets/custom/DATA` holds new font sheets (HFONT, MFONT, OLFNT, OSFNT, PFONT at both resolutions)
drawn from scratch: every entry keeps the original's width, height and ink box, so text lays out
exactly as before, but the glyphs are new.

### Models

`assets/custom/OBJECTS/DATA` holds all 62 models made from scratch (48 crafts, recovery truck,
cones, barrels, marshal car, sign, and the six track previews of the carousel, built from the level
files) and a new object texture sheet with the original's layout. They are written by a local generator, `modelgen/` (not in
git; see its README, which also covers previews for custom levels).

### Sound effects

`assets/custom/SOUND/SOUND.DAT` holds new sound effects synthesised from scratch: both banks (the 35
race and front-end entries, NULL placeholders kept, and the 14 intro sounds) in slot 0 (16-bit
44.1 kHz) and in slots 3, 4 and 5 (8-bit 22 and 11 kHz, converted from the same sounds).

The game takes each SOUND.DAT bank from slot 0 (16-bit 44.1 kHz) when the file has it there and
falls back to slot 3 (8-bit 22 kHz) otherwise, so the original file plays as before (spec 09 §9).
The editor's sound panel shows every slot the file has; "Replace from WAV…" converts to the slot's
format (16-bit 44.1 kHz in slot 0), in every slot of the bank at once with "Replace in all formats".

### Music

`assets/custom/SOUND` holds new music composed from scratch, as FM tunes like the original's:
`MUSIC.DAT` (the five tunes in the FM slot) and `SAMPLE.OPL` / `SAMPLE.AD` (our own two-operator
timbre bank, with a timbre for every General MIDI program and a percussion bank). The tunes are written by a local generator, `musicgen/`
(not in git; see its README).

**OGG replacements.** Any tune can be replaced by a recording: put an OGG Vorbis file named after
the tune in `SOUND/MUSIC/` of the asset package (folder or bundle):

| Tune | Plays | File |
|---|---|---|
| 1 | race, tracks 1–2 | `SOUND/MUSIC/TGAME1.OGG` |
| 2 | race, tracks 3–4 | `SOUND/MUSIC/TGAME2.OGG` |
| 3 | race, tracks 5–6 | `SOUND/MUSIC/TGAME4.OGG` |
| 4 | intro movie | `SOUND/MUSIC/TINTRO2.OGG` |
| 5 | (unused) | `SOUND/MUSIC/TGAME3.OGG` |

The file loops between the Vorbis comments `LOOPSTART` and `LOOPSTART + LOOPLENGTH` (or `LOOPEND`),
in samples; without them the whole file loops. Any sample rate, mono or stereo; it plays at the
music volume, at half amplitude so a full-scale recording sits with the sound effects like the FM
music does. For example, with the loop starting at 12.5 s and lasting 96 s at 44.1 kHz:

```
ffmpeg -i tune.wav -c:a libvorbis -q:a 6 -metadata LOOPSTART=551250 -metadata LOOPLENGTH=4233600 TGAME1.OGG
```

(an ffmpeg without libvorbis has its own encoder: `-c:a vorbis -strict -2`). In the editor, open
`SOUND/MUSIC.DAT`, select a tune and use "Use OGG…": the file is written to `MUSIC/<name>.OGG` beside
the MUSIC.DAT when you save it (the game data folder itself is never written: saving there asks for
another place, e.g. `assets/custom/SOUND/MUSIC.DAT`), and "Test in game" takes it along. The panel
shows the loop over the waveform, edits it in samples ("Play loop seam" plays the last 3 s before
the jump), and "Remove OGG" goes back to XMIDI. "Export as WAV…" writes the OGG decoded, or the
XMIDI rendered on the OPL3 (48 kHz, up to the loop end of a second pass so the loop is seamless)
with the loop in a `smpl` chunk and the `LOOPSTART`/`LOOPLENGTH` values to use.

**MIDI.** Bring your own tune as a Standard MIDI file: General MIDI programs (each maps to a timbre
of the bank), drums on channel 10, and the loop from `loopStart` / `loopEnd` markers (or controller
111; otherwise the whole piece loops). "Replace from MIDI…" in the editor converts it, and so does
the command line:

```
c8h18-edit --midi-to-xmi tune.mid TGAME1.XMI [--no-loop]   # note-offs → durations, ticks → 1/120 s, loop → FOR/NEXT
c8h18-edit --xmi-to-midi TGAME1.XMI tune.mid               # for MuseScore, LMMS or a DAW
```

### Maps

`assets/custom` holds a new track 1 made from scratch (`MAPS/LEVEL0-1.DAT`): a clockwise canyon
circuit with a start grid, a banked sweeper, a crest, a chicane, a dip, a hairpin and a pit straight,
with checkpoints, computer-driver waypoints, charger pads, collectables and props. It passes the
editor's checks and the computer drivers complete laps. The package also holds its minimaps
(`DATA/TRACK0-0/1`) and its carousel model (`OBJECTS/DATA/TRACK0-0.DAT`). It races on our own
terrain textures and colour table (see below). It is written by a local generator, `mapgen/` (not
in git; see its README).

### Terrain textures

`assets/custom/DATA/TEXTU0-0.DAT` holds track 1's 256 terrain textures, generated procedurally from
scratch: sandy desert ground with gravel, clay and scrub patches (and transition tiles between
them), sand verges, sandstone banks with strata, asphalt with yellow edge lines, the chequered
start/finish line and grid boxes, charger pads in the HUD minimap's colours, corner chevrons,
barrier and billboard faces (with an invented brand), and a general set for levels made in the
editor (rock, sand, concrete, metal, grates, walls, stripes, signs, road markings, water). Track 1's
colour table `DATA/TABLE0-0.DAT` takes its flat colours (TEXTURE MAPPING off) from them. They are
written by a local generator, `terraingen/` (not in git; see its README, which lists the ids).

### Sky

`assets/custom/DATA` holds track 1's sky: a warm late-afternoon sky over the canyon, a deep blue
zenith fading to a pale warm horizon, soft cumulus with sunlit edges and thin high cirrus, and a
low west-south-west sun (azimuth 248°, elevation 13°). It comes as a [true-colour
sky](#true-colour-skies) (`SKY0-0.PNG`, a 4096 × 1024 panorama, with `SKY0-0.TXT`) and as the
classic 256-colour `SKY0-0.DAT` resampled from it and dithered to our palette, for tools and as the
fallback. The sun stays behind or beside the crafts on the start straight and along the top, and
comes into view in the hairpin, on the infield's westward stretch and along the bottom, where it
drops behind the canyon banks in the turns. It is written by a local generator, `skygen/` (not in
git; see its README).

### HUD and icons

`assets/custom/DATA` holds the HUD (`PANEL0-0/1`), the icon sheets (`HSPR0-0`, `MSPR0-0`) and the
pointer (`POINT0-0`) drawn from scratch: every entry keeps the original's size and ink box (the
gauge pieces also their outlines, so they still join up), the drawings are new. The port only uses `PANEL0-1`. They are written by a local
generator, `spritegen/` (not in git; see its README).

## Debug automation

`C8H18_AUTO="frame:op:arg;..."` runs scripted input. The ops are `shot:<png path>`,
`key:<Key>`, `hold:<Key>`, `release:<Key>`, `padhold:<Button>`, `padrelease:<Button>` (buttons: Up,
Down, Left, Right, A, B, X, Y, LB, RB, LT, RT, Start, Back) and `quit`; `frame` counts display frames. Used for
screenshot tests. A `shot` is also taken when the window has no drawable surface (hidden, display asleep or
locked): that frame is drawn offscreen. Frames then run unthrottled, so space the frame numbers widely. In the editor, `HIEDIT_SHOT=<png path>` saves a screenshot of the window after
30 frames and quits; `HIEDIT_TOOL=height|texture|light|select|objects|blocks` starts with that tool, `HIEDIT_ENTITY=<record>` / `HIEDIT_COLUMN=<id>` with that entity / column selected, `HIEDIT_VIEW=3d|split` with the 3D preview. `HIEDIT_SHOT_FRAMES=<n>` takes the screenshot after n frames; `HIEDIT_MCP_TOKEN` / `HIEDIT_MCP_PORT` fix the live MCP token and port (and keep the endpoint on with `HIEDIT_SHOT`), `HIEDIT_MCP_PANEL=1` opens the MCP panel.
