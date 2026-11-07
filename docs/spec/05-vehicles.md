# 05 — Vehicles and the flight model

Scope: the hover craft as a simulated object: units and fixed-point formats, the per-craft
constants, the complete per-tick update in its original order (control input, friction, steering,
thrust, momentum, collisions with crafts, blocks, slopes and wall vectors, hover height, pitch and
roll), fuel / shield / ammo, chargers, the damage model, destruction, the view-only tumble, the
recovery truck and the repair, the booster's effect on the motion, the catch-up effect, the
supercar and spin bonuses, and the camera view data a craft provides. Weapons themselves are in
chapter 06, computer steering in 07, race rules in 04.

## 1. Units and formats

| Quantity | Format |
|---|---|
| position x (map column), row (map row), height | 8.8 fixed point cells (256 = one cell); one height unit = one cell |
| per-tick displacement | 8.8 cells per tick |
| momentum | 8.8, **4 × the per-tick move** (the move is momentum / 4) |
| angle (yaw, pitch, roll, turn rate) | 16-bit, 65536 per turn; yaw 0 = towards decreasing row, clockwise positive; pitch positive = nose down; roll positive = left side up |
| sin / cos | 2048-entry tables, value `trunc(65536 × sin(i × π / 1024))`, index = angle >> 5; cos(i) = sin(i + 512) |
| fuel, shield, ammo | signed 16-bit, 10000 = full bar, up to 20000 after "double" collectables |
| throttle ("speed") | 0..240 (8.8 cells, i.e. up to 240/256) |
| behind factor, thrust effectiveness | percent |
| 3D engine units | 2048 per cell (8.8 position << 3), used by the renderer and some distance tests |

Moving a point by distance d along (yaw, pitch): `height −= sin(pitch)·d; d' = cos(pitch)·d;
x += sin(yaw)·d'; row −= cos(yaw)·d'` (all products / 65536, truncated toward zero). Displacements
below 1/256 are treated as 0. The heading from a to b is `atan2(dx, −drow)`; the pitch from a to b
is `atan2(a.height − b.height, 2D distance)`.

The original uses integer arithmetic with truncation at each step; constants below are given in
their native units (n/256 cells, raw angle units) with conversions.

## 2. Craft constants

All crafts — every type, every team, computer or human — get **the same constants** at
initialisation; there is no per-type, per-team or per-difficulty table in the flight model. The
craft type only chooses the 3D model (chapter 04, 1.2: the weight/armour/speed/firepower numbers are
display-only).

| Constant | Value | Notes |
|---|---|---|
| steering input per tick | ±100 raw (half of the "angle add" 200) | 0.55° |
| turn-rate limit | ±3640 raw | 20° per tick |
| throttle input per tick | ±26/256 | |
| throttle limit | 240/256 (half, 120/256, without fuel) | |
| friction start / default / max | 10/256 / 15/256 (attribute 0) / 15/256 | |
| thrust effectiveness | 100 % at init, then from the shield (section 9) | |
| bounce (block hit) | 50/256 | |
| ride height above sensor floor | 50/256 | |
| minimum height above surface | 100/256 | |
| airborne threshold | 151/256 above surface | |
| sideslip friction (extra) | 7/256 | |
| sideslip recovery | 60 % of the lost sideways speed goes forward | |
| brake | momentum × 234/256 per tick | |
| booster impulse / burn / burn time | 150/256 × charge %, 7/256 × charge % per tick, 70 ticks | |
| sizes forward / rear / side | 60/256 each (sensor-point offsets) | |
| collision half size (vs. things) | 32/256 (1/8 cell) | |
| craft-vs-craft radius | 60/256 | |
| sensor rebound front / rear | 50/256 / 200/256 | the rear value is an unsigned byte 200 |
| sensor rebound limit | 80/256 | |
| start height | lowest surface + 128/256 | |

## 3. The per-tick update

Each tick the race processes all things in list order. A craft's own update runs:

```
clear the craft's "incoming missile distance"
action state machine (section 4)      -- includes reading the control input
unless the craft is being carried by a recovery truck ("reposition" flag):
    a  hit memory
    b  track friction
    c  steering -> turn rate -> heading
    d  thrust
    e  slope vector of the cell
    f  momentum
    g  movement delta
    h  craft-vs-craft collision
    i  map collision (sensor points)
    j  wall vectors
    k  overlap stop
    l  hover height (altitude)
    m  roll
    n  pitch (tilt)
    o  apply the move
    p  view data for the camera
    q  post-process: fuel, damage, chargers, collectables, death, warnings
    r  behind factor and thrust effectiveness
    s  bonuses (supercar, spin)
    t  terrain spray effect
the craft's tick counter ("time slice", 8-bit) += 1
```

Crafts are updated one after the other and each sees the others where they are at that moment.
The weapon, booster and shot objects are separate things processed in the same list (chapter 06).

### 3.a Hit memory

Per attacker (8 counters): if the attacker hit this craft during the last tick, its running count
grows by the number of hits; a tick without hits stores the maximum running count and resets it to
0. The computer evasion rule (chapter 07) reads "running count > 10".

### 3.b Track friction

- Airborne: friction = 0.
- On the ground, on even time slices only: target = friction nibble of the floor texture's
  attribute (bits 10..13, mask 0x3C00, of the level's texture attribute table) at the craft's cell;
  0 means 15. Friction moves 1/256 towards the target.

### 3.c Steering

`input` = steering input of this tick (raw), `rate` = turn rate.

```
if input != 0:
    if rate is 0 or has the other sign than input: rate = rate × 54/64
    rate += input
else:
    rate = rate × 57/64
rate = clamp(rate, −3640, +3640)
yaw += rate
```

Full lock reaches 20° per tick after about 37 ticks.

### 3.d Thrust

```
if shield > 0:
    if |speed input| > 1/256: throttle += speed input; brake flag = (speed input < 0)
else:
    throttle = 0
throttle = clamp(throttle, 0, 240/256)
if fuel <= 0: throttle = min(throttle, 120/256)
thrust = displacement of (effectiveness% × throttle / 16) along yaw
```

The brake flag keeps its value until the next non-zero speed input. The HUD speedometer shows the
throttle (chapter 08).

### 3.e Slope vector

For the cell under the craft with corner heights h1 = (i, j), h2 = (i+1, j), h3 = (i+1, j+1),
h4 = (i, j+1): `slope = (h1 − h2 − h3 + h4, h1 + h2 − h3 − h4)` (x, row) — the downhill vector.

### 3.f Momentum

```
delta = thrust + slope / 64
catch-up displacement = momentum / 4 × (behind − 100) / 100     (stored for 3.g)
if airborne: stop here (momentum unchanged)
v = |momentum / 4|
if v < 100/256:
    boost = clamp(50/256 − v, 0, 50/256) / (100/256)            (0 .. 0.5)
    delta = delta × (1 + boost)                                 (low-speed help)
momentum += delta
if the booster fires this tick (release tick, chapter 06):
    booster active, burn = 70
    momentum += along yaw: 150/256 × charge/100 × effectiveness/100
else:
    if booster active:
        if burn > 0: momentum += along yaw: 7/256 × charge/100 × effectiveness/100; burn −= 1
        else: booster inactive, charge = 0
    split momentum into the part along the heading (F) and the sideways part (S)
    momentum = F × (250/256 − friction) + S × (250/256 − 7/256 − friction)
    lost = |S| × (1 − (250/256 − 7/256 − friction))
    momentum += along yaw: lost × 60 / 100
if brake flag: momentum = momentum × 234/256
```

Top speed check: on flat ground with friction nibble 7 at full throttle the momentum settles at
15 × 243 / 13 (8.8 units), i.e. a move of 0.2723 cells per tick before the catch-up factor
(0.3268 for the leading human at factor 120).

### 3.g Movement delta

```
move = momentum / 4 + push (received from another craft's collision last time) ; vertical part = last vertical displacement
measured velocity = |move|                       (HUD lower bar, engine pitch; × 1.3 at the end of 3.q)
move += catch-up displacement
clamp each component: below −250/256 → −250/256; at or above 251/256 → 250/256
stored displacement = momentum / 4
```

### 3.h Craft-vs-craft collision

Against every other craft that is solid (racing, waiting on the grid, or waiting for the recovery
truck) whose height differs by less than 100/256:

- if both positions are identical the move becomes (60/256, 60/256);
- swept-circle test, both circles radius r = 60/256, own centre s moving by d, other centre o:
  if the centres are closer than 2r − 10/256 it is a hit (contact point unchanged); else, if o lies
  ahead (`(o − s)·d > 0`), the gap `|o − s| − 2r` is smaller than |d| and the perpendicular distance
  of o from the line of motion is < 2r, it is a hit with the contact point where the circles first
  touch; else no hit.

The nearest contact (distance from s to the contact point) wins. Response with n = unit vector
from s to o, tangent (n.row, −n.x), a = own move / 2, b = other's momentum / 4 / 2:

```
p = (a + b) · n
t = 2 × (a.x × n.row − a.row × n.x)
own move = p × n + t × tangent           (keeps its tangential part, takes the average normal part)
the other craft's push = p × n           (added to its next movement delta)
```

A non-zero push sets the "craft contact" affect on both crafts (contact sounds, 3.q). The craft's
own push is then cleared.

### 3.i Map collision: sensor points

Four sensor points, built by successive moves from the craft position (each move uses the
pitch/roll as the tilt of the step, see section 1):

| Step | Direction, tilt, distance | Result |
|---|---|---|
| 1 | yaw + 90°, tilt = roll, 60/256 | |
| 2 | yaw, tilt = pitch, 60/256 | front right |
| 3 | yaw − 90°, tilt = −roll, 120/256 | front left |
| 4 | **−yaw** (the negated heading, not yaw + 180°), tilt = −pitch, 120/256 | rear left |
| 5 | yaw + 90°, tilt = roll, 120/256 | rear right |

Because of step 4 the rear pair lies behind the craft only when it heads east or west; heading
north or south it lies in front. This is the original's behaviour and must be kept.

For each sensor k (order front left, front right, rear left, rear right), with the move added:

1. Block test of the horizontal move from the sensor point: "x blocked" if the new point is
   inside a solid block and so is (new x, old row) at the old height; "row blocked" likewise with
   (old x, new row).
2. floor = the surface (terrain or top of a column block below the sensor height) at the new point.
3. Sensor process (rest = floor + 50/256; zpos and zdisp persist per sensor; behind = the craft's
   behind factor):

```
if zpos >= rest:
    zpos = zpos + zdisp                          (free: keeps its vertical speed)
else:
    if texture attribute bit 2 (0x4, "track") is clear at the new point:
        |slope.x| >= 501/256  -> x blocked
        |slope.row| >= 501/256 -> row blocked
    zdisp = min((rest − zpos) × rebound / 256, 80/256)
    zpos = rest + zdisp
if zpos − rest > 3/256: zdisp −= (3 × behind / 100) / 256      (gravity)
zdisp = max(zdisp, −(70 × behind / 100) / 256)
if zdisp >= 101/256: zdisp = 100/256
```

Then for all four sensors together:

- any "x blocked": if |move.x| ≥ 11/256 the craft gets a "bump" affect; move.x = 0;
  momentum.x = −momentum.x × 50/256. Same for the row axis.
- each sensor that was blocked pushes the move sideways by 15/256: left sensors towards
  yaw + 90°, right sensors towards yaw − 90° (away from the wall).

### 3.j Wall vectors

The level's invisible wall segments (entity chains, see the level chapter) are tested against the
horizontal move with a closed segment intersection (touching counts). Up to 5 times:

```
if the move from the position crosses a wall with direction w:
    tries −= 1; if tries == 0: move = 0; stop
    along = w or w + 180°, whichever is within 90° of the move's direction
    dir = along, plus 30 raw angle units towards the move's side when they differ
    move = |move| along dir (vertical 0)
    h = w or w + 180°, whichever is within 90° of the craft's yaw
    err = h − yaw
    yaw += 364 raw if |err| < 182 raw (1°) else err / 16
```

### 3.k Overlap stop

If the move would end within 60/256 of another solid craft on both axes and within 100/256 in
height, the horizontal move is zeroed.

### 3.l Hover height

```
y = average zpos of the two front sensors
ground = surface below y at the new position
if y − ground < 151/256:
    airborne = false
    if y − ground < 100/256: y = ground + 100/256
else:
    airborne = true
vertical move = y − height
```

### 3.m Roll

- Airborne: roll × 15/16.
- Ground: d = floor(front left) − floor(front right); if |d| < 120/256:
  roll += (atan2(d, 120/256) − roll) / 8; clamp ±25° (±4551 raw).

### 3.n Pitch

- Airborne: pitch += 45 raw (0.247°, nose dropping); clamp ±45°.
- Ground: d = ground at the new position − average floor of the front sensors; if |d| < 60/256:
  pitch += (atan2(d, 60/256) − pitch) / 4; clamp ±25°.

### 3.o Apply the move

Clamp the horizontal components to ±230/256 and the vertical one to ±150/256, add to the position,
re-link the craft into its map cell.

### 3.p View data

The camera (chapter 08) reads: the craft position, yaw, pitch and **roll + 4 × turn rate** (the
view leans into turns), plus during the death tumble `rate × remaining tumble ticks` on yaw,
pitch and roll (section 7.1). The craft's model is drawn with the same three angles.

### 3.q Post-process

In this order:

1. **Fuel**: if the lap counter > 0, fuel −= (throttle × 256 + 39) / 40 (0..6 per tick, not
   below 0); else the craft gets the "invincible" affect (keeps it invincible until it first crosses
   the line, chapter 04).
2. Invincibility and invisibility counters −1 each if > 0 (while the invisibility counter runs the
   craft is flagged "not drawn").
3. **Damage**: if any damage affect is set (explosion, bullet, bump, contact bumps) and the
   invincibility counter is 0: sample 13 HELEHIT (rate 240 % when the damage came from an
   explosion); damage = summed amount clamped to ±10000; shield −= damage (not below 0); the
   cockpit shake count += damage (capped 1000); bullet damage +1 bullet-hit counter, missile
   damage +1 missile counter.
4. **Contact sounds** (craft-vs-craft affect, not invincible): phase 0 → 1 plays 13 HELEHIT;
   1 → 2 starts 19 SCRAPE2 looping at 60 % rate; no contact while phase ≠ 0 → phase 0, SCRAPE2
   stops.
5. **Chargers** (while racing or being repaired), each refilling only below 10000:

   | Charger | Effect per tick |
   |---|---|
   | ammo | ammo += 200 |
   | fuel | fuel += 200 |
   | shield | only if shield > 0: shield += 200 and the bullet / missile counters × (16 − shield × 16 / 10000) / 16; at full shield the counters are reset to 0; during the repair state also invincibility = 2 |

   Charging start / stop raise the RECHARGING / FULL banners (chapter 04); sample 7 REFUEL loops
   while charging.
6. **Collectable effects** (only while racing; chapter 06 lists them): invisible 250, invincible
   100, upgrades +1 (max 3), extra +2500 (no cap), full = 10000, double = 20000; Extra Shield
   also −10 bullet hits, Shield Full / Double reset both counters.
7. **Death check** (racing, shield ≤ 0): deaths +1, killer and kills (chapter 04, 11.3).
8. **Low warnings**: per value (fuel, shield, ammo): above 4000 the warning counter is 0, else it
   counts up to 64; when it becomes 1 and the value is > 0 the LOW banner is requested.
9. Clear the affect flags and amounts; measured velocity × 1.3.

### 3.r Behind factor and thrust effectiveness

Behind factor: chapter 04, section 9.2. Thrust effectiveness =
`(shield + 999) × 10 / 10000 + 90` (integer): 90 % when the shield is 0, 100 % at 10000, 110 % at
20000.

### 3.s Bonuses

**Supercar** (per tick):

```
clear "awarded"
on a lap-completed tick: if armed: set "next" else if place == 1: arm
if place != 1: disarm, supercar kills = 0
if armed and this craft scored a kill this tick: supercar kills += 1
if "next": clear it; if supercar kills >= level:
    supercar = 200 ticks, supercar kills = 0, set "awarded", remember the upgrades,
    all three upgrades = 3, level += 1, bonus score += 1000
if supercar > 0: fuel = shield = ammo = 10000; supercar −= 1; at 0 restore the remembered upgrades
```

The level starts at 0, so the first award needs only two consecutive laps completed while leading.
"Awarded" shows SUPER CAR with a fanfare (chapter 04, 12.3); a human's catch-up target is +10
while the supercar runs.

**Spin bonus** (while airborne; yaw in raw units, d = signed 16-bit difference yaw − last yaw):

| Phase | Action |
|---|---|
| 0 | sum = 0, phase 1 |
| 1 | d < 1: sum −= d, phase 3; else sum += d, phase 2 |
| 2 (turning right) | d < 0: phase 0; else sum += d |
| 3 (turning left) | d < 1: sum −= d; else phase 0 |

Last yaw = yaw after each airborne tick. Then: sum > 120150 → flag 720! (shown at once, refreshed
every airborne tick); else sum > 87382 → bit 2; > 54614 → bit 1; > 27307 → bit 0 (bits accumulate;
27307 ≈ 150°, 54614 ≈ 300°, 87382 ≈ 480°, 120150 ≈ 660°).

On the ground: phase 0, the message flags clear, then the highest bit reached pays: bit 3 → 720!
and 720 points (never happens: no rule sets bit 3), bit 2 → 360! and 540 points, bit 1 → 360! and
360 points, bit 0 → 180! and 180 points; then bits 0..3 clear. Points go to the bonus score of the
assessment (chapter 04, section 6). The message flags live one tick and request the big messages.

### 3.t Terrain spray

On the ground with a non-zero throttle: the texture attribute at position + this tick's move;
bit 0 (sand) → 4 Sand things, bit 1 (water) → 4 Water things at the craft position (chapter 06).

## 4. Action state machine

| Action | Name | Per tick |
|---|---|---|
| 0 | init | height = lowest surface + 128/256, sensors to that height, fuel/shield/ammo 10000, invincible affect; then racing (or waiting for the start) |
| 1 | racing | fuel check, smoke, death check, control (below) |
| 9 | tumble | inputs 0; count the tumble down (7.1); then 17 |
| 17 | drop collectables | throw the craft's collectables (7.2); explode with count 5 |
| 19 | exploding | every third time slice a medium explosion without damage; a smoke-fire puff every tick; count −1; below 0: call recovery |
| 20 | call recovery | pitch and roll × 250/256, momentum × 240/256, inputs 0 |
| 22 | truck on its way | momentum × 230/256 (pitch and roll untouched), inputs 0 |
| 23 | repair (held by the truck) | 8.2 |
| 24 | repaired | waits for the truck |
| 25 | reset | 8.3, then racing |

Racing tick:

1. **Fuel empty**: while fuel ≤ 0 a counter runs: on the first empty tick it is set to 200 and
   FUEL EMPTY is shown; it then counts down and is re-armed every 200 ticks; when it reaches 1 the
   craft calls the recovery truck (fuel death), CALLING REPAIR is shown. Until then the craft coasts
   with half the throttle limit.
2. **Damage smoke**: shield 1..3000: one smoke-fire puff every `shield / 1000 + 1` time slices
   (when the time slice is a multiple of it); below 500 one more puff 20/256 higher every tick.
3. **Death**: shield ≤ 0: an "Explosion big" without damage at the craft (chapter 06), action 9
   (destroyed), tumble setup, DEAD MEAT.
4. **Control**: computers and finished humans: the autopilot (chapter 07). Humans: targeting
   (chapter 06), then the keys:

| Input | Effect |
|---|---|
| accelerate | speed input +26/256 (wins over brake) |
| brake | speed input −26/256 |
| right | steering input +100 raw (right wins over left) |
| left | steering input −100 raw |
| minigun | minigun trigger, its target = current primary target |
| missile | launcher trigger, its target = current primary target |
| booster | booster trigger, only while fuel > 0 |

## 5. Fuel, shield and ammo

| Value | Start | Used by | Refilled by |
|---|---|---|---|
| fuel | 10000 | throttle (3.q) from the first crossing on; booster charging 3 per tick | fuel charger 200/tick, collectables, repair (+2000 after running dry, or 1000/tick after a destruction) |
| shield | 10000 | damage | shield charger 200/tick, collectables, repair 1000/tick |
| ammo | 10000 | missiles 1666 each; booster charging 3 per tick (the minigun uses none) | ammo charger 200/tick, collectables, repair |

Full throttle burns 6 fuel per tick: a full tank lasts about 1666 ticks (83 s at 20 Hz).

Chargers are rectangles from the level's region table (kind 1 shield, 2 fuel, 3 ammo; centre ±
half sizes, clipped to the map). Each tick, after all crafts moved, a craft whose cell lies inside a
rectangle gets that charger's affect, applied in its next post-process.

## 6. Damage model

| Source | Damage | Notes |
|---|---|---|
| minigun bullet (first spark of a hit) | 60 | at every upgrade level (chapter 06) |
| explosion particle placed by the level | 200 | particles made by other explosions deal none |
| missile explosion (medium) | 1500 | upgrade 2: three blasts of 3000 each |
| level explosion big / massive | 10000 | |
| craft destruction / dying-craft explosions | 0 | made without damage |
| craft-vs-craft contact, block bumps | 0 | only the HELEHIT / SCRAPE2 sounds and the camera shake |
| falling | none | no fall damage exists |

Amounts received in a tick are summed, clamped to ±10000 and applied in the post-process unless
the invincibility counter runs (chapter 04, 11.1).

## 7. Destruction

### 7.1 Tumble (view only)

At death, b = relative bearing (raw) of the craft the targeting search returns (0 if none),
r = b + 65536:

- time slice mod 4 < 2: yaw rate r / 20, roll rate −r / 7;
- otherwise: yaw rate r / 40, roll rate 36;
- pitch rate 0, remaining count 20.

Each tumble tick the count decreases; the tumble ends when the count drops below 1 (20 ticks).
Only the **view** (camera and drawn model angles) adds `rate × remaining count`; the craft's own
yaw/pitch/roll are untouched, so the spin winds down to the real attitude.

### 7.2 Dropped collectables

The destroyed craft throws collectables (each a "dropped collectable" thing living 200 ticks,
showing the three smaller sprite variants during its last 3 ticks): 3 Extra Shield, 3 Extra Fuel,
3 Extra Ammo, plus one upgrade collectable for each weapon whose upgrade is > 0 (see Known gaps).

### 7.3 Exploding

Action 19 for 6 ticks (count 5 down to −1): a medium explosion without damage on every third time
slice, a smoke-fire puff every tick; then the craft calls the recovery truck (action 20).

## 8. Recovery truck and repair

Levels place recovery trucks as entities of type 10, sub-type 9. A truck is a vehicle thing drawn
with model 56 (`RECOV0-0`), **heading 0** at spawn and only its heading ever changes (pitch and roll
stay 0). It spawns 4 cells (1024/256) above the ground and hovers in its idle state. It plays **no
sound** and has **no rotor animation** (the model has one frame).

### 8.1 Truck states

Distances are 2D in cells; positions 8.8. "Move toward" (speed s, target T, distance D):

```
diff = signed 16-bit (heading to T − heading)
if D < 8 cells: s = min(s, 100/256) × D / 8 + 15/256
if diff > 91 raw (a clockwise turn of more than half a degree): s = 0      (signed test)
heading += clamp(diff, −1820, +1820)                                         (10° per tick)
move s along heading
height += clamp(ground(new position) + 6 cells − height, −30/256, +40/256)
```

| State | Behaviour |
|---|---|
| idle | takes the first craft in "call recovery" (action 20) not served by another truck: craft → action 22, truck → approach |
| approach | until within 6 cells of the craft: move toward it at 220/256 cells per tick, holding 6 cells above ground; then descend with count 20 |
| descend | each tick: truck += ((craft x, row, height + 200/256) − truck) / count, count −1; at 0: craft → repair (action 23), choose the drop waypoint, carry with count 0 |
| carry, count 0 | if within 4 cells of the drop waypoint: count 1 (the craft is not moved this tick); else move toward it at 150/256 and put the craft at truck − 200/256 in height. Every tick the carried craft's heading turns towards the waypoint after the drop point by at most 60° (10922 raw) |
| carry, count 1 | if another truck is carrying to (with count ≠ 0), lowering at or releasing at the same waypoint: drop waypoint = its child, count 0; else lower with count 14 |
| lower | truck += ((waypoint x, row, waypoint floor + 400/256) − truck) / count; craft = truck − 200/256; count −1; at 2: release with count 0 (12 steps, the last dividing by 3) |
| release, count 0 | wait until the craft is repaired (action 24), keep turning it |
| release, count 1 | craft's waypoint target = drop waypoint, craft → reset (action 25), count 2 |
| release, count 2 | climb 25/256 per tick until higher than ground + 5 cells, then idle (hovering there) |

Drop waypoint: starting from the waypoint inside the last passed checkpoint's box, follow the
children up to the next checkpoint's waypoint (at most 30 steps) and take the one nearest to the
craft; if none is found the nearest waypoint is used; never 0.

While carried, the craft's own flight update is off ("reposition" flag) but its view data follows
the truck, so the camera follows the carried craft without any special case.

### 8.2 Repair (craft action 23)

Per tick: pitch × 250/256 then × 252/256; reposition on; throttle, measured velocity and momentum
0. Destroyed craft: +1000 shield until full, then +1000 fuel per tick until full, then ammo (one
value per tick, capped at 10000), sample 7 REFUEL each refilled tick; when all are full → action 24.
Fuel death: fuel +2000 once → action 24. Action 24 (destroyed): bullet and missile counters 0.

### 8.3 Reset (craft action 25)

Killer cleared. After a destruction: every upgrade −1 (not below 0) and the invincible affect (→
100 ticks of invincibility). Reposition off, the computer's charger wishes cleared, sensors to the
craft's height with zdisp 0, momentum 0, turn rate 0, roll 0, tumble off, death flags cleared,
stuck counter reset; action 1 (racing).

## 9. Catch-up effect

The behind factor (chapter 04, 9.2) acts on the motion in two places:

1. The move of each tick gets an extra `momentum / 4 × (behind − 100) / 100` (computed before the
   momentum update, added after the measured velocity is taken, not fed back into the momentum).
2. Hover gravity (3/256 per tick) and the downward clamp (−70/256) of the sensors scale with
   behind / 100.

The thrust effectiveness (3.r) scales the thrust and the booster with the shield.

## 10. Booster effect

The booster's charge and release are handled by the booster weapon thing (chapter 06: charge
+2 + upgrade per tick while held, costing 3 fuel and 3 ammo per tick, release on letting go while
on the ground). Its effect on the motion is 3.f: an impulse of 150/256 × charge/100 ×
effectiveness/100 on the release tick, then 7/256 × charge/100 × effectiveness/100 per tick for 70
ticks. Sounds: 8 POWERUP looping at `2 × charge + 100` % while charging, 9 BOOST on release. **No
visual effect** (no flame, no thing is created).

## Port deviations

- The reference recreation computes the flight model in floating point instead of the original's
  truncating integer steps (small numeric differences; the constants are the original's).
- Fixed 20 Hz tick with render interpolation of crafts and trucks (a truck moves at most 220/256
  cells per tick, so interpolation never snaps).
- Without a recovery truck in the level, a craft waiting for recovery is put back on the nearest
  waypoint after 100 ticks.
- Dropped collectables are placed on a ring of radius 1 cell around the wreck instead of being
  thrown.

## Known gaps

- The swept-circle contact point is reproduced geometrically; the original's integer formula was
  only used to pick the nearest of several crafts and was not transcribed.
- The tumble's bearing uses the craft's primary target in the reference; the original calls its
  targeting search at that moment.
- The network-mode behind-factor table is not covered.
- The exact composition of the collectables thrown by a destroyed craft and their throw trajectory
  (random throw, life 200, shrinking during the last 3 ticks) were not fully decoded; "3 + 3 + 3 +
  one per upgraded weapon" is the reference's choice.
- The exact interleaving of the separate weapon/booster things with the craft's update in the
  thing list is not documented (the reference charges the booster before the craft update and fires
  the guns after all crafts moved).
- Whether a destroyed craft waiting for the truck emits smoke (the reference adds a smoke-fire puff
  every 4th tick) is not confirmed by the ledger.
- The checkpoint progress routine's place in the per-tick order relative to the post-process is not
  documented.
- The booster-charge requirement is recorded inconsistently: the ledger says "fuel / ammo > 5", the
  reference tests ammo ≥ 6 and shield ≥ 6 (while deducting fuel and ammo).
