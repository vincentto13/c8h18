//! Audio / intro test harness.
//!
//!   cargo run --release --example audio_test -- check          parse banks, tunes, intro (headless)
//!   cargo run --release --example audio_test -- samples        play a few bank-0 samples
//!   cargo run --release --example audio_test -- tune N [secs]  play tune N live (default 1, 10 s)
//!   cargo run --release --example audio_test -- wav N secs OUT render tune N to a WAV file
//!   cargo run --release --example audio_test -- introwav OUT   render the intro sound track (script) to WAV
//!   cargo run --release --example audio_test -- intro          run the intro script live (sound only)
//!   cargo run --release --example audio_test -- loops          dump XMIDI loop/volume controllers
//!   cargo run --release --example audio_test -- note P N OUT   render one note N on melodic timbre P to WAV
#![allow(dead_code)]

use c8_data as data;
#[path = "../../src/audio/mod.rs"]
mod audio;
#[path = "../../src/intro.rs"]
mod intro;

use audio::Audio;
use std::time::{Duration, Instant};

fn write_wav(path: &str, rate: u32, buf: &[f32]) {
    let mut out = Vec::with_capacity(44 + buf.len() * 2);
    let data_len = (buf.len() * 2) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &x in buf {
        out.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).expect("write wav");
    println!("wrote {path}");
}

fn stats(buf: &[f32]) {
    let peak = buf.iter().fold(0f32, |a, &x| a.max(x.abs()));
    let rms = (buf.iter().map(|&x| (x * x) as f64).sum::<f64>() / buf.len().max(1) as f64).sqrt();
    let clipped = buf.iter().filter(|&&x| x.abs() >= 0.999).count();
    println!("peak {peak:.3}  rms {rms:.4}  clipped samples {clipped} / {}", buf.len());
}

fn check() {
    let mut a = Audio::new_offline(44100);
    for bank in 0..2 {
        let s = a.load_samples(bank).expect("bank");
        println!("SOUND.DAT bank {bank}: {} samples", s.len());
        for (i, x) in s.iter().enumerate() {
            println!("  {:2} {:12} {:5} Hz {:6.2} s", i + 1, x.name, x.rate, x.duration());
        }
    }
    for t in 1..=5 {
        let p = a.make_player(t, 44100);
        match p {
            Ok(_) => {}
            Err(e) => println!("tune {t}: ERROR {e}"),
        }
    }
    let file = data::files::read_raw("sound/music.dat").unwrap();
    let items = audio::container::load_bank(&file, 4, 2, 0).unwrap();
    for (i, it) in items.iter().enumerate() {
        let seqs = audio::xmidi::parse(&it.data).unwrap();
        let s = &seqs[0];
        let notes = s.events.iter().filter(|e| matches!(e.ev, audio::xmidi::Ev::Note { .. })).count();
        let fors = s.events.iter().filter(|e| matches!(e.ev, audio::xmidi::Ev::Ctrl { ctrl: 116, .. })).count();
        let nexts = s.events.iter().filter(|e| matches!(e.ev, audio::xmidi::Ev::Ctrl { ctrl: 117, .. })).count();
        println!(
            "tune {} {:12} {:6} bytes  {} seqs  {} timbres  {} events  {} notes  len {:.1} s  FOR {} NEXT {}",
            i + 1,
            it.name,
            it.data.len(),
            seqs.len(),
            s.timbres.len(),
            s.events.len(),
            notes,
            s.length() as f64 / 120.0,
            fors,
            nexts
        );
        let bank = audio::music::TimbreBank::parse(&data::files::read_raw("sound/sample.opl").unwrap()).unwrap();
        let missing: Vec<_> = s.timbres.iter().filter(|t| !bank.map.contains_key(&(t.1, t.0))).collect();
        if !missing.is_empty() {
            println!("   timbres missing from SAMPLE.OPL: {missing:?}");
        }
    }
    // Intro.
    let raw = data::files::read_raw("data/intro.dat").unwrap();
    println!("INTRO.DAT {} bytes", raw.len());
    let flc = intro::Flc::new(raw).unwrap();
    println!("header frames {}  chunks {}  {}x{}", flc.frame_count, flc.chunks(), flc.width, flc.height);
    let mut px = vec![0u8; 64000];
    let mut pal = [[0u8; 3]; 256];
    let mut pal_frames = Vec::new();
    let mut no_pic = 0;
    let mut max_pal = 0;
    let t0 = Instant::now();
    for n in 0..flc.chunks() {
        let info = flc.decode(n, &mut px, &mut pal);
        if info.palette {
            pal_frames.push(n + 1);
        }
        if !info.picture {
            no_pic += 1;
        }
        for c in pal.iter() {
            max_pal = max_pal.max(c[0]).max(c[1]).max(c[2]);
        }
    }
    println!(
        "decoded {} chunks in {:?}; palettes at frames {:?}; frames without picture {}; max palette value {}",
        flc.chunks(),
        t0.elapsed(),
        pal_frames,
        no_pic,
        max_pal
    );
    // Ring frame should bring the picture back to frame 1.
    let mut px1 = vec![0u8; 64000];
    let mut pal1 = [[0u8; 3]; 256];
    flc.decode(0, &mut px1, &mut pal1);
    let diff = px.iter().zip(px1.iter()).filter(|(a, b)| a != b).count();
    println!("ring frame vs frame 1: {diff} differing pixels");
    // Timed run through the Intro type with a silent Audio.
    let mut a = Audio::new(false);
    let mut it = intro::Intro::new().unwrap();
    let mut t = 0.0;
    let mut calls = 0;
    while !it.advance(1.0 / 70.0, &mut a) {
        t += 1.0 / 70.0;
        calls += 1;
    }
    println!("Intro::advance finished after {calls} calls = {t:.2} s simulated, last frame {}", it.frame());
}

fn play_samples() {
    let mut a = Audio::new(true);
    println!("enabled {}", a.enabled());
    a.load_bank(0);
    for (s, rate, pan) in [(25, 100, 0x8000), (26, 100, 0x8000), (6, 100, 0x0000), (6, 100, 0xffff), (5, 100, 0x8000), (29, 100, 0x8000), (35, 100, 0x8000), (9, 150, 0x8000)] {
        println!("sample {s} rate {rate} pan {pan:#x}");
        a.play(0, s, 0x7fff, pan, rate, false, audio::MODE_RESTART);
        while a.is_playing(0, s) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    println!("engine loop with rising rate");
    for r in (0..=300).step_by(5) {
        a.update(0, 1, 0x7fff, 0x8000, r.max(1), true, audio::MODE_ONCE);
        std::thread::sleep(Duration::from_millis(40));
    }
    a.stop_all();
}

fn play_tune(n: usize, secs: f64) {
    let mut a = Audio::new(true);
    println!("enabled {}", a.enabled());
    a.play_tune(n, true);
    std::thread::sleep(Duration::from_secs_f64(secs));
    a.stop_music();
}

fn render_tune(n: usize, secs: f64, out: &str) {
    let rate = 44100;
    let mut a = Audio::new_offline(rate);
    a.play_tune(n, true);
    let mut buf = vec![0f32; (secs * rate as f64) as usize * 2];
    let t0 = Instant::now();
    a.render(&mut buf);
    println!("rendered {secs} s in {:?}", t0.elapsed());
    stats(&buf);
    write_wav(out, rate, &buf);
}

fn intro_wav(out: &str) {
    let rate = 44100;
    let mut a = Audio::new_offline(rate);
    let mut it = intro::Intro::new().unwrap();
    let step = 1.0 / 120.0;
    let mut buf = Vec::new();
    let mut chunk = vec![0f32; (rate as f64 * step) as usize * 2];
    while !it.advance(step, &mut a) {
        a.render(&mut chunk);
        buf.extend_from_slice(&chunk);
    }
    stats(&buf);
    write_wav(out, rate, &buf);
}

fn intro_live() {
    let mut a = Audio::new(true);
    let mut it = intro::Intro::new().unwrap();
    let mut last = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(14));
        let now = Instant::now();
        let dt = (now - last).as_secs_f64();
        last = now;
        if it.advance(dt, &mut a) {
            break;
        }
    }
    it.finish(&mut a);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned();
    match arg(0).as_deref() {
        Some("samples") => play_samples(),
        Some("tune") => play_tune(arg(1).and_then(|s| s.parse().ok()).unwrap_or(1), arg(2).and_then(|s| s.parse().ok()).unwrap_or(10.0)),
        Some("wav") => render_tune(
            arg(1).and_then(|s| s.parse().ok()).unwrap_or(1),
            arg(2).and_then(|s| s.parse().ok()).unwrap_or(30.0),
            &arg(3).unwrap_or("tune.wav".into()),
        ),
        Some("introwav") => intro_wav(&arg(1).unwrap_or("intro.wav".into())),
        Some("intro") => intro_live(),
        Some("loops") => dump_loops(),
        Some("note") => note_test(arg(1).unwrap().parse().unwrap(), arg(2).unwrap().parse().unwrap(), &arg(3).unwrap()),
        _ => check(),
    }
}

#[allow(unused)]
pub fn dump_loops() {
    let file = data::files::read_raw("sound/music.dat").unwrap();
    let items = audio::container::load_bank(&file, 4, 2, 0).unwrap();
    for it in &items {
        let s = &audio::xmidi::parse(&it.data).unwrap()[0];
        let mut t = 0u64;
        for (i, e) in s.events.iter().enumerate() {
            t += e.delta as u64;
            if let audio::xmidi::Ev::Ctrl { ch, ctrl, val } = e.ev {
                if ctrl >= 110 || ctrl == 7 && i < 40 || ctrl == 10 && i < 40 {
                    println!("{} ev {i} t {:.2}s ch {ch} ctrl {ctrl} val {val}", it.name, t as f64 / 120.0);
                }
            }
        }
        println!("{} timbres {:?}", it.name, s.timbres);
    }
}

/// Render a single A4 (440 Hz) note on timbre `prog` and write it out for pitch checking.
pub fn note_test(prog: u8, note: u8, out: &str) {
    use audio::xmidi::{Ev, Sequence, XEvent};
    let bank = audio::music::TimbreBank::parse(&data::files::read_raw("sound/sample.opl").unwrap()).unwrap();
    let seq = Sequence {
        timbres: vec![],
        events: vec![
            XEvent { delta: 0, ev: Ev::Program { ch: 0, prog } },
            XEvent { delta: 0, ev: Ev::Ctrl { ch: 0, ctrl: 7, val: 127 } },
            XEvent { delta: 0, ev: Ev::Note { ch: 0, note, vel: 127, dur: 120 } },
            XEvent { delta: 200, ev: Ev::End },
        ],
    };
    let mut p = audio::music::MusicPlayer::new(seq, bank, 44100);
    p.looping = false;
    let mut buf = vec![0f32; 44100 * 2];
    p.render(&mut buf, 1.0, true);
    stats(&buf);
    write_wav(out, 44100, &buf);
}
