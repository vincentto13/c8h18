# 11 — Network game

Scope: what is known about the original's network (multi-machine) mode: how it is switched on, the menu items and
screens, the settings involved, the features that are disabled or changed in network mode, the traces of the
command-exchange model in the race logic, and an explicit list of what has **not** been reverse-engineered. The Rust
recreation does not implement the network game.

## 1. Status

The network code was only surveyed, not reverse-engineered in detail. This chapter separates (a) behaviour
established from the menus, the race logic and the text strings, (b) the transport and exchange pattern, read from
the disassembly of the race loop and the packet routines, and (c) what is unknown. A recreation that wants network
play will have to design its own protocol; nothing below is enough to interoperate with the original.

## 2. Established behaviour

### 2.1 Availability and menus

- At every front-end entry the game passes a "network available" flag. When it is false, OPTIONS → **NETWORK OPTIONS**
  is hidden. How availability is detected is not documented.
- NETWORK OPTIONS (chapter 02):
  - **NETWORK** on/off: toggles the network mode flag. Not saved in the config; off at every start.
  - **SESSION NUMBER** (n): 0..9, Left −1, Enter/Right +1, wrapping; stored in the human player's record (config
    offset 0x094d) and therefore saved.
- While NETWORK is on, RACE → **CHAMPIONSHIP is hidden**: only single races can be started.

### 2.2 Starting a network race

The race start (front-end state 8) differs from section 10 of chapter 02 after the level has been loaded:

1. A status message is printed (format not documented).
2. The network layer is asked for the **local player's slot**; −1 means failure: the front end fades out, reloads its
   backdrop, fades in and returns to the main menu.
3. The local player's record (in that slot) is filled from the config (name, team, craft, ...), the race distance is
   the track's single-race laps setting, the record's player type is set to 3 (a local single-player human uses 1,
   computer players 8), and the slot index is stored.
4. A session set-up / waiting screen runs (strings ATTEMPTING CONNECTION (88), WAITING FOR OTHER PLAYERS (178), PRESS
   FIRE TO CONTINUE (179) belong to it; its layout is not documented). If it fails, the front end fades and returns
   to the main menu.
5. **No computer players are created** in network mode (the COMPUTER PLAYERS setting applies only to non-network
   races).

The starting-grid backdrop `ONET0-N` ("net") is shown before every race, network or not.

### 2.3 Differences during a race

| Feature | Network mode |
|---|---|
| pause (P) | the pause command does not toggle the pause |
| sliding puzzle (Ctrl+Tab) | disabled |
| demo race (attract mode) | not started |
| catch-up ("behind factor", flight-model chapter) | computed from a separate table for network races |
| a few internal commands (e.g. the one that toggles the cheat flag and the screen mode together) | refused |

### 2.4 Command model

The race logic does not act on keys directly: each key-table match **queues a command for a player** (command number
+ 32-bit value, one 6-byte queue entry per player in the race state), and the race logic executes the queued commands
of every player. Several commands only take effect when their player is the local human (ending the race, quitting,
the settings commands below). The command set includes, besides the keyboard commands of chapter 03, commands no key issues in the shipped
game: zoom ±, screen-mode change, toggles of sky, shading, near texture distance (cycled 0..3), texture level (cycled
0..4), texture mapping, effects/music volume steps of ±0x100 (clamped 0..0x7fff), two sound on/off flags (config
0x0984 / 0x0988), and the language. This structure (commands per player slot, applied in the same order on every
machine) fits a command-exchange (lockstep) network model; the recorded input streams `movie/movie%03d` and
`movie/paket%03d` referenced by the executable use the same idea.

Keys 1–4 issue command 0x1e with values 0..3. In a single-player race this selects the camera view slot (chapter 03);
whether remote players see it as a message or taunt is not established.

### 2.5 Strings

| Text | Where |
|---|---|
| `Player %d has quit out.` | English only, outside the translated string tables (format string in the executable) |
| ATTEMPTING CONNECTION (88), WAITING FOR OTHER PLAYERS (178), PRESS FIRE TO CONTINUE (179) | translated (chapter 02, appendix A) |
| NETWORK ON / OFF (57 / 58), NETWORK (183), NETWORK OPTIONS (89), SESSION NUMBER (86) | translated |

## 3. Transport and exchange pattern (from the disassembly)

The following was read from the race loop and the packet routines of the retail executable; the payloads were not
decoded:

- transport: IPX, driven through DPMI "simulate real-mode interrupt" calls with a real-mode register block;
- a message is sent and received in chunks of 2 KiB (the byte count is split at 2048; the remainder is the last
  chunk);
- up to 8 players (the race state has exactly 8 player slots);
- the session number selects the session to join;
- one machine is the host. Once per race frame, before the input commands are processed, each machine refreshes a
  presence table (one byte per player slot, polled from the driver) and then exchanges data: the host sends to every
  other present player and then receives from every other present player; a client sends to the host and then
  receives from it. The race then processes the commands of all players for that frame, so the game runs in lockstep
  at the pace of the slowest machine;
- the message "Player %d has quit out." is shown when a player disappears from the presence table;
- keys 1–4 queue a "taunt" command with values 0–3; what it sends and shows is not decoded.

## 4. Not reverse-engineered

- How the "network available" flag is determined (driver detection).
- The join handshake, session discovery and the meaning of the session number on the wire.
- How the local slot is assigned and how many players a session admits.
- Packet layout and payloads, the command encoding on the wire, timing, retransmission, desynchronisation handling.
- The session set-up / waiting screen's layout, keys and timeouts.
- What happens on the other machines when a player quits, ends the race or disconnects; how results and championship
  points (championships are unavailable anyway) are handled.
- The network catch-up table's values.

## 5. Port deviations

The port has no network mode: NETWORK OPTIONS is always hidden, NETWORK stays off, and races always use the local
single-player set-up. The SESSION NUMBER value is kept in the config untouched.
