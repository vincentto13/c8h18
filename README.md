# C8H18

A recreation of the 1995 hover-craft combat racer from the specification in `docs/spec/`, running on the
original data files in `resources/`.

- Rendering: `wgpu` (Metal / Vulkan / DX12). Textures stay 8-bit palette indices and the shaders go
  through the per-track light table, so lighting and haze match the original. The 3D view uses the
  whole window, widescreen included. The 2D layer (menus, HUD) is drawn at 640×480 in palette space.
- Audio: `cpal` with a software mixer. Music is XMIDI played on an emulated OPL3 using `SAMPLE.OPL`.
- Simulation: fixed 20 Hz tick, drawing interpolated between ticks; the front end runs at 70 frames/s.
- The 320×200 mode and the network game are not included. Where the spec lists "port deviations",
  this version follows them.

## Repository layout

A Cargo workspace with three crates:

| Crate | What | Binary |
|---|---|---|
| `crates/c8-data` | the data file formats (spec 01), read and write; shared by the game and the editor | |
| `crates/c8-game` | the game | `c8h18` |
| `crates/c8-edit` | level and resource editor (see `docs/editor-plan.md`) | `c8h18-edit` |

## Build and run

```
cargo build --release                # the game (the workspace's default member)
./target/release/c8h18            # data directory defaults to ./resources
cargo build --release --workspace    # game and editor
cargo test --workspace               # all tests
```

Options:

| Option | Effect |
|---|---|
| `-s`, `/s`, `--silent` | no sound or music |
| `--no-intro` | skip the intro movie |
| `--speed N` | game speed in percent of the original 20 ticks/s (default 90; e.g. 100 = original, 80 = slower) |
| `<path>` or `GAME_DATA=<path>` | data directory (DATA/, MAPS/, OBJECTS/, SOUND/) |
| `--race N` / `--demo N` | debug: go straight to a race / demo race on track N |
| `--level <file>` | with `--race N` / `--demo N` / `--sim N`: race on this level file (e.g. saved by the editor) instead of track N's; textures, sky and music still come from track N |
| `--sim N TICKS` | debug: run a headless computer-only race and print the crafts' progress |
| `--override <dir>` | load data files from `<dir>` (same layout, e.g. `<dir>/DATA/TEXTU0-0.DAT`) in place of the data directory's; used by the editor's Test in game |

Saved files (`config.dat`, `save00.dat`..) go to `save/` inside the data directory.

## Display options

Port additions in OPTIONS → DETAIL OPTIONS (also in the pause menu), saved in `save/video.txt`;
none of them changes the game itself:

| Option | Effect | Default |
|---|---|---|
| VIEW DISTANCE | how far the world is drawn: 16 cells (the original), 32 or 48; the haze keeps the original proportions, and beyond the original distance crafts and objects fade into it like the terrain | 48 |
| ANTI-ALIASING | 4× multisampling of the 3D view | on |
| SMOOTHING | filtered sky, sharp-bilinear scaling of the 640×480 menus and HUD (even pixel sizes at any window size) | on |
| TEXTURE FILTERING | smooth textures on the ground, blocks, crafts and billboards, mipmapped and anisotropically filtered in the distance (no shimmer), and smooth light gradients instead of the 64 light steps | on |

Texture filtering keeps the original colours: close up, the four nearest texels are each lit through
the track's light table and then blended; in the distance, where texels are smaller than a pixel,
the mipmapped colour copy is used with the light table applied as a fitted per-row colour transform
(about 1 % mean difference). With VIEW DISTANCE at the first step and the other options off the
picture matches the original.

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
cargo run --release -p c8-edit -- [--data <game data folder>] [<file>]
```

`c8h18-edit` asks for the game data folder on the first start (or takes `--data`) and lists its
files by kind. Levels open in a top-down map with switchable views (textures with lighting,
height, light), overlays (block columns, entities with walls, waypoint links and checkpoints,
chargers and start area, grid) and a cell inspector. Tools (keys in brackets):

| Tool | Use |
|---|---|
| Inspect (I) | cell details under the pointer |
| Height (H) | drag to raise, Shift+drag to lower; radius and step |
| Texture (T) | paint or flood-fill with a texture from the level's set, in any orientation (R rotates, F mirrors); Alt+click picks from the map; edit the texture's attributes (e.g. the "track" bit) |
| Light (L) | paint brighter / darker (Shift), smooth, or bake from a light direction and the slopes into the selection or the whole map |
| Select (S) | drag a rectangle; ⌘C copies, ⌘V pastes (click to place) heights, textures and light |

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
matched to the nearest palette colour. The palette (PALET0-0) has its own editor; the colour tables
are not recalculated from it.

Saving writes unpacked files (the game reads both; sheets also get their `.TAB`); originals in the
data folder are never overwritten (Save asks for a new name). **Test in game** (button or ⌘R)
writes the active file and every other open file with changes into a temporary override folder and
starts `c8h18 --override <dir>` (the game next to the editor, or on the PATH): textures and
skies on their track's race, levels on the race of the track set they are shown with, the rest in
the menus. Colour tables, models, sounds and language files open read-only. The plan, including
the built-in MCP server, is in `docs/editor-plan.md`.

## Debug automation

`C8H18_AUTO="frame:op:arg;..."` runs scripted input. The ops are `shot:<png path>`,
`key:<Key>`, `hold:<Key>`, `release:<Key>`, `padhold:<Button>`, `padrelease:<Button>` (buttons: Up,
Down, Left, Right, A, B, X, Y, LB, RB, LT, RT, Start, Back) and `quit`; `frame` counts display frames. Used for
screenshot tests. In the editor, `HIEDIT_SHOT=<png path>` saves a screenshot of the window after
30 frames and quits; `HIEDIT_TOOL=height|texture|light|select` starts with that tool.
