//! Audio system (spec chapter 09). PUBLIC API IS FIXED — other modules call it.
//! Implementation: software mixer on cpal, SOUND.DAT banks, XMIDI music on an emulated OPL3.
//!
//! Samples come from SOUND.DAT slot 0 (16-bit 44100 Hz) when the file has it, else slot 3
//! (8-bit 22050 Hz, the shipped file), and are resampled by linear
//! interpolation to the device rate; music is MUSIC.DAT slot 2 (FM .XMI) driven through the
//! GTL timbre bank SAMPLE.OPL on an emulated OPL3, sequenced at 120 Hz inside the callback.
//! All state shared with the callback sits behind one `Arc<Mutex<Shared>>`.

pub mod mixer;
pub use c8_audio::{container, music, ogg, tune, wav, xmidi};

use crate::data::files;
use mixer::Shared;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Voice choice modes (spec 09 §2).
pub const MODE_ANY: u8 = 1;
pub const MODE_ONCE: u8 = 2; // do nothing if (owner, sample) still playing
pub const MODE_RESTART: u8 = 3; // restart voice holding (owner, sample)

pub struct Audio {
    enabled: bool,
    shared: Arc<Mutex<Shared>>,
    _stream: Option<cpal::Stream>,
    sound_dat: Option<Vec<u8>>,
    timbres: Option<music::TimbreBank>,
    tunes: Option<Vec<container::Item>>,
    /// Bumped by every play_tune / stop_music, so a late OGG decode does not start a stale tune.
    music_gen: Arc<AtomicU64>,
    /// Decoded OGG tunes by path (with the file's length and modification time).
    oggs: OggCache,
}

type OggCache = Arc<Mutex<HashMap<PathBuf, (u64, Option<std::time::SystemTime>, Arc<ogg::OggTune>)>>>;

impl Audio {
    /// Create the audio system. `enabled == false` (the -s switch) makes every call a no-op.
    pub fn new(enabled: bool) -> Audio {
        if enabled {
            match open_stream() {
                Ok((stream, shared)) => return Audio::with(true, shared, Some(stream)),
                Err(e) => log::warn!("audio disabled: {e}"),
            }
        }
        Audio::with(false, Arc::new(Mutex::new(Shared::new(44100))), None)
    }

    /// An enabled audio system without an output device; pull samples with [`Audio::render`].
    #[allow(dead_code)] // used by examples/audio_test
    pub fn new_offline(rate: u32) -> Audio {
        Audio::with(true, Arc::new(Mutex::new(Shared::new(rate))), None)
    }

    /// Mix the next `out.len() / 2` stereo frames (interleaved L R) — offline use only.
    #[allow(dead_code)] // used by examples/audio_test
    pub fn render(&mut self, out: &mut [f32]) {
        self.lock().mix(out);
    }

    /// Whether sound output is working.
    #[allow(dead_code)] // used by examples/audio_test
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[allow(dead_code)] // used by examples/audio_test
    pub fn is_playing(&self, owner: u32, sample: u16) -> bool {
        self.enabled && self.lock().find(owner, sample).is_some()
    }

    fn with(enabled: bool, shared: Arc<Mutex<Shared>>, stream: Option<cpal::Stream>) -> Audio {
        Audio { enabled, shared, _stream: stream, sound_dat: None, timbres: None, tunes: None, music_gen: Arc::default(), oggs: OggCache::default() }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load SOUND.DAT bank (0 = game, 1 = intro) from the 16-bit 44.1 kHz slot, or the 8-bit 22 kHz
    /// one when that is absent; replaces the current bank.
    pub fn load_bank(&mut self, bank: usize) {
        if !self.enabled {
            return;
        }
        let samples = match self.load_samples(bank) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("SOUND.DAT bank {bank}: {e}");
                Vec::new()
            }
        };
        let mut sh = self.lock();
        for v in sh.voices.iter_mut() {
            v.active = false;
        }
        sh.bank = samples;
    }

    /// Decode bank `bank` of SOUND.DAT: slot 0 (16-bit 44.1 kHz) when the file has the bank there,
    /// else slot 3 (8-bit 22 kHz, the shipped file) (also used by tools/tests).
    pub fn load_samples(&mut self, bank: usize) -> Result<Vec<Arc<wav::Sample>>, String> {
        if self.sound_dat.is_none() {
            self.sound_dat = Some(files::read_raw("sound/sound.dat")?);
        }
        let file = self.sound_dat.as_ref().unwrap();
        let (slot, items) = container::load_sound_bank(file, bank)?;
        log::debug!("SOUND.DAT bank {bank}: {} samples from slot {slot}", items.len());
        Ok(items
            .into_iter()
            .map(|it| {
                let d = crate::data::rnc::maybe_unpack(it.data).unwrap_or_default();
                Arc::new(wav::parse(&it.name, &d))
            })
            .collect())
    }

    /// Play a sample (1-based catalogue entry of the loaded bank).
    /// volume 0..0x7fff, pan 0..0xffff (0x8000 centre), rate in percent, mode = MODE_*.
    pub fn play(&mut self, owner: u32, sample: u16, volume: i32, pan: i32, rate: i32, looping: bool, mode: u8) {
        self.play_ex(owner, sample, volume, pan, rate, looping, mode, false);
    }

    /// Like `play` at full volume, centred, 100 %, ignoring the effects volume (intro script).
    pub fn play_full_volume(&mut self, owner: u32, sample: u16, looping: bool, mode: u8) {
        self.play_ex(owner, sample, 0x7fff, 0x8000, 100, looping, mode, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn play_ex(&mut self, owner: u32, sample: u16, volume: i32, pan: i32, rate: i32, looping: bool, mode: u8, full: bool) {
        if !self.enabled {
            return;
        }
        let mut sh = self.lock();
        let Some(data) = (sample as usize).checked_sub(1).and_then(|i| sh.bank.get(i)).cloned() else {
            return;
        };
        let existing = sh.find(owner, sample);
        let slot = match mode {
            MODE_ONCE if existing.is_some() => return,
            MODE_RESTART if existing.is_some() => existing,
            _ => sh.free_voice(),
        };
        let Some(i) = slot else { return };
        let pan = if sh.stereo { pan } else { 0x8000 };
        sh.stamp += 1;
        let stamp = sh.stamp;
        let v = &mut sh.voices[i];
        v.active = !data.data.is_empty();
        v.owner = owner;
        v.sample = sample;
        v.data = Some(data);
        v.pos = 0.0;
        v.volume = volume;
        v.pan = pan;
        v.rate = rate;
        v.looping = looping;
        v.full = full;
        v.stamp = stamp;
    }

    /// play (mode as given), then set volume, rate and pan of the running voice (engine loops).
    pub fn update(&mut self, owner: u32, sample: u16, volume: i32, pan: i32, rate: i32, looping: bool, mode: u8) {
        if !self.enabled {
            return;
        }
        self.play(owner, sample, volume, pan, rate, looping, mode);
        let mut sh = self.lock();
        let stereo = sh.stereo;
        if let Some(i) = sh.find(owner, sample) {
            let v = &mut sh.voices[i];
            v.volume = volume;
            if rate > 0 {
                v.rate = rate;
            }
            if stereo {
                v.pan = pan;
            }
        }
    }

    pub fn stop(&mut self, owner: u32, sample: u16) {
        if !self.enabled {
            return;
        }
        let mut sh = self.lock();
        for v in sh.voices.iter_mut() {
            if v.matches(owner, sample) {
                v.active = false;
            }
        }
    }

    pub fn stop_all(&mut self) {
        if !self.enabled {
            return;
        }
        for v in self.lock().voices.iter_mut() {
            v.active = false;
        }
    }

    /// Configured volumes 0..0x8000.
    pub fn set_effects_volume(&mut self, v: i32) {
        self.lock().effects_volume = v.clamp(0, 0x8000);
    }

    pub fn set_music_volume(&mut self, v: i32) {
        self.lock().music_volume = v.clamp(0, 0x8000);
    }

    /// Stereo flag (config 0x0990); when false every pan is forced to centre.
    pub fn set_stereo(&mut self, on: bool) {
        self.lock().stereo = on;
    }

    /// Play MUSIC.DAT tune (1-based: 1 TGAME1, 2 TGAME2, 3 TGAME4, 4 TINTRO2, 5 TGAME3), looping.
    /// `full_volume` = ignore the configured music volume (intro).
    ///
    /// Port addition: an OGG Vorbis file `SOUND/MUSIC/<tune name>.OGG` replaces the XMIDI tune
    /// (see `c8_audio::tune`). It is decoded on a helper thread (the first time; decoded tunes
    /// are kept) and starts as soon as it is ready; if it cannot be decoded the FM tune plays.
    pub fn play_tune(&mut self, tune: usize, full_volume: bool) {
        if !self.enabled {
            return;
        }
        let rate = self.lock().out_rate;
        let generation = self.music_gen.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(path) = tune::find_ogg(tune) {
            let fallback = self.make_fm_player(tune, rate).ok();
            let (shared, current, cache) = (self.shared.clone(), self.music_gen.clone(), self.oggs.clone());
            let spawned = std::thread::Builder::new().name("ogg-decode".into()).spawn(move || {
                let p: tune::TunePlayer = match load_ogg(&cache, &path) {
                    Ok(t) => ogg::OggPlayer::new(t, rate).into(),
                    Err(e) => {
                        log::warn!("{}: {e}; playing the FM tune", path.display());
                        match fallback {
                            Some(f) => f.into(),
                            None => return,
                        }
                    }
                };
                let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
                if current.load(Ordering::SeqCst) == generation {
                    log::info!("tune {tune}: playing {} ({})", path.display(), if p.is_ogg() { "OGG" } else { "FM fallback" });
                    sh.music = Some(p);
                    sh.music_full = full_volume;
                }
            });
            if let Err(e) = spawned {
                log::warn!("tune {tune}: {e}");
            }
            return;
        }
        match self.make_fm_player(tune, rate) {
            Ok(p) => {
                log::info!("tune {tune}: playing the FM tune ({})", tune::tune_name(tune).unwrap_or("?"));
                let mut sh = self.lock();
                sh.music = Some(p.into());
                sh.music_full = full_volume;
            }
            Err(e) => log::warn!("tune {tune}: {e}"),
        }
    }

    /// Build a player for `tune` at output rate `rate`: its OGG replacement if there is one,
    /// else the XMIDI tune on the OPL3 (also used for offline rendering).
    #[allow(dead_code)] // used by examples/audio_test and the tests
    pub fn make_player(&mut self, tune: usize, rate: u32) -> Result<tune::TunePlayer, String> {
        if let Some(path) = tune::find_ogg(tune) {
            match load_ogg(&self.oggs, &path) {
                Ok(t) => return Ok(ogg::OggPlayer::new(t, rate).into()),
                Err(e) => log::warn!("{}: {e}; using the FM tune", path.display()),
            }
        }
        self.make_fm_player(tune, rate).map(Into::into)
    }

    /// The XMIDI tune `tune` of MUSIC.DAT slot 2 on the OPL3.
    pub fn make_fm_player(&mut self, tune: usize, rate: u32) -> Result<music::MusicPlayer, String> {
        if self.timbres.is_none() {
            log::debug!("FM timbres from {:?}", files::resolve("sound/sample.opl"));
            self.timbres = Some(music::TimbreBank::parse(&files::read_raw("sound/sample.opl")?)?);
        }
        if self.tunes.is_none() {
            log::debug!("tunes from {:?}", files::resolve("sound/music.dat"));
            let file = files::read_raw("sound/music.dat")?;
            self.tunes = Some(container::load_bank(&file, container::MUSIC_SLOTS, container::MUSIC_SLOT, 0)?);
        }
        let item = tune
            .checked_sub(1)
            .and_then(|i| self.tunes.as_ref().unwrap().get(i))
            .ok_or("no such tune")?;
        let seq = xmidi::parse(&item.data)?.swap_remove(0);
        Ok(music::MusicPlayer::new(seq, self.timbres.clone().unwrap(), rate))
    }

    pub fn stop_music(&mut self) {
        if !self.enabled {
            return;
        }
        self.music_gen.fetch_add(1, Ordering::SeqCst);
        self.lock().music = None;
    }
}

/// Decode an OGG tune, or take it from the cache when the file is unchanged.
fn load_ogg(cache: &OggCache, path: &Path) -> Result<Arc<ogg::OggTune>, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let stamp = (meta.len(), meta.modified().ok());
    if let Some((len, time, t)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        if (*len, *time) == stamp {
            return Ok(t.clone());
        }
    }
    let t = Arc::new(ogg::decode(&std::fs::read(path).map_err(|e| e.to_string())?)?);
    log::info!("{}: OGG tune, {:.1} s at {} Hz, loop {}..{}", path.display(), t.seconds(), t.rate, t.loop_start, t.loop_end);
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (stamp.0, stamp.1, t.clone()));
    Ok(t)
}

fn open_stream() -> Result<(cpal::Stream, Arc<Mutex<Shared>>), String> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let shared = Arc::new(Mutex::new(Shared::new(config.sample_rate)));
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, shared.clone()),
        SampleFormat::I16 => build::<i16>(&device, config, shared.clone()),
        SampleFormat::U16 => build::<u16>(&device, config, shared.clone()),
        SampleFormat::I32 => build::<i32>(&device, config, shared.clone()),
        SampleFormat::F64 => build::<f64>(&device, config, shared.clone()),
        f => return Err(format!("unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, shared))
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, shared: Arc<Mutex<Shared>>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let channels = config.channels as usize;
    let mut buf: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels.max(1);
                if buf.len() < frames * 2 {
                    buf.resize(frames * 2, 0.0);
                }
                let mix = &mut buf[..frames * 2];
                match shared.lock() {
                    Ok(mut sh) => sh.mix(mix),
                    Err(_) => mix.fill(0.0),
                }
                for (f, s) in data.chunks_mut(channels.max(1)).zip(mix.chunks_exact(2)) {
                    match f.len() {
                        1 => f[0] = T::from_sample((s[0] + s[1]) * 0.5),
                        _ => {
                            f[0] = T::from_sample(s[0]);
                            f[1] = T::from_sample(s[1]);
                            for x in f[2..].iter_mut() {
                                *x = T::from_sample(0.0);
                            }
                        }
                    }
                }
            },
            |e| log::warn!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod music_tests {
    use super::*;

    /// An OGG at SOUND/MUSIC/<name>.OGG (any case, override folder) replaces that tune only.
    #[test]
    fn ogg_replaces_the_xmidi_tune() {
        files::set_data_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources").into());
        let over = std::env::temp_dir().join(format!("c8-ogg-tune-{}", std::process::id()));
        std::fs::create_dir_all(over.join("sound/music")).unwrap();
        std::fs::write(over.join("sound/music/tgame1.ogg"), include_bytes!("../../../c8-audio/tests/data/loop-test.ogg")).unwrap();
        files::set_override_dir(Some(over.clone()));
        let mut a = Audio::new_offline(44100);
        let p1 = a.make_player(1, 44100);
        let p2 = a.make_player(2, 44100);
        // the game's path: decoded on a helper thread, then playing in the mixer
        a.play_tune(1, false);
        files::set_override_dir(None);
        assert!(p1.unwrap().is_ogg(), "tune 1 should be the OGG");
        if let Ok(p) = p2 {
            assert!(!p.is_ogg(), "tune 2 has no OGG");
        }
        let t0 = std::time::Instant::now();
        while !a.lock().music.as_ref().is_some_and(|m| m.is_ogg()) {
            assert!(t0.elapsed().as_secs() < 10, "the OGG tune did not start");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut buf = vec![0f32; 4410 * 2];
        a.render(&mut buf);
        assert!(buf.iter().any(|x| x.abs() > 0.05), "the OGG tune is silent");
        // a stop before a late decode wins: nothing starts afterwards
        a.stop_music();
        assert!(a.lock().music.is_none());
        let _ = std::fs::remove_dir_all(over);
    }
}
