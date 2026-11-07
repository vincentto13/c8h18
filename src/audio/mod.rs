//! Audio system (spec chapter 09). PUBLIC API IS FIXED — other modules call it.
//! Implementation: software mixer on cpal, SOUND.DAT banks, XMIDI music on an emulated OPL3.
//!
//! Samples come from SOUND.DAT slot 3 (8-bit 22050 Hz) and are resampled by linear
//! interpolation to the device rate; music is MUSIC.DAT slot 2 (FM .XMI) driven through the
//! GTL timbre bank SAMPLE.OPL on an emulated OPL3, sequenced at 120 Hz inside the callback.
//! All state shared with the callback sits behind one `Arc<Mutex<Shared>>`.

pub mod container;
pub mod mixer;
pub mod music;
pub mod opl3;
pub mod wav;
pub mod xmidi;

use crate::data::files;
use mixer::Shared;
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
}

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

    fn with(enabled: bool, shared: Arc<Mutex<Shared>>, stream: Option<cpal::Stream>) -> Audio {
        Audio { enabled, shared, _stream: stream, sound_dat: None, timbres: None, tunes: None }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load SOUND.DAT bank (0 = game, 1 = intro) from the 8-bit 22 kHz slot; replaces the current bank.
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

    /// Decode bank `bank` of SOUND.DAT slot 3 (also used by tools/tests).
    pub fn load_samples(&mut self, bank: usize) -> Result<Vec<Arc<wav::Sample>>, String> {
        if self.sound_dat.is_none() {
            self.sound_dat = Some(files::read_raw("sound/sound.dat")?);
        }
        let file = self.sound_dat.as_ref().unwrap();
        let items = container::load_bank(file, container::SOUND_SLOTS, container::SOUND_SLOT, bank)?;
        Ok(items
            .into_iter()
            .map(|it| {
                let d = crate::data::rnc::maybe_unpack(it.data).unwrap_or_default();
                Arc::new(wav::parse(&d))
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
    pub fn play_tune(&mut self, tune: usize, full_volume: bool) {
        if !self.enabled {
            return;
        }
        let rate = self.lock().out_rate;
        match self.make_player(tune, rate) {
            Ok(p) => {
                let mut sh = self.lock();
                sh.music = Some(p);
                sh.music_full = full_volume;
            }
            Err(e) => log::warn!("tune {tune}: {e}"),
        }
    }

    /// Build a music player for `tune` at output rate `rate` (also used for offline rendering).
    pub fn make_player(&mut self, tune: usize, rate: u32) -> Result<music::MusicPlayer, String> {
        if self.timbres.is_none() {
            self.timbres = Some(music::TimbreBank::parse(&files::read_raw("sound/sample.opl")?)?);
        }
        if self.tunes.is_none() {
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
        self.lock().music = None;
    }
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
