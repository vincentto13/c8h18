# Game specification

This directory specifies the game 
in enough detail to create it from the existin data files
recreation. It was written from a completed disassembly study of the
retail DOS/4GW binary and from a working recreation that was verified
against it, frame by frame where that was possible.

No chapter refers to code: there are no function names, addresses or
structure offsets. Hexadecimal appears only for magic numbers, flag bits
and enumerations stored in data files.

## Chapters

| # | File | Contents |
|---|------|----------|
| 00 | [00-overview.md](00-overview.md) | Scope, conventions (units, coordinates, fixed point, tick, screen modes, palette), game flow, languages |
| 01 | [01-file-formats.md](01-file-formats.md) | Every data file byte by byte: RNC compression, palettes, sprite tables, texture maps, levels, object models, sounds, music, config/save, intro stream |
| 02 | [02-front-end.md](02-front-end.md) | Menu tree, settings, championship, name entry, track records, results screens, pause menu, vehicle carousel, fades, demo race |
| 03 | [03-controls.md](03-controls.md) | Key table for the race, menus, camera, cheats and the puzzle; port additions |
| 04 | [04-race-rules.md](04-race-rules.md) | Start, laps, placings, finish and post-finish, classification, counters, messages and banners, death match, difficulty, catch-up |
| 05 | [05-vehicles.md](05-vehicles.md) | Craft stats, flight model tick by tick, collisions, booster, fuel/shield/ammo, damage, destruction and the recovery truck |
| 06 | [06-weapons-and-pickups.md](06-weapons-and-pickups.md) | Minigun, missiles, target lock, explosions and effects, collectables, chargers |
| 07 | [07-ai.md](07-ai.md) | Computer drivers: waypoints, junctions, personalities, shooting, autopilot, behind factor |
| 08 | [08-rendering.md](08-rendering.md) | Camera, projection, terrain, models, sprites, shadows, HUD, minimap, target marker, text, fades, screen modes |
| 09 | [09-audio.md](09-audio.md) | Sound events, panning, loops, music, mono fallback, intro sound script |
| 10 | [10-intro.md](10-intro.md) | The intro movie: stream, timing, sound script, skip, palette |
| 11 | [11-network.md](11-network.md) | What is known of the IPX network game (not recreated) |

## How to use it

Read 00 first for the conventions, then 01 to build the loaders, then
05–08 for the race and 02–04 for everything around it. A recreation
that follows 05, 06 and 07 exactly, with the tick order given in 05,
reproduces the original's races deterministically from the same inputs.
