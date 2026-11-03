//! Linux and the other Unix desktops' side of the audio worker's bridge
//! ([`crate::audio`]), as `tools/audio_mac.cpp` is macOS': PCM voices mixed
//! here into one stereo stream of the default output device (ALSA, which
//! PipeWire and PulseAudio also serve, through cpal), and MIDI played by a
//! SoundFont synthesizer (rustysynth) with a General MIDI SoundFont of the
//! system. The functions keep the C bridges' signatures, so the worker and
//! its scheduler stay shared.
#![allow(non_snake_case)]
use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::{
    ffi::c_void,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// As many voices as the macOS bridge mixes.
const VOICES: usize = 32;

#[derive(Default)]
struct Voice {
    samples: Vec<f32>,
    position: f64,
    step: f64,
    gain: f32,
    pan: f32,
    id: i32,
    group: i32,
    channels: usize,
    looped: bool,
}
struct Mix {
    voices: Vec<Voice>,
    serial: i32,
}
impl Mix {
    /// Adds the voices to `out`, interleaved stereo (`tools/audio_mac.cpp`).
    fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        for v in self.voices.iter_mut().filter(|v| v.id != 0) {
            let count = v.samples.len() / v.channels;
            // The centibel pan law of the Windows output matrix.
            let left = v.gain * 10f32.powf(-v.pan.max(0.) / 2000.);
            let right = v.gain * 10f32.powf(v.pan.min(0.) / 2000.);
            for f in 0..frames {
                if v.position >= count as f64 {
                    if v.looped {
                        v.position %= count as f64;
                    } else {
                        v.id = 0;
                        break;
                    }
                }
                let a = v.position as usize;
                let b = if a + 1 < count {
                    a + 1
                } else if v.looped {
                    0
                } else {
                    a
                };
                let fraction = (v.position - a as f64) as f32;
                for c in 0..2 {
                    let channel = if v.channels == 1 { 0 } else { c };
                    let x = v.samples[a * v.channels + channel];
                    let y = v.samples[b * v.channels + channel];
                    out[f * 2 + c] += (x + (y - x) * fraction) * if c == 0 { left } else { right };
                }
                v.position += v.step;
            }
            if !v.looped && v.position >= count as f64 {
                v.id = 0;
            }
        }
    }
}

/// A stream of the default output device fed by `fill`, which writes
/// interleaved stereo; `make` receives the device's sample rate.
fn open_stream<F>(make: impl FnOnce(u32) -> F) -> Result<(cpal::Stream, u32), String>
where
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no audio output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = config.sample_rate().0;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let fill = make(rate);
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32, F>(&device, &config, fill),
        cpal::SampleFormat::I16 => build::<i16, F>(&device, &config, fill),
        cpal::SampleFormat::U16 => build::<u16, F>(&device, &config, fill),
        cpal::SampleFormat::I32 => build::<i32, F>(&device, &config, fill),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}
fn build<T, F>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut fill: F,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let channels = config.channels as usize;
    let mut stereo = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels.max(1);
                stereo.clear();
                stereo.resize(frames * 2, 0.);
                fill(&mut stereo);
                for (f, frame) in data.chunks_mut(channels.max(1)).enumerate() {
                    let (l, r) = (stereo[f * 2], stereo[f * 2 + 1]);
                    for (c, sample) in frame.iter_mut().enumerate() {
                        let x = match (channels, c) {
                            (1, _) => (l + r) / 2.,
                            (_, 0) => l,
                            (_, 1) => r,
                            _ => 0.,
                        };
                        *sample = T::from_sample(x.clamp(-1., 1.));
                    }
                }
            },
            |e| eprintln!("ReTzar audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

pub struct Engine {
    mix: Arc<Mutex<Mix>>,
    rate: u32,
    _stream: cpal::Stream,
}
fn engine<'a>(e: *mut c_void) -> &'a Engine {
    // SAFETY: the worker passes the pointer `tz_audio_create` returned.
    unsafe { &*(e as *const Engine) }
}

pub unsafe fn tz_audio_create(error: *mut i32) -> *mut c_void {
    let mix = Arc::new(Mutex::new(Mix {
        voices: (0..VOICES).map(|_| Voice::default()).collect(),
        serial: 0,
    }));
    let shared = mix.clone();
    match open_stream(move |_| {
        move |out: &mut [f32]| {
            if let Ok(mut m) = shared.lock() {
                m.render(out);
            }
        }
    }) {
        Ok((stream, rate)) => Box::into_raw(Box::new(Engine {
            mix,
            rate,
            _stream: stream,
        })) as *mut c_void,
        Err(e) => {
            eprintln!("ReTzar audio: {e}");
            // SAFETY: the worker passes a valid out pointer.
            unsafe { *error = -1 };
            std::ptr::null_mut()
        }
    }
}
pub unsafe fn tz_audio_destroy(e: *mut c_void) {
    if !e.is_null() {
        // SAFETY: made by `tz_audio_create`, destroyed once.
        drop(unsafe { Box::from_raw(e as *mut Engine) });
    }
}
#[allow(clippy::too_many_arguments)]
pub unsafe fn tz_audio_play(
    e: *mut c_void,
    bytes: *const u8,
    size: u32,
    rate: u32,
    channels: u16,
    bits: u16,
    group: i32,
    looped: i32,
    gain: f32,
    pan: f32,
) -> i32 {
    if bytes.is_null()
        || rate == 0
        || !(1..=2).contains(&channels)
        || !(bits == 8 || bits == 16)
        || size == 0
        || size % (channels as u32 * bits as u32 / 8) != 0
    {
        return -50;
    }
    // SAFETY: the worker passes a PCM buffer of `size` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(bytes, size as usize) };
    let samples: Vec<f32> = if bits == 8 {
        bytes
            .iter()
            .map(|&b| (b as i32 - 128) as f32 / 128.)
            .collect()
    } else {
        bytes
            .chunks_exact(2)
            .map(|w| i16::from_le_bytes([w[0], w[1]]) as f32 / 32768.)
            .collect()
    };
    let e = engine(e);
    let mut voice = Voice {
        samples,
        position: 0.,
        step: rate as f64 / e.rate as f64,
        gain,
        pan: pan.clamp(-10000., 10000.),
        id: 0,
        group,
        channels: channels as usize,
        looped: looped != 0,
    };
    let Ok(mut mix) = e.mix.lock() else {
        return -108;
    };
    let Some(free) = mix.voices.iter().position(|v| v.id == 0) else {
        return -108;
    };
    // No signed overflow, and no collision with a voice still playing.
    loop {
        mix.serial = if mix.serial == i32::MAX {
            1
        } else {
            mix.serial + 1
        };
        let serial = mix.serial;
        if !mix.voices.iter().any(|v| v.id == serial) {
            break;
        }
    }
    voice.id = mix.serial;
    // The previous buffer is freed after the lock, away from the audio thread.
    let old = std::mem::replace(&mut mix.voices[free], voice);
    let id = mix.voices[free].id;
    drop(mix);
    drop(old);
    id
}
pub unsafe fn tz_audio_voice_active(e: *mut c_void, id: i32) -> i32 {
    let mix = engine(e).mix.lock();
    (id > 0 && mix.is_ok_and(|m| m.voices.iter().any(|v| v.id == id))) as i32
}
pub unsafe fn tz_audio_stop_voice(e: *mut c_void, id: i32) {
    if let Ok(mut m) = engine(e).mix.lock() {
        m.voices
            .iter_mut()
            .filter(|v| v.id == id)
            .for_each(|v| v.id = 0);
    }
}
pub unsafe fn tz_audio_stop(e: *mut c_void, group: i32) {
    if let Ok(mut m) = engine(e).mix.lock() {
        m.voices
            .iter_mut()
            .filter(|v| v.group == group)
            .for_each(|v| v.id = 0);
    }
}
pub unsafe fn tz_audio_set(e: *mut c_void, id: i32, gain: f32, pan: f32) {
    if let Ok(mut m) = engine(e).mix.lock() {
        for v in m.voices.iter_mut().filter(|v| v.id == id) {
            v.gain = gain;
            v.pan = pan.clamp(-10000., 10000.);
        }
    }
}

/// A General MIDI SoundFont: `RETZAR_SOUNDFONT`, one beside the executable,
/// or one of the system's (`soundfont-fluid` and the like).
fn soundfont() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RETZAR_SOUNDFONT") {
        return Some(PathBuf::from(path));
    }
    let sf2 = |dir: PathBuf| -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sf2")))
            .collect();
        found.sort();
        found.into_iter().next()
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let known = [
        "/usr/share/soundfonts/FluidR3_GM.sf2",
        "/usr/share/soundfonts/default.sf2",
        "/usr/share/sounds/sf2/FluidR3_GM.sf2",
        "/usr/share/sounds/sf2/default-GM.sf2",
        "/usr/share/sounds/sf2/TimGM6mb.sf2",
    ];
    beside
        .and_then(sf2)
        .or_else(|| known.iter().map(PathBuf::from).find(|p| p.is_file()))
        .or_else(|| {
            [
                "/usr/share/soundfonts",
                "/usr/share/sounds/sf2",
                "/usr/local/share/soundfonts",
            ]
            .into_iter()
            .find_map(|d| sf2(PathBuf::from(d)))
        })
}

pub struct Midi {
    synth: Arc<Mutex<Synthesizer>>,
    _stream: cpal::Stream,
}
fn midi<'a>(m: *mut c_void) -> &'a Midi {
    // SAFETY: the worker passes the handle `midiOutOpen` gave.
    unsafe { &*(m as *const Midi) }
}

/// WinMM's argument layout, as the macOS bridge keeps it.
pub unsafe fn midiOutOpen(
    handle: *mut *mut c_void,
    _device: u32,
    _callback: usize,
    _instance: usize,
    _flags: u32,
) -> u32 {
    // SAFETY: the worker passes a valid out pointer.
    unsafe { *handle = std::ptr::null_mut() };
    let Some(path) = soundfont() else {
        eprintln!(
            "ReTzar: no General MIDI SoundFont found, the music stays silent. \
             Install one (soundfont-fluid, fluid-soundfont-gm…), put a .sf2 \
             beside the executable or set RETZAR_SOUNDFONT."
        );
        return 1;
    };
    let font = match std::fs::File::open(&path)
        .map_err(|e| e.to_string())
        .and_then(|mut f| SoundFont::new(&mut f).map_err(|e| format!("{e:?}")))
    {
        Ok(font) => Arc::new(font),
        Err(e) => {
            eprintln!("ReTzar: SoundFont {}: {e}", path.display());
            return 2;
        }
    };
    let mut made = None;
    let opened = open_stream(|rate| {
        let synth = SynthesizerSettings::new(rate as i32);
        let synth = Synthesizer::new(&font, &synth).map(|s| Arc::new(Mutex::new(s)));
        let shared = synth.as_ref().ok().cloned();
        made = Some(synth);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        move |out: &mut [f32]| {
            let frames = out.len() / 2;
            left.resize(frames, 0.);
            right.resize(frames, 0.);
            if let Some(Ok(mut s)) = shared.as_ref().map(|s| s.lock()) {
                s.render(&mut left, &mut right);
                for f in 0..frames {
                    out[f * 2] += left[f];
                    out[f * 2 + 1] += right[f];
                }
            }
        }
    });
    match (opened, made) {
        (Ok((stream, _)), Some(Ok(synth))) => {
            // SAFETY: as above.
            unsafe {
                *handle = Box::into_raw(Box::new(Midi {
                    synth,
                    _stream: stream,
                })) as *mut c_void
            };
            0
        }
        (Err(e), _) => {
            eprintln!("ReTzar music: {e}");
            3
        }
        (_, Some(Err(e))) => {
            eprintln!("ReTzar music: {e:?}");
            4
        }
        _ => 5,
    }
}
pub unsafe fn midiOutShortMsg(m: *mut c_void, message: u32) -> u32 {
    let status = (message & 0xff) as i32;
    if let Ok(mut s) = midi(m).synth.lock() {
        s.process_midi_message(
            status & 0x0f,
            status & 0xf0,
            ((message >> 8) & 127) as i32,
            ((message >> 16) & 127) as i32,
        );
    }
    0
}
pub unsafe fn midiOutReset(m: *mut c_void) -> u32 {
    if let Ok(mut s) = midi(m).synth.lock() {
        // All sound off on every channel, as the macOS bridge resets.
        for channel in 0..16 {
            s.process_midi_message(channel, 0xb0, 120, 0);
        }
    }
    0
}
pub unsafe fn midiOutClose(m: *mut c_void) -> u32 {
    if !m.is_null() {
        // SAFETY: made by `midiOutOpen`, closed once.
        drop(unsafe { Box::from_raw(m as *mut Midi) });
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires the default audio device and a General MIDI SoundFont; plays a chord"]
    fn the_soundfont_synthesizer_plays_a_note() {
        let mut handle = std::ptr::null_mut();
        // SAFETY: the handle is used only between open and close.
        unsafe {
            assert_eq!(midiOutOpen(&mut handle, u32::MAX, 0, 0, 0), 0);
            assert_eq!(midiOutShortMsg(handle, 0xc0), 0); // piano
            for note in [60u32, 64, 67] {
                midiOutShortMsg(handle, 0x90 | (note << 8) | (100 << 16));
            }
        }
        let peak = {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let mut left = vec![0.; 4800];
            let mut right = vec![0.; 4800];
            midi(handle)
                .synth
                .lock()
                .unwrap()
                .render(&mut left, &mut right);
            left.iter().chain(&right).fold(0f32, |m, x| m.max(x.abs()))
        };
        std::thread::sleep(std::time::Duration::from_millis(700));
        // SAFETY: as above.
        unsafe {
            midiOutReset(handle);
            midiOutClose(handle);
        }
        assert!(peak > 0.01, "silent synthesizer: peak {peak}");
    }
}
