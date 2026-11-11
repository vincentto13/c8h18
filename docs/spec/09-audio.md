# 09 — Audio

Scope: the sound system of the original: the sample banks and how one is chosen, the sample-playback model (volume,
pan, rate, looping, voice reuse), the race sound dispatcher and its classes, the distance and pan model, the engine
loops, every sound event with its trigger and sample, the front-end sounds, the music (which tune plays when), the mono
fallback, the `-s` switch, and the semantics of the intro's sound script. File layouts are in chapter 01, the intro's
script table in chapter 10.

## 1. Overview

- The original uses the Miles Audio Interface Library 3.02: a digital driver (`.DIG`, e.g. Sound Blaster) for samples
  and a MIDI driver (`.MDI`) for music, configured by `SOUND/DIG.INI` and `MDI.INI`.
- **Samples**: `SOUND/SOUND.DAT` bank 0 (35 entries) for the game, bank 1 (14 entries) for the intro. The format slot
  follows the digital driver (8-bit 22050 Hz on a Sound Blaster, chapter 01). Sample numbers are 1-based catalogue
  entries of bank 0.
- **Music**: `SOUND/MUSIC.DAT`, XMIDI tunes in the variant matching the MIDI driver (FM `.XMI` with the timbre bank
  `SAMPLE.OPL`/`SAMPLE.AD`, General MIDI `.GEN`, MT-32 `.ROL`, wavetable `.WTB`).
- The title splash and the front end play **no music**.

## 2. Sample playback model

Every effect goes through one routine with these parameters:

| Parameter | Range | Meaning |
|---|---|---|
| owner | integer | who plays it (a thing's owner number, or a fixed id); together with the sample it identifies the voice |
| sample | 1..35 | bank-0 catalogue entry |
| volume | 0..0x7fff | scaled by the configured effects volume (config 0x0980; the exact scaling formula is not documented) |
| pan | 0..0xffff | 0x8000 = centre; **forced to 0x8000 when the stereo flag (config 0x0990) is clear** |
| rate | percent | playback rate as a percentage of the bank's native rate (100 = as recorded) |
| loop | −1 or 0 | −1 = loop forever, 0 = play once |
| mode | 1, 2, 3 | voice choice (below) |

Voice choice: **mode 1** takes any finished voice; **mode 2** does nothing if a voice with the same (owner, sample)
is still playing, else takes a finished voice; **mode 3** restarts the voice that holds the same (owner, sample) if
there is one, else takes a finished voice.

The driver receives `pan >> 9` (0..127, 0x8000 → 64; 0 = full left, 127 = full right; the left channel gets
127 − pan) and `(volume − 1) >> 8`.

Further operations by (owner, sample): **stop**; **set rate** (ignored for rate ≤ 0 and for finished voices);
**update** = play (mode as given) and then set volume, rate and, with the stereo flag only, pan of the running voice
(used by the engine loops). A rate of 0 given when a loop starts leaves it silent until a non-zero rate is set.

## 3. Race sounds

### 3.1 The listener

The listener is the **vehicle the camera follows** (normally the human's craft), not the camera: its position and its
heading.

**Distance volume** of a source: 0x7fff for the listener's own vehicle; otherwise, with d = 3D distance in 8.8 units
(256 per cell),

```
d < 4608 (18 cells):  volume = 1000 + (4608 − d) · 0x7c17 / 4608
else:                 volume = 1000
a positioned sound is not played at all unless volume > 1000
```

**Pan** of a source: centre (0x8000) when it is within 129/256 cell of the listener; otherwise, with
`a = bearing from the listener to the source` and `δ = (a − listener heading)` as a signed 16-bit angle (positive =
clockwise):

```
f = δ                     if |δ| ≤ 0x4000
f = sign(δ)·(0x8000 − |δ|) otherwise           ; a source behind folds to the front
pan = 0x8000 + 2·f, clamped to 0..0xffff       ; full right at 90° clockwise, full left at 90° anticlockwise
```

The rate of dispatched sounds is always 100 % unless set explicitly afterwards.

### 3.2 Dispatcher classes

The race code plays effects through a dispatcher keyed by the sample number; "viewed" means the source thing is the
listener's vehicle.

| Class | Samples | Behaviour |
|---|---|---|
| A | 1 TESTENG3, 7 REFUEL, 8 POWERUP, 15 LOCKON4, 17 WARNING | viewed vehicle only; full volume, centred, loop, mode 2 |
| B | 2 ENGINE1 | other vehicles only; positioned (distance volume and pan), loop, mode 2 |
| C | 3 MINISHOT, 4 MINBLANK | viewed: full volume, centred, once, mode 3; others: positioned at volume × 0x5dc0 / 0x8000 (73 %), once, mode 3 |
| D | 5 MISSILE, 6 EXPLODE, 10–12 RICCO1–3, 13 HELEHIT, 18 PICKUP | positioned (viewed: full volume), once, mode 3 |
| E | 9 BOOST, 22–24, 25 START1, 26 START2, 27–29 STUNT180/360/720, 30–32 | viewed only; full volume, centred, once, mode 2 |
| F | 14 SWITCH | viewed only, positioned pan, once, mode 3 |
| G | 19 SCRAPE2 | positioned, loop, mode 2 |
| — | 16, 20, 21, 33–35 | the dispatcher plays nothing |

Samples 16, 22–24 and 30–32 are 8-byte empty placeholders (NULL.WAV): the events exist but are silent (22, 23, 24 are
raised when the shield drops below 7500 / 5000 / 2500; 31 and 32 on a kill and on dying).

### 3.3 Event list

| Event | Sample | Trigger and parameters |
|---|---|---|
| minigun shot | 3 MINISHOT | each bullet created (restarted per shot, mode 3) |
| minigun overheated | 4 MINBLANK | trigger held while the minigun is overheated: MINISHOT stopped, MINBLANK played every tick |
| missile launch | 5 MISSILE | each launch |
| booster charging | 8 POWERUP | while the booster trigger is held and it may charge (not restricted, charge time < 100, fuel/ammo > 5): loop, rate set to `2 · charge time + 100` % each tick |
| booster fired | 9 BOOST | first tick after release: POWERUP stopped, BOOST played |
| damage taken | 13 HELEHIT | every tick in which the craft takes damage (bullet, explosion, bumps), unless invincible; rate 240 % when the damage comes from an explosion |
| craft contact | 13 HELEHIT, then 19 SCRAPE2 | contact begins: HELEHIT; contact continues: SCRAPE2 loop at rate 60 %; contact ends: SCRAPE2 stopped |
| charging on a pad | 7 REFUEL | loop while a charger pad is refilling the craft (value below 10000); stopped otherwise |
| refill after a repair | 7 REFUEL | played on every tick of the post-repair refill (shield +1000, then fuel, then ammo); stopped when full |
| explosion | 6 EXPLODE | first tick of an explosion thing (particle, medium, big, massive); the particles they spawn are silent |
| bullet ricochet | 10 / 11 / 12 RICCO1..3 | a bullet dies on impact: random one of the three, rate 75..125 % (75 + random mod 51) |
| collectable taken | 14 SWITCH | a craft picks up a collectable (PICKUP, 18, is never used in the race) |
| low fuel / shield / ammo | 17 WARNING | loop while the FUEL LOW / SHIELD LOW / AMMO LOW banner is shown; stopped when it closes |
| homing missile on you | 15 LOCKON4 | loop while a homing missile is steering at the viewed craft this tick; stopped otherwise |
| finish in place 1 / 2 / 3 | 29 / 28 / 27 STUNT720 / 360 / 180 | when the lap logic flags the viewed craft as finished, together with big message 1 (string 172); no fanfare for places 4..8 |
| SUPER CAR award | 29 STUNT720 | with the SUPER CAR message |
| spin bonus 720! / 360! / 180! | 29 / 28 / 27 | with the spin-bonus big messages (race chapters) |
| countdown | 25 START1, 26 START2 | START1 after the first and after the second light, START2 when the third light comes on (owner 0, once, mode 2) |
| vehicle removed | stop 1, 2 | a vehicle thing's own engine voices are stopped |

The recovery truck makes no sound. When a craft's event is raised for a non-viewed craft in a viewed-only class,
nothing is played.

### 3.4 Engine loops

Once per race tick (not while paused) the engine mixer runs over the race crafts (vehicle sub-type 0; recovery trucks
excluded). For each craft, `rate = min(v, 180) · 300 / 110` percent, with v the craft's measured speed value (the
HUD's lower speed bar, race chapters).

- The **viewed craft**: TESTENG3 (1) at full volume, centred, looping, at its rate (update, mode 2).
- **Other crafts** whose distance volume is above 1000:
  - with the stereo flag: split into a left group (pan below centre) and a right group; each group plays one
    ENGINE1 (2) voice with the sum of its distance volumes (capped at 0x7fff), the average of its pans and the rate of
    the last craft added to it; a group with no craft has its voice stopped;
  - without stereo: one ENGINE1 voice with the summed volume (capped), centred, at the last craft's rate.

### 3.5 Pause and race end

Entering the pause ends every playing sample voice; the engines and all other race sounds stay silent while paused
(nothing is processed); the music goes on. Leaving the race for the front end ends all samples and stops the music.

## 4. Front-end sounds

| Sample | Use |
|---|---|
| 7 REFUEL | every frame while the text box morphs (restarted on its voice, mode 3, rate 300 %) |
| 35 WINOPEN | the box reaches its place |
| 33 PRINTTYP | typewriter characters, text-entry keys |
| 34 PRINTRET | `$S1` in a text |
| 20 CHANGE | cursor movement, Escape, carousel turns, value steps on the carousels |
| 21 SELECT | selecting items |

All are played with owner 0 at the effects volume, centred. The MUSIC VOLUME item does not play SELECT; its handler
plays a sample itself (which one is not documented).

## 5. Music

| When | Tune |
|---|---|
| intro (`INTRO.EXE`) | TINTRO2 (tune 4), from its script (chapter 10) |
| title splash | none |
| front end (all screens, results) | none: music is stopped on entry |
| race on track 1 or 2 | TGAME1 (tune 1) |
| race on track 3 or 4 | TGAME2 (tune 2) |
| race on track 5 or 6 | TGAME4 (tune 3) |
| (track number above 6: unreachable) | TGAME3 (tune 5) |

- The race tune starts at the **end of the start countdown**, at volume `(music volume − 1) / 256` (0..127).
- It keeps playing through the pause, after the player's finish and in the demo race, and stops when the front end is
  entered.
- Whether the driver restarts a tune when its sequence ends is up to the XMIDI data (FOR/NEXT loop controllers,
  chapter 01); the game does not restart it itself as far as documented.
- The game ignores TGAME3 for the six real tracks.

## 6. Mono fallback

The config's stereo flag (offset 0x0990) is never written by the game, so a config created by the game has it clear:
**the original plays everything centred (mono)** unless the shipped/edited config sets it. Without the flag: every
`play` uses pan 0x8000, `update` does not set the pan, and the other crafts' engines are mixed into one centred voice
(section 3.4). The front-end sounds are centred anyway.

## 7. The `-s` switch

A command-line argument beginning with `-S`, `-s`, `/S` or `/s` turns off both digital sound and music: no sound
driver is loaded, no samples or tunes play, the intro is started as `exec\intro.exe -s` (silent too), and OPTIONS
hides SOUND OPTIONS (and the volume items). Without a sound card the same items are hidden.

## 8. The intro's sound script

`INTRO.EXE` carries a table of **46 records** (7 bytes each: `i16 frame`, `u8 op`, `i32 argument`, terminated by frame
−1). Before frame 0 is shown and before each frame n (1-based) is decoded, every record with `frame == n` is executed
in table order. Op letters are case-insensitive:

| Op | Argument | Effect |
|---|---|---|
| A | n | frame delay = n ticks of the 120 Hz timer (a frame lasts n + 1 ticks; 0 = no wait and nothing shown) |
| B | n | load music bank n (only with music) |
| E | n | load sample bank n of SOUND.DAT, slot chosen by the digital driver (only with sound) |
| F | n | fade sample n out (not used by the table) |
| L | n | read n bytes of the movie ahead into memory (not used) |
| M | n | play tune n (1-based) at volume 127 |
| R | n | play sample n looping forever: volume 127 (maximum), centre pan, 100 % rate, mode 2 (reuse the voice already playing it) |
| S | n | play sample n once: volume 127, centre, 100 %, mode 1; n = 0 stops every sample |
| T | n | stop sample n; 0 = stop every sample |
| X | — | stop the music |
| other | | ignored |

The intro ignores the game's volume settings. The table itself (frame → op) is in chapter 10. When the intro ends it
stops all samples and shuts the sound system down; the music does not carry over into the game.

## 9. Port deviations

- Music is always the FM variant (`.XMI` with `SAMPLE.OPL`) played on an emulated OPL3. Samples are mixed in
  software. Volumes are linear gains, not the AIL driver's curves.
- The port picks the SOUND.DAT slot for each bank on its own, with no driver involved: it uses **slot 0 (16-bit
  44100 Hz)** when the file has that bank there, and falls back to **slot 3 (8-bit 22050 Hz)** otherwise. The shipped
  file has no slot 0, so it plays from slot 3 as before. Each sample plays at its own WAV header rate (rate 100 % = as
  recorded), so the rate percentages in this chapter mean the same in both slots. The editor shows every slot the file
  has and converts an imported WAV to the slot's bits and rate.
- The port loops the race tune and the intro tune.
- **OGG music** (port addition): an OGG Vorbis file named after a tune, `SOUND/MUSIC/<name>.OGG` with `<name>` =
  TGAME1, TGAME2, TGAME4, TINTRO2 or TGAME3 by tune number (section 5), replaces that tune's XMIDI. It is looked up
  like every data file (the `--override` folder first, any letter case). It loops between the Vorbis comments
  `LOOPSTART` and `LOOPSTART + LOOPLENGTH` (or `LOOPEND`, exclusive), counted in samples from the start of the file;
  without `LOOPSTART` the whole file loops. Any sample rate (resampled to the output), mono or stereo (more channels
  are folded to stereo). It plays at half amplitude times the music volume, so a full-scale recording peaks where
  the FM tunes do. The file is decoded on a helper thread the first time (the tune starts when it is ready); if it
  cannot be decoded, the FM tune plays.
- The port's fresh config has the stereo flag set (the original's is mono); a config with the flag clear gives the
  mono behaviour of section 6.
- Engines, chargers, warnings, lock-on and the finishing fanfares are derived from the race state every tick rather
  than raised at the original's call sites; the audible result is the same.
