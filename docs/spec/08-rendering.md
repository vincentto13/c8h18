# 08 — Rendering

Scope: everything drawn during a race: the two screen modes and the virtual 2D coordinate space,
the composition of a race frame, the camera (all views, keys and forced views), the projection,
the terrain renderer with its detail settings, the sky, things drawn per map cell (3D models,
billboard sprites, shadows), the HUD with every element and position, the target marker, the
minimap, the start lights, banners, big messages, the finishers list and the KILLED BY line,
palette, light and blend tables, palette fades, the colour cube, and the FLIC screen recorder (a
port feature). File formats of the images, models and tables are described in the data-format
chapters; the sprite table and the collectable/effect sprite ids are in chapter 06.

## 1. Screen modes and virtual coordinates

| Mode | Resolution | Present | Sprite sets |
|---|---|---|---|
| low (default of the original) | 320 × 200 (VGA mode 13h) | copy 64000 bytes | `*0-0` files: MSPR0-0, PANEL0-0, MFONT0-0, OLFNT0-0, OSFNT0-0, PFONT0-0, LOGO0-0, TRACK0-0 |
| high | 640 × 480 (VESA mode 257) | 4 banks of 64 KB + 45056 bytes | `*0-1` files: HSPR0-0, PANEL0-1, HFONT0-0, OLFNT0-1, OSFNT0-1, PFONT0-1, LOGO0-1, TRACK0-1 (MFONT0-0 stays font slot 1) |

Font slots: 0 MFONT / HFONT, 1 MFONT0-0, 2 OLFNT, 3 OSFNT, 4 PFONT.

**All 2D drawing uses a virtual space 640 units wide**: 640 × 400 in low mode, 640 × 480 in high
mode. In low mode every 2D blit halves x, y, width and height, and at load time the width and height
of every entry of the loaded sprite sets are doubled so that sizes are virtual too (halved again
before the set is freed). Nothing is pixel-doubled: each mode draws its own sprite set 1:1. In this
chapter "virtual" means this space; "H" is the virtual height (400 or 480).

The 3D race view always covers the whole screen (320 × 200 or 640 × 480, not 640 × 400), centred at
(w/2, h/2).

Mode changes: the RESOLUTION menu (front end and pause menu) and, in demo races and spectator mode
only, the R key toggle the mode; a change fades the palette out (16 steps), frees and reloads the
mode's sprite set, sets the video mode and the palette. The choice is written to the config but
not read back at start (the original always starts in low mode).

## 2. A race frame

Per drawn frame, for the viewed player:

```
if the countdown runs (chapter 04, 2):          (blocking loop, every frame)
    camera (cockpit default) ; 3D view ; start lights ; present
elif demo race, or spectator mode, or paused:
    camera: with the player's cheat flag the free camera (window keeps its own action and follow,
            camera keys act); otherwise the trackside camera (action 6) on computer player
            (race frame / 200) mod 7 + 1
    3D view                                       (no lights, minimap, HUD or messages)
elif the player's craft is not racing, or is on the autopilot after its finish:
    camera action 6 on the player's own craft (forced every frame) ; 3D view
    "KILLED BY <name>" while the killer is set (chapter 04, 5.4)
    messages + finishers list (section 12)
else:
    camera (player's action) ; 3D view
    if the craft's lap-timer flag is clear: start lights with lamp 3 lit (section 11.11)
    elif the craft's finish place is 0: minimap (11.9), then HUD (11)
    messages + finishers list (section 12)
FLIC recorder capture (port feature, section 15)
debug overlay if enabled
present
```

The pause menu box and the CTRL-Tab puzzle are drawn after this by the race loop.

## 3. Camera

### 3.1 Camera window

One window per view, kept across frames:

| Field | Meaning | Initial (each race) |
|---|---|---|
| position | x, row, height (8.8 cells) | from the view |
| yaw, pitch, roll | 16-bit angles; pitch positive = looking down; roll positive = left side up | kept from the previous race |
| focal | 94 (pixels at 320 wide) | 94 |
| followed player | 0..7 | 0 (the player) |
| zoom | 0..1000 | 300 |
| action | view mode 1..11 | 7 (cockpit) |
| distance | current eye distance behind the position (8.8) | 0 |
| chase yaw, pitch, roll, chase distance | targets the chase views ease towards | kept from the previous race |
| static count | chase obstruction counter | |

The **view data** of the followed craft (chapter 05, 3.p) gives the position and the angles yaw,
pitch, roll + 4 × turn rate (+ the death tumble). The camera runs once per frame: first the key of
this frame (one scan code per frame, consumed, so a press acts once), then the action — a new action
is shown in the same frame. Keys set only the action, zoom and followed player; angles, distance
and chase targets stay, so every switch eases from the previous view. Easing below means
`value += difference / 8` with the difference of 16-bit angles taken signed, integer division
truncated toward zero (so a distance stops up to 7/256 short).

### 3.2 Keys

| Key | Effect |
|---|---|
| `1`..`8` | follow player 0..7 (no range check; keys 1..4 also issue the view-slot command, chapter 04, 13) |
| `-` | zoom + 20 (max 1000) |
| `=` | zoom − 20 (min 0) |
| F1..F9 | action 1..9 |
| F10 | action 11 (orbit) |
| arrows, Z, X | steer the orbit view while action 11 runs (they also drive / fire the craft) |

There is no look-back key.

### 3.3 Actions

| Action | Key | View |
|---|---|---|
| 1 | F1 | behind: chase yaw = view yaw, chase pitch = view pitch + 2730 (15°), chase roll = view roll, chase distance 256 (1 cell); ease yaw, pitch, roll, distance |
| 2 | F2 | in front: as 1 with chase yaw + 32768 |
| 3 | F3 | side: as 1 with chase yaw + 16384 |
| 4 | F4 | other side: as 1 with chase yaw − 16384 |
| 5 | F5 | above: as 1 with chase pitch = view pitch + 16384 |
| 6 | F6 | level camera (below) |
| 7 | F7 | **cockpit** (default): angles = view angles, distance 0, damage shake (below) |
| 8 | F8 | chase, distance 120/256, 15° (below) |
| 9 | F9 | chase, distance 240/256, 10° |
| 10 | — | chase, distance 360/256, 5° (not reachable from the keys) |
| 11 | F10 | orbit (below) |

All actions take the position from the view data.

**Action 6, level camera.** Among all things of type 3 (scenery: the invisible level camera things,
cones, barrels, sign posts, trees, lamp posts) within a 3D distance of 32 cells of the craft, take the
nearest one that can see the craft: Chebyshev distance ≤ 1 cell counts as visible; otherwise the
line is sampled at `(d + 64) >> 7` half-cell steps from the craft (d in 1/256 cells), each point
tested with the map collision (solid block or below the terrain = blocked). Then yaw = heading from
the thing to the craft, pitch = pitch from the thing to the craft, roll 0, distance =
`zoom × d / 1000` (the eye sits on the line towards the thing, 30 % of the way at zoom 300). If no
thing qualifies the cockpit (7) is used for this frame.

**Action 7, cockpit shake.** Each damage tick adds the damage to the craft's shake count (capped at
1000). Per cockpit frame: `amount = (count × 64) / 1000` with sign + on odd race frames, − on even;
`count = count × 5 >> 3`; a non-zero amount moves the position by `amount` along (yaw + 16384, pitch)
(sideways). The count decays only while a cockpit view runs.

**Actions 8..10, chase with obstruction probe** (distance D, angle a):

```
position = view position; height += 128/256 (after the angles are taken)
chase yaw, roll = view yaw, roll; chase pitch = view pitch + a × 65536 / 360
chase distance = D (reset every frame)
free = probe(position, chase yaw, chase pitch, −D)
if free != −D:
    static += 1
    if static > 8: chase distance = max(−free − 64/256, 16/256); static = 8; chase pitch −= the offset
else static = 0
ease yaw, roll, pitch, distance
```

probe(p, yaw, pitch, d), only for 8/256 ≤ |d| ≤ 10 cells: step k = −8/256, −16/256, … down to d;
point = p moved k along (yaw, pitch), lifted 16/256; blocked if the map collision is non-zero at
(x − 64/256, row + 64/256) or (x + 64/256, row + 64/256) (the two corners on the +row side; the
original's four-corner test only keeps these two); returns the first blocked k, else d.

**Action 11, orbit.** Per frame: Left / Right chase yaw ∓ / ± 546, Up / Down chase pitch ∓ / ± 546,
Z / X chase roll ∓ / ± 546; angles = chase angles (it starts from the last chase targets); distance
= `zoom × 8064 / 1000 + 128` (8.8: about 10 cells at zoom 300).

### 3.4 Final eye

If the distance is 0 the followed craft is hidden while rendering. Otherwise the eye moves
−distance along (yaw, pitch) (back and, for a nose-down pitch, up). Finally
`height = max(height, terrain height under the eye + 128/256)` (terrain height = the interpolated
floor, block columns ignored).

### 3.5 Forced views

| Situation | Camera |
|---|---|
| player's craft destroyed / waiting / carried, or finished (autopilot) | action 6 on the player's own craft every frame; camera keys have no effect |
| pause, demo race | action 6 on computer player `(race frame / 200) mod 7 + 1` (the pause freezes the race frame, so one fixed craft) |
| spectator mode (Y, only with the cheat flag) | switching on: action 6, follow 0; then the window's own action and follow, camera keys act; switching off: window init (action 7, zoom 300, follow 0, distance 0) |

## 4. Projection

| Item | Value |
|---|---|
| engine units | 2048 per cell (8.8 positions << 3) |
| camera-relative point | X = (x − camera x) × 2048 (east), Y = (height − camera height) × 2048 (up), Z = (camera row − row) × 2048 (north) |
| rotation | about the vertical axis by yaw, about the right axis by pitch, about the view axis by −roll (angles = 16-bit >> 5, 2048 per turn); 3×3 matrix with 16384 = 1.0 |
| focal | 94 × screen width / 320 (94 or 188): same horizontal field of view in both modes (about 119°), taller vertical view in 640 × 480 |
| projection | `k = (focal << 16) / Z`, `sx = cx + (X × k >> 16)`, `sy = cy − (Y × k >> 16)`, centre (w/2, h/2); a vertex with Z < 200 is not divided (`sx = cx + X`, `sy = cy − Y`) |
| outcodes | left of 0, right of width−1, above 0, below height−1, deeper than the far distance, nearer than 200 |
| far distance | R × 2048, R = 16 cells (constant; no setting changes it) |
| fog start | 0.75 × R × 2048 (12 cells) |

A triangle whose three vertices share an outcode is dropped.

## 5. Terrain

### 5.1 Visible cells

Per map row offset dy = −R..R from the camera's cell, a column span starts as the circle
`lo = −isqrt((R + 1)² − dy²)`, `hi = −lo` (isqrt = floor square root; note R + 1).

**Frustum clipping** happens only while the camera's pitch word (16-bit pitch >> 5, taken unsigned,
positive = looking down) is **below 93**, i.e. level or looking down by less than about 16°; looking
up by any amount disables it. With s, c = sin, cos of the yaw word (65536 = 1.0) and
`t = (screen width << 7) / focal` (= 435, 256 × tan of the half field of view):

```
apex      (ax, ay) = ((−4 s) >> 16, (4 c) >> 16)                (whole cells, 4 cells behind)
edge 1 end (ax + ((t c − 256 s) >> 16), ay + ((t s + 256 c) >> 16))
edge 2 end (ax + ((−t c − 256 s) >> 16), ay + ((−t s + 256 c) >> 16))
per edge: slope = (ex − ax) × 256 / (ey − ay); x(dy) = ((dy − ay) × slope >> 8) + ax  (accumulated from row −R)
edge 1: ey < ay lowers hi to x(dy), ey > ay raises lo; edge 2: the other way round
an edge with ey == ay empties (lo = hi = 0) every row from ay to R (edge 1 when ax < ex,
edge 2 when ex < ax), or from ay down to −R otherwise
```

**Walk**: 2R rows, dy = R down to −R + 1 (row −R is never drawn), far to near; a row is skipped
unless lo < hi; the row visits hi + 2 − lo cells, columns **hi down to lo − 1** (the map column
wraps at 256). Every visited cell draws its terrain and then its things (section 7) — this cell set
is the **only** draw-distance test for vehicles, props and sprites.

### 5.2 Cell geometry

- Corner heights h1 = (i, j), h2 = (i+1, j), h3 = (i+1, j+1), h4 = (i, j+1); vertices at
  Y = height × 8 − camera height × 8 in engine units.
- A plain cell is one floor quad with the cell's texture and orientation (the orientation nibble
  selects a UV entry).
- A column cell (negative texture word) refers to a column record: a 16-bit occupancy mask (bit k
  = a block spanning heights k..k+1 above the sloped corners), a floor texture and block ids; each
  block record has 6 face textures (N, E, S, W, top, bottom) and 6 face orientations. The floor quad
  is drawn only when bit 0 is clear; a side face only where the neighbouring cell has no block at
  that level; a top face when the block above is missing; a bottom face when the block below is
  missing (and k > 0).
- UV orientation: the floor, tops and the south face use UV entry `compose(cell orientation, face
  orientation)`; **north, east and west faces always use entry 0** (an original quirk); bottom faces
  use the second half of both tables (+8).
- Every quad is split into two triangles along a diagonal that alternates with `(x + row) AND 1`
  (corners 0–2 for odd cells, 1–3 for even).

UV entries (corner order of the quad; 63 stands for 64 − 1/65536 texels):

| Entry | Corners (u, v) | Entry | Corners (u, v) |
|---|---|---|---|
| 0 | (0,0) (63,0) (63,63) (0,63) | 8 | (0,0) (63,0) (63,63) (0,63) |
| 1 | (63,0) (0,0) (0,63) (63,63) | 9 | (63,0) (0,0) (0,63) (63,63) |
| 2 | (0,63) (63,63) (63,0) (0,0) | 10 | (0,63) (63,63) (63,0) (0,0) |
| 3 | (63,63) (0,63) (0,0) (63,0) | 11 | (63,63) (0,63) (0,0) (63,0) |
| 4 | (0,0) (0,63) (63,63) (63,0) | 12 | (0,0) (0,63) (63,63) (63,0) |
| 5 | (0,63) (0,0) (63,0) (63,63) | 13 | (63,0) (63,63) (0,63) (0,0) |
| 6 | (63,0) (63,63) (0,63) (0,0) | 14 | (0,63) (0,0) (63,0) (63,63) |
| 7 | (63,63) (63,0) (0,0) (0,63) | 15 | (63,63) (63,0) (0,0) (0,63) |

Compose table (row = cell orientation × 8 + face orientation, 8 values per row; first half for tops
and the south face (south uses row 0 = its own orientation), second half for bottoms):

```
tops:     0 1 2 3 4 5 6 7 | 1 0 3 2 5 4 7 6 | 2 3 0 1 6 7 4 5 | 3 2 1 0 7 6 5 4
          4 6 5 7 0 2 1 3 | 5 7 4 6 1 3 0 2 | 6 4 7 5 2 0 3 1 | 7 5 6 4 3 1 2 0
bottoms:  0 1 2 3 4 5 6 7 | 1 0 3 2 5 7 4 6 | 2 3 0 1 6 4 7 5 | 3 2 1 0 7 6 5 4
          7 6 5 4 0 2 1 3 | 5 4 7 6 1 3 0 2 | 6 7 4 5 2 0 3 1 | 4 5 6 7 3 1 2 0
```

The index is `(cell orientation × 8 + (face orientation AND 7)) AND 63`; bottom faces add 8 to the
result.

### 5.3 Lighting and haze

Each cell corner has a 16-bit illumination word (typically 8191): an 8.8 **light-table row**
(row 32 = colours unchanged). Per vertex (16.16 light): `light << 8` up to the fog start; between
0.75 R and R it fades linearly to 0.5 row (row 0 = the haze colour 255) at R; beyond R row 0.

### 5.4 Detail settings

| Setting | Default | Effect |
|---|---|---|
| SHADING on/off | on | on: textured Gouraud (per-vertex light rows); off: one light per triangle = average of the three vertex lights |
| SKY on/off | on | on: sky (section 6); off: the screen is cleared to colour 255 |
| TEXTURE MAPPING on/off with level 0..4 | on, level 3 | off: terrain textures whose texture attribute byte (table below) is non-zero become flat colours from the solid-colour tables, chosen by the UV corner of the triangle's second vertex ((0,0), (u,0), (u,v), (0,v) → table 0..3), lit by the average light; attribute-0 textures stay textured with the average light |
| near texture distance | 2 | set to min(level, 3) by every key of the TEXTURE MAPPING item; see subdivision |
| resolution | low | section 1 |

Menu handling of TEXTURE MAPPING: Left lowers the level, below 0 turns mapping off; Right turns it
on or raises it up to 4; every key sets the near distance to min(level, 3). Objects ignore all
detail settings.

Texture mapping is **affine**. Perspective comes only from subdivision: the projection flags a
vertex with bit 0 when its depth Z < level × 2048 and bit 1 when Z < near distance × 2048; the
pattern index `f1 + 4 × f0 + 16 × f2` (f = flags of the three vertices) selects:

```
pattern by index 0..63:
0 0 0 0 0 1 1 1 0 1 5 5 0 1 5 5 0 2 2 2 3 4 4 4 3 4 8 8 3 4 8 8
0 2 6 6 3 4 9 9 7 10 11 11 7 10 11 11 0 2 6 6 3 4 9 9 7 10 11 11 7 10 11 11
```

0 = not split; 1/2/3 = edge 0-1 / 1-2 / 2-0 split once; 4 = all three once; 5/6/7 = edge 0-1 / 1-2
/ 2-0 split twice; 8/9/10 = all once and edge 0-1 / 1-2 / 2-0 twice; 11 = all twice. An edge is
split at its camera-space midpoint (u, v and light averaged, re-projected); all three edges split
make four triangles (inner edges split again only where both neighbouring edges are split twice);
each piece is mapped affinely.

Texture attribute bytes per terrain texture 0..255 (0 = textured even with mapping off and always
loaded, 1 = flat colour with mapping off and not loaded in low-memory mode, 255 = unused):

```
  0- 31: 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1
 32- 63: 1 1 1 1 0 0 0 0 0 0 0 1 0 0 0 1 0 0 0 1 1 1 1 1 0 0 0 0 0 0 0 0
 64- 95: 0 0 0 0 0 0 1 1 0 1 1 1 0 0 0 0 1 0 0 1 1 1 0 1 1 1 1 1 1 1 1 1
 96-127: 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 0 0 0 0 0 0 1 1 0 0 0 0
128-159: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 1 0 0 0 0 0 0 0 1 1 0 0 255 255 255 255
160-255: 255
```

### 5.5 Painter's order

Everything 3D (terrain triangles, model triangles, sprites, shadows) is queued into depth buckets
and drawn far to near after the walk. Sprites use bucket `(Z − 64) >> 4` (slightly in front of
polygons at the same depth), shadows `(Z − 2048) >> 4` (one cell nearer than their ground point).

## 6. Sky

Drawn first when SKY is on. Angles are the camera words >> 5 (yaw, pitch positive nose-down, roll);
s, c = sin, cos(roll) from the 2048-entry tables (65536 = 1.0); W, H = screen size.

```
cs = (c << 8) / W ; ss = (s << 8) / W                 (16.16 texels per pixel: the screen width
                                                      always spans 256 texels, in both modes)
hx = W/2 − (s × (sin(pitch) >> 10) >> 16)
hy = H/2 − (c × (sin(pitch) >> 10) >> 16)             (pitch moves the horizon at most 64 px)
row start: u = yaw × 16384 − (cs × hx − ss × hy) ; v = −(hx × ss + hy × cs)
per row:   u −= ss ; v += cs
per pixel: u += cs ; v += ss, through a table of integer parts: pixel k uses start + ((k − 1) × cs >> 16)
           (pixels 0 and 1 share a texel)
texel = sky[(v >> 16 AND 255) × 256 + (u >> 16 AND 255)]
```

The 256 × 256 image (`data/sky0-N`) wraps: its last rows lie just above the horizon and it repeats
twice per turn.

### 6.1 True-colour sky (port addition)

Not in the original. When the track has `DATA/SKY0-N.PNG` (chapter 01 §4.1) it is drawn instead of
the palette sky above; without it the sky is drawn exactly as above. Per pixel the view ray d (world
axes x, height, row) gives the elevation asin(d.height) and the azimuth atan2(d.x, −d.row)
(clockwise from map north = camera yaw 0); the panorama is sampled bilinearly at
u = (azimuth − left edge) / 360° × repeat (wrapping), v = (top − elevation) / (top − bottom)
(clamped). The result is blended into the haze colour (colour 255 through light row 0, the colour
the terrain fades to, §5.3) by smoothstep(0, horizon blend, elevation), so below the horizon it is
the haze colour. With a sun, a disk of the sidecar's angular radius (anti-aliased over one pixel,
the sun colour pushed towards white) and a corona `colour × (0.45·e^(−a/2.2r) + 0.10·e^(−a/9r))`
(a = angle to the sun, r = disk radius) are added.

Sun glare (SUN GLARE display option, on by default; only with a true-colour sky that has a sun and
`glare` > 0): when the sun lies in front of the camera, its screen position is projected and an edge
fade computed, 1 up to 75 % of the way from the centre to the screen edge, 0 at 120 %. After the 3D
view is drawn, a 1 × 1 pass reads the depth buffer (sample 0 when multisampled) at the points of an
8 × 8 grid inside a disc of 1.6 × the sun's pixel radius (at least 3 pixels) around the sun,
clamped to the screen; the share of points still at the clear depth (only sky there) is the sun's
visibility. The composite then adds, on the 3D view below the HUD, `glare × visibility × fade ×`
(a bloom and warm veil in the sun colour falling off with the distance to the sun, five soft ghost
discs of different tints and sizes at 0.45, 0.8, 1.3, 1.7 and 2.15 of the way from the sun through
the screen centre, and a faint ring). The glare changes no game state.

## 7. Things on a cell

For each visited cell, its thing list is walked; a thing flagged "hidden" (an invisible craft, the
followed craft when the camera distance is 0) is skipped entirely (no model, no shadow). There is
**no per-thing distance test**: a thing is drawn when its cell is visited.

| Thing draw type | Drawn as |
|---|---|
| sprite | billboard (section 9), queued at bucket `(Z − 64) >> 4` |
| model | shadow first (section 10, only when the ground point's depth > 64 engine units), then the model (section 8) |
| none | nothing (bullets, Flares, explosion emitters) |

The recovery truck, cones and barrels are model things and get shadows like crafts.

## 8. 3D models

- Vertices are in 1/2048 cell; a file vertex (x, y, z) is transformed as (x, z, −y) (file z is up,
  file y runs along +row), rotated by the model matrix built from yaw, pitch and **negated roll**
  (the same angles the camera uses for a craft: yaw, pitch, roll + 4 × turn rate + tumble) and added
  to the camera-relative position. A craft is about half a cell long (car 1000 units, tank 2060).
  The truck is drawn with its heading only.
- Craft models: `objects/data/<stem>0-<colour>.dat` (stems car, tank, jugga, jet, bike, skim;
  colour 0..7); the truck uses RECOV0-0.
- Triangle kinds: type 0 → flat colour, unlit; type 4 → flat colour lit through light row
  `64 − average(shade[v] >> 2)` of its three vertices (models with per-vertex shade bytes; without
  them the original flickers with random rows 16..31); types 2 and 3 → textured from the object
  texture page, unlit; types 5 and 6 → textured at light row 32.
- **Culling**: a triangle is dropped when its three projected vertices share an outcode, or when
  `(y2 − y1)(x1 − x0) − (x2 − x1)(y1 − y0) ≤ 0` on the integer screen coordinates (screen y down):
  front faces run **clockwise on the screen** in the file's vertex order; edge-on faces are dropped.
  The model matrix is a rotation, so it never flips the winding.
- **No distance fade**: model light never depends on depth. A model pops in at full brightness when
  its cell enters the walk (about 16..17 cells, where the terrain has already faded to the haze),
  and its triangles beyond depth R are cut away whole by the outcode test.

## 9. Billboard sprites

A sprite thing shows sprite-table entry id (chapter 06, section 8) plus, for the 4-frame images, its
animation frame.

**Size**: `h = height × focal / Z` (whole pixels, height in engine units), `w = image width × h /
image height`; each raised to at least 3; the sprite is **skipped when either exceeds 64**; then
both + 1. The limits are the same in both modes (the doubled focal in 640 × 480 makes sprites vanish
at twice the distance there; see Port deviations). No mip levels: one image scaled by column / row
tables.

**Light**: depth Z ≤ fog start → unlit; fog start < Z < far → light row
`((far − Z) × 32 / (far − fog start))` (31 down to 0, integer steps: 32 light rows over 0.75 R..R,
the same window as the terrain); Z ≥ far → row 0 (haze colour).

**Blit mode** from the sprite's blend byte:

| Blend byte | Unfogged | Fogged |
|---|---|---|
| 0 | copy | `light[row][src]` |
| 1 | copy | copy (never fogged) |
| 2 | `blend[src][dst]` | `light[row][blend[src][dst]]` |
| 3 | `blend[dst][src]` | `light[row][blend[dst][src]]` |
| 4, 5 | constant blends (unused by the sprite table) | |

**Placement, upright path** (every sprite id drawn this way uses a tmap of mode 21 or 24): the
projected thing position (sx, sy) is the sprite's **bottom centre**; the image is never rotated:

```
k = (w + h) >> 2
corner = (sx − k − (sin(roll) × k >> 16), sy − k − (cos(roll) × k >> 16))     roll = camera roll word >> 5
draw an unrotated w × h box from the corner
```

With the camera rolled the box moves around the point along the rolled vertical. Only tmap 47
(mode 1, the shadow) takes the rotated path.

**Rotated path** (shadow; also defined for anchor "bottom centre" though unused): the camera roll
θ = −roll AND 2047 falls in octant `θ >> 8`; per octant a master line runs from a screen edge
along its major axis one pixel per step and steps one pixel along the minor axis whenever a 16-bit
accumulator of the slope (tan of the octant angle for even octants, cot for odd ones, 65536 at
exactly 45°) carries. Major axis per octant 0..7: +x, +y, +y, −x, −x, −y, −y, +x; minor axis:
+y, +x, −x, +y, −y, −x, +x, −y. With `along` = cos of the octant angle (even) or sin (odd), the box
is drawn as `n = (h << 16) / along` copies of the master line one pixel apart along the minor axis,
each `l = w × along >> 16` steps long; texture column = `j × (image width << 16) / l`, row =
`i × (image height << 16) / n`; each copy's start moves by `(h × across) / n` (16.16) along the
major axis. Anchor bottom centre: corner `(sx − ((h × sin + (w × cos >> 1)) >> 16),
sy − ((h × cos − (w × sin >> 1)) >> 16))`; anchor top centre: corner `(sx − (w × cos >> 17),
sy + (w × sin >> 17))` with texture rows counted from the bottom (image upside down).

## 10. Shadows

For every drawn model thing: the ground point (thing x, row, terrain height there with columns
ignored) is projected; if its depth > 64 engine units the shadow is queued at bucket
`(Z − 2048) >> 4`. It uses sprite-table entry 178: tmap 47, a 64 × 32 ellipse of colour 25, height
480 engine units (0.234 cell), blend byte 2 (`blend[src][dst]`, fogged `light[row][blend[src][dst]]`),
size rule as section 9 but limited to 3..80 px, then + 1; drawn on the rotated path with anchor top
centre: the box's top edge is centred on the ground point and runs along the rolled horizontal, the
box extends h pixels along the rolled downward direction, image upside down. On raised blocks the
shadow lies at the floor under them and is hidden by nearer block faces.

## 11. HUD

Drawn only in the normal branch (section 2) while the lap-timer flag is on and the finish place is 0,
in this order. All positions below are **320 × 200 table positions**; the game doubles x and y into
virtual units, so in 640 × 480 PANEL0-1 sits at twice these positions (the same layout at double
size in the top part of the screen). Sprite numbers are PANEL entries.

### 11.1 Cracked glass

First of all the up to 16 PANEL 230 cracks (chapter 04, 13), translucent.

### 11.2 Gauges

A gauge is a list of `{x, y, sprite, threshold}` records drawn in order while `threshold < value`
(ammo frame and weapon pips: `threshold ≤ value`), stopping at the first that fails. Values are on a
0..10000 scale; values above 10000 show a full gauge.

| Gauge | Value | Records (x, y, sprite, threshold) |
|---|---|---|
| fuel bar (purple, top) | fuel | (257,2,17,0) (244,2,16,384) (232,2,15,768) (220,2,14,1152) (207,2,13,1536) (195,2,12,1920) (183,2,11,2304) (171,2,10,2688) (162,2,9,3072) (153,2,8,3456) (143,2,7,3840) (132,2,6,4224) (120,2,5,4608) (108,2,4,5376) (91,2,3,6528) (68,2,2,7680) (41,2,1,8832) |
| velocity bar (y 13) | measured velocity (see Known gaps) | (64,13,56,0) (74,13,57,384) (84,13,58,768) (94,13,59,1152) (104,13,60,1536) (115,13,61,1920) (124,13,62,2304) (134,13,63,2688) (144,13,64,3072) (153,13,65,3456) (162,13,66,3840) (171,13,67,4608) (180,13,68,5376) (190,13,69,6528) (209,13,70,7680) (227,13,71,8832) |
| speed bar (green, y 8) | throttle × 10000 / 240 | (60,8,18,0) (71,8,19,526) (81,8,20,1052) (93,8,21,1578) (103,8,22,2104) (114,8,23,2630) (123,8,24,3156) (134,8,25,3682) (144,8,26,4208) (153,8,27,4734) (162,8,28,5260) (171,8,29,5786) (182,8,30,6312) (194,8,31,6838) (207,8,32,7364) (219,8,33,7890) (231,8,34,8416) (242,8,35,8942) (254,8,36,9468) |
| booster overlay | booster charge % × 100, drawn only while non-zero | as the speed bar with sprites 37..55 |
| shield frame (green, left) | shield | (2,2,72,0) (21,2,73,1666) (34,2,74,3332) (38,2,75,4998) (48,2,76,6664) (57,2,77,8330) |
| ammo frame (yellow, right) | ammo | (298,2,91,0) (277,2,90,1666) (263,2,89,3332) (253,2,88,4998) (244,2,87,6664) (230,2,86,8330) |
| heat box (small, top right) | minigun heat % × 100 | (251,14,92,0) (259,14,93,625) (262,14,94,1250) (265,14,95,1875) (268,14,96,2500) (271,14,97,3125) (274,14,98,3750) (277,14,99,4375) (283,14,100,5000) (289,14,101,5625) (293,14,102,6250) (297,14,103,6875) (301,14,104,7500) (305,14,105,8125) (309,14,106,8750) (313,14,107,9375) |

**Low warnings**: the fuel bar, shield frame and ammo frame are drawn translucent (through the blend
table) while their warning counter runs (value 1..4000) and bit 2 of the race frame is clear: they
blink every 4 frames.

### 11.3 Digits and icons

| Element | Sprites and positions |
|---|---|
| position | big digit 108 + place at (2,16); `/` 138 at (28,38); small digit 128 + number of crafts at (43,38) |
| kills | skull 228 at (277,34); red digits 118 + tens at (293,34), 118 + ones at (306,34) |
| laps | icon 227 at (64,47); blue digits 139 + tens / ones of the current lap at (89,48) / (96,48); `/` 150 at (102,48); laps tens / ones at (109,48) / (116,48) |

### 11.4 Weapon boxes

Gauges on the upgrade level 0..3 (threshold ≤ value):

| Weapon | Box | Pips for levels 1, 2, 3 |
|---|---|---|
| minigun | 268 at (74,35) | 270 at (109,35), (116,35), (123,35) |
| missile | 271 at (132,35) | 273 at (158,35), (165,35), (172,35) |
| booster | 274 at (181,35) | 275 at (206,35), 275 at (213,35), 276 at (220,35) |

### 11.5 Lap-time list (bottom left)

Rows for laps `max(1, lap − 3) .. lap` (lap = current lap, at least 1); if the best lap is older
than the first row it gets a row of its own on top. Row format at x = 2, virtual units: lap tens
digit (skipped when 0, still advancing width − 2), lap ones digit (advance width + 4), separator
161 (advance width + 4), then the four digits of the lap time (the running lap uses the live lap
time) with leading zeros skipped (each advance width − 2). Digits are PANEL 151..160. Rows are
`height(PANEL 139) + 2` apart, the first at virtual `H − rows × (height + 2) − 2`.

### 11.6 Minimap

Drawn before the HUD (solid, no translucency), bottom right, images at one virtual unit per map
cell (TAB entries counted from 0):

- low mode: TRACK0-0 entry (race number + 1) at virtual (640 − w − 4, 400 − h − 4);
- high mode: TRACK0-1 pieces for track 1..6 — first piece at (640 − w₁ − 4, 480 − (h₁ + h_last) − 4)
  (track 2 also subtracts the second piece's width from x), the others at offsets from it:

| Track | Pieces (entry @ offset) |
|---|---|
| 1 | 1, 2 @ (6,120) |
| 2 | 3, 4 @ (120,5), 5 @ (74,120) |
| 3 | 6 |
| 4 | 7, 8 @ (28,120) |
| 5 | 9, 10 @ (18,120) |
| 6 | 11 |

Markers for every craft at virtual (x0 + column, y0 + row) of its cell, in its team colour
(chapter 04, 1.2): the own craft and the leader 6 × 6 at (x − 3, y − 3), others 4 × 4 at (x, y); a
10 × 10 translucent halo (`blend[dst][colour]`) at (x − 5, y − 5) behind the own craft while bit 2
of the race frame is set. Low mode halves corners (truncating) and sizes (3 × 3, 2 × 2, 5 × 5 px).
Invisible crafts are still shown.

### 11.7 Target marker

Last element of the HUD. Shown while the viewed craft's lock (chapter 06, 2) is non-zero and its
target's position projects with no outcode (on screen, depth ≥ 200 engine units, not beyond the far
distance), in every camera view that shows the HUD. The projected pixel position is doubled in low
mode into virtual units. All marker pieces have the same pixel size in both PANEL sets (box 258 /
259: 21 × 19; top/bottom pointers 260..263: 21 × 4; side pointers 264..267: 4 × 19; glyphs 7 × 8), so
the box is relatively twice as large in 320 × 200. With w, h = virtual size of 258:
`left = sx − w/2, top = sy − h/2, right = left + w, bottom = top + h`.

| Lock | Pieces (translucent, a piece leaving the virtual screen is skipped whole) | Name | Shield bar |
|---|---|---|---|
| 1..2 | green box 258 at (left, top) | — | — |
| 3..99 | with `d = (100 − lock) / 4 + 2` (26 down to 2): 258 at (left, top); 260 at (left, top − d + 2); 262 at (left, bottom + d − h(262)); 264 at (left − d, top); 266 at (right + d − w(266), top) | green capitals (glyph base 232) at (sx + w/2 + 2, top), only while bit 2 of the viewer's race frame is set (4-frame blink) | at (left + 2, sy + h/2 + 2), width `shield × (h − 4) / 10000`, height 4 |
| 100 | red box 259 at (left, top); 261 at (left, top − 2); 263 at (left, bottom + 2 − h(263)); 265 at (left − 2, top); 267 at (right + 2 − w(267), top) | red capitals (glyph base 201) at (sx + w/2 + 4, top), always | width `shield × (h − 2) / 10000`, height 4 |

Name drawing: the target pilot's name; A–Z and a–z are PANEL `base + letter`, a space advances by
the width of glyph `base`, any other character is skipped. The bar is a solid rectangle (halved in
low mode, no clipping) in colour-cube entry 48 (the palette colour nearest to 6-bit RGB 0, 48, 0, a
green); it is not clamped (a double shield draws it twice as long) and it is drawn only when the
target's invincibility counter is 0 or bit 2 of the target pilot's race frame is set (so it blinks
on an invincible target). There is no warning box around a craft that targets you (only the
LOCKON4 sound).

### 11.8 Start lights

PANEL sprites a, b, c left to right at virtual y = 12, the first at
x = `(2 × w(a) + w(b) − 16) / 3` (integer division), each next at the previous x + w + 8 (virtual
widths). Lamp pairs (off, lit): 281/282, 283/284, 285/286; sequences in chapter 04, section 2.

## 12. Messages

Run in the bare and normal branches (section 2).

- **Banner** (chapter 04, 12.1): at virtual y = H − 40. Opening/closing at half-width T (30, 60, 90,
  120, 150, 180 by sweep index): filler PANEL 200 tiled from x = 320 − w(198) − T while x < 320 + T
  (step w(200)), then cap 198 at (320 − w(198) − T, y) and cap 199 at (320 + T, y). Shown states:
  caps at T = 180, no filler, the message text in font slot 2 centred with spacing −6, glyphs in their
  own colours.
- **Big message** (chapter 04, 12.2): font slot 4 centred at virtual y = H/2, spacing −6, on the
  visible frames of its blink.
- **KILLED BY** line: `"KILLED BY " + name` in font slot 4, centred at H/2, every frame (section 2).
- **Finishers list**: chapter 04, 5.4 (PANEL digits 151 + place at x = 2 from y = 120, red names).

Centred text: total width = Σ(glyph width − 6) (a space: the font's space width − 6),
x = (640 − width) >> 1; glyphs crossing the virtual screen edge are skipped.

## 13. Palette, light and blend tables

- Palette `data/palet0-0.dat`: 256 × 3 six-bit DAC values.
- Light table `data/table0-N.dat` (per track, 328 rows of 256): rows 0..63 are light rows (row 32 =
  identity; row r maps a colour to its shade); rows 64..319 are the 256 × 256 blend table
  (`blend[a][b]` = row 64 + a, column b); rows 320..327 are four 512-byte solid-colour tables for the
  flat terrain mode.
- Translucent 2D blits (HUD bars when blinking, marker pieces, cracks) write `blend[src][dst]` for
  every opaque pixel. Rectangle fills: mode 0 solid, mode 1 `blend[dst][colour]`, mode 2
  `blend[colour][dst]`.
- Billboards use 32 of the light rows (31..0) for their haze fade (section 9).
- **Colour cube** (built at level load): 8 × 8 × 8 entries, `cube[r × 64 + g × 8 + b]` = the palette
  index nearest to the 6-bit colour (8r, 8g, 8b): least squared distance, ties broken by the smallest
  Manhattan distance, then the smallest `2r² + 2g² + b²` of the candidate, then the lowest index.
  Entry 48 = (0, 48, 0) is the target marker's shield-bar green.

### 13.1 Palette fades

Blocking: for i = 0..steps every 6-bit DAC value becomes `cur + i × (target − cur) / steps`, one step
per vertical retrace (70 per second); target black for fade-outs. Fade-outs use 16 steps, fade-ins
32. Used: title → front end (out 16, in 32); before LOADING LEVEL (out 16, the starting-grid picture
`onet0-N`, in 32); screen-mode change (out 16, new palette set at once); quitting (out). **No fade at
the race start**; after a race the front-end fade-in starts from the race palette (no visible change).

## 14. Low-memory mode

With less than about 2.1 MB free the game forces low mode, loads only the low-mode sprite group and
the low-resolution object texture page `objects/data/tex0-1`, skips terrain textures with attribute
1, hides DETAIL OPTIONS and disables the puzzle.

## 15. FLIC recorder (port feature)

The original contains a screen recorder that its menu never reaches (its hot key calls an empty
function); the reference recreation exposes it as a port feature:

- Alt+C starts (low mode only) or stops a recording; the race end also stops it. File name
  `flics/game%04d.flc` (the original: `flics/game%04d.flc`), n = 0, 1, … the first
  unused, at most 9999.
- Header (128 bytes, written at start, rewritten at the end): size, magic `0xAF12`, frame count,
  width 320, height 200, depth 8, flags 3, speed 57 (ms per frame), creator and updater `"BILF"`,
  aspect 6:5, offsets of frames 0 and 1 (set as they are written), the rest 0.
- One frame per race frame, captured after the 3D view and overlays and before the "Recording FLIC"
  text: a frame chunk (type `0xF1FA`) with a COLOR_256 sub-chunk (type 4) when the palette changed;
  frame 0 BYTE_RUN (type 15, or FLI_COPY 16); later frames encode DELTA_FLI / LC (12), DELTA_FLC / SS2
  (7) and BYTE_RUN and keep LC if smaller than both, else SS2 if smaller than BYTE_RUN, else BYTE_RUN
  when smaller than w × h + 16, else FLI_COPY. The previous picture and palette are kept for the next
  delta.
- Stopping: frame count − 1 (excluding the ring frame), header rewritten, file closed.
- "Recording FLIC" is printed at (0, 0) in font slot 0 after each capture.

## Port deviations

- Fixed 20 Hz simulation with interpolated drawing (crafts, trucks, camera); a new camera view is
  shown at once, not blended. A missile is drawn at its position on its launch tick (the original
  first shows it after its first 4 sub-steps, about 1.1 cells out).
- Billboard size limits are scaled with the screen width: 3..64 px in 320 × 200, **6..128 px** in
  640 × 480 (shadows 3..80 / 6..160), so both modes cull at the same angular size (in the original a
  nearby explosion in 640 × 480 shows only its first, white frame).
- The port starts in 640 × 480 (`-320` for the original's 320 × 200); the original always starts in
  low mode.
- Spectator mode is reset at every race start.
- The FLIC recorder is reachable (Alt+C, also stops a running recording) and captures once per
  simulation tick.
- The terrain and models use a depth buffer instead of the original's depth buckets (same visible
  order rules for sprites and shadows).
- The cracked-glass statics are drawn whenever the HUD is shown (no view-slot gate).
- Tab shows a port-only top-down debug map.
- Port addition: an optional true-colour sky with a sun and sun glare (§6.1), drawn instead of the
  palette sky when the track has `DATA/SKY0-N.PNG`; tracks without one are drawn as before.
- Port addition, crossings (chapter 01 §9.7): blocks on a flat base are drawn with all corners at
  that base (with the floor quad under them and a bottom face on level 0); on layered levels shadows
  lie on the surface right below the model, and the chase eye stays below a deck above the craft and
  short of the first block between craft and eye.

## Known gaps

- The frustum clipping was verified only at roll 0 (the clipped set contains every cell whose centre
  projects onto the screen at roll 0).
- The bucket key of terrain and model polygons is not documented (only the sprite and shadow keys).
- The scaling of the velocity bar (y 13) is uncertain: the reference shows the measured velocity with
  200/256 cells per tick as full scale; the original's exact conversion of that value to the
  0..10000 thresholds was not confirmed.
- The view-slot word that gates the cracked glass (keys 1..4, command 30) is only partly understood.
- The difference between model triangle types 2 and 3 (both textured, unlit) and between 5 and 6 is
  not documented.
- The constant blend modes 4 and 5 are not described (unused by the sprite table).
- The HUD build-up animation present in the executable is never called by the game and is not part
  of the specification.
- Stereo rendering (a ±16-pixel centre offset in the code) is not covered.
