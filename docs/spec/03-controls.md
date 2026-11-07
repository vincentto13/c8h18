# 03 — Controls

Scope: every key the original game reacts to, per context (race, pause, puzzle, spectator mode, demo race, front end,
intro), with the key's meaning, whether it acts on the press ("edge") or while held, and the conditions under which
it is accepted; the cheats and the sliding puzzle that unlocks them; what the original does not have; and, separately
labelled, the additions of the Rust port. Keys are named by their position on a US/UK PC keyboard; "scan code" values
are the PC set-1 make codes the original compares.

## 1. How keys are read

- The keyboard interrupt keeps a **held** state per scan code and the **last make code** received ("last key"). A key
  handler that consumes the last key clears it, so each received make code acts once. (Whether a held key's
  typematic repeats re-trigger edge keys depends on the keyboard repeat; the original does not filter them.)
- **Edge** keys act when the last key equals the key (and, for combinations, the modifier is held at that moment).
- **Held** keys act every simulation tick while all their keys are down.
- The race keys come from a fixed table; each record names up to three keys, a command and a value. Commands are
  queued for the player and executed by the race logic in the same tick. There is no way to change the table.
- Some race records also accept the joystick (stick up/down/left/right, buttons 1/2/3).

## 2. Race

Processed every race tick, except while paused (section 3), while the sliding puzzle is visible (section 5), in
spectator mode (section 6) and in demo races (section 7).

### 2.1 Driving and weapons (held)

| Action | Keys | Joystick |
|---|---|---|
| accelerate | Up arrow | stick up |
| brake / reverse thrust | Down arrow | stick down |
| steer left | Left arrow | stick left |
| steer right | Right arrow (wins when Left and Right are both held) | stick right |
| minigun | **Z**, **,** (comma), **V** | button 1 |
| missile | **X**, **.** (period), **B** | button 2 |
| booster (charge while held, fire on release) | **C**, **/** (slash), **Space** | button 3 |

The joystick only feeds the Up/Down/Left/Right/Z/X/C records; the alternative weapon keys are keyboard-only. The
meaning of each input bit is in the flight-model and weapon chapters.

### 2.2 Edge keys

| Keys | Effect | Condition |
|---|---|---|
| **P** | pause on (section 3) | not in a demo race, not spectating; ignored in network games |
| **Escape** | end the race at once, finished or not (classification rules in chapter 02) | while paused it continues the race instead |
| **right Shift + Q** | quit the game: end the race and exit to DOS without results screens (the config is saved) | |
| **right Shift + E** | end the race: requested now, carried out at the next tick whose race-frame counter has bit 5 clear (within 32 ticks) | |
| **1**, **2**, **3**, **4** | select the view slot 0..3: the race camera becomes view 7 + slot: 1 = cockpit (default), 2 = near chase, 3 = middle chase, 4 = far chase (camera chapter) | the slot is stored in the player record and kept in the config |
| **Alt + F1 .. F6** | cheats 0..5 (section 4) | only with the cheat flag |

### 2.3 Other race keys

| Keys | Effect | Condition |
|---|---|---|
| **Ctrl + Tab** | show / hide the sliding puzzle (section 5) | not in network games, not in low-memory mode |
| **Y** | spectator mode on / off (section 6) | only with the cheat flag |
| **R** | toggle 320×200 / 640×480 (stored in the player record) | only in a demo race or spectator mode; not in low-memory mode |
| **C** | ignored (consumed) | in a demo race or spectator mode |
| **F1 .. F10**, **1 .. 8**, **−**, **=**, and in the orbit view **arrows** and **Z / X** | camera keys (section 6) | **only in spectator mode**: in a normal race the camera is always the player's view slot (2.2) |

There is **no look-back key**, no rear-view mirror, and no in-race key to change the volume or the detail level (only
the pause menu).

## 3. Pause

P toggles the pause flag in the player record (P does nothing in demo races, in spectator mode and in network games).
While paused (full description in chapter 02):

| Keys | Effect |
|---|---|
| Up / Down / Enter / Left / Right | navigate the pause menu (SOUND OPTIONS, DETAIL OPTIONS), as in the front end |
| Escape | **continue the race** |
| P | continue the race |

The arrow and Enter keys are consumed by the menu. The race does not advance while paused.

## 4. Cheats (Alt + F1 .. F6)

Accepted only when the human player's **cheat flag** is set. The flag is set by solving the sliding puzzle (section
5) or by starting the game with **exactly three command-line arguments whose first letter after `-` or `/` is L, P or
Y** (either case, e.g. `game -L -P -Y`); it lives in the player record (config byte 0x22, bit 2), so it survives
races, and it is cleared whenever the config is saved (at quit). A race driven with the flag set gives the driver
assessment 20, "CHEATING SUCKS".

| Keys | Cheat | Effect |
|---|---|---|
| Alt + F1 | 0 | own shield set to 0xFFFF, i.e. −1: the own craft is destroyed |
| Alt + F2 | 1 | the shield of every other active craft set to 0xFFFF: all other crafts are destroyed |
| Alt + F3 | 2 | own fuel, ammo and shield set to 20000 (doubled) |
| Alt + F4 | 3 | own lap counter + 1 |
| Alt + F5 | 4 | all three weapon upgrades to the maximum (3) |
| Alt + F6 | 5 | fuel, ammo and shield 10000 (full) and all upgrades 3 |

Alt + F1 .. F6 are consumed by the cheat records, so they never reach the camera.

## 5. The sliding puzzle (Ctrl + Tab)

Ctrl + Tab toggles the puzzle's visibility (not in network games, not in low-memory mode). The race keeps running
underneath, but **while the puzzle is visible the race key table of section 2 is not processed** (no new driving,
weapon, pause or Escape input from the keyboard). The puzzle state is part of the config (chapter 01) and survives
races and restarts.

The picture `PUZZLE.DAT` (112 × 96) is cut into 4 × 4 tiles of 28 × 24 pixels numbered 0..15 row by row; tile (col,
row) is drawn **unscaled** at screen pixels (104 + 28·col, 52 + 24·row) in both screen modes. One state step per race
frame while visible:

| State | Per frame |
|---|---|
| 0 set-up | grid = tiles 0..15 in order, cell 15 = empty; move counter = 150; → state 1 |
| 1 shuffle | draw; move the empty cell once at random: repeat `seed = seed · 0x24a1 + 0x24df` (32-bit), `r = seed mod 5` (unsigned) until a legal move results (r = 0 down, 1 up, 2 right, 3 left; 4 and moves off the grid are re-drawn); then if the counter was already below 0 → state 2, and the counter is decremented (so 152 shuffle moves happen) |
| 2 play | draw; the arrow keys slide a tile into the empty cell and are consumed: **Up** moves the tile below the gap up, **Down** the tile above down, **Left** the tile right of the gap left, **Right** the tile left of the gap right; when at least 15 tiles are in their home cells and the empty cell is cell 15: set the **cheat flag**, put tile 15 into cell 15, counter = 200, → state 3 |
| 3 solved | draw the complete picture; when the counter (decremented each frame) has gone below 0 → state 0 (a new shuffle starts if still visible) |

## 6. Spectator mode (Y) and the camera keys

With the cheat flag set, **Y** toggles spectator mode. Turning it on sets the camera to view 6 (level camera) with
follow player 0; turning it off resets the camera to the cockpit view (zoom 300, follow 0, distance 0; angles kept).
In spectator mode the race key table is not processed (the own craft coasts without input; no P, Escape, cheats),
the HUD and messages are not drawn, R toggles the screen mode, C is ignored, and the camera keys work:

| Key | Effect (camera chapter for the geometry) |
|---|---|
| F1 | view 1: fixed behind the followed craft |
| F2 | view 2: in front, looking back |
| F3 | view 3: left side |
| F4 | view 4: right side |
| F5 | view 5: above |
| F6 | view 6: nearest invisible trackside camera within 32 cells that sees the craft (falls back to the cockpit) |
| F7 | view 7: cockpit (default) |
| F8 | view 8: chase, 120/256 cell behind, 15° down |
| F9 | view 9: chase, 240/256 cell behind, 10° down |
| F10 | view 11: orbit; while it runs Left/Right turn it (∓0x222 per tick), Up/Down change its pitch, Z/X roll it |
| 1 .. 8 | follow player 0..7 (no check that the player exists) |
| − | zoom + 20 (maximum 1000) |
| = | zoom − 20 (minimum 0) |

Zoom (default 300) scales the distance of view 6 and of the orbit view. The keys only change the view, the followed
player and the zoom; the camera eases from its previous position. View 10 (chase, 360/256 cell, 5°) is reachable only
through key 4 of section 2.2.

## 7. Demo race

Any key **except R and C** ends the demo race; R toggles the screen mode, C is ignored. The race key table is not
processed (all crafts are computer players).

## 8. Front end

| Keys | Effect |
|---|---|
| Up / Down | move the menu cursor (wrapping); laps (track selection); team colour (craft selection); LOAD/SAVE rows |
| Left / Right | adjust values; turn the carousels |
| Enter | select / confirm |
| Escape | back / close submenu / cancel |
| Space (held) | show the current text at once (skip the typewriter) |
| printable keys, Backspace, Enter | text entry (name: 8 characters, save names: 12; upper case; UK layout) |
| Shift + Q (either Shift) | save the config and quit to DOS |
| Alt + 1 .. 4 | language English, German, French, Spanish |
| Alt + V | toggle the in-race debug overlay |
| Alt + C | nothing in the shipped game |
| any key | leave the high-score table |
| Enter / Escape | leave each results screen |

In the front end the joystick is translated into these key codes (exact mapping not documented). A mouse pointer
routine exists (it works in the virtual 640-wide space); whether the menus use the mouse is not documented.

## 9. Intro

Any key ends the intro after the current frame (chapter 10). A debug left-over: the F key skips the frame wait and
disables the samples, but like any key it also ends the movie.

## 10. Not in the original

- No key-definition menu and no key configuration file; the race keys of section 2 are fixed.
- No look-back key.
- No way to start the built-in FLIC screen recorder (its start routine is not connected; Alt+C is an empty stub).
- The camera keys F1..F10, 1..8 and −/= do nothing outside spectator mode.

## 11. Port additions and deviations

Port additions (not in the original):

- **Key rebinding**: OPTIONS → CONTROLS (chapter 02) and the file `keys.txt` (chapter 01). Bindable:
  accelerate, brake, left, right, minigun, missile, booster, pause, zoom in, zoom out; up to 4 keys each. Defaults:
  the original table plus **W / S / A / D** for accelerate / brake / left / right. Escape, Enter, Tab, Alt, F1..F10
  and 1..8 keep their fixed meanings and cannot be bound; binding a key removes it from any other action.
- **Alt + C** on any screen starts a FLIC recording (320×200 only) into `flics/game%04d.flc` of the data directory, or
  stops a running one; the race end also stops it. Frames are captured once per simulation tick, "Recording FLIC" is
  drawn at (0, 0) in font 0.
- **Tab** in a race toggles a top-down debug map. Page Up / Page Down are read by the port's input layer but have no
  effect (the original has no Page Up/Down function either; its zoom keys are − and =).
- Command line: `-s` (also `/s`, `--silent`) no sound (the intro too); `-320` start in 320×200; `-640` accepted and
  ignored; `--no-intro` skip the intro; the data directory as an argument or `GAME_DATA`.
- Menus: Space also selects.

Port deviations:

- The port starts in 640×480.
- The camera keys F1..F10, 1..8 and −/= work in every race (the original: spectator mode only), and keys 1..4 only
  follow other crafts (they do not select the chase views of section 2.2).
- A new race always starts outside spectator mode (the original's spectator switch outlives the race).
- The command-line cheat letters L/P/Y are not implemented.
