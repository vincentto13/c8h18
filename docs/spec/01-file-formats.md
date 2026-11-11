# 01 — File formats

Scope: every data file the game reads or writes, byte-exact: the RNC compression wrapper, the palette, sprite sheets and
fonts, raw images (backdrops, textures, sky, puzzle), colour tables, billboard images (TMAPS), model texture atlases,
3D models, the level file with all its record types and enumerations, the search-ring grid, the sound and music
containers, the config / save-game block, the intro movie and the FLIC files written by the screen recorder.

All multi-byte integers are **little-endian** unless stated otherwise (only the RNC header is big-endian). "u8/u16/u32"
are unsigned, "i8/i16/i32" two's-complement signed. Offsets are from the start of the (unpacked) file. "8.8" is the
fixed-point format of chapter 00 (fraction byte first, integer byte second). Offsets of five or more hex digits are
written in decimal.

## 1. Loading rule and compression

### 1.1 Loading

The game loads a data file whole into memory and then unpacks it **in place if it starts with the RNC signature**; any
other file is used as stored. Unpack errors print `ERROR decompressing %s` and abort. Files inside containers
(TMAPS images, SOUND.DAT blobs) follow the same rule per blob.

RNC-packed in the shipped data: `HFONT0-0`, `HSPR0-0`, `LOGO0-0/1`, `MFONT0-0`, `MSPR0-0`, `OLFNT0-0/1`, `OSFNT0-0`,
`POINT0-0`, `ONET0-0/1`, `OSCR0-0/1`, `TITLE`, `SKY0-0..5`, `TEXTU0-0..5`, `TABLE0-0..5`, `SRCH0-0` (all `.DAT` in
`DATA/`), all six `MAPS/LEVEL0-N.DAT`, every blob of `TMAPS.DAT` and most blobs of `SOUND/SOUND.DAT`. Stored
uncompressed: every `.TAB`, `OSFNT0-1.DAT`, `PANEL0-0/1.DAT`, `PFONT0-0/1.DAT`, `TRACK0-0/1.DAT`, `PALET0-0.DAT`,
`PUZZLE.DAT`, `INTRO.DAT`, `TMAPS.DAT` as a whole, all of `OBJECTS/DATA/`, `SOUND/MUSIC.DAT`.

### 1.2 RNC ProPack, method 1

Header, 18 bytes, **big-endian**:

| Offset | Size | Field |
|---|---|---|
| 0 | 3 | signature `RNC` (0x52 0x4E 0x43) |
| 3 | 1 | method; only **1** occurs and the game's unpacker accepts only method 1 |
| 4 | 4 | unpacked length |
| 8 | 4 | packed length (bytes following the header) |
| 12 | 2 | CRC-16 of the unpacked data |
| 14 | 2 | CRC-16 of the packed data (the `packed length` bytes after the header) |
| 16 | 1 | leeway (in-place safety margin; not needed for decoding) |
| 17 | 1 | chunk count (informative; chunks are self-delimiting) |

Method 2 of ProPack is not used by any file and is not supported by the game. Example: `DATA/SKY0-0.DAT` is 1228 bytes
and unpacks to 65536; `MAPS/LEVEL0-1.DAT` is 20613 bytes and unpacks to 896140.

**CRC-16**: reflected polynomial 0xA001, initial value 0, no final XOR (CRC-16/ARC; "123456789" gives 0xBB3D):
```
crc = 0
for each byte b: crc ^= b; repeat 8 times: crc = (crc & 1) ? (crc >> 1) ^ 0xA001 : crc >> 1
```

**Bit reader.** Bits are taken least-significant first from consecutive little-endian 16-bit words starting at offset
18. The reader always holds one 16-bit word of look-ahead beyond the bits being consumed:
```
pos   = 18                    ; byte position of the look-ahead word
buf   = u32le(pos)            ; reading past the end of the file yields zero bytes
count = 16                    ; the look-ahead word occupies bits [count-16, count) of buf
peek(n)    = buf & ((1 << n) - 1)
advance(n) = buf >>= n; count -= n;
             if count < 16: pos += 2; buf |= u32le(pos) << count; count += 16
read(n)    = v = peek(n); advance(n); return v
```
Literal bytes are copied **from the input starting at `pos`** (the position of the look-ahead word); afterwards
`pos += length` and the look-ahead is refilled:
```
count -= 16; buf &= (1 << count) - 1; buf |= u32le(pos) << count; count += 16
```

**Huffman tables.** A table is read as: `n = read(5)`; if `n == 0` the table is empty; else `n` code lengths of
`read(4)` each (symbol j has length `len[j]`, 0 = unused). Codes are canonical: `code = 0`; for L = 1 .. max length,
for every symbol j in increasing order with `len[j] == L`: assign `code` (L bits, most significant first), `code += 1`;
after each L: `code <<= 1`. Because the stream is LSB-first, compare against the bit-reversed code: a symbol matches
when `peek(L) == reverse_bits(code, L)`. After matching, `advance(L)`. The decoded **value** of symbol v is v for
v < 2, else `(1 << (v − 1)) | read(v − 1)`.

**Decoding.**
```
advance(2)                                   ; two flag bits, ignored
while output length < unpacked length:
    raw  = read_table(); dist = read_table(); len = read_table()
    count = read(16)
    loop:
        n = value(raw)
        if n > 0: append the n literal input bytes at pos; pos += n; refill look-ahead
        count -= 1 (16-bit wrap); if count == 0: break      ; a stored count of 0 ends after one literal run
        d = value(dist) + 1;  m = value(len) + 2
        copy m bytes from output[len − d] onward, byte by byte (overlap allowed)
verify CRC of the packed bytes (may be done first) and of the output
```

## 2. Palette — `DATA/PALET0-0.DAT`

768 bytes, not packed: 256 entries × (red, green, blue), each a **6-bit VGA DAC value 0..63**. To display on an 8-bit
device use `v8 = (v << 2) | (v >> 4)`. Loaded at start; it is the only game palette (the intro has its own).

## 3. Sprite sheets and fonts — `NAME.DAT` + `NAME.TAB`

Used for `HFONT0-0`, `HSPR0-0`, `LOGO0-0/1`, `MFONT0-0`, `MSPR0-0`, `OLFNT0-0/1`, `OSFNT0-0/1`, `PANEL0-0/1`,
`PFONT0-0/1`, `POINT0-0`, `TRACK0-0/1` (all in `DATA/`).

**TAB** (never packed): an array of 6-byte entries; the entry count is `file size / 6`.

| Offset | Size | Field |
|---|---|---|
| 0 | u32 | offset of the sprite's RLE stream in the unpacked DAT |
| 4 | u8 | width in pixels |
| 5 | u8 | height in pixels |

Entry 0 is always all zero (unused). Entries with width or height 0 are empty.

**DAT** (after unpacking): starts with a u16 that equals the number of TAB entries (e.g. 0x0136 = 310 for PANEL), so
the first sprite starts at offset 2. Each sprite is an RLE stream decoded row by row, top to bottom, starting with
column 0 of row 0:

| Byte n (as i8) | Meaning |
|---|---|
| n < 0 | skip −n pixels (transparent), column += −n |
| n = 0 | end of the row: next row, column 0 |
| n > 0 | n literal palette-index bytes follow; draw them at successive columns |

Decoding stops after `height` rows. Pixels never written are transparent. A literal pixel of value 0 is opaque.

**Fonts**: character byte c (code page 437) is entry **c + 1**. 242-entry fonts cover c = 0..240; `HFONT0-0` has 129
entries (c = 0..127). The glyph's advance is its width (the front end draws with an extra −6 virtual units per glyph,
chapter 02). Glyphs of `OLFNT`/`OSFNT` are masks with pixel values 0 and 1 (1 = text colour, 0 = background/outline
colour, see the text markup in chapter 02); `PFONT` and `MFONT`/`HFONT` glyphs hold real colours.

| Sheet | Entries | Largest entry (w × h) | Content |
|---|---|---|---|
| HFONT0-0 | 129 | 12 × 10 | font slot 0 in 640×480 |
| MFONT0-0 | 242 | 8 × 7 | font slots 0/1 in 320×200, slot 1 in 640×480 |
| OLFNT0-0 / -1 | 242 | 19 × 10 / 38 × 20 | large outlined menu font |
| OSFNT0-0 / -1 | 242 | 19 × 10 / 38 × 20 | small outlined font |
| PFONT0-0 / -1 | 242 | 21 × 19 / 41 × 39 | big message font |
| PANEL0-0 / -1 | 310 | 81 × 87 (640 set) | HUD pieces, box corners 300..303 |
| HSPR0-0 / MSPR0-0 | 13 | 24 × 24 / 12 × 12 | icons |
| LOGO0-0 / -1 | 5 / 7 | 127 × 39 / 127 × 78 | logo pieces 1..4 / 1..6 |
| POINT0-0 | 7 | 17 × 17 | markers, mouse pointer |
| TRACK0-0 | 7 | 68 × 74 | minimap of track t is entry t + 1 (one image per track) |
| TRACK0-1 | 12 | 120 × 122 | minimap pieces for 640×480 (1..3 per track, chapter 08) |

In 320×200 mode the game doubles the width and height bytes of these entries in memory (chapter 00, virtual
coordinates); the files hold real pixel sizes.

## 4. Raw images

| File | Packed | Unpacked size | Layout |
|---|---|---|---|
| `DATA/TITLE.DAT` | RNC | 64000 | 320 × 200 boot splash, row-major palette indices |
| `DATA/OSCR0-0.DAT` / `OSCR0-1.DAT` | RNC | 64000 / 307200 | front-end backdrop, 320 × 200 / 640 × 480 |
| `DATA/ONET0-0.DAT` / `ONET0-1.DAT` | RNC | 64000 / 307200 | starting-grid picture shown while a level loads |
| `DATA/SKY0-N.DAT` (N = 0..5) | RNC | 65536 | 256 × 256 sky image of track N + 1 |
| `DATA/TEXTU0-N.DAT` | RNC | 1048576 | 256 terrain textures of 64 × 64: one 64-pixel-wide image of 16384 rows; texture i is rows 64i .. 64i + 63 |
| `DATA/PUZZLE.DAT` | no | 10752 | 112 × 96 picture (the studio logo) for the sliding puzzle; tile (col, row) is the 28 × 24 block at (28·col, 24·row) |

## 5. Colour tables — `DATA/TABLE0-N.DAT`

RNC, unpacked exactly **83968 bytes = 328 rows × 256**; every row is a 256-entry remap `out = row[in]`. Track t uses
`TABLE0-(t−1)`.

| Rows | Offset | Use |
|---|---|---|
| 0..63 | 0 | brightness: row L maps a colour to brightness L; **row 32 is the identity**, lower rows are brighter, higher rows darker; row 0 is the haze colour used at the far edge of the view |
| 64..319 | 16384 | 256 × 256 **blend table** `blend[a][b]` = byte at 16384 + a·256 + b (translucent sprites, the front-end box, `$G` text modes) |
| 320..327 | 81920 | four 512-byte **flat-colour tables** (variants 0..3), entry = texture id, used when texture mapping is off |

## 6. Billboard images — `DATA/TMAPS.DAT` + `DATA/TMAPS.TAB`

**TAB**: 10-byte entries, 49 in the shipped file = 48 images + 1 sentinel.

| Offset | Size | Field |
|---|---|---|
| 0 | u32 | unpacked size of the image (header + pixels) |
| 4 | u32 | offset of the image's blob in TMAPS.DAT |
| 8 | u16 | animation group: consecutive entries with the same group are frames of one animation and are loaded together |

The sentinel's offset is the DAT file size; its other fields are 0. Image i's blob is `DAT[offset(i) .. offset(i+1))`.
The game hard-codes 48 images.

**DAT**: starts with the 8 ASCII bytes `BULLFROG` (the first TAB offset is 8), then the blobs. Each blob is RNC-packed
(a blob without the signature would be used as stored). Unpacked image:

| Offset | Size | Field |
|---|---|---|
| 0 | u16 | flags; byte 0 bit 0 = "register as texture" (never set); **byte 1 = sprite mode**: 0x15 (images 0..12 and 29..46), 0x18 (images 13..28, animated in groups of 4 frames), 0x01 (image 47, the craft shadow) |
| 2 | u16 | width |
| 4 | u16 | height |
| 6 | w·h | palette indices, row-major |

`unpacked size == 6 + width · height` for every entry (example: entry 0, 88 × 74, 6518 bytes). Which sprite uses
which image, their world sizes and blend modes are in the rendering chapter. Colour 0 is the transparent colour as used
by the port; the original's billboard scaler's transparency rule is not separately documented.

## 7. Model texture atlases — `OBJECTS/DATA/TEX0-0` and `TEX0-1`

Not packed. Unlike the other sheets the DAT is a **raw image 256 pixels wide** (`TEX0-0.DAT` 196608 bytes = 256 × 768;
`TEX0-1.DAT` 22528 bytes = 256 × 88). The TAB has the 6-byte layout of section 3 with 51 entries, but the u32 is
`y · 256 + x` of the texture's top-left corner in that image, and **entry 0 is used**. `TEX0-0` holds the full-size
textures (e.g. entry 0 is 32 × 32), `TEX0-1` the small textures loaded instead in low-memory mode (entry 0 16 × 8).
Triangles of the 3D models select an entry by index (section 8).

## 8. 3D models — `OBJECTS/DATA/*.DAT` ("BULLFROG OBJECT DATA")

62 models are loaded at start, in this fixed order (the model number is the index):

| Numbers | Files |
|---|---|
| 0..7 | CAR0-0 .. CAR0-7 (Dart in team colours 0..7) |
| 8 | MARSH0-0 |
| 9 | SIGN0-0 |
| 10..17 | TANK0-0 .. 7 (Bulwark) |
| 18..25 | JUGGA0-0 .. 7 (Mauler) |
| 26..33 | JET0-0 .. 7 (Kestrel) |
| 34..41 | BIKE0-0 .. 7 (Scythe) |
| 42..49 | SKIM0-0 .. 7 (Skimmer) |
| 50..55 | TRACK0-0 .. 5 (the track models of the track-selection carousel) |
| 56 | RECOV0-0 (recovery truck) |
| 57..58 | CONE0-0, CONE0-1 |
| 59..61 | BAREL0-0 .. 2 |

Layout (not packed):

| Offset | Size | Field |
|---|---|---|
| 0x00 | 20 | ASCII `BULLFROG OBJECT DATA` (the loader only searches the 36-byte header for this text) |
| 0x14 | u32 | 0 |
| 0x18 | u32 | triangle block size in bytes |
| 0x1c | u32 | vertex block size = vertex count × 6 |
| 0x20 | u32 | shade block size = vertex count |
| 0x24 | u8 | flags: bit 1 (2) = several animation frames, bit 2 (4) = type-4 triangles use the per-vertex shade bytes; **5 in every shipped file** |
| 0x25 | u8 | frame count; **1 in every shipped file** |
| 0x26 | u16 | triangle count |
| 0x28 | u16 | vertex count (must be < 500) |
| 0x2a | 16 | overwritten by run-time pointers when loaded; on disk `4f 00`, a u32 n, `7f 1f 00 00 7f 12 00 00 d9 dd` (n per model: CAR 32, BIKE 21, JET 34, JUGGA 31, TANK 20, SKIM 25, MARSH 22, RECOV 31, TRACK 1..5, CONE/SIGN 2, BAREL 4; meaning unknown, never read) |
| 0x3a | frames × 8 | frame table, present only when flags bit 1 is set (zero bytes on disk) |
| then | triangle block | triangles, variable size |
| then | per frame | vertex block (count × 3 × i16: x, y, z), then shade block (count × u8) |

**Triangles** (walk by the type byte; 12 bytes for types 2, 3, 5, 6, otherwise 6 bytes):

| Byte | Meaning |
|---|---|
| 0 | type (below) |
| 1 | unused (0) |
| 2, 3, 4 | vertex indices |
| 5 | flat types: palette colour; textured types: texture atlas entry (section 7) |
| 6..11 | textured types only: (u, v) texel pairs of the three vertices, relative to the atlas entry's corner (halved by the 320×200 renderer) |

| Type | Drawing |
|---|---|
| 0 | flat colour, unlit |
| 4 | flat colour with per-vertex brightness: vertex light = shade byte >> 2 when flags bit 2 is set, else a random value 16..31 per frame (flicker); colour = brightness row (64 − average of the three) of the colour table |
| 2, 3 | textured, unlit (the difference between 2 and 3 is not documented) |
| 5, 6 | textured at fixed light row 32 (the difference between 5 and 6 is not documented) |
| other | 6-byte record, treated like the flat types |

**Units and axes.** Vertex coordinates are in **1/2048 cell** (a car is about 1000 units long, RECOV0-0 spans
±4480 × 3900 × 2100). The renderer feeds the vertex as (x, z, −y), i.e. the file's y axis is vertical; the full
orientation convention is in the rendering chapter. Front faces are clockwise on screen after projection.

## 9. Level file — `MAPS/LEVEL0-N.DAT` (N = 1..6)

RNC-packed; unpacked **exactly 896140 bytes**, otherwise the level does not load. The file is a snapshot of the game's
whole race-state block: the game loads it over that block and then rebuilds or overwrites the run-time parts. There is
no header; sections sit at fixed offsets:

| Offset | Size | Records | Content |
|---|---|---|---|
| 0 | 96000 | 4000 × 24 | **entities** (section 9.4); record 0 unused |
| 96000 | 2012 | | unknown (non-zero in the shipped files) |
| 98012 | 26624 | 1024 × 26 | **column definitions** (section 9.2); record 0 unused |
| 124636 | 16384 | 1024 × 16 | **block definitions** (section 9.3); record 0 unused |
| 141020 | 8 | | unknown |
| 141028 | 19136 | 8 × 2392 | player records (run time) |
| 160164 | 2440 | | run time (includes the per-player command queue) |
| 162604 | 85000 | 1000 × 85 | thing records (run time); the shipped files pre-place the **map regions** here (section 9.5) |
| 247604 | | | run time: vehicle records of 452 bytes from 247806, and other state |
| 357668 | 512 | 256 × u16 | **texture attributes** (section 9.6) |
| 358198 | 8 | 4 × u16 | written by the loader: track extent (section 9.6) |
| 402206 | | | run time: wall-vector and waypoint lists, rebuilt from the entities at load (the stored bytes are not used) |
| 404620 | 491520 | 160 × 256 × 12 | **map cells** (section 9.1) |

Other non-zero areas (around 292108, 354852, 358180..360875 and 362210..366189) are of unknown use; a recreation can
ignore them.

### 9.1 Map cells

Cell (x, y) (x = column 0..255, y = row 0..159) is at `404620 + (y · 256 + x) · 12`:

| Offset | Size | Field |
|---|---|---|
| 0 | i16 | light level of the cell's corner, 8.8: `value >> 8` is the brightness row of the colour table (32 = unchanged); typical 8191 |
| 2 | u16 | height of the cell's corner, 8.8 cells |
| 4 | i16 | ≥ 0: terrain texture id (0..255, into `TEXTU0-N`); < 0: **−column index** into the column definitions (the cell carries a column of blocks; its floor texture comes from the column record) |
| 6 | u16 | run time: head of the list of things standing on the cell (rebuilt by the game) |
| 8 | u16 | unknown |
| 10 | u8 | high nibble: texture orientation 0..15 (index into the renderer's UV-corner table, rendering chapter); low nibble unknown |
| 11 | u8 | unknown |

A cell's surface spans the corner heights of cells (x, y), (x+1, y), (x+1, y+1), (x, y+1). The terrain morph "swap"
exchanges bytes 0, 1, 4, 5 and 10 of two cells and morphs the heights separately (race chapters).

### 9.2 Column definitions

Record id (1..1023) at `98012 + id · 26`; a record whose first byte is 0 is empty:

| Offset | Size | Field |
|---|---|---|
| 0 | u16 | occupancy mask: bit k set = a solid block occupies heights k .. k+1 above the cell's corner heights (k = 0..7) |
| 2 | i16 | floor texture id (drawn when bit 0 is clear) |
| 4 | i16 | unknown |
| 6 | 8 × i16 | block definition id for levels 0..7, bottom to top; 0 = none |
| 22 | i16 | unknown |
| 24 | i16 | unknown |

### 9.3 Block definitions

Record id (1..1023) at `124636 + id · 16`; a record whose first byte is 0 is empty:

| Offset | Size | Field |
|---|---|---|
| 0 | 6 × u8 | texture id of the faces north, east, south, west, top, bottom |
| 6 | 6 × u8 | orientation of the same faces (the original honours it only for the south, top and bottom faces) |
| 12 | 4 | unknown |

### 9.4 Entities

4000 records of 24 bytes from offset 0; a record whose type byte is 0 is empty; record 0 is never used.

| Offset | Size | Field |
|---|---|---|
| 0 | u8 | type (table below) |
| 1 | u8 | sub-type |
| 2 | u16 | **group**: all entities of group g are created when g is activated; group 1 is activated when the level loads |
| 4 | u16 | target group (triggers: the group they activate) |
| 6 | 4 | always 0 |
| 10 | u16 | always 1 in the shipped levels; meaning unknown |
| 12 | u16 | link: index of another entity (next point of a chain, source area of a morph); 0 = none |
| 14 | u16 | value (checkpoint order, timer delay, ...) |
| 16 | 8.8 | x (column); the fraction byte is always 0 (whole cells) |
| 18 | 8.8 | y (row); fraction always 0 |
| 20 | u16 | width parameter (extent in cells, meaning per type) |
| 22 | u16 | height parameter |

Created things are placed at the centre of the entity's cell (fraction 0x80) on the terrain, with all angles 0.

**Types and sub-types** (names are the original's member names; counts are over all six shipped levels):

| Type | Sub-type | Name / meaning | Uses of the fields | In levels |
|---|---|---|---|---|
| 1 | 5 | checkpoint | value = order along the lap, 0 = start/finish line; width/height = extent − 1 in cells (the box starts at the entity's cell; half-cell placement as used by the port, not verified) | 48 |
| 2 | 0 | Explosion (particle) | | |
| 2 | 1 | Explosion medium (damage 1500 once) | | 18 |
| 2 | 2 | Explosion big (damage 10000) | | 5 |
| 2 | 3 | Explosion massive (damage 10000) | | 32 |
| 2 | 4 | Bullet (impact sparks) | | |
| 2 | 5 | Fire (emits smoke for 15 ticks; "steam strong" in community tools) | | 11 |
| 2 | 6 | Hole | | |
| 2 | 7 | Smoke | | |
| 2 | 8 | Smoke fire ("steam light") | | 5 |
| 2 | 9 | Light (used only as the link target that marks a morph's source area) | | 23 |
| 2 | 10..12 | Earthquake, Tremor, Gas | | |
| 2 | 13 | Altitude (height morph of one cell, created by Swap area) | | |
| 2 | 14 | Shade | | |
| 2 | 15 | Move area | link = source entity, width/height = area size | |
| 2 | 16 | **Swap area**: exchanges the (width+1) × (height+1) cells starting at this entity's cell with those starting at the linked entity's cell | link, width, height | 23 |
| 2 | 17, 18, 19 | Copy area, Remove area, Mark area | link, width, height (15/17/18) | |
| 2 | 20, 21, 22 | Sand, Water, Flare | | |
| 3 | 0 | Sign post | | |
| 3 | 1 | Tree | | |
| 3 | 2 | Lamp post | | |
| 3 | 3 | Camera static (invisible trackside camera used by the level-camera views) | | 82 |
| 3 | 4 | Camera follow | | |
| 3 | 5 | Clone | | |
| 3 | 6, 7 | Cone1, Cone2 (models CONE0-0 / CONE0-1, knocked over by crafts) | | 129 (sub-type 6) |
| 3 | 8, 9, 10 | Barrel1..3 (models BAREL0-0..2, static) | | |
| 4 | | shots (run time only; 3 = Missile) | | |
| 5 | 0 | collectable Invincible ("Invincable") | | 5 |
| 5 | 1 | collectable Invisible | | 7 |
| 5 | 2, 3, 4 | collectable extra / full / double shield ("Health") | | 15, 10, 5 |
| 5 | 5, 6, 7 | collectable extra / full / double ammo ("Energy") | | 11, 1, 7 |
| 5 | 8, 9, 10 | collectable extra / full / double fuel | | 8, 2, 13 |
| 5 | 11 | collectable minigun upgrade ("Mini gun") | | 46 |
| 5 | 12 | collectable missile upgrade ("Rocket gun") | | 36 |
| 5 | 13 | collectable booster upgrade | | 25 |
| 8 | 0 | trigger Boundary: fires when a craft or recovery truck crosses the ring of cells around the rectangle centred on its cell with half-extents width × height | target group, width, height | 19 |
| 8 | 1 | trigger Timer: fires value + 1 ticks after its activation | target group, value | 9 |
| 8 | 2 | trigger Action | | |
| 8 | 3 | trigger On effect: absorbs 5000 points of explosion damage, then fires | target group | 11 |
| 9 | 0 | wall vector: a solid segment from this entity's cell centre to the linked entity's cell centre (chains form invisible walls) | link | 886 |
| 9 | 1..10 | waypoint link for the computer players, from this entity to the linked one; sub-type = kind: 1 normal, 2 fuel, 3 ammo, 4 shield (charger branches), 6 shortcut, 7 brake, 8 hold fire, 9 fire, 10 special (sub-type 5 does not occur) | link | 172, 7, 7, 8, 6, 26, 9, 70, 2 |
| 10 | 9 | **recovery truck** (vehicle sub-type 9, named "Police helicopter" in the thing table): spawned at the entity's cell, heading 0 | | 32 |

A fired trigger activates its target group and deletes itself. Behaviour of every type is specified in the race
chapters; this table fixes the numbering.

### 9.5 Map regions (pre-placed thing records)

The shipped levels store their charger pads and the start area as 85-byte thing records at the **end** of the thing
table (records 999, 998, ... downwards; 4 to 9 records per level). A record whose first byte is 0 is empty.

| Offset | Size | Field |
|---|---|---|
| 0 | u8 | region kind: **1 shield charger, 2 fuel charger, 3 ammo charger, 4 race start area** |
| 12 | u16 | 1000 + record index (observed; meaning unknown) |
| 16 | u16 | record index (observed) |
| 24 | 8.8 | centre x |
| 26 | 8.8 | centre y |
| 45 | 8.8 | half width (x) |
| 47 | 8.8 | half height (y) |
| others | | unknown / run-time thing fields |

Example, level 1, record 996: kind 4 (start), centre (11.5, 118.5), half size 2.5 × 1.5. Level 1 has kinds 4, 1, 3, 2
in records 996..999.

The original's way of picking the 8 grid positions inside the start area was not decoded (see the race chapters).

### 9.6 Texture attributes and track extent

256 u16 at 357668, indexed by terrain texture id:

| Bits | Meaning |
|---|---|
| 0 (1) | sand: a craft moving over it throws up sand particles |
| 1 (2) | water: throws up water particles |
| 2 (4) | track surface: counts for the track extent and the minimap; steep slopes do not block crafts on it |
| 10..13 | friction class 0..15 (0 is treated as 15) |
| others | unknown |

After loading, the game scans all cells and stores at 358198 / 358200 the largest x and y of cells whose texture has
bit 2 set, each plus 1, and at 358202 / 358204 the values `318 − maxX` and `198 − maxY` (minimap placement).

## 10. Search-ring grid — `DATA/SRCH0-0.DAT`

RNC, unpacked 1024 bytes = a 32 × 32 grid (row-major) describing rings of cells around a centre, used by the big
explosions to place their effects ring by ring. Byte values: 0x00..0x0F = the **start cell** of ring k (all in
column 15, rows 15 down to 0: ring 0 is the centre at (15, 15)); 0x10 + k = another cell of ring k; 0x20 = outside
the rings (low nibble 0 but never reached); 0xFE = unused border (last row and column).

Ring building (done once at start), for k = 0..15:
```
find the first cell, row-major, whose byte == k; centre = that cell when k == 0
add it (offset = cell − centre), mark it used (−1)
base = 0
repeat: for d in [2, 0, 1, 3, 4, 5, 6, 7]:
            step = directions[(d + base) & 7]     ; (dx, dy): (1,0) (1,1) (0,1) (−1,1) (−1,0) (−1,−1) (0,−1) (1,−1)
            neighbour inside the grid, byte ≥ 0 (as i8) and (byte & 15) == k  → take it:
                base = (base + d) & 6; add it; mark used; continue the repeat
        stop when no neighbour qualifies
```
The rings hold 749 cells in all; ring sizes start 1, 4, 16, 16, 24, 36 (rings 0..5 are all the explosions use). Ring 1
in tracing order is (0, −1), (1, 0), (0, 1), (−1, 0); offsets are (dx, drow) in cells.

## 11. Sample and music containers

`SOUND/SOUND.DAT` and `SOUND/MUSIC.DAT` share one container layout ("bank directory"); SOUND has **6 format slots**,
MUSIC **4**.

**Directory**: the u32 at `file size − 4` is the directory offset. At that offset: one u16 per format slot = number
of banks available in that format; then the bank records, one per bank (the largest count), each holding one 16-byte
slot descriptor per format:

| Offset in descriptor | Size | Field |
|---|---|---|
| 0 | i32 | catalogue offset (−1 = slot absent) |
| 4 | i32 | data offset (−1 = absent; the game tests only this one) |
| 8 | i32 | catalogue length |
| 12 | i32 | data length |

A bank is loaded by choosing the slot from the sound driver (below), refusing `bank ≥ count[slot]`, and reading both
blobs: read 8 bytes; if they start with `RNC`, read the whole RNC stream (18 + packed length bytes) and unpack it;
otherwise take `length` bytes as stored.

**Catalogue**: 32-byte entries. Entry 0 is a header whose u32 at offset 0x1a is the data blob length (for SOUND bank 0
it is 0x4e0..0x5290 larger than the real blob, so do not rely on it). Entries 1.. describe the items:

| Offset | Size | Field |
|---|---|---|
| 0 | 12 | DOS 8.3 file name, NUL-padded (e.g. `ENGINE1.WAV`, `TGAME1.XMI`) |
| 12 | 6 | 0 |
| 0x12 | u32 | offset of the item in the data blob |
| 0x16 | 4 | 0 |
| 0x1a | u32 | item length |
| 0x1e | u16 | always 90; never read |

Item numbers used by the game are **1-based** catalogue entry numbers.

### 11.1 `SOUND/SOUND.DAT`

Slot by the digital driver's code `bits · 100 + kHz`: slot 0 = 1644 (16-bit 44 kHz), 1 = 1622, 2 = 1611, 3 = 822
(8-bit 22 kHz), 4 = 811 (8-bit 11 kHz), 5 = 800 or 801 (8-bit 11 kHz, reduced set). The shipped file fills only slots
3, 4 and 5, for **bank 0** (35 samples: race and front-end effects) and **bank 1** (14 samples: the intro). Each item
is a complete **RIFF WAVE** file, PCM 8-bit unsigned mono; the sample rate is the WAV header's (22050 Hz in slot 3,
11025 Hz in slots 4 and 5; the NULL placeholders say 22050). Read only the `fmt ` and `data` chunks: some files (e.g.
ENGINE1.WAV) end with a chunk whose length runs past the item.

Bank 0 (slot 3 durations):

| # | Name | s | # | Name | s | # | Name | s |
|---|---|---|---|---|---|---|---|---|
| 1 | TESTENG3 | 6.01 | 13 | HELEHIT | 0.70 | 25 | START1 | 0.13 |
| 2 | ENGINE1 | 3.64 | 14 | SWITCH | 0.74 | 26 | START2 | 0.28 |
| 3 | MINISHOT | 0.60 | 15 | LOCKON4 | 0.41 | 27 | STUNT180 | 1.39 |
| 4 | MINBLANK | 0.60 | 16 | NULL (8 bytes) | 0 | 28 | STUNT360 | 1.64 |
| 5 | MISSILE | 1.81 | 17 | WARNING | 0.41 | 29 | STUNT720 | 2.49 |
| 6 | EXPLODE | 1.37 | 18 | PICKUP | 0.74 | 30–32 | NULL | 0 |
| 7 | REFUEL | 0.37 | 19 | SCRAPE2 | 4.86 | 33 | PRINTTYP | 0.11 |
| 8 | POWERUP | 0.36 | 20 | CHANGE | 0.18 | 34 | PRINTRET | 0.17 |
| 9 | BOOST | 1.84 | 21 | SELECT | 0.05 | 35 | WINOPEN | 0.94 |
| 10–12 | RICCO1, RICCO2, RICCO3 | 0.11–0.13 | 22–24 | NULL | 0 | | | |

Slot 5 shortens many samples (e.g. TESTENG3 3.33 s, ENGINE1 2.61 s, SCRAPE2 2.59 s) and repeats some: entry 26 is a
copy of START1, entries 28 and 29 copies of STUNT180, entry 34 a copy of PRINTTYP.

Bank 1 (intro): 1 RICCOS, 2 MINIGUN, 3 TURBO, 4 EXPLODE, 5 MISSILE, 6 HELEHIT, 7 FIREPAST, 8 FIRE, 9 BOOSTER,
10 CURTAIN, 11 PAST, 12 SCRAPE2, 13 SMALLCAR, 14 LARGECAR (slot 5 has a NULL in place of LARGECAR).

### 11.2 `SOUND/MUSIC.DAT`

Not compressed. Slot by the MIDI driver's letter: 0 = `G` (General MIDI, `.GEN`), 1 = `R` (Roland MT-32, `.ROL`),
2 = `F` (FM / OPL, `.XMI`), 3 = `W` (wavetable, `.WTB`); letters are case-insensitive. One bank (0) with 5 tunes per
slot, in this order (tune numbers are 1-based):

| Tune | Name | Use | Sizes GEN / ROL / XMI / WTB (bytes) |
|---|---|---|---|
| 1 | TGAME1 | race tune, tracks 1–2 | 21616 / 20944 / 23216 / 19696 |
| 2 | TGAME2 | race tune, tracks 3–4 | 19600 / 19600 / 20640 / 17408 |
| 3 | TGAME4 | race tune, tracks 5–6 | 16272 / 16288 / 18032 / 10064 |
| 4 | TINTRO2 | intro movie | 4624 / 4624 / 5040 / 3184 |
| 5 | TGAME3 | race tune for track numbers above 6 (unreachable) | 22464 / 19584 / 22448 / 22464 |

Every tune is an **XMIDI** file (Miles AIL "extended MIDI"): an IFF tree `FORM XDIR` (`INFO`: u16 sequence count)
followed by `CAT XMID` holding one `FORM XMID` per sequence with `TIMB` (u16 n, n × {u8 patch, u8 bank}), optional
`RBRN` (branch points) and `EVNT` (events). IFF chunk lengths are big-endian; the contents little-endian. EVNT differs
from Standard MIDI: time is counted in intervals of 1/120 s (tempo events do not change it); a delay is a run of
bytes below 0x80 that are **added**; every event has a status byte (no running status); a note-on is followed by a
variable-length duration in intervals and there are no note-off events; AIL controllers travel as control changes
(110 channel lock, 111 lock protect, 112 voice protect, 113 timbre protect, 114 patch bank select, 115 indirect
prefix, 116 FOR loop start (value = repeat count, 0 = forever), 117 NEXT (value ≥ 64 jumps back to the matching
FOR), 118 clear beat/bar, 119 callback, 120 sequence branch).

### 11.3 FM timbre bank — `SOUND/SAMPLE.OPL` (= `SAMPLE.AD`)

The Miles "Global Timbre Library" used by the OPL drivers for the `.XMI` tunes; the two files are byte-identical
(162 timbres). Header: entries `{u8 patch, u8 bank, u32 offset}` terminated by patch = bank = 0xFF. Timbre at
`offset`: u16 length (14 = two-operator), i8 transpose (for the percussion bank 127: the absolute note the drum plays
at), modulator {0x20, 0x40, 0x60, 0x80, 0xE0 register values}, u8 feedback/connection (register 0xC0), carrier {same
five registers}. Bank 0 holds the melodic timbres, bank 127 the percussion keyed by MIDI note.

## 12. Config and save games — `save/config.dat`, `save00.dat` .. `save04.dat`

One **11449-byte** (0x2cb9) block, read and written verbatim; the only check on loading is the exact size (a file of
another size counts as missing). A save game is the same block; its first 32 bytes are the slot name. The block is
all zero before the defaults are applied. Whenever the config is written, bit 2 of the byte at 0x22 (the cheat flag)
is cleared first.

| Offset | Size | Field | Notes, fresh-config value |
|---|---|---|---|
| 0x0000 | 32 | save-slot / championship name, NUL-terminated | typed up to 12 characters; "C8H18" when saved empty |
| 0x0020 | 2392 | copy of the human player's record (copied back from the race after every race, into the race at its start) | fields below |
| 0x0020 | u16 | player index | |
| 0x0022 | u8 | player flags: bit 0 player active (set when the record is created), **bit 2 cheat flag** (puzzle solved or command-line cheat) | |
| 0x0036 | u16 | player state flags: value 2 = debug overlay (Alt+V), value 0x10 = paused | run time |
| 0x004c | u16 | screen mode word: 1 = 320×200, 8 = 640×480 | written by RESOLUTION and the demo-race R key; **not applied at start** |
| 0x0949 | u16 | view slot 0..3 (keys 1–4; camera view 7 + slot) | |
| 0x094b | u8 | team colour 0..7 | 0 |
| 0x094c | u8 | craft type 0..5 | 0 |
| 0x094d | u8 | network session number 0..9 | 0 |
| 0x094f | string | player name, NUL-terminated, at most 8 characters typed | empty (→ name entry) |
| 0x0977 | u8 | difficulty 0..3 | 0 |
| 0x097c | i32 | music volume 0..0x8000 | 0x7800 |
| 0x0980 | i32 | effects volume 0..0x8000 | 0x7800 |
| 0x0984 | i32 | flag toggled only by an internal command (0x1d) that no key or menu issues; meaning unknown (probably music on/off) | 0 |
| 0x0988 | i32 | the same for internal command 0x1b (probably effects on/off) | 0 |
| 0x098c | i32 | unknown | 4 |
| 0x0990 | i32 | **stereo flag**: 0 = every sound centred (mono) | 0 (the game never writes it) |
| 0x0994 | 50 × 39 | high-score table (below) | |
| 0x1132 | 20 × 312 | second score table (below); not shown by any screen, use unknown | |
| 0x2992 | u32 | index of the human player's record in the race | 0 |
| 0x2996 | i32 | SHADING on/off | 1 |
| 0x299a | i32 | SKY on/off | 1 |
| 0x299e | i32 | near texture distance = min(texture detail, 3), set with every TEXTURE MAPPING change | 2 |
| 0x29a2 | i32 | texture detail level 0..4 | 3 |
| 0x29a6 | i32 | TEXTURE MAPPING on/off | 1 |
| 0x29aa | 6 × 74 | track records, one per track (below) | |
| 0x2b66 | 272 | sliding-puzzle state (survives races and is saved): state at 0x2b66, move/frame counter at 0x2b6a, visible flag at 0x2b6e, random seed (u32) at 0x2b72, grid at 0x2b76 (rows of 16 bytes, 4 × 4 used; tile numbers, −1 = empty) | |
| 0x2c76 | u16 | **championship race number 0..5 = selected track − 1** (shared by single races) | 0 |
| 0x2c7a | u16 | language 0..4 (0 EN, 1 DE, 2 FR, 3 ES, 4 IT) | |
| 0x2c7c | u16 | last used save slot 0..4 | 0 |
| 0x2c7e | 8 × i16 | championship points per player slot | 0 |
| 0x2c8e | 43 | unknown | |

Fresh-config values apply when `config.dat` is missing; the detail options (0x2996..0x29a6) are set to their
defaults at every start before the config is loaded, so a loaded config overrides them. In low-memory mode all five
detail options are forced to 0 after loading.

**High-score row** (39 bytes, 50 rows from 0x0994; the screen shows rows 0..18):

| Offset | Size | Field | Fresh value |
|---|---|---|---|
| 0 | i32 | score (championship points) | 0 |
| 4 | u8 | driver assessment 0..20 (string 149 + n, chapter 02) | 15 (BACK MARKER) |
| 5 | 32 | name, NUL-terminated | "C8H18" |
| 37 | u8 | team colour | row % 8 |
| 38 | u8 | craft type | row % 6 |

**Second score table** (20 records of 312 bytes from 0x1132): 8 × i32 scores (0), 8 × 32-byte names ("C8H18"),
8 × u8 (row % 8), 8 × u8 (row % 6), 8 × u8 (15). Probably the unused team/clone tables; never displayed.

**Track record** (74 bytes, track t at 0x29aa + 74·t):

| Offset | Size | Field | Fresh value |
|---|---|---|---|
| 0 | i32 | best lap, in ticks | 9999 |
| 4 | 32 | best-lap holder's name | "C8H18" |
| 36 | i32 | best race time, in ticks | 9999 |
| 40 | 32 | best-race holder's name | "C8H18" |
| 72 | u8 | laps of a single race on this track, 1..50 | 11, 8, 9, 8, 9, 5 for tracks 1..6 |
| 73 | u8 | unknown | 15 |

## 13. Intro movie — `DATA/INTRO.DAT`

Not packed, 12817488 bytes: a stream of Autodesk FLC chunks without the usual 128-byte FLC header. Every chunk is
`u32 size` (including these 6 bytes), `u16 type`, body.

1. **Header chunk** (12 bytes): size 12, type **0xAF12**, then `u16 frames` = **548**, `u16 width` = 320,
   `u16 height` = 200. Raw bytes: `0c 00 00 00 12 af 24 02 40 01 c8 00`.
2. **Frame chunks**, type **0xF1FA**: `u16 sub-chunk count`, 8 reserved bytes, then the sub-chunks (each `u32 size`,
   `u16 type`, body). The file holds 549 frame chunks: frames 1..548 and a final "ring" frame that the player never
   shows. Any top-level type other than 0xF1FA ends playback.

Sub-chunk types:

| Type | Name | Body |
|---|---|---|
| 4 | COLOR_256 | `u16 packets`; per packet: `u8 skip` (colour index advance), `u8 count` (0 = 256), count × (r, g, b). **In this file the values are 6-bit (0..63) and are loaded into the DAC unchanged.** |
| 11 | COLOR_64 | same layout (6-bit values); not present in the file |
| 15 | BYTE_RUN | full image: per row, 1 ignored byte (old packet count), then until the row has `width` pixels: `i8 n`; n ≥ 0: the next byte repeated n times; n < 0: −n literal bytes |
| 7 | DELTA_FLC | `u16 line count`; per line, words until a packet count: top bits 11 = skip `65536 − word` lines; top bits 10 = put the low byte into the last pixel of the current line, then read the next word; top bits 00 = packet count p; then p packets `u8 column skip`, `i8 n`: n > 0 copies n 2-byte words, n < 0 repeats one 2-byte word −n times; then the next line |
| 12 | DELTA_FLI | `u16 first line`, `u16 line count`; per line `u8 packets`; packet `u8 column skip`, `i8 n`: n > 0 copies n bytes, n < 0 repeats one byte −n times; not present |
| 16 | FLI_COPY | `width · height` raw bytes; not present |
| other | | ignored |

Contents: frame 1 = COLOR_256 (all 256 colours) + BYTE_RUN; frames 2..548 are DELTA_FLC frames (some frames carry no
image sub-chunk and leave the picture unchanged); full palettes arrive with frames 96, 192, 234, 277, 407, 480 and
528, partial ones (2 packets) with frames 140, 162 and 359; the ring frame 549 carries a palette too. Playback,
timing and the sound script are in chapter 10.

## 14. Screen recordings — `flics/game0000.flc` ..

Written by the screen recorder (chapter 03: the shipped game has no way to start it). Standard Autodesk FLC:

| Offset | Size | Value |
|---|---|---|
| 0 | u32 | file size (written when closing) |
| 4 | u16 | 0xAF12 |
| 6 | u16 | frame count (written when closing; excludes the ring frame) |
| 8 | u16 | width 320 |
| 10 | u16 | height 200 |
| 12 | u16 | depth 8 |
| 14 | u16 | flags 3 |
| 16 | u32 | speed 57 (milliseconds per frame) |
| 26 | 4 | creator `BILF` |
| 34 | 4 | updater `BILF` |
| 38 | u16 | aspect x 6 |
| 40 | u16 | aspect y 5 |
| 80 | u32 | offset of frame 1 |
| 84 | u32 | offset of frame 2 |
| other | | 0 (header is 128 bytes) |

The file name is the first unused `game%04d.flc` (0..9999). Recording works only in 320×200. Each captured frame is a
0xF1FA chunk with a COLOR_256 sub-chunk (one packet of 256 colours; whether the original scales its 6-bit palette to
8 bits is not documented, the port writes 8-bit values) when the palette changed since the previous frame, then one image sub-chunk: the first frame BYTE_RUN, or FLI_COPY if BYTE_RUN would not be smaller than
`width · height + 16`; later frames DELTA_FLI if smaller than both DELTA_FLC and BYTE_RUN, else DELTA_FLC if smaller
than BYTE_RUN, else BYTE_RUN (or FLI_COPY under the same rule). Closing appends a ring frame (last picture back to the
first) and completes the header.

## 15. Other files

- `movie/movie%03d` and `movie/paket%03d`: recorded input streams referenced by the executable; none ship and their
  layout is not documented.
- `SOUND/*.DIG`, `*.MDI`, `SOUND/DIG.INI`, `MDI.INI`: Miles AIL 3.02 drivers and their card settings
  (`DRIVER`, `IO_ADDR`, `IRQ`, `DMA_8_BIT`, `DMA_16_BIT` lines). A recreation does not need them; the driver choice
  only selects the SOUND.DAT and MUSIC.DAT slots.
- `SOUND/*.SBK`: an AWE32 SoundFont bank referencing ROM samples; not needed.

## 16. Port deviations

- The port reads its files from an asset package: a folder in the original layout or a `.c8pak` bundle (a zip with
  the same layout at its root), names matched in any letter case. It reads and writes `save/config.dat`,
  `save/save%02d.dat` (and its own `keys.txt`, `pad.txt`, `video.txt`) under the package folder; for a bundle, which
  is never written, in a per-user folder (`~/Library/Application Support/c8h18/<bundle name>/save/` on macOS),
  or in `$C8H18_SAVE_DIR` when that is set. Screen recordings (`flics/game%04d.flc`) are not written.
- Port addition, partial packages: a missing or unusable file prints one warning on stdout
  (`warning: missing asset DATA/SKY0-0.DAT: drawing a plain sky`) and is replaced: a built-in palette, empty fonts and
  sprite sheets, plain backdrops, generated search rings (§10), placeholder terrain textures, a gradient sky, colour
  tables computed from the palette (§5: rows 0..31 fade towards the haze, row 32 the identity, rows 33..63 darken,
  nearest palette colours; the blend table; flat colours averaged from the textures). Without `PUZZLE.DAT` the
  sliding puzzle is disabled, without `TMAPS` no billboards are drawn.
- The port's fresh config (when no file exists) follows the original's defaults except: the stereo flag is set (the
  original's fresh config is mono), the track records start at 0 instead of 9999 / "C8H18" (the port treats 0 as
  "no record"), and the second score table is left zero.
- Port addition: `keys.txt`, a UTF-8 text file with one line per bindable action,
  `action = Key Key ...` (up to 4 key names per action; actions `accelerate`, `brake`, `left`, `right`, `fire`,
  `missile`, `booster`, `pause`, `zoom_in`, `zoom_out`); deleting it restores the defaults (chapter 03).
