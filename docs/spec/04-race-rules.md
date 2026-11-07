# 04 — Race rules

Scope: everything that turns a loaded level into a race and back into results: who takes part, the
grid, the countdown and the start lights, checkpoints, laps, placings, timers, the finish and the
post-finish phase, the classification of unfinished crafts, the statistics content, demo races,
difficulty, the catch-up ("behind factor") rules, pause and race-end commands, the per-craft
counters (invincibility, invisibility, killer), the in-race banner / big-message system and the
cracked-glass HUD damage rule. The flight model is in chapter 05, weapons and pickups in 06, the
computer drivers in 07 and all drawing in 08.

## Conventions used in this chapter

| Term | Meaning |
|---|---|
| tick / frame | One pass of the race loop. The original runs the simulation exactly once per drawn frame (no limiter); all its timers count these frames. The design rate is 20 per second (50 ms), so "N ticks" ≈ N/20 s. |
| race frame | Per-player frame counter of the race, counted only while the race runs (not while paused). Its bit 2 drives every 4-frame blink, its bit 5 the race-end timing. |
| cell | One map cell. Positions are 8.8 fixed point cells (256 = 1 cell). |
| angle | 16-bit, 65536 per turn, 0 = towards decreasing map row ("north"), clockwise positive. |
| LCG | `seed = seed * 9377 + 9439` (32-bit wrap) — the one generator formula the game uses everywhere (each user has its own seed). |
| full | Fuel, shield and ammo are signed 16-bit values; 10000 = a full bar, "double" pickups go to 20000. |

## 1. Race setup

### 1.1 Participants

The game state holds **8 player records**, one per grid slot; slot 0 is the local human in a single
race or championship. Computer players fill slots 1..7.

| Situation | Humans | Computer crafts |
|---|---|---|
| Single race / championship, COMPUTER PLAYERS on | 1 (slot 0) | 7 (slots 1..7) |
| COMPUTER PLAYERS off | 1 | 0 |
| Demo race (attract mode) | 0 | 8 (slots 0..7) |

(The network game is not covered by this specification.)

### 1.2 Craft types, colours and names

Six craft types exist; each comes in 8 team colours (48 models,
`objects/data/<stem>0-<colour>.dat`).

| Type | Stem | Name | Weight | Armour | Speed | Firepower |
|---|---|---|---|---|---|---|
| 0 | car | DART | 5 | 4 | 6 | 5 |
| 1 | tank | BULWARK | 6 | 5 | 3 | 6 |
| 2 | jugga | MAULER | 6 | 5 | 4 | 5 |
| 3 | jet | KESTREL | 6 | 4 | 6 | 4 |
| 4 | bike | SCYTHE | 4 | 3 | 8 | 5 |
| 5 | skim | SKIMMER | 4 | 4 | 8 | 4 |

The four numbers are **display values only** (the craft-select bars). Every craft gets the same
flight, weapon and armour constants (chapter 05): the type chooses the model, nothing else.

| Colour | Team name (craft panel) | Colour string | Minimap marker palette index |
|---|---|---|---|
| 0 | SALT SAINTS | WHITE | 1 |
| 1 | NIGHT SHIFT | BLACK | 0 |
| 2 | RUST HOUNDS | RED | 15 |
| 3 | SULPHUR | YELLOW | 179 |
| 4 | COLD IRON | BLUE | 160 |
| 5 | FLARE CREW | ORANGE | 250 |
| 6 | STATIC | PURPLE | 198 |
| 7 | OCTANE | AQUA-GREEN | 181 |

Slot assignment (made by the front end when a race starts):

- Slot 0 (human): the player's chosen colour and type and the player's name.
- Slot k ≥ 1 (computer): colour = (k + player colour) mod 8, type = (k + player type) mod 6.
- In a demo race slot 0 is a computer too and uses the same formula with k = 0.
- Computer names by slot: 0 SCOPER, 1 BARNSY, 2 SHUNTLY, 3 COPSE, 4 MANNY, 5 MCLALIN, 6 MAD,
  7 ATROW (slot 0's name appears only in demo races).

### 1.3 Laps

- Single race: the track's laps byte from the config's track record (1..50, changed with Up/Down
  on the track carousel; defaults 11, 8, 9, 8, 9, 5 for tracks 1..6).
- Championship: always 11, 8, 9, 8, 9, 5 for tracks 1..6.
- Internally every craft stores **race laps = laps + 1**: the first crossing of the start/finish
  line only starts lap 1.

### 1.4 Grid and start positions

- The level's region table has a "race start" region (region kind 4); the start cells are the map
  cells inside it whose floor texture is the start-grid texture (texture 122).
- Every craft starts on a cell centre, hovering at the surface below it + 0.5 cell (128/256), at full
  fuel, shield and ammo, upgrades 0, pitch and roll 0.
- Start heading: the start positions face the start/finish line (see Known gaps for the exact
  heading and slot-to-cell order).

## 2. Countdown and start lights

The race starts with a **blocking countdown** that does not advance the simulation: for every
drawn frame it runs the camera on the viewed player, draws the 3D view and the start lights and
presents. The countdown is timed by the 100 Hz system clock, not by frames:

| Phase | Duration | Lamps (PANEL sprites, left to right) | Sound at the end of the phase |
|---|---|---|---|
| 1 | 101 clock ticks (1.01 s) | all dark: 281, 283, 285 | START1 (sample 25) |
| 2 | 101 clock ticks | lamp 1 lit: 282, 283, 285 | START1 (25) |
| 3 | 101 clock ticks | lamp 2 lit: 281, 284, 285 | START2 (26) |
| end | one frame | lamp 3 lit: 281, 283, 286 | race tune starts |

Total 3.03 s. Each lamp is a PANEL pair (off, lit): 281/282, 283/284, 285/286. During the
countdown no HUD, no minimap and no messages are drawn. At its end every racer gets its
"race started" flag: from the next frame on the crafts accept control (human keys, computer
autopilot). There is no palette fade at the race start.

After the countdown, **lamp 3 (281, 283, 286) stays on screen** every frame until the lap-timer
flag goes on (section 4.1), i.e. until the first craft — any craft, often a computer starting ahead —
crosses the start/finish line. Only then do the HUD and minimap replace the lights (chapter 08
gives positions).

## 3. Checkpoints, laps and placings

### 3.1 Checkpoints

Checkpoints come from the level's entity table (entity type 1, sub-type 5). Each is an
axis-aligned box: its corner is at the entity cell's centre, half sizes
`(offset_x + 1) / 2` and `(offset_y + 1) / 2` cells, the box centre is corner + half size. The
entity's value orders them along the lap; **value 0 is the start/finish line**.

Each craft holds a *target* checkpoint, starting with the start/finish line. Per tick, after its
movement, the craft's position is tested against the checkpoint boxes; only entering the target
counts (others are ignored, so short-cutting past a checkpoint does not progress the craft). On
reaching the target:

1. The previous and next checkpoint are remembered (the recovery truck uses the last two passed,
   chapter 05).
2. The next target is the checkpoint with value + 1; if none exists, the smallest larger value;
   if none, the start/finish line.
3. If the reached checkpoint is the start/finish line, the lap logic runs (3.2).

The craft also keeps its 2D distance to the current target (used by the placings).

### 3.2 Crossing the start/finish line

| Step | Effect |
|---|---|
| lap counter > 0 before the crossing | the running lap time is stored as the completed lap (lap list), the best lap is updated if smaller (or none yet), the lap time restarts at 0 |
| always | lap counter += 1; the "lap completed" event bit is set for one tick (used by the supercar rule, chapter 05); the global lap-timer flag goes on |
| lap counter reaches race laps and the craft has no finish place yet | the craft finishes (section 5) |

There is no finish check on later crossings: laps, lap times and the best lap keep counting after
the finish.

### 3.3 Placings

Recomputed every tick after all things moved. Order (1 = leader):

1. Finished crafts first, by finish place.
2. Then by progress = lap counter × number of checkpoints + value of the target checkpoint
   (when the target is the start/finish line and the lap counter is > 0, the number of
   checkpoints is used instead of 0), larger first.
3. Then by the distance to the target checkpoint, smaller first.
4. Ties: lower slot first.

The current place feeds the HUD, the minimap leader marker, the catch-up factor (section 9), the
supercar rule and the classification (6.3).

## 4. Timers

### 4.1 The lap-timer flag

A global flag that turns on (for every racer at once) in the tick in which **any** active racer's
lap counter first becomes non-zero. It gates the HUD and minimap (chapter 08) and the timers.

### 4.2 Counters per craft

| Counter | Runs | Limit | Used for |
|---|---|---|---|
| lap time | +1 per tick while the lap-timer flag is on | capped at 9999 | HUD lap list, best lap |
| race time | +1 per tick while the lap-timer flag is on, also after the finish | none | finish time, statistics |
| finish time | copied from the race time when the craft finishes | — | results |
| race frame | +1 per frame while not paused | — | blink, race-end timing, demo end |

**All times are raw tick counts.** The HUD, the track panel, the statistics and the track records
print these numbers directly; nothing converts them to seconds.

### 4.3 Best lap and track records

- Best lap: the smallest completed lap time (also laps completed after the finish).
- Track records (6 records in the config, one per track: best lap + holder name, best race + holder
  name, laps byte) are updated after a **championship** race only: best lap = smallest best lap of
  the drivers, accepted only below 9999; best race = smallest race time, accepted only below
  laps × 9999. A stored record of 0 can never be beaten in the original (see Port deviations).

## 5. The finish

When a craft's lap counter reaches race laps for the first time:

- finish place = its current place (1..8);
- finish time = race time;
- its **hold counter** = 100 (ticks it keeps the race open, 5.2);
- from now on its **invincibility counter is set to 10 every tick** (no damage, nobody targets it);
- the HUD/message "finished" event of the viewed craft fires (12.3: big message FINAL LAP with a
  place fanfare).

The race does **not** end with the human's finish.

### 5.1 Finished crafts drive on

Every tick, for each finished craft whose hold counter is still > 0: its waypoint target is set to
the nearest waypoint, the **locked autopilot** is switched on (human keys can no longer take it back)
and the hold counter decreases by 1. A finished human therefore flies on exactly like a computer
player (waypoints, targeting and shooting at the player's difficulty level, booster use, pickups,
charger stops), except that the computers' random shortcut junction choice stays the computers'.
Its catch-up factor target becomes 130 (section 9). Laps and timers keep counting.

### 5.2 End of the race

1. When **every** active racer has a finish place **and** a hold counter of 0, every racer gets the
   "race over" flag. This happens 100 ticks after the last craft finished. There is no timeout
   after the first finisher.
2. At the start of each frame, if the viewed player's craft has the "race over" flag (or the
   end-race request flag of right Shift+E is set) and `(race frame AND 32) == player number × 4`
   (player 0: bit 5 of the race frame clear), the "end race" command is queued — so up to 32
   frames more may pass.
3. The end-race command (also issued at once by **Escape**, finished or not) sets the game flag
   "race ends" and puts the player on the autopilot; the race loop finishes the current frame and
   stops.

Escape while paused continues the race instead (section 10).

### 5.3 Classification of unfinished crafts

After the race loop, **only if the local player's craft has finished**:

```
base = race time of the craft with the highest finish place (first in slot order)
seed = base;  t = base
for place = 1 .. 8:
    for each other active craft (slot order) still at finish place 0 whose current place == place:
        finish place = place
        seed = seed * 9377 + 9439          (32-bit)
        finish time = t
        t = t + (seed mod 250)
```

(The seed is stepped before its first use; the first classified craft gets exactly `base`.)
Then each player gets its results; a player with finish place 0 gets no points and is "not
classified". Consequences:

- Escape before the own finish: nobody is classified, no statistics are shown.
- Escape (or the normal end) after the own finish: everybody is classified.

### 5.4 What the player sees after the finish

The render driver checks the viewed craft every frame. When it is **not racing** (destroyed,
tumbling, waiting for or carried by the recovery truck) **or on the autopilot after its finish**:

- the camera is forced to the level-camera view (action 6) on the player's own craft every frame;
  view, follow and zoom keys do nothing;
- no start lights, no HUD, no minimap ("bare view");
- while the killer counter (11.3) is set: the line `KILLED BY <name>` (string 174, a space, the
  killer's player name) in font slot 4, centred at half screen height, every frame, as long as the
  craft is down;
- then the message routine runs: banner and big message only while finish place and killer are both
  0 (otherwise the banner's sample is stopped); after them, in every branch, the **finishers list**.

Finishers list: for place = 1..8, for each active player whose finish place equals it, draw PANEL
digit sprite `151 + place` at virtual (2, y) and the player's name in the red PANEL capitals
(glyph base 201) at (2 + width(digit) + 8, y + 4); y starts at 120 and grows by
height(PANEL 139) + 2 per row drawn. The list is drawn in every non-countdown frame, so it also
appears under the HUD while the player is still racing and others have finished.

No music change happens at the finish; the race tune plays on.

## 6. Results and statistics content

After a race that was not a demo, for each player with a finish place (classified):

| Value | Definition |
|---|---|
| race time | the finish time |
| average lap | finish time / (race laps − 1) |
| best lap | best lap (laps after the finish can still improve it) |
| kills / deaths | counted during the race (a kill counts for the hitter when the shield runs out, 11.3) |
| hit accuracy | `((bullets hit + rockets hit × 10) × 100) / (rockets launched × 10 + bullets fired)`, 0 unless both rockets launched and bullets fired are non-zero |
| race points | by place 1..8: 20, 16, 13, 11, 9, 7, 6, 5 (0 for no place) |
| assessment | see below |

Assessment ("god factor" gf):

```
gf = clamp(bonus score, 0, 3000)          (supercar awards +1000, spin bonuses +180..720, chapter 05)
gf += hit accuracy × 20
gf += clamp((kills − deaths) × 1000, −10000, 10000)
gf += (8 − place) × 700
gf = clamp(gf, 0, 19999)
assessment = clamp((19999 − gf) / 1000 − (3 − difficulty), 0, 19)
assessment = 20 if the player's cheat flag is set
```

The text is UI string 149 + assessment: 149 IMMORTAL, 150 HARDWIRED, 151 OUTTA CONTROL, 152 DEMON,
153 DEADLY, 154 WRECKER, 155 DANGEROUS, 156 TAILGATER, 157 ANTISOCIAL, 158 DUST DEVIL,
159 UPWARDLY MOBILE, 160 MEDIOCRE, 161 TOO SLOW, 162 ENDANGERED SPECIES, 163 TARGET PRACTICE,
164 BACK MARKER, 165 CANYON KISSER, 166 VICTIM, 167 SCRAP, 168 SMEAR, 169 CHEATING SUCKS.

STATISTICS screen content (shown when the local player is classified; layout belongs to the
front-end chapters): one column per driver (8 columns of 78 virtual units; names of unclassified
drivers replaced by a space), rows HIT ACCURACY `n%`, KILLS, DEATHS, AVERAGE LAP, BEST LAP,
RACE TIME, RATING `(20 − assessment)/20`, POSITION, then the player's assessment text in the large
font. Race points are added to the championship totals after every classified race, **also after a
single race** (an original quirk).

## 7. Death match

String 33 "DEATH MATCH" exists in the UI string table, but the rules of a death-match mode were not
decoded (see Known gaps). A recreation should treat it as absent.

## 8. Demo race (attract mode)

Started by the front end after 15 s idle on the root menu (alternating with the high-score table,
demo first). Differences from a normal race:

| Aspect | Demo race |
|---|---|
| crafts | 8 computer crafts; slot 0 uses the computer formula and name SCOPER |
| track | the current race-number track (the one the track select shows) |
| camera | every frame the trackside camera (action 6) on computer player `(race frame / 200) mod 7 + 1`, i.e. a different craft every 200 frames |
| overlays | no start lights, HUD, minimap, banners, big messages or finishers list |
| keys | the race key tables are not run (no driving input, no pause, no Escape command, no cheats); R toggles the screen resolution; C is swallowed |
| end | any other key, race frame 3000 (150 s at 20 Hz), or the normal end rule |
| afterwards | no results, no statistics, no points; back to the main menu |

## 9. Difficulty and the catch-up ("behind") factor

### 9.1 Difficulty

The DIFFICULTY menu sets 0..3 (default 0 in a fresh config). When a race starts every player
record gets an effective difficulty: the setting in a single race; `min(race number, setting)` in a
championship (race number 0..5, so the first championship race is always played at 0). What it
changes:

| Effect | Where |
|---|---|
| computer catch-up target + difficulty × 5 / 3 | 9.2 |
| computer weapon upgrade level = difficulty whenever they fire | chapter 07 |
| computer booster use needs difficulty ≠ 0 | chapter 07 |
| assessment − (3 − difficulty) | section 6 |
| a finished human on the autopilot shoots at its own difficulty | 5.1 |

There is no difficulty table in the flight model.

### 9.2 Behind factor

Every craft has a behind factor (percent, starts at 100). Once per tick, at the end of its flight
update, it steps by **1** towards a target:

| Craft | Target |
|---|---|
| finished (any) | 130 (set directly, not stepped) |
| human | 118 + 2 × place (+10 while the supercar bonus runs) |
| computer | base + difficulty × 5 / 3 + 4 × place − 4 (integer division), base 120; personality "fast" uses 125, or 131 when its place is > 3; personality "no catch-up" drops the `difficulty × 5 / 3 + 4 × place − 4` part |

Effects (chapter 05): the craft's per-tick move gets an extra `momentum/4 × (behind − 100)/100`,
and the hover gravity and down-clamp of the sensor points scale with behind/100. So the leading
human drives at 120 % and an 8th-placed human at 134 % of the base speed.

The same step also sets the thrust effectiveness from the shield (chapter 05).

## 10. Pause

P toggles pause (single and championship races; ignored in demo races and in spectator mode).
While paused:

- no thing is processed: the race, its timers, the race frame and all effects stand still; the
  engine sound mixer stops; entering the pause ends every playing sample (music continues);
- the 3D view is still drawn, using the trackside camera on computer player
  `(race frame / 200) mod 7 + 1` (a fixed craft, because the race frame stands still), and no HUD
  and no messages;
- a pause menu runs in the front-end box (rect 16, 39, 288, 121 in 320x200 units; menu y0 78,
  height 242): SOUND OPTIONS (music / effects volume) and DETAIL OPTIONS (shading, sky, texture
  mapping, screen resolution). Opening the pause parks the box above the screen, closes the
  submenus, puts the cursor on SOUND OPTIONS and restarts the typewriter;
- Escape continues (it is turned into the pause toggle), P continues;
- the banner and big-message counters do not run down.

## 11. Per-craft counters

### 11.1 Invincibility counter

While > 0 the craft takes no damage (no damage, no damage sound) and no craft can target it. It
decreases by 1 per tick.

| Source | Value |
|---|---|
| Invincible collectable | 100 |
| until the craft first crosses the start line: set again every tick | 100 (so it lasts until 100 ticks after its first crossing) |
| after a repair reset that followed a destruction (and at vehicle init) | 100 |
| affect flag "bit 5" (see Known gaps: described as a bullet hit) | 2 |
| finished: every tick | 10 |

### 11.2 Invisibility counter

Invisible collectable: 250 ticks. While > 0 (decreasing by 1 per tick) the craft's model and its
shadow are not drawn. Nothing else reads it: an invisible craft is still targeted, still on the
minimap and still gets a target marker.

### 11.3 Killer

When a craft's shield runs out while it is racing and the damage of that tick has a known hitter,
the killer = that hitter (stored as player number + 1); the hitter's kill count +1 and its "kill"
event bit (supercar rule) are set; the victim's death count +1. A death without a hitter (out of
fuel, level explosions) leaves the killer unchanged (normally none). The killer is cleared when the
recovery reset (chapter 05) puts the craft back into the race, and at vehicle init. While set it
suppresses banners and big messages and shows `KILLED BY` (5.4).

## 12. Banners and big messages

Two independent state machines, stepped **once per frame for the viewed human player**, after the
things moved, and drawn by the HUD message routine. Both are stepped and drawn only while the
player's finish place is 0 and its killer is 0 (else the banner's sound is stopped).

### 12.1 Banner messages

Each message is a record `{string index, priority, repeat byte, sample}`; the message id is the UI
string index (0..33). All priorities are 10.

| Id | Text | Repeat | Sample | When requested |
|---|---|---|---|---|
| 1 | DEAD MEAT | 0 | — | the craft is destroyed (shield ≤ 0) |
| 2 | FUEL LOW | 0 | 17 WARNING (loops while shown) | fuel warning counter becomes 1 (fuel 1..4000) |
| 3 | SHIELD LOW | 0 | 17 WARNING | shield warning counter becomes 1 |
| 4 | AMMO LOW | 0 | 17 WARNING | ammo warning counter becomes 1 |
| 5 | FUEL EMPTY | 0 | — | fuel 0: on the first empty tick and again every 200 ticks while coasting |
| 6 / 7 | SHIELD EMPTY / AMMO EMPTY | 0 | — | not documented (Known gaps) |
| 8 / 9 / 10 | FUEL / SHIELD / AMMO RECHARGING | 2 | — | a charger starts refilling that value |
| 11 / 12 / 13 | FUEL / SHIELD / AMMO FULL | 0 | — | the charger stops refilling and the value is full |
| 14 / 15 / 16 | EXTRA FUEL / SHIELD / AMMO | 0 | — | collectable taken |
| 17 / 18 / 19 | MAX FUEL / SHIELD / AMMO | 0 | — | "full" collectable taken |
| 20 / 21 / 22 | DOUBLE FUEL / SHIELD / AMMO | 0 | — | "double" collectable taken |
| 23 / 24 / 25 | MINIGUN / MISSILE / BOOSTER UPGRADED | 0 | — | upgrade collectable taken |
| 26 / 27 | AUTO PILOT ON / OFF | 0 | — | not documented |
| 28 / 29 | AUTO REPAIR / AUTO REFUEL | 0 | — | not documented |
| 30 | CALLING REPAIR | 0 | — | the craft calls the recovery truck after running dry |
| 31 | AWAITING REPAIR | 1 | — | not documented |
| 32 | REPAIRING | 0 | — | not documented |
| 33 | DEATH MATCH | 0 | — | not documented |

**Request rule.** A request is accepted when its priority ≥ the current priority **and** (it is a
different message **or** its repeat byte is 1):

| Current state | Result |
|---|---|
| 0 idle | state 1 (opening), sweep counter 0 |
| 1 opening, 2 shown | request lost |
| 3 shown | state 2 with counter 2 (current sample stopped) |
| 4 shown, priority 0 | state 2 with counter 40 (current sample stopped) |
| 5 closing | state 1, keeping the current sweep counter (reopens from the current width) |

**Per-frame step.**

| State | Draw | Step |
|---|---|---|
| 1 opening | sweep at width index c | c += 1; after c = 5: state 2, counter 10 |
| 2 shown | caps + text | play the message's sample; counter −1 (not while paused); below 0: state 3 (state 4 if repeat == 2), counter 40 |
| 3 shown | caps + text | counter −1; below 0: state 4, counter 4 |
| 4 shown | caps + text | priority = 0; counter −1 (not while paused); below 0: state 5, counter 5 |
| 5 closing | sweep at width index c | stop the sample; c −= 1; below 0: idle |

Resulting durations: open 6 frames, shown 11 + 41 + 5 frames (charger messages, repeat 2: 11 + 5),
close 6 frames. Half-widths of the sweep by index 0..5: 30, 60, 90, 120, 150, 180 virtual units
(drawing in chapter 08). The sample of a message plays from its first shown frame until the
closing sweep or a replacing request stops it.

### 12.2 Big messages

| Id | String | Text |
|---|---|---|
| 1 | 172 | FINAL LAP |
| 2 | 189 | SUPER CAR |
| 3 | 191 | 720! |
| 4 | 192 | 360! |
| 5 | 193 | 180! |
| 6 | 173 | WRONG WAY |

A request (always accepted unless the state is above 5, which never happens) stops the previous
big message's sample and sets state 2 with counter 50. Per frame (not while paused): counter −1,
below 0 off. Drawn in font slot 4 centred at virtual y = H/2 while `(counter >> 2) AND 1` is 1: 24 of
the 50 frames visible, 4 on / 4 off, starting with 4 frames off.

### 12.3 Big-message requests

Checked every frame for the viewed craft in this order; a later request in the same frame replaces
an earlier one:

| Order | Condition | Big message | Sample (once) |
|---|---|---|---|
| 1 | the "finished / final lap" vehicle flag set by the lap logic | 1 FINAL LAP | 29 STUNT720 / 28 STUNT360 / 27 STUNT180 when in place 1 / 2 / 3 |
| 2 | supercar awarded this tick | 2 SUPER CAR | 29 STUNT720 |
| 3 | spin flag "720!" this tick | 3 720! | 29 STUNT720 |
| 4 | spin flag "360!" | 4 360! | 28 STUNT360 |
| 5 | spin flag "180!" | 5 180! | 27 STUNT180 |

WRONG WAY's request site is not documented. The spin and supercar flags are explained in chapter 05.

## 13. HUD cracked-glass damage

The craft keeps a **bullet-hit counter**: +1 on every damage tick whose damage includes a bullet
(spark) hit; −10 per Extra Shield collectable (not below 0); reset to 0 by Shield Full / Double
Shield collectables, by the repair after a destruction, and by a shield charger at full shield;
while a shield charger refills it is scaled by `(16 − shield × 16 / 10000) / 16` each tick. The
number of cracks is `n = counter / 10`.

The HUD keeps 16 crack slots `{x, y, state}` (state 0 free, 1 waiting, 2 shown) and one 32-bit LCG
seed. Per frame, before anything else of the HUD is drawn:

```
for i = 0 .. 15:
    if i >= n: slot.state = 0; continue
    if slot.state != 0: continue                         (keeps its place)
    seed = LCG(seed); slot.x = seed mod (640 − w)
    seed = LCG(seed); slot.y = seed mod (480 − h)        (480 also in 320x200)
    slot.state = 1 if some slot in state 2 lies closer than w (integer square root of the
                 squared distance, both deltas taken as signed 16-bit) else 2
draw every state-2 slot: PANEL sprite 230 at virtual (x, y), translucent through the blend table,
skipped when it would leave the virtual screen
```

w, h = virtual size of PANEL 230. The integer square root is Newton's method from the start guess
`floor(sqrt(2)^(e + 1))`, e = index of the highest set bit: repeat `q = n / b; if q >= b return b;
b = (b + q) >> 1`. The cracks are drawn only while the player's view-slot word is 0 (see Known gaps).

## Port deviations

- Fixed 20 Hz simulation tick with render interpolation; the original simulates once per drawn frame.
- The countdown is the first 60 simulation ticks (3 s) with the crafts held on the grid (input
  cleared), lights changing at thirds, instead of the blocking 3 × 101 clock-tick loop.
- Track records of 0 are treated as unset (the original could never beat a zero record).
- A new race always starts outside spectator mode (the original's spectator flag survives races).
- Without a recovery truck in the level, a craft waiting for recovery is put back on the nearest
  waypoint after 100 ticks (refilled after a destruction, +2000 fuel otherwise).
- The port raises only the banner messages listed with a "when" above, in call order rather than in
  the original's request-table order, and does not raise FINAL LAP or WRONG WAY.
- The demo race uses the championship lap counts.

## Known gaps

- The original's start-position search was not decoded: slot-to-cell order and the exact start
  heading are a recreation choice (the reference uses the start cells in row-major order, front row
  first, slot k on cell k, heading towards the start/finish checkpoint).
- Whether the lap time is reset at a craft's very first crossing of the line (lap counter 0 → 1)
  is not confirmed; the reference does not reset it there, so a craft starting behind the leader has
  a slightly longer first lap time.
- The placing tie-break rules were not audited instruction by instruction.
- The request sites of banners 6, 7, 26..29, 31..33 and of the big message WRONG WAY are not
  documented, nor the per-pickup message bits and player counters that the original's request list
  also reads; the original requests banners in its table order (exact order unverified).
- The flag that triggers FINAL LAP + place fanfare is described as "set on the last lap" by the
  lap/finish code; whether it fires when the final lap starts or at the finish is not pinned down.
- The source of the 2-tick invincibility is ambiguous: the ledger calls the affect flag a bullet
  hit (so a craft just hit by a bullet cannot be targeted for 2 ticks), the reference recreation
  treats the same flag as the shield-charger flag and applies it only while the craft is being
  repaired.
- The view-slot word that gates the cracked-glass statics (set to 0..3 by keys 1..4, command 30)
  was only partly understood; the reference draws the cracks whenever the HUD is shown.
- Death-match rules are unknown.
- The COMPUTER PLAYERS option is treated as on/off; its exact value range was not documented.
- The demo race's lap count is not documented.
- The exact interleaving of the hold-counter countdown with the finish in the same tick follows the
  reference (placings run at the start of each tick, before the crafts move).
