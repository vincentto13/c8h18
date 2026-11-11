//! Sound output for previews: one sample or one tune at a time, on the default output device
//! (opened on first use).
use c8_audio::tune::TunePlayer;
use std::sync::{Arc, Mutex, OnceLock};

enum Source {
    Sample { data: Arc<Vec<f32>>, rate: u32, pos: f64 },
    /// An FM (XMIDI) or OGG tune.
    Music(Box<TunePlayer>),
}

#[derive(Default)]
struct Shared {
    source: Option<Source>,
    /// what is playing (shown by the documents), and the seconds played
    tag: String,
    played: f64,
}

struct Output {
    shared: Arc<Mutex<Shared>>,
    rate: u32,
}

static OUTPUT: OnceLock<Result<Output, String>> = OnceLock::new();

fn output() -> Result<&'static Output, String> {
    OUTPUT.get_or_init(open).as_ref().map_err(|e| e.clone())
}

/// The output's sample rate (for building music players).
pub fn rate() -> Result<u32, String> {
    output().map(|o| o.rate)
}

/// Play a mono sample (values -1..1 at `rate` Hz); `tag` names it for [`playing`].
pub fn play_sample(tag: &str, data: Arc<Vec<f32>>, rate: u32) -> Result<(), String> {
    let o = output()?;
    let mut sh = o.shared.lock().unwrap_or_else(|e| e.into_inner());
    sh.source = Some(Source::Sample { data, rate, pos: 0.0 });
    sh.tag = tag.into();
    sh.played = 0.0;
    Ok(())
}

pub fn play_music(tag: &str, player: impl Into<TunePlayer>) -> Result<(), String> {
    play_music_from(tag, player, 0.0)
}

/// Like [`play_music`] for a player already moved to `start` seconds (shown by [`playing`]).
pub fn play_music_from(tag: &str, player: impl Into<TunePlayer>, start: f64) -> Result<(), String> {
    let o = output()?;
    let mut sh = o.shared.lock().unwrap_or_else(|e| e.into_inner());
    sh.source = Some(Source::Music(Box::new(player.into())));
    sh.tag = tag.into();
    sh.played = start;
    Ok(())
}

pub fn stop() {
    if let Some(Ok(o)) = OUTPUT.get() {
        let mut sh = o.shared.lock().unwrap_or_else(|e| e.into_inner());
        sh.source = None;
        sh.tag.clear();
    }
}

/// The tag of what is playing and the seconds played so far.
pub fn playing() -> Option<(String, f64)> {
    let Some(Ok(o)) = OUTPUT.get() else { return None };
    let sh = o.shared.lock().unwrap_or_else(|e| e.into_inner());
    sh.source.as_ref().map(|_| (sh.tag.clone(), sh.played))
}

fn mix(sh: &mut Shared, out: &mut [f32], out_rate: u32) {
    out.fill(0.0);
    let frames = out.len() / 2;
    let mut done = false;
    match &mut sh.source {
        None => return,
        Some(Source::Sample { data, rate, pos }) => {
            let step = *rate as f64 / out_rate as f64;
            for f in out.chunks_exact_mut(2) {
                let k = *pos as usize;
                if k >= data.len() {
                    done = true;
                    break;
                }
                let t = (*pos - k as f64) as f32;
                let a = data[k];
                let b = data.get(k + 1).copied().unwrap_or(a);
                let v = (a + (b - a) * t) * 0.8;
                f[0] = v;
                f[1] = v;
                *pos += step;
            }
        }
        Some(Source::Music(p)) => {
            p.render(out, 0.8, true);
            done = p.finished();
        }
    }
    sh.played += frames as f64 / out_rate as f64;
    if done {
        sh.source = None;
    }
}

/// Open the output on a thread of its own, which keeps the stream alive (streams cannot move
/// between threads on every system).
fn open() -> Result<Output, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("audio-preview".into())
        .spawn(move || match start() {
            Ok((stream, shared, rate)) => {
                let _ = tx.send(Ok(Output { shared, rate }));
                let _keep = stream;
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

fn start() -> Result<(cpal::Stream, Arc<Mutex<Shared>>, u32), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no sound output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("output sample format {} not supported", config.sample_format()));
    }
    let config: cpal::StreamConfig = config.into();
    let rate = config.sample_rate;
    let channels = config.channels.max(1) as usize;
    let shared = Arc::new(Mutex::new(Shared::default()));
    let sh = shared.clone();
    let mut buf = Vec::new();
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                buf.resize(frames * 2, 0.0);
                match sh.lock() {
                    Ok(mut s) => mix(&mut s, &mut buf, rate),
                    Err(_) => buf.fill(0.0),
                }
                for (f, s) in data.chunks_mut(channels).zip(buf.chunks_exact(2)) {
                    if f.len() == 1 {
                        f[0] = (s[0] + s[1]) * 0.5;
                    } else {
                        f[0] = s[0];
                        f[1] = s[1];
                        f[2..].fill(0.0);
                    }
                }
            },
            |e| log::warn!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, shared, rate))
}
