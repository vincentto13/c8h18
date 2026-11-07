# 06 — Weapons, explosions, effects and collectables

Scope: the three craft weapons (minigun, missile launcher, booster) and their upgrades, the target
lock system, bullets, sparks, missiles and their smoke, every explosion kind with its particles,
Flares and search rings, the small effect things (fire, smoke, sand, water), the billboard sprite
table they use, the collectables (every kind, effect, respawn, sprites), chargers and the
Invincible / Invisible pickups. The motion effect of the booster and the damage bookkeeping on the
craft are in chapter 05; the target marker drawing is in chapter 08.

Units as in chapter 05: positions 8.8 cells, angles 65536 per turn, "units" = 1/256 cell unless
"engine units" (1/2048 cell) are named. Every thing has its own LCG seed
(`seed = seed × 9377 + 9439`, 16-bit thing word); "r" below means the next value of the
thing's generator.

## 1. Weapon things and upgrades

Each craft owns three weapon things, driven by its control input (chapter 05, section 4, and the
computer rules in chapter 07). Each weapon has an upgrade level 0..3 on the craft (raised by the
upgrade collectables, all set to 3 by the supercar bonus, each lowered by 1 by a repair after a
destruction).

| Weapon | Keys (original fixed table) | Ammo | Upgrade effect |
|---|---|---|---|
| minigun | Z, `,` or V | none | faster cooling (10/17/25/33 per idle tick); 8 sparks instead of 4 at upgrade 2..3 (cosmetic) |
| missile launcher | X, `.` or B | 1666 per launch | lock grows by upgrade + 1 per tick; upgrade 2: triple blast with double damage; upgrade 3: two missiles per launch |
| booster | C, `/` or Space | 3 fuel + 3 ammo per charging tick | charge grows by 2 + upgrade per tick |

The weapon triggers are set by the control step and cleared by the weapon's own processing every
tick.

## 2. Targeting and the lock

Every racing human runs the targeting step each tick before reading its keys; computers run it in
their autopilot.

1. **Search**: the nearest vehicle thing of another owner within a 3D distance of 15 cells
   (3840 units) whose bearing differs from the craft's yaw by less than the cone **and** whose
   elevation differs from the craft's pitch by less than the cone; cone = 5461 raw (30°).
2. If the result differs from the current primary target: primary target = result, lock = 0.
3. The target is dropped again for this tick (lock 0, no fallback to the next nearest) when:
   the tracking craft is a computer with the personality bit "spare weak targets" and the
   target's shield is below 2000; or the target's invincibility counter runs (finished crafts,
   Invincible pickup, start phase, the 2-tick case of chapter 04, 11.1); or the target's shield is
   below 1; or the target is not racing.
4. With a target: lock = min(lock + missile upgrade + 1, 100). Without: lock = 0.

There is no gradual decay: losing the target resets the lock to 0 at once. The Invisible pickup
does not affect targeting (an invisible craft is targeted and marked). The launcher reads the lock
as its homing probability; the HUD target marker reads it for its layout (chapter 08).

## 3. Minigun

Per tick:

```
if trigger held:
    if heat < 100: fire
    else:                                     (overheated)
        if restriction == 0: fire; restriction = 10
        else: restriction −= 1; stop sample 3 MINISHOT, play sample 4 MINBLANK (every such tick)
        heat = 100
    on fire:
        angles = the craft's view yaw and pitch, or, with a target, the heading and pitch from the
                 craft straight to the target's position (no lead)
        barrel side toggles; bullet at position moved 32/256 along (yaw ± 90°), height − 8/256
        heat += 2; bullets fired += 1; sample 3 MINISHOT (restarted per shot)
    trigger cleared
else:
    heat = max(0, heat − [10, 17, 25, 33][upgrade])
```

So the gun fires one bullet per tick for 50 ticks from cold, then one per 11 ticks while held.
The HUD's small top-right bar shows the heat (chapter 08).

### 3.1 Bullet

| Property | Value |
|---|---|
| sprite | none (bullets are invisible, no tracer) |
| half size | 32/256 |
| per tick | 16 sub-steps of 64/256 along its angles (4 cells per tick) |
| life | 128 sub-steps (32 cells, 8 ticks) |

Each sub-step: move; if the map collision test is non-zero (inside a solid block or below the
terrain) the bullet ends; if its box overlaps a thing of another owner it ends and the shooter's
"bullets hit" +1. A bullet that runs out of life vanishes silently.

When it ends by hitting something: ricochet sample 10 + (r mod 3) (RICCO1..3) at rate
75 + (r mod 51) %; then 4 sparks (8 at minigun upgrade 2 or 3) at its position, each at height
+ (r mod 48 − 24)/256 (the generator is stepped three times per spark, the third value used). The
**first spark** hands 60 damage (bullet affect) to every other owner's thing overlapping its box of
half size 1 cell — so a wall hit next to a craft also damages it. The later sparks deal nothing.
(The original writes upgrade multipliers ×1.5 / ×3 / × spark count to the bullet after the first
spark exists, so they change nothing: a hit is 60 at every upgrade.)

### 3.2 Spark

Group 2 member 4: sprite 165 + (age) (165..168), life 4 ticks, half size 1 cell, yaw += 2912 raw per
tick (cosmetic). It deals its damage on its first tick only.

## 4. Missile launcher

```
if reload > 0: reload −= 1                      (also while the trigger is held)
else if trigger held and ammo > 1665:
    ammo −= 1666
    angles as the minigun (aimed straight at the target if there is one)
    count = 2 at missile upgrade 3, else 1
    for each missile:
        homing = (r mod 100) < lock
        rockets launched += 1
        side toggles; missile at position moved 80/256 along (yaw ± 90°), height − 8/256
        missile target = the launcher's target; missile upgrade = launcher upgrade
    reload = 12
    sample 5 MISSILE
trigger cleared
```

Holding the key fires one launch per 13 ticks. Every craft pays the ammo, computers included.

### 4.1 Missile

| Property | Value |
|---|---|
| sprite | 152 (tmap 0, 160 engine units tall), static |
| half size | 64/256 |
| per tick | 4 sub-steps of 72/256 (1.125 cells per tick) |
| life | 113 sub-steps (about 31.8 cells, 28 ticks) |

Each sub-step:

1. If homing and the target exists: yaw and pitch each turn towards the target by the angle
   difference clamped to ±182 raw (1°). The target craft's "incoming missile distance" = the 3D
   distance if smaller than the value already set this tick (the viewed craft loops sample 15
   LOCKON4 while it is set; nothing else reads it).
2. Move 72/256 along (yaw, pitch).
3. Map test: inside a solid block → explode. Below the terrain: more than 18/256 below the floor →
   explode; less than 8/256 above the floor → lifted to floor + 8/256 (no explosion).
4. Box overlap with a thing of another owner → the shooter's "rockets hit" +1, explode.
5. Otherwise, on 2 of 3 sub-steps (random), a Smoke thing 1/16 cell higher (8.2).
6. life −1.

When the life drops below 1 (also by exhaustion, i.e. a missile always explodes): at missile
upgrade 2 three medium explosions at 2 cells from the missile at absolute headings 120°, 0° and
240°, each with double damage (3000); otherwise one medium explosion (1500) at the missile.
Explosions take the missile's angles.

## 5. Booster (weapon side)

```
if trigger not held, or restriction != 0, or charge >= 100, or the craft lacks the resources:
    if charge > 0 and not airborne:
        if restriction == 0:                  (release tick)
            craft gets the "booster" affect, booster strength = charge
            stop sample 8, play sample 9 BOOST
        charge −= 1
        restriction = restriction + 1 if charge > 0 else 0
else:                                          (charging)
    ammo −= 3; fuel −= 3
    charge += 2 + booster upgrade
    sample 8 POWERUP looping, rate 2 × charge + 100 %
trigger cleared
```

Reaching charge 100 releases it even while the key is held. After a release the charge drains by 1
per tick before a new charge can start. Humans can pull the trigger only while fuel > 0. The motion
effect (impulse 150/256 × charge %, then 7/256 × charge % for 70 ticks, scaled by the thrust
effectiveness) is in chapter 05. The booster creates **no** thing and has no visual effect. The
HUD draws the charge over the speed bar while it is non-zero (chapter 08).

## 6. Explosions

All explosions are things of group 2. An explosion thing deals its damage **once, on its first
process**, to every thing overlapping its box (half size below), plays sample 6 EXPLODE once
(positioned), and then spawns visible particles or Flares; the explosion thing itself has no
sprite. Every particle or Flare made by another thing is created already past its first-process
branch and with damage disabled: it is silent, deals nothing, gets no random rise and is not
deleted inside walls. Creators that should not hurt (a craft's own destruction, the dying craft's
explosions) disable the damage of the explosion thing as well.

| Member | Name | Life (ticks) | Half size | Damage | Spawns per tick |
|---|---|---|---|---|---|
| 0 | Explosion (particle) | 12 | 1 cell | 200 (only when placed by the level) | — |
| 1 | Explosion medium | 3 | 1 cell | 1500 | 6 particles + 1 Flare |
| 2 | Explosion big | 6 | 1.5 cells | 10000 | 1 Flare per cell of search ring (n mod 2) |
| 3 | Explosion massive | 3 | 2 cells | 10000 | 1 particle per cell of search rings 2n and 2n+1 |
| 22 | Flare | 20 | — | none | 1 fast particle |

Users: missiles make medium explosions; a destroyed craft makes one big explosion (no damage) at
death and medium explosions (no damage) every third tick while exploding; levels place all kinds
(level explosions get no angles, so their particles drift towards row 0).

### 6.1 Particle (member 0)

Creation: sprite 139, life 12, displacement = 150/256 along the thing's own yaw and pitch (copied
from the creator), damage 200 only if not created by another thing.

First process (level-placed particles only): inside a block or below the floor → deleted; else
EXPLODE, damage, and a rise `(r2 mod 100 + 100) × ((r1 mod 157) / 79 × 2 − 1)` (±100..199/256 per
tick; r1 drawn first).

Every tick:

```
life −= 1; below 0: ends
if horizontal displacement != 0:
    target = position + displacement (all three components)
    horizontal displacement −= trunc(displacement / 8)
    if the target is inside a solid block: horizontal displacement = 0, no move
    else move to the target
sprite += 1   (fast particles: += 2 and life −= 1 more)
height += rise; at least the floor height; rise = trunc(rise × 11 / 16)
```

Visible result: sprites 140..151 over 12 ticks (fast particles 141, 143, …, 151 over 6), drifting
along the creator's heading at 150/256 per tick and slowing by 1/8 per tick.

### 6.2 Medium explosion (member 1)

Each of its 3 ticks: 6 times { d = r AND 255, a = r; a particle at the centre moved d/256 along
heading a (pitch 0), with the explosion's angles; one more r is drawn (a value the original writes
into the explosion's own displacement, without effect) }; then one Flare at the centre. Total 18
particles and 3 Flares within one cell.

### 6.3 Big explosion (member 2)

Each of its 6 ticks n = 0..5: for every cell (dx, drow) of search ring (n mod 2) a Flare at
centre + (dx, drow) × 180 units, at the floor height there. Rings 0, 1, 0, 1, 0, 1 → 1, 4, 1, 4,
1, 4 Flares.

### 6.4 Massive explosion (member 3)

Each of its 3 ticks n = 0..2: for every cell of search rings 2n and 2n + 1 a particle at
centre + (dx, drow) × 128 units on the floor, with the explosion's angles: 5, 32, 60 particles.

### 6.5 Flare (member 22)

Creation (r drawn in this order): speed = 40 + (r mod 100); yaw = r mod 65535;
pitch = −(3640 + (r mod 12743)) (20° to 90° upwards); life 20; no sprite; displacement = speed
along (yaw, pitch).

Every tick: life −1, below 0 deleted; horizontal displacement −= trunc((v + 15) / 16); vertical
displacement −= 6, not below −100; move by the displacement (Flares never bounce and fall through
the floor); leave one **fast** particle at the new position (no displacement, no rise, no damage).
The visible result is a 20-tick arc of short fireballs, clamped to the floor by the particles.

### 6.6 Search rings

Built once at start-up from `data/srch0-0.dat` (RNC packed, 32 × 32 bytes, row-major). For ring
k = 0, 1, …: find the first byte (row-major) equal to k (ring 0's cell is the centre; all ring
starts lie in column 15 above it); append it and mark it used (−1); then trace: from the current
cell try the 8 neighbours in the direction order 2, 0, 1, 3, 4, 5, 6, 7 added to a base
(directions 0..7 = +x, +x+row, +row, −x+row, −x, −x−row, −row, +x−row, taken modulo 8); the first
unused neighbour whose byte is ≥ 0 with low nibble k is appended, becomes current and sets
`base = (base + dir) AND 6`; stop when none is found. Offsets are relative to the ring-0 cell. The
result for the rings explosions use:

| Ring | Size | Offsets (dx, drow) in tracing order |
|---|---|---|
| 0 | 1 | (0,0) |
| 1 | 4 | (0,−1) (1,0) (0,1) (−1,0) |
| 2 | 16 | (0,−2) (1,−2) (1,−1) (2,−1) (2,0) (2,1) (1,1) (1,2) (0,2) (−1,2) (−1,1) (−2,1) (−2,0) (−2,−1) (−1,−1) (−1,−2) |
| 3 | 16 | (0,−3) (1,−3) (2,−2) (3,−1) (3,0) (3,1) (2,2) (1,3) (0,3) (−1,3) (−2,2) (−3,1) (−3,0) (−3,−1) (−2,−2) (−1,−3) |
| 4 | 24 | (0,−4) (1,−4) (2,−3) (3,−3) (3,−2) (4,−1) (4,0) (4,1) (3,2) (3,3) (2,3) (1,4) (0,4) (−1,4) (−2,3) (−3,3) (−3,2) (−4,1) (−4,0) (−4,−1) (−3,−2) (−3,−3) (−2,−3) (−1,−4) |
| 5 | 36 | (0,−5) (1,−5) (2,−5) (2,−4) (3,−4) (4,−3) (4,−2) (5,−2) (5,−1) (5,0) (5,1) (5,2) (4,2) (4,3) (3,4) (2,4) (2,5) (1,5) (0,5) (−1,5) (−2,5) (−2,4) (−3,4) (−4,3) (−4,2) (−5,2) (−5,1) (−5,0) (−5,−1) (−5,−2) (−4,−2) (−4,−3) (−3,−4) (−2,−4) (−2,−5) (−1,−5) |

The file holds 749 offsets in total; explosions only reach ring 5.

## 7. Other effect things

| Member | Name | Sprite | Life | Behaviour |
|---|---|---|---|---|
| 5 | Fire | 11 (static) | 15 | level "steam" emitter: one Smoke fire per tick, 0..30/256 above it |
| 7 | Smoke | 60 → 69 (+1 per tick, capped) | 6 | missile trail; created 1/16 above the missile with its angles and speed 16 + (r AND 15); first tick sets its displacement to −speed along its angles; moves by it every tick (drifts back along the flight path, 16..31/256 per tick); deleted inside walls |
| 8 | Smoke fire | 5 → 20 (+1 every second tick, capped at 20) | 22 | burning crafts and steam: height += vertical speed first; then while the speed is below 20/256 it grows by 16 + (r mod 16) (/256); deleted when the map test is non-zero |
| 20 | Sand | 24 → 35 (+1 per tick) | 12 | terrain spray (chapter 05, 3.t) |
| 21 | Water | 40 → 51 (+1 per tick) | 12 | terrain spray |

Sand / Water creation: on the floor; yaw = craft yaw + 16384 + (r mod 32767) (somewhere in the half
circle behind the craft); pitch 16384 (straight down: the first move snaps it to the floor and
flattens it); speed 100/256; displacement = the craft's move of this tick. Process: move by
(yaw, pitch, speed) plus the displacement (which loses 1/64 per tick); inside a block → deleted;
below the floor → put on the floor, pitch 0; free → +5/256 height; speed × 12/16.

Burning-craft smoke and the dying craft's explosions are listed in chapter 05 (section 4). Other
group-2 members named in the member table — 6 Hole, 9 Light, 10 Earthquake, 11 Tremor, 12 Gas,
13 Altitude, 14 Shade, 15..19 area morphs — are level mechanics (see the level chapter) or
undocumented. **There is no mine or other craft weapon.**

## 8. The billboard sprite table

179 records; the game keeps per record: the first tmap image (`data/tmaps.dat`), a height in engine
units (2048 per cell; the drawn width follows the image's aspect), a blend byte, and a mode byte
copied from the tmap image's header byte 1 at start-up (images 13..28 have mode 24 and are 4-frame
animations: the thing's animation frame is added to the tmap index; images 0..12 and 29..46 have
mode 21; image 47 has mode 1). Blend byte: 0 opaque, 2 = `blend[src][dst]`, 3 = `blend[dst][src]`
(chapter 08).

| Sprite ids | tmap | Heights (engine units, in id order) | Blend | User |
|---|---|---|---|---|
| 0, 1, 2, 3 | 0 | 640, 640, 1600, 160 | 0 | none found |
| 4 | 39 | 1440 | 0 | tree (unused by the levels) |
| 5..20 | 17 | 160, 320, …, 2560 (step 160) | 2 | Smoke fire (5..20); Fire (11) |
| 21 | 17 | 200 | 2 | none found |
| 22..37 | 13 | 160 … 2560 (step 160) | 2 | Sand (24..35) |
| 38..53 | 25 | 160 … 2560 (step 160) | 2 | Water (40..51) |
| 54..69 | 17 | 312, 384, …, 1392 (step 72) | 2 | Smoke (60..69) |
| 70 + 4k .. 73 + 4k, k = 0..13 | 29 + k | 1280, 960, 640, 320 | 0, 0, 3, 2 | collectables: base image and the three shrinking variants |
| 126..132 | 0..6 | 800 each | 0 | none found |
| 133..135 | 7..9 | 800 | 3 | none found |
| 136..138 | 10..12 | 800 | 2 | none found |
| 139 | 0 | 1200 | 0 | Explosion particle (creation image, never shown) |
| 140 | 1 | 800 | 0 | particle frame 1 (white fireball) |
| 141 | 2 | 1200 | 0 | particle |
| 142..145 | 3..6 | 1600 | 0 | particle |
| 146..148 | 7..9 | 1600 | 3 | particle |
| 149..151 | 10..12 | 1600 | 2 | particle |
| 152..158 | 0..6 | 160 | 0 | missile (152) |
| 159..161 | 7..9 | 160 | 3 | none found |
| 162..164 | 10..12 | 160 | 2 | none found |
| 165..168 | 43..46 | 160 | 0 | bullet sparks |
| 169..171 | 43..45 | 160 | 0 | none found |
| 172 | 46 | 160 | 3 | none found |
| 173, 174 | 44, 45 | 160 | 3 | none found |
| 175..177 | 46 | 160 | 2 | none found |
| 178 | 47 | 480 | 2 | craft shadow (64 × 32 ellipse, chapter 08) |

## 9. Collectables

Collectables are things of group 5 placed by the level entity table (entity type 5, sub-type =
member) or thrown by destroyed crafts (chapter 05, 7.2).

| Member | Name | Sprite (tmap) | Effect when taken | Taken only if | Banner |
|---|---|---|---|---|---|
| 0 | Invincible | 118 (41) | invincibility counter 100 | see Known gaps | — |
| 1 | Invisible | none | invisibility counter 250 | see Known gaps | — |
| 2 | Extra shield | 94 (35, green small) | shield + 2500, bullet-hit counter − 10 | shield < 10000 | 15 EXTRA SHIELD |
| 3 | Shield full | 98 (36) | shield = 10000, hit counters 0 | shield < 10000 | 18 MAX SHIELD |
| 4 | Double shield | 102 (37) | shield = 20000, hit counters 0 | shield < 20000 | 21 DOUBLE SHIELD |
| 5 | Extra ammo | 82 (32, yellow) | ammo + 2500 | ammo < 10000 | 16 EXTRA AMMO |
| 6 | Ammo full | 86 (33) | ammo = 10000 | ammo < 10000 | 19 MAX AMMO |
| 7 | Double ammo | 90 (34) | ammo = 20000 | ammo < 20000 | 22 DOUBLE AMMO |
| 8 | Extra fuel | 70 (29, purple) | fuel + 2500 | fuel < 10000 | 14 EXTRA FUEL |
| 9 | Fuel full | 74 (30) | fuel = 10000 | fuel < 10000 | 17 MAX FUEL |
| 10 | Double fuel | 78 (31) | fuel = 20000 | fuel < 20000 | 20 DOUBLE FUEL |
| 11 | Minigun | 106 (38) | minigun upgrade + 1 | upgrade < 3 | 23 MINIGUN UPGRADED |
| 12 | Rocket gun | 110 (39) | missile upgrade + 1 | upgrade < 3 | 24 MISSILE UPGRADED |
| 13 | Booster | 114 (40) | booster upgrade + 1 | upgrade < 3 | 25 BOOSTER UPGRADED |

Image 42 (a grey hexagon, sprites 122..125) is not used by any collectable.

Rules:

- Collision box 2 × 2 × 2 cells (half size 1 cell) standing on the surface; craft half size 1/8 cell.
- Each tick, a visible collectable is taken by the **first racing craft (slot order) overlapping
  it that can use it** (the "taken only if" column). The effect is handed to the craft as an affect
  bit and applied in its next post-process (chapter 05, 3.q). Taking plays sample 14 SWITCH (the
  sample named PICKUP, 18, is not used in the race).
- A level collectable then hides for **2000 ticks** (100 s) and reappears in place.
- A thrown collectable lives **200 ticks** and is gone once taken; during its last 3 ticks it shows
  sprite base + 1, + 2, + 3 (smaller, translucent).
- Base images are 1280 engine units = 0.625 cell tall, drawn as billboards standing on their
  position (chapter 08).

## 10. Chargers

Rectangular charger pads from the level's region table (kind 1 shield, 2 fuel, 3 ammo; kind 4 is
the race-start region). A craft whose cell lies in a pad is refilled by 200 per tick while the value
is below 10000 (shield only while above 0); rules, banners and the REFUEL loop are in chapter 05,
3.q. Computer players head for and stop on pads when low (chapter 07).

## 11. Invincible and invisible

- **Invincible** pickup: invincibility counter = 100 ticks (5 s). While it runs the craft takes no
  damage and cannot be targeted; the target marker of a craft that already holds it as a target
  blinks its shield bar (chapter 08). Other sources of the same counter: chapter 04, 11.1.
- **Invisible** pickup: invisibility counter = 250 ticks (12.5 s). The craft's model and shadow are
  not drawn; it is still targetable, on the minimap and marked by the target marker.

## Port deviations

- The reference recreation uses one shared generator for all effect things instead of a seed per
  thing (the seed of a new thing is its thing index in the original).
- Thrown collectables are placed on a ring instead of being thrown.
- The reference does not place Invincible / Invisible collectables from the level entities.

## Known gaps

- Per-thing generator seeding: the original seeds each thing's generator with its thing index at
  creation; exact random sequences therefore depend on thing-slot allocation, which is not specified.
- The conditions under which the Invincible and Invisible collectables can be taken, their banner
  messages (if any) and whether the shipped levels place them are not documented.
- Whether the minigun's restriction counter is reset when the trigger is released while overheated
  is not documented (the reference keeps it).
- The behaviour of group-2 members Hole, Light, Earthquake, Tremor, Gas and Shade is not documented.
- The two unnamed 16-bit fields of each sprite-table record are unknown (not needed for drawing).
- The 4-frame animation step of the mode-24 images (13..28) for smoke / sand / water was not found;
  the reference advances the frame every tick.
