# 00 — Overview and conventions

Scope: this chapter fixes what the specification covers, the units and conventions every other chapter uses (map
coordinates, fixed-point formats, angles, the simulation tick, the two screen modes and their virtual 2D coordinates,
the palette, fonts and the PANEL sprite sheet, the data directory), the supported languages, and gives a one-page
summary of the game flow from program start to quitting.

## 1. What is specified

It is a hover-craft combat racer for up to 8 crafts on 6 tracks. The specification describes the
DOS program so that it can be rebuilt on top of the existing data files:

| Chapter | Content |
|---|---|
| 00 | this overview |
| 01 | every data file format, byte-exact |
| 02 | the front end: menus, settings, championship, results screens, attract mode |
| 03 | keyboard controls |
| 04–08 | (separate chapters) simulation, flight model, weapons, computer players, rendering, HUD |
| 09 | sound effects and music |
| 10 | the intro movie |
| 11 | what is known of the network game |

Each chapter ends with a "Port deviations" section where the known Rust recreation deliberately differs; the main text
always describes the original.

## 2. Coordinates and units

### 2.1 The map

- A level is a grid of **256 columns × 160 rows** of cells. One cell is one terrain tile; all distances below are in
  cells unless stated otherwise.
- **x** is the column (0..255, growing to the right/east on the map), **y** or **row** is the row (0..159, growing
  downwards/south on the map), **height** (z) grows upwards.
- Cell (x, y) is stored row-major (see chapter 01, level file). Terrain heights are stored per cell and act as the
  height of the cell's corner (x, y); a cell's surface spans the four corner heights (x, y), (x+1, y), (x+1, y+1),
  (x, y+1).
- Blocks stacked on a cell (columns) are exactly 1.0 cell high each, counted upwards from the cell's corner heights.

### 2.2 Fixed-point formats

| Quantity | Format | Notes |
|---|---|---|
| Thing / craft position (x, row, height) | 8.8 fixed point, 256 units per cell | stored little-endian as 16-bit words; in files the fraction byte comes first, then the integer byte |
| Terrain height of a cell | 8.8 fixed point (0..255.996) | |
| Model vertices, camera depth, sprite heights, "view units" | 2048 units per cell | the renderer works at `position << 3`; a craft model is about 0.5 cell long, the tank 1.0 |
| Engine sounds' hearing range, distances in sound code | 8.8 cells | e.g. 18 cells = 4608 units |
| Shield, fuel, ammo | 16-bit integers, 10000 = full (20000 = doubled) | |
| Volumes in the config | 0..32768 (0x8000) | see chapter 02 |
| Lap and race times | simulation ticks (frames), never converted to seconds | records are capped at 9999 ticks per lap |

### 2.3 Angles

- Craft headings, pitch and roll, camera angles: **unsigned 16-bit, 65536 units per full turn**.
- Heading 0 faces **north = decreasing row**; positive angles turn **clockwise** seen from above. Moving at heading h
  with distance d: `x += sin(h)·d`, `row −= cos(h)·d`.
- Pitch is positive **nose-down**.
- The sine/cosine tables have **2048 entries per turn** (the 16-bit angle is shifted right by 5), with amplitude 65536
  (= 1.0): `sin[i] = trunc(65536 · sin(i · π / 1024))`, `cos[i] = sin[(i + 512) & 2047]`.
- The front-end 3D carousels use 2048-per-turn angles directly.
- Degrees appearing in the text are conversions for the reader (e.g. 0x0e38 = 20°).

### 2.4 Time: the simulation tick

- The original's per-frame loop has **no frame limiter** and does not wait for the vertical retrace: the race
  simulation, the race frame counter and every per-frame constant advance once per drawn frame, so the original's
  speed depended on the machine.
- The design rate is **20 frames per second** (periodic logic tests `frame % 20`; the PlayStation version of the same
  engine runs at about 20 Hz). A recreation shall run the race simulation at a **fixed 20 Hz tick (50 ms)**. Every
  "per tick" or "per frame" constant in the race chapters means one 50 ms step. Example: lap times count ticks; a lap
  time of 1200 means 60 s.
- Exceptions that use real time in the original:
  - the race-start countdown: 3 lights of 101 hundredths of a second each (3.03 s, measured with the 100 Hz DOS
    `clock()`), see the race chapters;
  - the front-end idle timer (15 s, DOS time in seconds);
  - palette fades (one step per vertical retrace, 70 Hz);
  - the intro movie (its own 120 Hz timer, chapter 10).
- The front-end screens count frames (box animation 32 frames, typewriter one character per frame, carousel turn 24
  frames, blink rates). They were tuned on 70 Hz VGA hardware; a recreation should run the front end at **70 frames
  per second**.

## 3. Screen modes and virtual 2D coordinates

| Mode word | Resolution | Hardware mode | Used for |
|---|---|---|---|
| 1 | 320 × 200, 256 colours | VGA mode 13h | default at every start; the only mode in low-memory configurations |
| 8 | 640 × 480, 256 colours | VESA mode 0x101 | selectable in OPTIONS → DETAIL OPTIONS → SCREEN RESOLUTION |

- The game always starts in 320×200. The chosen mode is written into the human player's record (config offset 0x4c,
  chapter 01) but is **not** applied at the next start.
- **Virtual 2D coordinates.** All 2D drawing (menus, text, HUD sprites, mouse range) works in a space **640 units
  wide**: 640×400 in 320×200 mode, 640×480 in 640×480 mode. In 320×200 every 2D coordinate and every sprite size is
  halved when drawn; in 640×480 coordinates are pixels.
- **Nothing is pixel-doubled.** Each mode has its own sprite and font sets (files ending in `-0` for 320×200, `-1` for
  640×480, chapter 01). To keep sizes virtual, the width and height bytes of every TAB entry of the 320×200 sets (and of
  `POINT0-0`, shared by both modes) are doubled in memory after loading and halved by the blitters, so each sprite is
  still drawn 1:1 from its own set. `POINT0-0` and `MFONT0-0` (font slot 1) therefore look half as big in 640×480.
- Some tables are kept in **320×200 units** and doubled for 640×480: the front-end box rectangles and the HUD layout
  tables.
- Text justification: centred text starts at x = (640 − width) / 2, right-justified text ends at x = 638.
- The 3D race view always covers the full screen (320×200 or 640×480). The focal length is 94 × screen width / 320
  (94 or 188), so both modes have the same horizontal field of view and 640×480 shows a taller vertical one.
- Screen mode changes fade the palette out (16 steps), reload the mode's sprite sets, set the mode and the palette
  at once (no fade in) and select font slot 0.

## 4. Palette and colour tables

- One 256-colour palette for the whole game, `data/palet0-0.dat` (6-bit VGA values). The intro movie carries its own
  palettes (chapter 10).
- Each track has a lighting/translucency table (`data/table0-N.dat`, chapter 01): 64 brightness remaps (row 32 = no
  change, row 0 = haze colour), a 256×256 blend table and four flat-colour tables. The front end's translucent box and
  blended text use whichever table is loaded at the time (which track's table is loaded before the first race is not
  documented).
- Fixed colour indices that recur in the 2D screens: 184 = normal menu text, 64 = its outline, 1 = white highlight,
  0 = black, 188 / 118 = box edges, 255 = haze/sky-off fill.
- Palette fades go between the palette and black, one step per vertical retrace (70 Hz). Each 6-bit DAC value is
  `current + step · (target − current) / steps` (integer division). Fade-out uses 16 steps (0.23 s), fade-in 32 steps
  (0.46 s). The program is frozen while a fade runs; keys pressed meanwhile are lost.

## 5. Fonts, sprite sheets and backdrops

- 2D graphics are RLE sprite sheets stored as `NAME.DAT` + `NAME.TAB` pairs (chapter 01).
- **Fonts.** A font is a sprite sheet where character byte c (code page 437) is entry c + 1. Five font slots are loaded
  per screen mode:

  | Slot | 320×200 | 640×480 | Use |
  |---|---|---|---|
  | 0 | MFONT0-0 | HFONT0-0 | small text, debug overlay, "Recording FLIC" |
  | 1 | MFONT0-0 | MFONT0-0 | small text |
  | 2 | OLFNT0-0 | OLFNT0-1 | large outlined font: menus, titles, in-race banners |
  | 3 | OSFNT0-0 | OSFNT0-1 | small outlined font: panels, tables, statistics |
  | 4 | PFONT0-0 | PFONT0-1 | big message font ("FINAL LAP", "KILLED BY ...") |

  Glyphs of OLFNT/OSFNT are two-colour masks (pixel 1 = text colour, 0 = optional outline/background colour). The large
  menu font's lower-case letters are not letters but symbols: `a` empty bar segment, `_` filled bar segment, `b` hollow
  cursor block, `c` hollow box (off), `d` filled box (on).
- **PANEL sheet** (`PANEL0-0` / `PANEL0-1`, 310 entries each, the 640×480 pieces at about double size): every HUD
  piece (gauges, lap digits 151..161, banner caps 198..200, cracked-glass static 230, start lights 281..286, target
  marker 258..267) and the red corners of the front-end text box (300..303). The HUD chapter lists the indices.
- Other sheets: `HSPR0-0`/`MSPR0-0` (24×24 / 12×12 icons), 
  `TRACK0-0`/`TRACK0-1` (minimaps), `POINT0-0` (map markers and mouse pointer).
- **Backdrops** are raw full-screen images: `OSCR0-0`/`OSCR0-1` (front end), `ONET0-0`/`ONET0-1` (starting-grid picture
  shown while a level loads), `TITLE.DAT` (320×200 boot splash only).

## 6. Data directory layout

The original addresses files with lower-case DOS paths relative to the game directory; on disk they are upper case. A
recreation should match names case-insensitively.

| Directory | Content |
|---|---|
| `DATA/` | palette, sprite sheets and fonts, backdrops, terrain textures (`TEXTU0-N`), skies (`SKY0-N`), colour tables (`TABLE0-N`), billboard images (`TMAPS`), `PUZZLE.DAT`, `SRCH0-0.DAT`, `INTRO.DAT`, `TITLE.DAT` |
| `MAPS/` | `LEVEL0-1.DAT` .. `LEVEL0-6.DAT`, one per track |
| `OBJECTS/DATA/` | 62 3D models (`*.DAT`, "BULLFROG OBJECT DATA") and the model texture atlases `TEX0-0`/`TEX0-1` |
| `SOUND/` | `SOUND.DAT` (samples), `MUSIC.DAT` (XMIDI tunes), `SAMPLE.OPL`/`SAMPLE.AD` (FM timbres), Miles sound drivers |
| `MOVIE/`, `EDITOR/` | empty in the shipped game |

Track N (1..6) uses `MAPS/LEVEL0-N.DAT` and the per-track sets `DATA/TEXTU0-(N−1)`,
`DATA/SKY0-(N−1)`, `DATA/TABLE0-(N−1)`; a track number 7 would use set 0 (no such track is reachable).

## 7. Languages

Five complete UI string tables of 194 strings each are built into the executable (chapter 02 appendix lists them):

| Number | Language | Selected by |
|---|---|---|
| 0 | English | first-run language menu, Alt+1 |
| 1 | German (DEUTSCH) | menu, Alt+2 |
| 2 | French (FRANCAIS) | menu, Alt+3 |
| 3 | Spanish (ESPAÑOL) | menu, Alt+4 |
| 4 | Italian (ITALIANO) | menu only |

Track, team, craft and computer-driver names are not translated. Texts are code page 437; the accented capitals used
are Ä, Ö, Ü and Ñ.

## 8. Program flow (summary)

```
start game [options]
 ├─ parse command line (-S / -s: no sound and music; three arguments starting with L, P or Y: cheat flag)
 ├─ run game (with -s when sound is off): the intro movie, any key skips   (chapter 10)
 ├─ initialise; show TITLE.DAT (320×200 boot splash) while loading; load config.dat
 ├─ fade the title out (16 steps)
 └─ outer loop until "quit":
      front end (chapter 02), entered with a 32-step fade-in:
        no config.dat  → language menu → name entry → main menu
        empty name     → name entry → main menu
        otherwise      → main menu   (after a race: results screens first)
        main menu idle 15 s → demo race / high-score table, alternating
        RACE → CHAMPIONSHIP or SINGLE RACE → track carousel → craft carousel
             → fade out, starting-grid picture, fade in, "LOADING LEVEL"
      race: countdown (3 lights), race until 100 ticks after the last craft finished, or Escape
      after the race: STATISTICS, then in a championship POINTS and CHAMPIONSHIP TABLE
 QUIT TO DOS / Shift+Q (front end) / right Shift+Q (race): fade out, save config.dat, exit
```

Low-memory mode: when less than 2,100,000 bytes of memory are free at start, the game forces every detail option off,
stays in 320×200, uses the small model texture atlas `TEX0-1`, hides DETAIL OPTIONS and disables the sliding puzzle.

## 9. Port deviations

The Rust recreation differs from the original in these global respects (each chapter lists its own):

- It starts in **640×480** unless `-320` is given; the original always starts in 320×200.
- It runs in one resizable 4:3 window (1280×960 by default) that stretches either mode.
- The race simulation runs at the fixed 20 Hz tick and the picture is interpolated between ticks on a 70 Hz display;
  the original drew exactly one frame per simulation step.
- The front end runs at a fixed 70 frames per second.
- The title splash stays for 3 seconds or until a key (the original shows it only while loading).
- The front end always uses the colour table of track 1 (`table0-0`) for its blends.
- Writable files live in `save/` and `flics/` under the data directory
- `--no-intro` skips the intro; key rebinding and a key file are additions (chapter 03).
