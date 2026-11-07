# 10 — Intro movie

Scope: the intro sequence shown at every start: how the original launches it, the movie file and its decoding quirks,
frame count, size, timing and duration, palette handling, the sound and music script (frame → sample), skipping, and
what happens afterwards. The byte layout of `DATA/INTRO.DAT` is in chapter 01, the script op semantics in chapter 09.

## 1. Launch

Nothing special

## 2. Player set-up

- Video: VGA mode 13h, **320 × 200**, 256 colours; the palette starts black.
- Timer: **120 Hz**. With sound it comes from the sound library's timer service; without sound the PC timer is
  reprogrammed to divisor 0x26af (≈ 120.5 Hz). Each timer tick increments a counter.
- Keyboard: its own interrupt handler records the last key.
- Sound: the Miles digital and MIDI drivers configured by `DIG.INI` / `MDI.INI` (unless silent).
- Initial frame delay: 21 ticks (replaced by the script before the first frame).

## 3. The movie

- `DATA/INTRO.DAT`, 12817488 bytes: a 12-byte header chunk (type 0xAF12: **548 frames, 320 × 200**) followed by
  549 frame chunks (frames 1..548 and a ring frame that is never shown).
- Frame 1: a full 256-colour palette and a BYTE_RUN picture. Frames 2..548: DELTA_FLC pictures (some frames carry no
  picture change). New palettes at frames 96, 192, 234, 277, 407, 480, 528 (all 256 colours) and 140, 162, 359 (partial).

**Quirks a recreation must reproduce:**

1. **Palette values are 6-bit.** The COLOR_256 chunks of this file hold 0..63 and the player writes them to the VGA
   DAC unchanged (a standard FLC player would treat them as 8-bit and show a very dark picture). COLOR_64 chunks are
   handled the same way (none occur). To display on an 8-bit device: `v8 = (v << 2) | (v >> 4)`.
2. **No 128-byte header**: the stream starts with the 12-byte 0xAF12 chunk; any top-level chunk that is not 0xF1FA
   ends playback.
3. The player's DELTA_FLC "last byte of the line" opcode (top bits 10) stores the byte and then wrongly reuses the
   opcode word as the packet count; the file contains no such opcode, so a correct decoder gives the same pictures.
4. Playback stops when the frame counter (548) runs out, so the ring frame is never shown.

## 4. Frame loop and timing

```
frame = 0
run the script records for frame 0
repeat:
    frame += 1
    run the script records for this frame        ; before decoding
    read and decode the next frame chunk into the 320×200 buffer
    if delay != 0:
        if the frame carried a palette: wait for the vertical retrace, load the palette into the DAC
        copy the 64000-byte buffer to the screen
        wait until the tick counter exceeds the delay, then reset the counter to 0
        load the palette again
    stop after frame 548, or after this frame if a key was pressed
```

- The script sets the delay to **8 ticks at frame 0**, so every frame lasts **9 ticks = 75 ms (13.33 frames per
  second)** and the whole movie **548 × 75 ms = 41.1 s** (≈ 40.9 s with the timer fallback of 120.5 Hz).
- A delay of 0 would show nothing and not wait.

## 5. Skipping

Any key ends the movie **after the current frame** (the key is noticed after the frame's wait). A debug left-over:
the last key being F skips the wait and turns the samples off, but it also ends the movie like any key.

## 6. Sound script

Samples are numbered 1-based in SOUND.DAT **bank 1** (loaded by the script): 1 RICCOS, 2 MINIGUN, 3 TURBO,
4 EXPLODE, 5 MISSILE, 6 HELEHIT, 7 FIREPAST, 8 FIRE, 9 BOOSTER, 10 CURTAIN, 11 PAST, 12 SCRAPE2, 13 SMALLCAR,
14 LARGECAR. Tune 4 of MUSIC.DAT is TINTRO2. "Loop" = R (endless, reuse the voice), "play" = S (once), "stop" = T.
All at volume 127 (maximum), centred, 100 % rate.

The 46 records, in table order:

| Frame | Operations |
|---|---|
| 0 | delay = 8 ticks; load sample bank 1 |
| 1 | loop 8 FIRE |
| 13 | play tune 4 (TINTRO2) |
| 50 | play 11 PAST |
| 51 | play 6 HELEHIT |
| 64 | play 11 PAST |
| 96 | stop 8 FIRE; loop 13 SMALLCAR |
| 104 | play 2 MINIGUN |
| 107 | play 1 RICCOS |
| 113 | stop 2; play 2 MINIGUN |
| 127 | stop 2 |
| 129 | stop 1; play 3 TURBO |
| 138 | stop 3; play 9 BOOSTER |
| 140 | stop 13; loop 14 LARGECAR |
| 141 | play 5 MISSILE |
| 162 | stop 14; loop 13 SMALLCAR |
| 170 | play 7 FIREPAST |
| 174 | stop 7; play 4 EXPLODE |
| 192 | stop 13 |
| 205 | play 11 PAST |
| 234 | loop 13 SMALLCAR |
| 257 | play 3 TURBO |
| 270 | stop 3; play 9 BOOSTER |
| 275 | play 11 PAST |
| 277 | stop 13 |
| 300 | play 12 SCRAPE2 |
| 330 | stop 12 |
| 349 | play 4 EXPLODE |
| 355 | play 11 PAST |
| 404 | loop 8 FIRE |
| 460 | play 3 TURBO |
| 470 | stop 3; play 9 BOOSTER |
| 477 | play 11 PAST |
| 480 | stop 8 |
| 532 | stop the music; play 10 CURTAIN |

At 75 ms per frame, frame n is shown at about (n − 1) × 0.075 s: the music starts at 0.9 s, the curtain at 39.8 s.
Without sound (`-s`) the B/E/M/R/S/T/F/X operations do nothing; the A operation still sets the delay.

## 7. Ending

When the movie ends (last frame or key), the player restores the keyboard handler, stops its timer, stops all samples,
shuts the sound system down, frees its buffers and returns to text mode. The game then continues its own start-up:
it shows `TITLE.DAT` in 320×200 while loading, fades it out (16 steps) and fades the front end in (chapter 02). The
intro's music does not continue into the game; the title and the front end are silent.

## 8. Port deviations

- The intro is the first screen of the same program (no separate executable); `--no-intro` skips it.
- Frames are timed at (delay + 1) / 120 s from the wall clock; a long stall does not fast-forward.
- In 640×480 the 320×200 movie is pixel-doubled into the middle 640×400 rows (the original always plays at 320×200).
- The script's sound operations are performed by the port's own mixer and the emulated OPL3 (FM variant of TINTRO2).
- After the intro the port shows the title splash for 3 s or until a key.
