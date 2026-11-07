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

## Build and run

```
cargo build --release
./target/release/c8h18            # data directory defaults to ./resources
```

Options:

| Option | Effect |
|---|---|
| `-s`, `/s`, `--silent` | no sound or music |
| `--no-intro` | skip the intro movie |
| `--speed N` | game speed in percent of the original 20 ticks/s (default 90; e.g. 100 = original, 80 = slower) |
| `<path>` or `GAME_DATA=<path>` | data directory (DATA/, MAPS/, OBJECTS/, SOUND/) |
| `--race N` / `--demo N` | debug: go straight to a race / demo race on track N |
| `--sim N TICKS` | debug: run a headless computer-only race and print the crafts' progress |

Saved files (`config.dat`, `save00.dat`..) go to `save/` inside the data directory.

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

| Action | Pad |
|---|---|
| accelerate / brake | left stick or D-pad up / down, RT / LT (R2 / L2) |
| steer | left stick or D-pad left / right |
| minigun / missile / booster | A (✕) / B (○) / X (□) |
| pause menu | Start (Options); Start or B resumes |
| camera view | Y (△) cycles cockpit and the three chase views |
| zoom out / in | LB / RB (L1 / R1) |
| end race | hold Back / Select for 1 second |
| menus | D-pad or stick to move, A to select, B to go back |

OPTIONS → CONTROLS → ANALOG STEERING (off by default, saved in `save/pad.txt`) makes the left
stick steer proportionally instead of fully left or right; the D-pad still steers fully. The name
entry needs the keyboard. Rumble is not supported (the controller library has none on macOS).

## Debug automation

`C8H18_AUTO="frame:op:arg;..."` runs scripted input. The ops are `shot:<png path>`,
`key:<Key>`, `hold:<Key>`, `release:<Key>`, `padhold:<Button>`, `padrelease:<Button>` (buttons: Up,
Down, Left, Right, A, B, X, Y, LB, RB, LT, RT, Start, Back) and `quit`; `frame` counts display frames. Used for
screenshot tests.
