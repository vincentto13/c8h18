# 02 — Front end

Scope: everything between races: the front-end state machine, the shared screen elements (backdrop, logo, morphing text
box, typewriter text, text markup), the complete menu tree with every item and its effect, hidden items, every setting
with its range, default and storage, the language menu, name entry, global hotkeys, the attract mode (demo race and
high-score table), track and craft selection carousels, race start (starting grid, LOADING LEVEL, computer players),
championships, load/save, the results screens (statistics, points, championship table), track records and high
scores, the in-race pause menu, quitting and when the config is saved. All coordinates are virtual (640 wide,
chapter 00) unless marked "320×200 units".

## 1. State machine

The front end is entered once before every race (and once at start). On entry it ends every playing sample and stops
the music; the front end itself is **silent** apart from its effects (no menu music). Entry is preceded by a 32-step
palette fade-in (after a race the fade starts from the race palette, so nothing visible happens).

Entry state:

| Condition | Action | First state |
|---|---|---|
| a race (single or championship) just ended | copy the human player's record back into the config; run the results screens (section 12); then, if the championship continues, open RACE → CHAMPIONSHIP with the cursor on CONTINUE (or NEW), else close all menus with the cursor on RACE | 2 |
| a demo race just ended | clear the demo flag | 2 |
| `config.dat` was missing at start (first run) | | 0 |
| the player name is empty | | 1 |
| otherwise | | 2 |

States (one handler call per frame; the joystick is translated into key codes every frame):

| State | Screen |
|---|---|
| 0 | language menu (first run only) |
| 1 | name entry |
| 2 | main menu and all its submenus, including LOAD / SAVE CHAMPIONSHIP |
| 3 | track selection |
| 4 | craft selection |
| 5 | high-score table |
| 7 | start a demo race (ignored while NETWORK is on) |
| 8 | start a race: starting grid, LOADING LEVEL, level load, leave the front end |
| 9 | quit: set the quit flag, fade out, save the config, leave |

## 2. Shared screen elements

### 2.1 Backdrop and logo

Every front-end screen first copies the backdrop `OSCR0-0` (320×200) or `OSCR0-1` (640×480) to the screen. The menu
screens (language menu, main menu, name entry, load/save lists):

The carousel screens, the high-score table and the results screens draw their box where the logo would be (the
recreation draws no logo there).

### 2.2 The morphing text box

All text sits in one translucent box whose rectangle (in **320×200 units**, doubled in 640×480) changes per screen:

| Screen | Rectangle (x, y, w, h) |
|---|---|
| menus, name entry, load / save lists | (16, 70, 288, 119) |
| track and craft carousels (info panel) | (16, 8, 288, 64) |
| high scores, POINTS, CHAMPIONSHIP TABLE | (16, 16, 288, 168) |
| STATISTICS | (4, 4, 312, 192) |
| in-race pause menu | (16, 39, 288, 121) |

**Animation.** The box keeps its current rectangle C and a frame counter. Each frame the screen requests a target T:

```
if counter == 0:
    if T == C: box is in place (draw the screen's text this frame)
    else: counter = 32
if counter > 0:
    counter -= 1; c = counter
    if c == 0:   C = T; shown = T; play WINOPEN (35)
    elif c < 16: shown = C + (T − C) · (16 − c) / 16          ; grow to the target, per coordinate, integer division
    else:        S = (Tx + Tw/2 − 25, Ty + Th/2 − 12, 50, 24)   ; a 50×24 box at the target's centre
                 shown = C + (S − C) · (32 − c) / 16           ; move/shrink towards S
                 if c == 16: C = shown
    play REFUEL (7) every frame of the morph, restarting the sample on its voice, at 300 % rate
draw the box at `shown`; the text is drawn only while the box is in place
```

The morph takes 32 frames; the text appears on the 33rd. At front-end entry and when the pause menu opens the box is
"parked": a 50×24 box centred above the screen top, (135, −12, 50, 24). To make the same rectangle animate again
(between the track and craft carousels, and between POINTS and CHAMPIONSHIP TABLE) the stored x is decreased by 1.

**Drawing** of rectangle (x, y, w, h): interior pixels are replaced by `blend[184][old pixel]` (blend table of the
colour table, chapter 01); the left and top edge lines in colour 188, the bottom and right edge lines in colour 118;
the PANEL sprites 300..303 at virtual (2x − 8, 2y − 8), (2x + 2w − 48, 2y − 8), (2x − 8, 2y + 2h − 24) and
(2x + 2w − 48, 2y + 2h − 24).

### 2.3 Text markup

Screens build one string with embedded codes and draw it from (0, 0) with an extra advance of **−6 virtual units per
glyph**. A code is `$` + letter, optionally followed by a decimal number and one separator character that is
swallowed.

| Code | Effect |
|---|---|
| `$C n` | text colour (default −1 = 255) |
| `$F n` ... `$E` | highlight: glyphs between use colour n |
| `$O n` | colour for the 0-pixels of mask glyphs (outline/background); values ≥ 256 = not drawn (default 300) |
| `$G n` | blit mode: 0 solid, 1 `blend[old][c]`, 2 `blend[c][old]` |
| `$J n` | justify the following text: 0 left, 1 centred ((640 − width) / 2), 2 right-aligned to 638 |
| `$X n`, `$Y n` | position |
| `$U n` | font slot 0..4 (chapter 00) |
| `$S1`, `$S` | play PRINTRET (34) / PRINTTYP (33) |

Mask glyphs (OLFNT, OSFNT): pixel 1 = text colour (or the highlight colour), pixel 0 = `$O` colour if < 256, else
transparent. In 320×200 the font metrics are doubled at load: the space glyph of `OLFNT0-0` is 18 × 20 virtual units,
a menu row (h + 2) is 22 virtual units = 11 pixels, and a glyph advances `2·width − 6`.

Colours used: 184 normal text with outline 64; 1 (white) highlight with outline 0 (black); 1 with outline 0 for
headers.

### 2.4 Typewriter

Texts appear one character per frame: a screen shows the first n visible characters (codes excluded), and every frame
`n += 1` while `n ≤ length + 3`, playing PRINTTYP (33) for each step. Holding **Space** shows everything at once; in
**640×480 everything is shown at once** too. The counter restarts whenever the screen or the drawn menu changes. The
carousel panels type their header first and then the body. The name-entry title types without sound.

## 3. Menus

### 3.1 Model

A menu is a list of items; each item has a label (a string number or a value text built from the settings), a kind
(submenu, action, adjustable value, back) and flags: hidden, adjustable with Left/Right, "no select sound". Only the
deepest open menu is drawn; menus show their items only (no title line).

**Navigation** (one key per frame, only while the box is in place):

| Key | Effect | Sound |
|---|---|---|
| Up / Down | move the cursor to the previous / next visible item, wrapping | CHANGE (20) |
| Enter | activate the item (below) | SELECT (21), except the MUSIC VOLUME item |
| Left / Right | on adjustable items: activate with the direction (other items: nothing) | SELECT (21), except MUSIC VOLUME |
| Escape | close the open submenu (nothing on the root) | CHANGE (20) |

Activation: a **submenu** item opens its menu with the cursor on the first visible item; an **action** item glows for
3 frames (drawn with outline colour 1) and runs its action; a **back** item closes the open menu; a value item runs
its handler with the key (Enter, Left or Right).

**Drawing** in the box rows y0 = 140, height = 238 (the pause menu: y0 = 78, height = 242), font slot 2, line height
`lh = height of the space glyph + 2`, n visible items:

```
row i:  y = y0 + (height − lh·n − 2) / 2 + lh·i, centred
cursor item:      "$Y<y> $J1 $O0 $F1 <label>$E$O64 "    (white, black outline)
glowing action:   "$Y<y> $J1 $O1 $F1 <label>$E$O64 "
other items:      "$Y<y> $J1 $C184 $O64 <label>"
```

**Value labels** (large-font symbols, chapter 00): on/off items `"<name> d"` (on, filled box) or `"<name> c"` (off,
hollow box); bars of `_` (filled segment) and `a` (empty segment); SESSION NUMBER `"<name> (<n>)"`.

### 3.2 Menu tree (items in order; numbers are string numbers, appendix A)

**MAIN MENU**

| # | Item | Effect |
|---|---|---|
| 1 | RACE (35) | opens RACE |
| 2 | OPTIONS (48) | opens OPTIONS |
| 3 | QUIT TO DOS (49) | state 9 (section 15) |

**RACE**

| # | Item | Effect |
|---|---|---|
| 1 | CHAMPIONSHIP (47) | sets championship mode; opens CHAMPIONSHIP with the cursor on CONTINUE if the race number ≠ 0, else on NEW. **Hidden while NETWORK is on.** |
| 2 | SINGLE RACE (45) | sets single-race mode, resets the track carousel, state 3 |
| 3 | MAIN MENU (51) | back |

**CHAMPIONSHIP** (no back item; Escape closes)

| # | Item | Effect |
|---|---|---|
| 1 | CONTINUE CHAMPIONSHIP (147) | state 3 with the current race number. **Hidden while the race number is 0.** |
| 2 | NEW CHAMPIONSHIP (186) | race number = 0, all 8 championship points = 0, state 3 |
| 3 | LOAD CHAMPIONSHIP (187) | load list (section 11) |
| 4 | SAVE CHAMPIONSHIP (188) | save list (section 11) |
| 5 | QUIT CHAMPIONSHIP (148) | leave championship mode, enter all 8 drivers into the high-score table with their current points (section 12.5), back to the main menu |

**OPTIONS**

| # | Item | Effect |
|---|---|---|
| 1 | CHANGE NAME (50) | state 1 |
| 2 | DETAIL OPTIONS (72) | opens DETAIL OPTIONS. **Hidden in low-memory mode.** |
| 3 | NETWORK OPTIONS (89) | opens NETWORK OPTIONS. **Hidden when the network is not available.** |
| 4 | SOUND OPTIONS (93) | opens SOUND OPTIONS. **Hidden when both sound and music are off.** |
| 5 | REINITIALISE JOYSTICK (170) | recalibrates the joystick (details not documented) |
| 6 | COMPUTER PLAYERS d/c (84) | toggle (Enter, Left or Right) |
| 7 | DIFFICULTY LEVEL + 3-segment bar (85) | Left −1, Right +1, clamped 0..3; Enter does nothing |
| 8 | MAIN MENU (51) | back |

**DETAIL OPTIONS**

| # | Item | Effect |
|---|---|---|
| 1 | SHADING d/c (180) | toggle |
| 2 | SKY d/c (181) | toggle |
| 3 | TEXTURE MAPPING + 5-segment bar (182) | Left: level − 1, below 0 turns mapping off; Right: turns mapping on if off, else level + 1 up to 4; Enter nothing; every Left/Right also sets the near distance = min(level, 3). Bar: level + 1 filled segments when on, 5 empty when off |
| 4 | SCREEN RESOLUTION (81) | opens SCREEN RESOLUTION |
| 5 | MAIN OPTIONS (56) | back |

**SCREEN RESOLUTION** (no back item)

| # | Item | Effect |
|---|---|---|
| 1 | 320 X 200 256 COLOURS (82) | switch to 320×200 |
| 2 | 640 X 480 256 COLOURS (83) | switch to 640×480 |

A switch does nothing if the mode is unchanged; otherwise: fade out (16 steps), free the old mode's sprite sets, load
the new ones, set the mode and the palette at once, reload the front-end backdrop of the new mode, select font slot 2.
The mode is stored in the human player's record (config 0x4c) but every start begins in 320×200.

**NETWORK OPTIONS**

| # | Item | Effect |
|---|---|---|
| 1 | NETWORK d/c (183) | toggle network mode (not saved; chapter 11) |
| 2 | SESSION NUMBER (n) (86) | Left −1, Enter or Right +1, wrapping 0..9 |
| 3 | MAIN OPTIONS (56) | back |

**SOUND OPTIONS**

| # | Item | Effect |
|---|---|---|
| 1 | MUSIC VOLUME + 16-segment bar (53) | Left lowers by 0x800 (if above 0), Right raises by 0x800 (if below 0x8000); Enter nothing; no select sound (the handler plays its own sample). **Hidden without music.** |
| 2 | EFFECTS VOLUME + 16-segment bar (54) | the same for the effects volume. **Hidden without sound.** |
| 3 | MAIN OPTIONS (56) | back |

Volume bars show `volume / 0x7e8` filled segments (16 at 0x8000 and 0x7e80, 15 at 0x7800).

Strings present in the tables but used by no menu of the shipped game: CLONE RACE (46), DEATH MATCH (33), CLEAR
HISCORES (55, 190), DO NOT / DON'T CLEAR HISCORES (145, 171), CONTINUE GAME (59), ABORT GAME (60), ARE YOU SURE (61),
YES (62), NO (63), the colour and team names (64..71, 129..136), SPLITTING LEVEL (77), SCREEN SIZE (80), NUMBER OF
CHANNELS (90), STEREO ON/OFF/STEREO (91, 92, 184), REINITIALISE DIGISTICK (52), RACE TEAM (185) and others.

## 4. Settings

| Setting | Values | Default | Stored |
|---|---|---|---|
| SHADING | on / off | on | config 0x2996 |
| SKY | on / off | on | config 0x299a |
| TEXTURE MAPPING | on / off | on | config 0x29a6 |
| texture detail level | 0..4 | 3 | config 0x29a2 |
| near texture distance | min(level, 3) | 2 | config 0x299e |
| SCREEN RESOLUTION | 320×200 / 640×480 | 320×200 at every start | config 0x4c (not applied at start) |
| MUSIC VOLUME | 0..0x8000, step 0x800 | 0x7800 | config 0x097c |
| EFFECTS VOLUME | 0..0x8000, step 0x800 | 0x7800 | config 0x0980 |
| NETWORK | on / off | off at every start | not saved |
| SESSION NUMBER | 0..9 | 0 | config 0x094d |
| COMPUTER PLAYERS | on / off | **on at every start** | not saved |
| DIFFICULTY LEVEL | 0..3 | 0 | config 0x0977 |
| language | 0..4 | chosen on first run | config 0x2c7a |
| player name | up to 8 characters | empty | config 0x094f |
| team colour / craft type | 0..7 / 0..5 | 0 / 0 | config 0x094b / 0x094c |
| laps per track (single race) | 1..50 | 11, 8, 9, 8, 9, 5 | track records, config 0x29aa + 74·t + 72 |

Low-memory mode forces SHADING, SKY, TEXTURE MAPPING, the level and the near distance to 0 after loading the config.
The detail settings take effect at the next race. The stereo flag (config 0x0990) has no menu item.

Effects of the race settings (details in the race chapters):

- **COMPUTER PLAYERS** off: the human races alone (no computer crafts); on: 7 computer players.
- **DIFFICULTY LEVEL** d: the computer players' skill/firing level is d in single races and **min(race number, d)**
  in a championship (so the first championship race is always at level 0); difficulty 0 means the computers do not
  shoot. The driver assessment also depends on it.

## 5. Language menu (state 0)

Shown only on the first run (no `config.dat`). Items, in this order (the language numbers differ from the order):

| Item | Language number |
|---|---|
| ENGLISH | 0 |
| DEUTSCH | 1 |
| FRANCAIS | 2 |
| ITALIANO | 4 |
| ESPAÑOL | 3 |

Choosing one sets the language and goes to the name entry (state 1). The menu is drawn like any menu (box (16, 70,
288, 119), logo).

## 6. Name entry (state 1)

Reached on the first run after the language, when the name is empty, and by OPTIONS → CHANGE NAME.

- Title "ENTER NAME" (34) at `$Y(4h + 140) $J1 $C184 $O64` (h = height of the space glyph of font 2), typewriter
  without sound once the box is in place.
- The field shows the name centred at y = 6h + 140 as `"$C184 $O64 ... <name><cursor>"`; the cursor glyph `b`
  (hollow block) blinks 32 frames on / 32 frames off. The field has its own typewriter counter, reset on every change,
  and each accepted key plays PRINTTYP (33).
- Editing happens in place in the config's name field. The typed length starts at 0, so **the first key replaces
  the old name**. Keys: printable characters (scan codes translated through a UK keyboard layout table, with a
  second table for Shift) are converted to upper case and accepted when in 0x20..0x7e; at the maximum length (8)
  the last character is replaced instead of appending; Backspace deletes the last character; **Enter or Escape**
  leave to the main menu (state 2). Leaving without typing keeps the old name.

## 7. Global hotkeys

On the menu screens, the carousels and the results screens:

| Keys | Effect |
|---|---|
| Shift + Q (either Shift) | save the config, fade out, quit to DOS |
| Alt + 1 / 2 / 3 / 4 | language English / German / French / Spanish; all item texts are rebuilt (Italian only from the language menu) |
| Alt + V | toggle the debug overlay flag in the player record (the race then shows the developer information page: product, version "BETA V3.0", date "17:45:48 Jun 10 1995", game turn, voices, timer, memory figures) |
| Alt + C | nothing (the screen recorder's start is not connected in the shipped game) |

## 8. Attract mode

On the main menu, while **no submenu is open**, an idle timer counts DOS time in whole seconds and is reset by every
handled menu key. When more than **15 seconds** have passed, a flag (initially off) is flipped: on → state 7 (demo
race), off → state 5 (high-score table). Demo and table therefore alternate, demo first.

**High-score table (state 5).** Box (16, 16, 288, 168); font 3, colour 184, outline 64. Title "HISCORES" (38) centred
at y = h + 32; rows 0..18 of the table at y = (i + 3)·h + 32: the name at x = w + 32, the score centred, the
assessment text (string 149 + assessment) right-aligned to end at x = 608 − w (h, w = space glyph of font 3).
Typewriter with sound. Any key, or 15 seconds, returns to the main menu.

**Demo race (state 7 → 8).** Not started while NETWORK is on. Track = race number + 1 (the championship race number,
which is also the last selected track). All 8 crafts are computer players named, by slot 0..7, SCOPER, BARNSY,
SHUNTLY, COPSE, MANNY, MCLALIN, MAD, ATROW, with team colour (slot + player's team) mod 8 and craft type (slot +
player's craft) mod 6, racing **20 laps**. The same starting sequence as a race is shown (section 10). During the
demo the view is the trackside level camera looking at computer player `(race frame / 200) mod 7 + 1`, without HUD
or messages. The demo ends on **any key except R** (R toggles 320×200 / 640×480) **and C** (ignored), at **race frame
3000** (150 s), or by the normal race end. Afterwards the front end returns to the main menu without results screens.

## 9. Race selection

### 9.1 Static tables

Tracks (not translated):

| Track | Name | Championship laps / default single-race laps |
|---|---|---|
| 1 | 1. AMAZON DELTA TURNPIKE | 11 |
| 2 | 2. TRANS-ASIA INTERSTATE | 8 |
| 3 | 3. SHANGHAI DRAGON | 9 |
| 4 | 4. NEW CHERNOBYL CENTRAL | 8 |
| 5 | 5. SLAM CANYON | 9 |
| 6 | 6. THRAK CITY | 5 |

Teams (by colour): 0 SALT SAINTS, 1 NIGHT SHIFT, 2 RUST HOUNDS, 3 SULPHUR, 4 COLD IRON, 5 FLARE CREW, 6 STATIC, 7 OCTANE.

Crafts (panel bar lengths):

| Type | Name | Model | Speed | Weight | Armour | Firepower |
|---|---|---|---|---|---|---|
| 0 | DART | CAR0-c | 6 | 5 | 4 | 5 |
| 1 | BULWARK | TANK0-c | 3 | 6 | 5 | 6 |
| 2 | MAULER | JUGGA0-c | 4 | 6 | 5 | 5 |
| 3 | KESTREL | JET0-c | 6 | 6 | 4 | 4 |
| 4 | SCYTHE | BIKE0-c | 8 | 4 | 3 | 5 |
| 5 | SKIMMER | SKIM0-c | 8 | 4 | 4 | 4 |

(c = team colour 0..7.) Computer driver names by slot: SCOPER, BARNSY, SHUNTLY, COPSE, MANNY, MCLALIN, MAD, ATROW.

### 9.2 Carousel geometry

Both selection screens show a ring of six 3D models below the info box, rendered with an identity view, focal length
240 (480 in 640×480), centred on the screen, in model units (2048 per cell). Item i (0..5) with the selected item s
sits at angle `a = (i − s)·341` (2048 per turn; 341 = 2048/6) plus a turn offset, at

```
x = sin(a) · 1856          (sin/cos as fractions of 1.0)
y = 350 − cos(a) · 716
z = zbase − cos(a) · 3336  zbase = 5995 for tracks, 6995 for crafts
```

so the selected item (a = 0) is in front (y = −366, z = zbase − 3336). Each model is rotated by
`M = Rx(tilt) · Ry(spin)` applied to the vertex (x, z, −y), tilt = −155 (2048 per turn, leaning its top towards the
viewer). Every frame each item's spin decreases: by 16 + j for track item j, by 16 for every craft.

**Turning**: Left or Right starts a 24-frame turn (CHANGE sound); keys are ignored during a turn. With the counter
c running from 24 to 0 (c negative for Right, positive for Left), the offset added to every angle is
`± 341 · (25 − |c|) / 24` (negative for Right). When the counter reaches 0 the selection changes (Right: next item,
Left: previous, wrapping over 6) and the offset is 0 again; the panel's typewriter restarts.

### 9.3 Track selection (state 3)

**Single race.** Ring of the six track models (models 50..55); the selected track is the championship race number
word (config 0x2c76), so a single race on track N also makes CONTINUE CHAMPIONSHIP start at race N.

| Key | Effect |
|---|---|
| Left / Right | turn the ring to the previous / next track |
| Up / Down | laps of the selected track +1 / −1, wrapping 1..50 (CHANGE) |
| Enter | SELECT; go to craft selection |
| Escape | CHANGE; back to the main menu (championship cursor fixed as at entry) |

**Championship.** Only the current race's track model, at (0, −80, 2140), spin −8 per frame; Left, Right, Up, Down do
nothing. Enter: craft selection if the race number is 0, else **start the race directly** (the craft chosen for the
first race is kept). Escape: main menu.

**Info panel** in box (16, 8, 288, 64), h2/h3 = space-glyph heights of fonts 2/3:

```
"$C1 $O0 $Y<h2+16> $J1 <track name>"                     font 2, white
font 3, colour 184, outline 64, centred, starting at y = h2 + 16 + 3·h3 − 4, rows h3 + 2 apart:
"NUMBER OF LAPS <laps>"              (95)  championship: the fixed table; single race: the track's laps setting
"BEST LAP <ticks> <holder>"          (40)  raw tick counts, no conversion to seconds
"BEST RACE <ticks> <holder>"         (41)
```

### 9.4 Craft selection (state 4)

On entry the box is re-animated and the six craft models (current team colour) start with staggered spins
(item j at (j + 1)·341).

| Key | Effect |
|---|---|
| Left / Right | turn the ring: previous / next craft type (changes when the turn ends) |
| Up / Down | team colour +1 / −1, wrapping 0..7 (CHANGE); the models change colour at once |
| Enter | SELECT; re-animate the box; start the race (state 8) |
| Escape | CHANGE; re-animate the box; back to track selection |

Panel in box (16, 8, 288, 64):

```
"$C1 $O0 $Y<h2+16> $J1 <craft name>"                           font 2, white
"$C1 $O0 $Y<h2+16 + h2+2> $J1 <team name>"                      white
font 3, colour 184, outline 64, centred, 2·(h3+2) further down:
"SPEED <speed × '_'>    WEIGHT <weight × '_'>"                  (98, 96)
next row (h3 + 2 lower): "ARMOUR <armour × '_'>    FIREPOWER <firepower × '_'>"   (97, 99)
```

## 10. Starting a race (state 8)

1. Fade out (16 steps).
2. Load the starting-grid picture `ONET0-0` / `ONET0-1` into the backdrop buffer, show it, fade in (32 steps).
3. Draw "LOADING LEVEL" (87) in font 2, centred at virtual y = 200 (240 in 640×480), present.
4. Load the level of track race number + 1 and the track's textures, sky and colour table.
5. Set up the players (not in network mode, chapter 11): the human is slot 0 with the record from the config; if
   COMPUTER PLAYERS is on, slots 1..7 become computer players named by slot (BARNSY .. ATROW) with team colour
   (slot + human team) mod 8 and craft type (slot + human craft) mod 6; each computer player gets the difficulty of
   section 4.
6. Race distance: single race = the track's laps setting; championship = the fixed table (11, 8, 9, 8, 9, 5).

There is no fade at the race start itself; the race begins with the 3-light countdown (race chapters), during which
the race tune starts.

## 11. Championship, LOAD and SAVE

**Flow.** RACE → CHAMPIONSHIP → NEW (race 0, points cleared) or CONTINUE → track selection (only the current track) →
craft selection (first race only) → race → results (section 12) → back to the CHAMPIONSHIP menu while the championship
continues. A championship is six races on tracks 1..6 in order; the human must be in the **top three of the
championship table** after each of races 1..5 to advance, otherwise that race's points are taken back from all
players and the same track must be raced again. After the sixth race the championship ends.

**LOAD / SAVE lists** (modal, in the menu box). The slot names are read from the first 32 bytes of `save00.dat` ..
`save04.dat`; a missing file shows EMPTY (177).

```
header "LOAD GAME" (175) / "SAVE GAME" (176):  "$C184 $O64 $Y<h+142> $J1 ..."
rows i = 0..4: slot names at y = (h + 2)·(i + 3) + 140, centred
row 5: "MAIN OPTIONS" (56) at y = (h + 2)·9 + 140
selected row: highlighted with $F1
```

Up/Down move over the 6 rows with wrap (CHANGE). The cursor starts on the last used slot (config 0x2c7c).

- **LOAD**: Enter on a non-EMPTY slot: SELECT; the whole config block is replaced by the file (settings, language,
  name, points, race number, records, high scores: everything); last slot = this slot; CONTINUE is shown again if
  the race number ≠ 0; the championship continues **at once** with track selection (state 3). Enter on an EMPTY slot
  does nothing. Enter on MAIN OPTIONS or Escape: back to the menu.
- **SAVE**: Enter on a slot (SELECT) edits its name in place (max 12 characters; an EMPTY slot starts empty; same
  editing rules as section 6, with the cursor glyph); Enter stores the name at config offset 0 (or "C8H18" when
  empty), sets the last slot and writes the whole block to `save%02d.dat`, then returns to the menu; Escape cancels
  the edit, a second Escape leaves.

## 12. After a race

### 12.1 Classification

When the race ends after the human finished, every craft still racing is classified by its current place with a
made-up finish time (race chapters). When the human ended the race with Escape before finishing, nobody is classified.
Race points by finish place: **20, 16, 13, 11, 9, 7, 6, 5**.

### 12.2 Sequence

```
demo race: nothing, back to the main menu
human classified:
    add every driver's race points to the championship totals   (also after a single race: original behaviour)
    show STATISTICS
    if championship:
        update the track records
        show POINTS, then CHAMPIONSHIP TABLE   (the box re-animates between the two)
championship progression:
    human not classified           → retry the same race (nothing was added)
    race number < 5 and classified → human in the top 3 of the totals: race number + 1
                                     else: subtract this race's points again (retry)
    race number 5 and classified   → championship over: race number = 0, leave championship mode,
                                     enter all 8 drivers into the high-score table
then: championship continues → CHAMPIONSHIP menu (cursor on CONTINUE, or NEW); otherwise all menus closed
```

Championship place of the human: start at 8 and subtract 1 for every other driver with fewer points, or with equal
points and a higher slot index (ties go to the lower index). Places 1..3 advance. Each results screen waits for Enter
or Escape.

### 12.3 STATISTICS

Box (4, 4, 312, 192). h2/h3 = space heights of fonts 2/3, widths measured in font 3.

```
"$U2 $C1 $O0 $Y24 $J1 STATISTICS"                       (137), font 2 white
names (font 3, white, outline 0), unclassified drivers' names replaced by " ":
  row A at y1 = 8 + h2 + h3 + h3 + 2: driver 0 left-aligned at x = 10; drivers 2, 4, 6 centred on x = 203, 359, 515
  row B at y2 = y1 + h3 + 2:          drivers 1, 3, 5 centred on x = 125, 281, 437; driver 7 right-aligned to 628
8 statistic rows from r0 = y2 + h3 + 2, each 2·(h3 + 2) apart:
  heading (colour 184, outline 64, centred):  HIT ACCURACY (111), KILLS (114), DEATHS (113), AVERAGE LAP (94),
                                              BEST LAP (40), RACE TIME (104), RATING (112), POSITION (128)
  values h3 + 2 below the heading, white: 8 columns of 78 units from x = 8, each value centred in its column:
     "<n>%", kills, deaths, average lap ticks, best lap ticks, race time ticks, "<20 − assessment>/20", place
  unclassified drivers' columns stay blank
finally the human's assessment text in font 2, white, centred at y = r0 + 7·2·(h3 + 2) + 3·(h3 + 2):
  string 149 + assessment (0..19), or CHEATING SUCKS (169) for 20
```

The statistics' race time is the finish time, the average lap is finish time / laps, the best lap may include laps
driven after the finish. Assessment and hit accuracy are defined in the race chapters.

Assessment texts (index → string): 0 IMMORTAL, 1 HARDWIRED, 2 OUTTA CONTROL, 3 DEMON, 4 DEADLY, 5 WRECKER,
6 DANGEROUS, 7 TAILGATER, 8 ANTISOCIAL, 9 DUST DEVIL, 10 UPWARDLY MOBILE, 11 MEDIOCRE, 12 TOO SLOW, 13 ENDANGERED
SPECIES, 14 TARGET PRACTICE, 15 BACK MARKER, 16 CANYON KISSER, 17 VICTIM, 18 SCRAP, 19 SMEAR, 20 CHEATING SUCKS.

### 12.4 POINTS and CHAMPIONSHIP TABLE

Box (16, 16, 288, 168), font 2. POINTS shows this race's points, CHAMPIONSHIP TABLE the totals (string 106 / 139).

```
title: "$C184 $O64 $Y<h·4/3 + 32> $J1 <title>"
rows sorted by value, descending, ties by lower slot first; ranks 0..2 white ($C1 $O0), the rest $C184 $O64
rank r at y = 32 + (r + 3)·h + (r + 1)·(h / 2): name at x = 32 + 2w, value at x = 608 − 5w
```

### 12.5 Track records and high scores

**Track records** (championship races only): over the classified drivers, the smallest best-lap time below 9999 and
the smallest race time below `championship laps × 9999` (the championship table's laps of that track) are found; each
replaces the stored record if it is smaller, together with the driver's name. Fresh configs start with 9999 /
"C8H18".

**High-score entry** (end of a championship and QUIT CHAMPIONSHIP): each of the 8 drivers in slot order is inserted
with score = championship points, assessment = the driver's last assessment, name = driver name: the entry goes above
the first row whose score is lower, or equal with a lower assessment value; the rows below move down one place and
row 49 is lost. No name is typed.

## 13. In-race pause menu

P (scan code 0x19) toggles the pause, not in demo races and not while spectating. Entering the pause: the box is parked
above the screen, all submenus closed, the cursor on SOUND OPTIONS, the typewriter restarted, and every playing sample
is ended (the music keeps playing). While paused nothing in the race moves (no thing processing, no engine sounds,
the race frame counter stops), the view switches to the trackside level camera on computer player
`(race frame / 200) mod 7 + 1` (fixed, since the counter stands still) without HUD or messages, and the menu runs in
box (16, 39, 288, 121) with rows y0 = 78, height = 242:

| # | Item | Opens |
|---|---|---|
| 1 | SOUND OPTIONS (93) | MUSIC VOLUME, EFFECTS VOLUME, MAIN OPTIONS (the front-end submenu) |
| 2 | DETAIL OPTIONS (72) | SHADING, SKY, TEXTURE MAPPING, SCREEN RESOLUTION, MAIN OPTIONS |

Arrows and Enter navigate as in section 3. **P or Escape continue the race** (Escape, which normally ends the race,
is turned into "continue" while paused).

## 14. Palette fades

| When | Fade |
|---|---|
| title splash → front end | out 16 steps, then the front end fades in 32 |
| front-end entry | in 32 |
| race start (state 8) | out 16, starting-grid picture, in 32 |
| screen-mode switch | out 16; the new mode's palette is set at once |
| quit (QUIT TO DOS, Shift+Q) | out 16 |
| network join failure | out, backdrop reloaded, in |

One step per 70 Hz vertical retrace; the program does nothing else meanwhile (chapter 00).

## 15. Quitting and saving the config

- **QUIT TO DOS** (state 9) and **Shift+Q** in the front end: set the quit flag, fade out, save `config.dat`, end the
  program; the shutdown writes the config once more.
- **Right Shift + Q** in a race: sets the quit flag and ends the race; no results screens are shown and the program
  shuts down, saving the config.
- The config is **only written when quitting** (plus SAVE CHAMPIONSHIP, which writes a slot file). Changes made in the
  menus, finished championship races, records and high scores are lost if the program is not quit normally.
- Saving clears the cheat flag (config byte 0x22 bit 2) first.
- LOAD CHAMPIONSHIP replaces the whole config block, including all settings.

## 16. Front-end sounds

All from SOUND.DAT bank 0, played at the effects volume, centred:

| Sample | Name | When |
|---|---|---|
| 7 | REFUEL | every frame while the box morphs (restarted on its voice each frame, 300 % rate) |
| 35 | WINOPEN | the box reaches its place |
| 33 | PRINTTYP | each typewriter character; each key in a text field; `$S` |
| 34 | PRINTRET | `$S1` in a text |
| 20 | CHANGE | cursor moves, Escape, carousel turns, laps and colour changes |
| 21 | SELECT | Enter on items, activation of adjustable items (not MUSIC VOLUME), carousel Enter, LOAD/SAVE Enter |

## 17. Port deviations

- The front end runs at a fixed 70 frames per second; the original is unthrottled (frame counts are the same).
- The typewriter also runs in 640×480 (the original shows texts at once there).
- The title splash waits 3 s or a key; the original shows it only while loading and then fades out.
- Menu keys: Space also selects (as Enter), besides holding it to skip the typewriter.
- DETAIL OPTIONS is never hidden (the port has no low-memory mode); NETWORK OPTIONS is always hidden (no network).
- Choosing a SCREEN RESOLUTION item closes that submenu.
- Track records of 0 count as "no record" (the port's fresh config starts the records at 0, see chapter 01).
- Port addition, partial track sets: a track is **available** when its level `MAPS/LEVEL0-t.DAT` and its
  `DATA/TEXTU0-(t-1).DAT`, `DATA/SKY0-(t-1).DAT` and `DATA/TABLE0-(t-1).DAT` are all found (override folder first,
  then the data folder). Its preview model (model 50 + t − 1) and its minimap pictures are optional (missing models are
  drawn as nothing). With all six tracks available everything below is the original behaviour. Otherwise:
  - The race number stays a track number (0..5) but always names an available track: any other value (fresh config,
    loaded slot, config from a full set) is replaced by the next available track after it, or else the first one.
  - Single race: the carousel holds only the available tracks, spaced 2048 / n apart (341 for six); with one track
    Left and Right do nothing. Up / Down still change that track's laps.
  - Championship: the races are the available tracks in track order. "Race number 0" in this chapter reads as "the
    first available track": NEW sets it, CONTINUE is hidden while it is set, craft selection happens on that race
    only, and the championship menu's cursor rule uses it. Progression (§12) goes to the next available track
    instead of race number + 1; after the last available track the championship ends (race number = the first
    available track). The computer players' championship level is min(k, d) for the k-th race (k = 0 for the first),
    which is the race number with all six tracks. A single available track makes a championship of one race.
  - Demo race: on the race number's track, hence always an available one. With no track available at all, RACE →
    track selection does nothing and the idle timeout shows only the high-score table.
- Port addition: OPTIONS → CONTROLS (after DIFFICULTY LEVEL, before MAIN MENU): items ACCELERATE, BRAKE, LEFT, RIGHT,
  MINIGUN, MISSILE, BOOSTER, each shown as `"<ACTION>: <keys>"`, a submenu PAUSE AND ZOOM (PAUSE, ZOOM IN, ZOOM OUT and
  a back item reading CONTROLS), RESET TO DEFAULTS and MAIN OPTIONS. Enter on a binding shows `"<ACTION>: PRESS A
  KEY"`; the next key becomes the action's only key (removed from any other action), Escape cancels. Labels exist in
  all five languages (e.g. German STEUERUNG / GAS / BREMSE / LINKS / RECHTS / KNARRE / RAKETEN / BOOSTER / PAUSE /
  ZOOM NAH / ZOOM FERN). See chapter 03.

## Appendix A — UI string table

The 194 strings in all five languages, by string number (code page 437 in the executable; the accented letters are
Ä, Ö, Ü and Ñ). Strings 0..34 are race messages, 35..93 menus, 94..146 results, 147..193 championship, assessments
and more.

| # | English | German | French | Spanish | Italian |
|---|---|---|---|---|---|
| 0 | NULL | NULL | NUL | NULA | NIENTE |
| 1 | DEAD MEAT | DAS WAR'S DANN WOHL | VIANDE FROIDE | DATE POR MUERTO! | SPACCIATO |
| 2 | FUEL LOW | SPRIT WIRD KNAPP | ALARME CARBURANT | GASOLINA ESCASA | SCARSA BENZINA |
| 3 | SHIELD LOW | SCHILD IST NIEDRIG | ALARME BLINDAGE | ESCUDO DEBIL | SCARSO SCUDO |
| 4 | AMMO LOW | MUNI WIRD KNAPP | ALARME MUNITIONS | MUNICION ESCASA | SCARSE MUNIZIONI |
| 5 | FUEL EMPTY | SPRIT IST LEER | PANNE CARBURANT | SIN GASOLINA | SENZA BENZINA |
| 6 | SHIELD EMPTY | SCHILD IST LEER | BLINDAGE DETRUIT | SIN ESCUDO | SENZA SCUDO |
| 7 | AMMO EMPTY | MUNI IST LEER | MUNITIONS EPUISEES | SIN MUNICION | SENZA MUNIZIONI |
| 8 | FUEL RECHARGING | SPRIT NACHGETANKT | PLEIN CARBURANT | CARGANDO GASOLINA | RICARICA BENZINA |
| 9 | SHIELD RECHARGING | SCHILD AUFGELADEN | ECHANGE BLINDAGE | CARGANDO ESCUDO | RICARICA SCUDO |
| 10 | AMMO RECHARGING | MUNI NEU GELADEN | REAPPRO MUNITIONS | CARGANDO MUNICION | RICARICA MUNIZIONI |
| 11 | FUEL FULL | SPRIT IST VOLL | CARBURANT OK | GASOLINA: REPLETO | SERBATOIO PIENO |
| 12 | SHIELD FULL | SCHILD IST VOLL | BLINDAGE OK | ESCUDO: COMPLETO | SCUDO COMPLETO |
| 13 | AMMO FULL | MUNI IST VOLL | MUNITIONS OK | MUNICION: COMPLETA | ARMI COMPLETE |
| 14 | EXTRA FUEL | EXTRA SPRIT | BONUS CARBURANT | GASOLINA EXTRA | EXTRA CARBURANTE |
| 15 | EXTRA SHIELD | EXTRA SCHILD | BONUS BLINDAGE | ESCUDO EXTRA | EXTRA SCUDO |
| 16 | EXTRA AMMO | EXTRA MUNI | BONUS MUNITIONS | MUNICION EXTRA | EXTRA MUNIZIONI |
| 17 | MAX FUEL | MAX. SPRIT | CARBURANT MAXI | MAXIMA GASOLINA | MASSIMO CARBURANTE |
| 18 | MAX SHIELD | MAX. SCHILD | BLINDAGE MAXI | MAXIMO ESCUDO | MASSIMO SCUDO |
| 19 | MAX AMMO | MAX. MUNI | MUNITIONS MAXI | MAXIMA MUNICION | ARMAMENTO MASSIMO |
| 20 | DOUBLE FUEL | ZWEIFACH-SPRIT | CARBURANT X2 | GASOLINA DOBLE | DOPPIO CARBURANTE |
| 21 | DOUBLE SHIELD | ZWEIFACH-SCHILD | BLINDAGE X2 | ESCUDO DOBLE | DOPPIO SCUDO |
| 22 | DOUBLE AMMO | ZWEIFACH-MUNI | MUNITIONS X2 | MUNICION DOBLE | DOPPIE MUNIZIONI |
| 23 | MINIGUN UPGRADED | BESSERE KNARRE | MITRAILLEUSE SURP. | MEJORA: CAÑON | POTENZIAMENTO ARMA |
| 24 | MISSILE UPGRADED | BESSERE RAKETEN | MISSILES SURP. | MEJORA: MISILES | POTENZIAMENTO RAZZI |
| 25 | BOOSTER UPGRADED | BESSERER BOOSTER | TURBO SURP. | MEJORA: PROPULSOR | INCREMENTO BOOSTER |
| 26 | AUTO PILOT ON | AUTO-PILOT AN | MODE PILOTE AUTO | AUTO-PILOTO ON | AUTO PILOTA ON |
| 27 | AUTO PILOT OFF | AUTO-PILOT AUS | MODE MANUEL | AUTO-PILOTO OFF | AUTO PILOTA OFF |
| 28 | AUTO REPAIR | AUTOM. REPARATUR | REPARATION AUTO | AUTO-REPARACION | AUTORIPARAZIONE |
| 29 | AUTO REFUEL | AUTOM. AUFTANKEN | PLEIN AUTOMATIQUE | AUTO-REPOSTAJE | AUTORICARICA |
| 30 | CALLING REPAIR | RUFE WERKSTATT | REPARATION A FAIRE | ESPERA REPARACION | ATTESA RIPARAZIONE |
| 31 | AWAITING REPAIR | MONTEUR UNTERWEGS | REPARATION REQUISE | AUXILIAR EN PISTA | VEICOLO IN ARRIVO |
| 32 | REPAIRING | MONTEUR ANGEKOMMEN | REPARATION | AUXILIAR AQUI | PRESENZA VEICOLO |
| 33 | DEATH MATCH | FREISTIL | COMBAT A MORT | DUELO A MUERTE | SCONTRO MORTALE |
| 34 | ENTER NAME | NAME EINGEBEN | TAPEZ VOTRE NOM | INTRODUZCA NOMBRE | DIGITA NOME |
| 35 | RACE | RENNEN | COURSE | CARRERA | GARA |
| 36 | SELECT TRACK | BAHN WÄHLEN | CHOIX DE LA PISTE | ESCOGER PISTA | SELEZIONA CIRCUITO |
| 37 | SELECT CAR | WAGEN WÄHLEN | CHOIX DU VEHICULE | ESCOGER COCHE | SELEZIONA AUTO |
| 38 | HISCORES | HIGHSCORES | MEILLEURS SCORES | RECORDS | CLASSIFICA |
| 39 | LAP TIMES | RUNDENZEITEN | TEMPS AU TOUR | TIEMPO VUELTA | TEMPO DEL GIRO |
| 40 | BEST LAP | BESTE RUNDE | MEILLEUR TOUR | MEJOR VUELTA | MIGLIOR GIRO |
| 41 | BEST RACE | BESTES RENNEN | MEILLEURE COURSE | MEJOR CARRERA | MIGLIOR GARA |
| 42 | TRACK NUMBER | BAHN NUMMER | PISTE NUMERO | PISTA NUMERO | CIRCUITO NUMERO |
| 43 | CHAMPIONSHIP POINTS | MEISTERSCHAFTSPUNKTE | POINTS AU CHAMPIONNAT | PUNTUACION CAMPEONATO | PUNTI CAMPIONATO |
| 44 | RACE TEAM POINTS | TEAM-PUNKTE | POINTS PAR EQUIPE | PUNTUACION EQUIPOS | PUNTI COSTRUTTORI |
| 45 | SINGLE RACE | EINZELRENNEN | COURSE UNIQUE | UNA CARRERA | GARA SINGOLA |
| 46 | CLONE RACE | DOPPELGÄNGERRENNEN | COURSE POURSUITE | CARRERA CLONICA | GIRO DI PROVA |
| 47 | CHAMPIONSHIP | MEISTERSCHAFT | CHAMPIONNAT | CAMPEONATO | CAMPIONATO |
| 48 | OPTIONS | OPTIONEN | OPTIONS | OPCIONES | OPZIONI |
| 49 | QUIT TO DOS | ZURÜCK ZU DOS | RETOUR AU DOS | SALIR AL DOS | ESCI AL DOS |
| 50 | CHANGE NAME | NEUER NAME | CHANGER DE NOM | CAMBIAR NOMBRE | CAMBIA NOME |
| 51 | MAIN MENU | HAUPTMENÜ | MENU PRINCIPAL | MENU PRINCIPAL | MENU' PRINCIPALE |
| 52 | REINITIALISE DIGISTICK | JOYSTICK NEU INITIALISIEREN | REINITIALISER JOYSTICK | REINICIALIZAR JOYSTICK | REINIZIALIZZA  DIGISTICK |
| 53 | MUSIC VOLUME | LAUTSTÄRKE MUSIK | MUSIQUE | VOLUMEN MUSICA | VOLUME MUSICA |
| 54 | EFFECTS VOLUME | LAUTSTÄRKE SFX | EFFETS SPECIAUX | VOLUMEN EFECTOS | VOLUME EFFETTI |
| 55 | CLEAR HISCORES | HIGHSCORES LÖSCHEN | EFFACER LES SCORES | BORRAR RECORDS | CANCELLA CLASSIFICA |
| 56 | MAIN OPTIONS | ZURÜCK ZU OPTIONEN | RETOUR | OPCIONES PRINCIPALES | OPZIONI GENERALI |
| 57 | NETWORK ON | NETZWERK EIN | RESEAU OUI | RED ON | RETE ON |
| 58 | NETWORK OFF | NETZWERK AUS | RESEAU NON | RED OFF | RETE OFF |
| 59 | CONTINUE GAME | WEITERSPIELEN | CONTINUER | CONTINUAR PARTIDA | CONTINUA GIOCO |
| 60 | ABORT GAME | ABBRECHEN | ABANDONNER | CANCELAR PARTIDA | ESCI DAL GIOCO |
| 61 | ARE YOU SURE | SIND SIE SICHER? | CONFIRMEZ | ESTAS SEGURO? | SEI SICURO |
| 62 | YES | JA | OUI | SI | SI' |
| 63 | NO | NEIN | NON | NO | NO |
| 64 | WHITE | WEISS | BLANC | BLANCO | BIANCO |
| 65 | BLACK | SCHWARZ | NOIR | NEGRO | NERO |
| 66 | RED | ROT | ROUGE | ROJO | ROSSO |
| 67 | YELLOW | GELB | JAUNE | AMARILLO | GIALLO |
| 68 | BLUE | BLAU | BLEU | AZUL | BLU |
| 69 | ORANGE | ORANGE | ORANGE | NARANJA | ARANCIONE |
| 70 | PURPLE | LILA | VIOLET | PURPURA | VIOLA |
| 71 | AQUA-GREEN | GRÜN | BLEU-VERT | VERDE | ACQUA MARINA |
| 72 | DETAIL OPTIONS | DETAIL-WAHL | OPTIONS GRAPHIQUES | OPCIONES DETALLE | DETTAGLIO OPZIONI |
| 73 | SKY ON | HIMMEL AN | CIEL OUI | CIELO ON | CIELO ON |
| 74 | SKY OFF | HIMMEL AUS | CIEL NON | CIELO OFF | CIELO OFF |
| 75 | SHADING ON | SCHATTIERUNG AN | OMBRAGES OUI | SOMBREADO ON | OMBREGGIATURA ON |
| 76 | SHADING OFF | SCHATTIERUNG AUS | OMBRAGES NON | SOMBREADO OFF | OMBREGGIATURA OFF |
| 77 | SPLITTING LEVEL | DETAIL-LEVEL | DETAIL DES OBJETS | NIVEL DE DETALLE | DIVISIONE LIVELLI |
| 78 | TEXTURE MAPPING ON | TEXTURE-MAPPING AN | TEXTURES OUI | MAPEADO DE TEXTURAS ON | MAPPATURA TEXTURE ON |
| 79 | TEXTURE MAPPING OFF | TEXTURE-MAPPING AUS | TEXTURES NON | MAPEADO DE TEXTURAS OFF | MAPPATURA TEXTURE OFF |
| 80 | SCREEN SIZE | BILDSCHIRMGRÖSSE | TAILLE ECRAN | TAMAÑO DE PANTALLA | DIMENSIONE SCHERMO |
| 81 | SCREEN RESOLUTION | BILDSCHIRMAUFLÖSUNG | RESOLUTION | RESOLUCION PANTALLA | RISOLUZIONE SCHERMO |
| 82 | 320 X 200 256 COLOURS | 320X200 256 FARBEN | 320 X 200 256 COULEURS | 320 X 200 256 COLORES | 320 X 200 256 COLORI |
| 83 | 640 X 480 256 COLOURS | 640X480 256 FARBEN | 640 X 480 256 COULEURS | 640 X 480 256 COLORES | 640 X 480 256 COLORI |
| 84 | COMPUTER PLAYERS | COMPUTERGEGNER | JOUEURS CYBERNETIQUES | JUGADORES ORDENADOR | CONTROLLO COMPUTER |
| 85 | DIFFICULTY LEVEL | SCHWIERIGKEITSGRAD | NIVEAU DE DIFFICULTE | NIVEL DE DIFICULTAD | LIVELLO DIFFICOLTA' |
| 86 | SESSION NUMBER | NETZWERKSPIEL | SESSION NUMERO | SESION NUMERO | NUMERO SESSIONE |
| 87 | LOADING LEVEL | LEVEL WIRD GELADEN | NIVEAU EN CHARGEMENT | CARGANDO NIVEL | CARICA LIVELLO |
| 88 | ATTEMPTING CONNECTION | VERSUCHE VERBINDUNG | ESSAI DE CONNEXION | INTENTANDO LA CONEXION | ATTESA CONNESSIONE |
| 89 | NETWORK OPTIONS | NETZWERK-OPTIONEN | OPTIONS DE RESEAU | OPCIONES DE RED | OPZIONI DI RETE |
| 90 | NUMBER OF CHANNELS | ZAHL DER KANÄLE | NOMBRE DE VOIES | NUMERO DE CANALES | NUMERO DI CANALI |
| 91 | STEREO ON | STEREO AN | STEREO OUI | ESTEREO ON | STEREO ON |
| 92 | STEREO OFF | STEREO AUS | STEREO NON | ESTEREO OFF | STEREO OFF |
| 93 | SOUND OPTIONS | SOUND-OPTIONEN | OPTIONS SONORES | OPCIONES DE SONIDO | OPZIONI SUONO |
| 94 | AVERAGE LAP | DURCHSCHNITTLICHE RUNDE | CHRONO AU TOUR | VUELTA MEDIA | MEDIA GIRO |
| 95 | NUMBER OF LAPS | ZAHL DER RUNDEN | NOMBRE DE TOURS | NUMERO DE VUELTAS | NUMERO DI GIRI |
| 96 | WEIGHT | GEWICHT | POIDS | PESO | PESO |
| 97 | ARMOUR | PANZERUNG | BLINDAGE | BLINDAJE | ARMATURA |
| 98 | SPEED | GESCHWINDIGKEIT | VITESSE | VELOCIDAD | VELOCITA' |
| 99 | FIREPOWER | FEUERKRAFT | PUISSANCE DE FEU | POTENCIA DE FUEGO | POTENZA DI FUOCO |
| 100 | NAME | NAME | NOM | NOMBRE | NOME |
| 101 | OVERALL TIME | GESAMTZEIT | TEMPS TOTAL | TIEMPO TOTAL | TEMPO TOTALE |
| 102 | TIME | ZEIT | TEMPS | TIEMPO | TEMPO |
| 103 | LAP TIME | RUNDENZEIT | TEMPS AU TOUR | TIEMPO VUELTA | TEMPO DEL GIRO |
| 104 | RACE TIME | GESAMTZEIT | TEMPS DE COURSE | TIEMPO CARRERA | TEMPO GARA |
| 105 | VEHICLE | WAGEN | VEHICULE | VEHICULO | VEICOLO |
| 106 | POINTS | PUNKTE | POINTS | PUNTOS | PUNTI |
| 107 | NEW FASTEST LAP | NEUE SCHNELLSTE RUNDE | NOUVEAU MEILLEUR TEMPS AU TOUR | NUEVA VUELTA RECORD | RECORD SUL GIRO |
| 108 | NEW FASTEST RACE | NEUES SCHNELLSTES RENNEN | NOUVEAU MEILLEUR TEMPS DE COURSE | NUEVA CARRERA RECORD | RECORD GARA |
| 109 | NEW | NEU | NOUVEAU | NUEVO | NUOVO |
| 110 | BEST | TOP | MEILLEUR | EL MEJOR | MIGLIORE |
| 111 | HIT ACCURACY | TREFFSICHERHEIT | PRECISION DE TIR | PRECISION DE TIRO | PRECISIONE DI FUOCO |
| 112 | RATING | BEWERTUNG | APPRECIATION | CLASIFICACION | CLASSIFICA |
| 113 | DEATHS | EIGENE TOTALSCHÄDEN | DESTRUCTIONS PAR L'ENNEMI | MUERTES | MORTI |
| 114 | KILLS | GELANDETE VOLLTREFFER | ENNEMIS DETRUITS | RIVALES ELIMINADOS | VITTIME |
| 115 | FUEL USED | SPRITVERBRAUCH | CARBURANT UTILISE | GASOLINA CONSUMIDA | BENZINA CONSUMATA |
| 116 | FIRE POWER USED | VERBRAUCHTE MUNITION | MUNITIONS UTILISEES | POTENCIA DE FUEGO USADA | MUNIZIONI UTILIZZATE |
| 117 | ARMOUR USED | VERBRAUCHTE SCHUTZSCHILDE | BLINDAGE DETRUIT | BLINDAJE USADO | ARMATURA USATA |
| 118 | CONGRATULATIONS | HERZLICHEN GLÜCKWUNSCH | FELICITATIONS | FELICIDADES | CONGRATULAZIONI |
| 119 | YOU HAVE COME FIRST | SIE WAREN ERSTER | VOUS TERMINEZ EN 1ERE POSITION | ERES EL PRIMERO | SEI ARRIVATO PRIMO |
| 120 | YOU HAVE COME SECOND | SIE WAREN ZWEITER | VOUS TERMINEZ EN 2EME POSITION | ERES EL SEGUNDO | SEI ARRIVATO SECONDO |
| 121 | YOU HAVE COME THIRD | SIE WAREN DRITTER | VOUS TERMINEZ EN 3EME POSITION | ERES EL TERCERO | SEI ARRIVATO TERZO |
| 122 | FIRST | ERSTER | PREMIERE | PRIMERO | PRIMO |
| 123 | SECOND | ZWEITER | SECONDE | SEGUNDO | SECONDO |
| 124 | THIRD | DRITTER | TROISIEME | TERCERO | TERZO |
| 125 | WINNER | SIEGER | VAINQUEUR | VENCEDOR | VINCITORE |
| 126 | YOU HAVE WON | SIE HABEN GEWONNEN | VOUS AVEZ GAGNE | HAS GANADO | HAI VINTO |
| 127 | YOU HAVE LOST | SIE HABEN VERLOREN | VOUS AVEZ PERDU | HAS PERDIDO | HAI PERSO |
| 128 | POSITION | POSITION | POSITION | POSICION | POSIZIONE |
| 129 | WHITE TEAM | WEISSES TEAM | TEAM BLANC | EQUIPO BLANCO | SQUADRA BIANCA |
| 130 | BLACK TEAM | SCHWARZES TEAM | TEAM NOIR | EQUIPO NEGRO | SQUADRA NERA |
| 131 | RED TEAM | ROTES TEAM | TEAM ROUGE | EQUIPO ROJO | SQUADRA ROSSA |
| 132 | YELLOW TEAM | GELBES TEAM | TEAM JAUNE | EQUIPO AMARILLO | SQUADRA GIALLA |
| 133 | BLUE TEAM | BLAUES TEAM | TEAM BLEU | EQUIPO AZUL | SQUADRA BLU |
| 134 | ORANGE TEAM | ORANGES TEAM | TEAM ORANGE | EQUIPO NARANJA | SQUADRA ARANCIONE |
| 135 | PURPLE TEAM | LILA TEAM | TEAM VIOLET | EQUIPO PURPURA | SQUADRA VIOLA |
| 136 | AQUA-GREEN TEAM | GRÜNES TEAM | TEAM BLEU-VERT | EQUIPO VERDE | SQUADRA ACQUA MARINA |
| 137 | STATISTICS | STATISTIK | STATISTIQUES | ESTADISTICAS | STATISTICHE |
| 138 | OVERALL RATING | GESAMTWERTUNG | CLASSEMENT GENERAL | CLASIFICACION GENERAL | PUNTEGGIO TOTALE |
| 139 | CHAMPIONSHIP TABLE | MEISTERSCHAFTSLISTE | TABLEAU DU CHAMPIONNAT | CLASIFICACION CAMPEONATO | CLASSIFICA CAMPIONATO |
| 140 | RACING TEAM TABLE | TEAMLISTE | TABLEAU DES EQUIPES | CLASIFICACION EQUIPOS | CLASSIFICA DI SQUADRA |
| 141 | WINNING RACE TEAM | SIEGERTEAM | TEAM VICTORIEUX | EQUIPOS GANADORES | SQUADRA VINCENTE |
| 142 | CLASSIC RACE TEAMS | SUPERTEAMS | TEAMS AU NIVEAU | LOS MEJORES EQUIPOS | LE MIGLIORI SQUADRE |
| 143 | CHAMPIONS ROSTRA | SIEGERLISTEN | GRANDS CHAMPIONS | LISTA DE CAMPEONES | ALBO D' ORO |
| 144 | DRIVERS | FAHRER | PILOTES | CONDUCTORES | PILOTI |
| 145 | DO NOT CLEAR HISCORES | HIGHSCORES NICHT LÖSCHEN | NE PAS EFFACER | NO BORRAR LOS RECORDS | NON CANCELLARE I PUNTEGGI |
| 146 | PLAYER | SPIELER | JOUEUR | JUGADOR | GIOCATORE |
| 147 | CONTINUE CHAMPIONSHIP | MEISTERSCHAFT WEITERSPIELEN | CONTINUER LE CHAMPIONNAT | CONTINUAR EL CAMPEONATO | CONTINUA CAMPIONATO |
| 148 | QUIT CHAMPIONSHIP | MEISTERSCHAFT VERLASSEN | QUITTER LE CHAMPIONNAT | ABANDONAR EL CAMPEONATO | ESCI DAL CAMPIONATO |
| 149 | IMMORTAL | GÖTTLICH | IMMORTEL | INMORTAL | IMMORTALE |
| 150 | HARDWIRED | UNSCHLAGBAR | GRANDE MAITRISE | INVENCIBLE | MASSICCIO |
| 151 | OUTTA CONTROL | UNHALTBAR | INCONTROLABLE | SALVAJE | FUORI CONTROLLO |
| 152 | DEMON | ÜBERIRDISCH | DEMONIAQUE | DIABOLICO | DEMONE |
| 153 | DEADLY | TÖDLICH | MORTEL | MORTIFERO | LETALE |
| 154 | WRECKER | SCHRECKEN DER STRASSE | DESTRUCTEUR | DESTRUCTOR | DISTRUTTORE |
| 155 | DANGEROUS | RAMBO | DANGEREUX | PELIGROSO | PERICOLOSO |
| 156 | TAILGATER | MACHO | COLLE-AU-CUL | MALEVOLO | SANGUISUGA |
| 157 | ANTISOCIAL | KINDERFRESSER | ANTISOCIAL | ANTISOCIAL | ANTISOCIALE |
| 158 | DUST DEVIL | RENTNERJÄGER | DIABOLIQUE | DEMONIO DEL ASFALTO | INDIAVOLATO |
| 159 | UPWARDLY MOBILE | ANGEBER | AMBITIEUX | VALOR EN ALZA | BORGHESE |
| 160 | MEDIOCRE | MAGERES MITTELMASS | MEDIOCRE | MEDIOCRE | MEDIOCRE |
| 161 | TOO SLOW | LAHME SCHNECKE | TROP LENT | MUY LENTO | LUMACONE |
| 162 | ENDANGERED SPECIES | NEANDERTALER | ESPECE EN VOIE DE DISPARITION | ESPCIE EN PELIGRO DE EXTINCION | SPECIE IN PERICOLO |
| 163 | TARGET PRACTICE | ZIELSCHEIBE | CIBLE MOBILE | BLANCO FACIL | BERSAGLIO FACILE |
| 164 | BACK MARKER | SCHLUSSLICHT | QUEUE DE PELOTON | CARACOL | SEMPLICIONE |
| 165 | CANYON KISSER | BEULENHUGO | FANA DE CHUTE LIBRE | INUTIL AL VOLANTE | SCARBURATO |
| 166 | VICTIM | FREI ZUM ABSCHUSS | VICTIME | VICTIMA | VITTIMA |
| 167 | SCRAP | SCHROTT | DECHET | BASURA | IMBRANATO |
| 168 | SMEAR | MATSCHE | TACHE | MANCHA DE ACEITE | SENZA SPERANZA |
| 169 | CHEATING SUCKS | MOGELN IST DOOF, MANN! | TRICHER NE SERT A RIEN | ESTA MUY FEO HACER TRAMPAS | VETO AL BARO |
| 170 | REINITIALISE JOYSTICK | JOYSTICK NEU INITIALISIEREN | REINITIALISER JOYSTICK | REINICIALIZAR JOYSTICK | REINIZIALIZZA JOYSTICK |
| 171 | DON'T CLEAR HISCORES | HIGHSCORES NICHT LÖSCHEN | NE PAS EFFACER | NO BORRAR LOS RECORDS | NON CANCELLARE I PUNTEGGI |
| 172 | FINAL LAP | LETZTE RUNDE | DERNIER TOUR | ULTIMA VUELTA | ULTIMO GIRO |
| 173 | WRONG WAY | FALSCHE RICHTUNG | SENS CONTRAIRE | DIRECCION EQUIVOCADA | STRADA SBAGLIATA |
| 174 | KILLED BY | TOTALSCHADEN DURCH | TUE PAR | ANIQUILADO POR | UCCISO DA |
| 175 | LOAD GAME | SPIEL LADEN | CHARGER JEU | CARGAR JUEGO | CARICA GIOCO |
| 176 | SAVE GAME | SPIEL SPEICHERN | SAUVEGARDER JEU | SALVAR JUEGO | SALVA GIOCO |
| 177 | EMPTY | LEER | LIBRE | NADA | VUOTO |
| 178 | WAITING FOR OTHER PLAYERS | WARTE AUF ANDERE SPIELER | EN ATTENTE DES AUTRES JOUEURS | ESPERANDO A OTROS JUGADORES | ATTENDERE ALTRI GIOCATORI |
| 179 | PRESS FIRE TO CONTINUE | ZUM WEITERSPIELEN FEUER DRÜCKEN | TIREZ POUR CONTINUER | PULSA FUEGO PARA CONTINUAR | PREMI FUOCO PER CONTINUARE |
| 180 | SHADING | SCHATTIERUNG | OMBRAGES | SOMBREADO | OMBREGGIATURA |
| 181 | SKY | HIMMEL | CIEL | CIELO | CIELO |
| 182 | TEXTURE MAPPING | TEXTURE-MAPPING | TEXTURES | MAPEADO DE TEXTURAS | MAPPATURA TEXTURE |
| 183 | NETWORK | NETZWERK | RESEAU | RED | RETE |
| 184 | STEREO | STEREO | STEREO | ESTEREO | STEREO |
| 185 | RACE TEAM | TEAM | TEAM DE COURSE | EQUIPO | SQUADRA |
| 186 | NEW CHAMPIONSHIP | NEUE MEISTERSCHAFT | NOUVEAU CHAMPIONNAT | NUEVO CAMPEONATO | NUOVO CAMPIONATO |
| 187 | LOAD CHAMPIONSHIP | MEISTERSCHAFT LADEN | CHARGER CHAMPIONNAT | CARGAR CAMPEONATO | CARICA CAMPIONATO |
| 188 | SAVE CHAMPIONSHIP | MEISTERSCHAFT SPEICHERN | SAUVEGARDER CHAMPIONNAT | SALVAR CAMPEONATO | SALVA CAMPIONATO |
| 189 | SUPER CAR | SUPERMANN | SUPERPUISSANCE | TODOPODEROSO | INVINCIBILE |
| 190 | CLEAR HISCORES | HIGHSCORES LÖSCHEN | EFFACER LES SCORES | BORRAR RECORDS | CANCELLA I PUNTEGGI |
| 191 | 720! | 720! | 720! | 720! | 720! |
| 192 | 360! | 360! | 360! | 360! | 360! |
| 193 | 180! | 180! | 180! | 180! | 180! |
