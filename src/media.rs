//! Movies through Windows Media Foundation (`tools/media_win.cpp`): the
//! MP4 files of the portable copy and the Italian launcher's MPEG intro,
//! read from memory as RGBA frames and 16-bit sound, and the MP4 files
//! (H.264 and AAC) the release builder writes from the original movies.
//!
//! A [`Clip`] plays any of them, and the American release's DDV movies
//! where the installation is read as it is, through a worker thread that
//! decodes a few frames ahead.
use crate::{Result, audio::Pcm, bitmap::Bitmap, ddv, invalid};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

/// What a movie's bytes are, for the source that reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// MP4: H.264 and AAC.
    Mp4,
    /// MPEG-1 system stream (the Italian `Avance\intro.MPG`).
    Mpeg,
}
impl Kind {
    fn mime(self) -> &'static str {
        match self {
            Kind::Mp4 => "video/mp4",
            Kind::Mpeg => "video/mpeg",
        }
    }
}

#[cfg(windows)]
mod ffi {
    use std::ffi::c_void;
    unsafe extern "C" {
        pub fn tz_media_open(
            bytes: *const u8,
            len: usize,
            kind: *const u16,
            error: *mut i32,
        ) -> *mut c_void;
        pub fn tz_media_close(media: *mut c_void);
        pub fn tz_media_info(
            media: *mut c_void,
            width: *mut u32,
            height: *mut u32,
            seconds: *mut f64,
            rate: *mut u32,
            channels: *mut u32,
            samples: *mut usize,
        );
        pub fn tz_media_pcm(media: *mut c_void, out: *mut i16);
        pub fn tz_media_frame(media: *mut c_void, rgba: *mut u8, time: *mut f64) -> i32;
        pub fn tz_mp4_create(
            path: *const u16,
            width: u32,
            height: u32,
            fps_num: u32,
            fps_den: u32,
            bitrate: u32,
            quality: u32,
            rate: u32,
            channels: u32,
            error: *mut i32,
        ) -> *mut c_void;
        pub fn tz_mp4_video(writer: *mut c_void, rgba: *const u8, time: i64, duration: i64) -> i32;
        pub fn tz_mp4_audio(
            writer: *mut c_void,
            pcm: *const i16,
            frames: u32,
            time: i64,
            duration: i64,
        ) -> i32;
        pub fn tz_mp4_finish(writer: *mut c_void) -> i32;
        pub fn tz_mp4_abandon(writer: *mut c_void);
    }
}

fn checked(hr: i32, what: &str) -> Result<()> {
    if hr < 0 {
        Err(invalid(format!(
            "{what}: errore Windows 0x{:08X}",
            hr as u32
        )))
    } else {
        Ok(())
    }
}
#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// A movie read from memory, frame by frame, on the thread that opened it.
pub struct Media {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
    pub width: u32,
    pub height: u32,
    /// The length the source declares, 0 when unknown.
    pub seconds: f64,
    pub audio: Option<Pcm>,
}
impl Media {
    pub fn open(bytes: &[u8], kind: Kind) -> Result<Self> {
        Self::open_mime(bytes, kind.mime())
    }
    /// A file of the type `mime`; a sound file opens with no picture.
    #[cfg(windows)]
    pub fn open_mime(bytes: &[u8], mime: &str) -> Result<Self> {
        let mime = wide(mime);
        let mut error = 0;
        let handle =
            unsafe { ffi::tz_media_open(bytes.as_ptr(), bytes.len(), mime.as_ptr(), &mut error) };
        checked(error, "Lettura del filmato")?;
        if handle.is_null() {
            return Err(invalid("Lettore dei filmati non disponibile"));
        }
        let (mut width, mut height, mut seconds) = (0, 0, 0.);
        let (mut rate, mut channels, mut samples) = (0, 0, 0);
        unsafe {
            ffi::tz_media_info(
                handle,
                &mut width,
                &mut height,
                &mut seconds,
                &mut rate,
                &mut channels,
                &mut samples,
            );
        }
        let audio = (samples > 0 && rate > 0 && channels > 0).then(|| {
            let mut pcm = vec![0i16; samples];
            unsafe { ffi::tz_media_pcm(handle, pcm.as_mut_ptr()) };
            Pcm {
                rate,
                channels: channels as u16,
                bits: 16,
                bytes: pcm.iter().flat_map(|s| s.to_le_bytes()).collect(),
            }
        });
        Ok(Self {
            handle,
            width,
            height,
            seconds,
            audio,
        })
    }
    #[cfg(not(windows))]
    pub fn open_mime(_: &[u8], _: &str) -> Result<Self> {
        Err(invalid("I filmati richiedono Windows"))
    }
    /// The next frame and its time in seconds; none at the end.
    #[cfg(windows)]
    pub fn next_frame(&mut self) -> Result<Option<(f64, Bitmap)>> {
        let mut rgba = vec![0; self.width as usize * self.height as usize * 4];
        let mut time = 0.;
        let result = unsafe { ffi::tz_media_frame(self.handle, rgba.as_mut_ptr(), &mut time) };
        checked(result, "Decodifica del filmato")?;
        Ok((result == 1).then_some((
            time,
            Bitmap {
                width: self.width,
                height: self.height,
                rgba,
            },
        )))
    }
    #[cfg(not(windows))]
    pub fn next_frame(&mut self) -> Result<Option<(f64, Bitmap)>> {
        Ok(None)
    }
}
#[cfg(windows)]
impl Drop for Media {
    fn drop(&mut self) {
        unsafe { ffi::tz_media_close(self.handle) }
    }
}

/// An MP4 file being written: H.264 pictures and, optionally, AAC sound.
#[cfg(windows)]
pub struct Mp4Writer {
    handle: *mut std::ffi::c_void,
    width: u32,
    height: u32,
    channels: u32,
}
#[cfg(windows)]
impl Mp4Writer {
    /// `fps` as a fraction; the pictures of constant `quality` (1 to 100),
    /// or of constant `bitrate` (bits per second) when it is 0; `sound` as
    /// rate (44 100 or 48 000) and channels.
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: (u32, u32),
        (bitrate, quality): (u32, u32),
        sound: Option<(u32, u32)>,
    ) -> Result<Self> {
        let path = wide(&path.to_string_lossy());
        let (rate, channels) = sound.unwrap_or((0, 0));
        let mut error = 0;
        let handle = unsafe {
            ffi::tz_mp4_create(
                path.as_ptr(),
                width,
                height,
                fps.0,
                fps.1,
                bitrate,
                quality,
                rate,
                channels,
                &mut error,
            )
        };
        checked(error, "Scrittura MP4")?;
        if handle.is_null() {
            return Err(invalid("Scrittura MP4 non disponibile"));
        }
        Ok(Self {
            handle,
            width,
            height,
            channels,
        })
    }
    /// One RGBA picture from `time` for `duration`, in seconds.
    pub fn frame(&mut self, picture: &Bitmap, time: f64, duration: f64) -> Result<()> {
        if (picture.width, picture.height) != (self.width, self.height) {
            return Err(invalid("Fotogramma di misura diversa"));
        }
        let hr = unsafe {
            ffi::tz_mp4_video(
                self.handle,
                picture.rgba.as_ptr(),
                (time * 1e7).round() as i64,
                (duration * 1e7).round() as i64,
            )
        };
        checked(hr, "Scrittura del fotogramma")
    }
    /// Interleaved sound from `time`, in seconds, at the rate given at
    /// creation.
    pub fn sound(&mut self, pcm: &[i16], time: f64, rate: u32) -> Result<()> {
        let frames = pcm.len() / self.channels.max(1) as usize;
        let hr = unsafe {
            ffi::tz_mp4_audio(
                self.handle,
                pcm.as_ptr(),
                frames as u32,
                (time * 1e7).round() as i64,
                (frames as f64 / rate as f64 * 1e7).round() as i64,
            )
        };
        checked(hr, "Scrittura del sonoro")
    }
    pub fn finish(mut self) -> Result<()> {
        let handle = std::mem::replace(&mut self.handle, std::ptr::null_mut());
        checked(unsafe { ffi::tz_mp4_finish(handle) }, "Chiusura MP4")
    }
}
#[cfg(windows)]
impl Drop for Mp4Writer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { ffi::tz_mp4_abandon(self.handle) }
        }
    }
}

/// The sound of a file Windows reads (`mime` such as "audio/flac"), as
/// 16-bit samples.
pub fn decode_sound(bytes: &[u8], mime: &str) -> Result<Pcm> {
    Media::open_mime(bytes, mime)?
        .audio
        .take()
        .ok_or_else(|| invalid("File senza suono"))
}

/// The folder of the movies turned into MP4, in the portable copy's pack.
pub const MOVIES: &str = "Filmati";
/// The MP4 name of a movie of the American release's archive
/// (`MOVIES/TALON.DDV`, a script's `movies\outro.ddv`): `talon`, `outro`.
pub fn movie_name(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    stem.to_ascii_lowercase()
}
/// A movie turned into MP4 (`intro`, `talon`…), when the data hold one.
pub fn converted(source: &Path, name: &str) -> Option<Vec<u8>> {
    crate::vfs::read(&source.join(MOVIES).join(format!("{name}.mp4"))).ok()
}
/// The Italian launcher's intro of the installation, `Avance\intro.MPG`,
/// whatever the case of its names.
pub fn italian_intro(source: &Path) -> Option<std::path::PathBuf> {
    let pick = |dir: &Path, name: &str| {
        crate::vfs::list(dir)
            .ok()?
            .into_iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    };
    let folder = pick(source, "Avance")?;
    pick(&folder, "intro.MPG").filter(|p| crate::vfs::is_file(p))
}

/// Writes a movie as MP4 at `path`: its pictures at their own times, of
/// constant `quality` (1 to 100), and its sound as AAC at 44.1 kHz
/// (22.05 kHz sound doubled first).
#[cfg(windows)]
pub fn to_mp4(source: Source, path: &Path, quality: u32) -> Result<()> {
    let mut decoder = Decoder::open(source)?;
    let header = decoder.header();
    let first = decoder
        .next()?
        .ok_or_else(|| invalid("Filmato senza immagini"))?;
    let second = decoder.next()?;
    let step = second.as_ref().map_or(0.04, |s| s.0 - first.0).max(0.001);
    // Frames per second in thousandths: 25 000/1000, 15 000/1000.
    let fps = ((1. / step) * 1000.).round() as u32;
    let sound = match &header.audio {
        Some(pcm) if pcm.bits == 16 && (1..=2).contains(&pcm.channels) => {
            let samples: Vec<i16> = pcm
                .bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            match pcm.rate {
                22050 => Some((
                    44100,
                    double_rate(&samples, pcm.channels as usize),
                    pcm.channels,
                )),
                44100 | 48000 => Some((pcm.rate, samples, pcm.channels)),
                other => {
                    return Err(invalid(format!(
                        "Frequenza del sonoro non prevista: {other}"
                    )));
                }
            }
        }
        Some(_) => return Err(invalid("Sonoro del filmato non previsto")),
        None => None,
    };
    let mut writer = Mp4Writer::create(
        path,
        header.width,
        header.height,
        (fps, 1000),
        (4_000_000, quality),
        sound.as_ref().map(|(rate, _, ch)| (*rate, u32::from(*ch))),
    )?;
    // Sound and pictures interleaved: the writer waits for a stream that
    // falls behind the other.
    let mut chunks = sound.as_ref().map(|(rate, samples, channels)| {
        (
            *rate,
            samples
                .chunks(1024 * *channels as usize)
                .enumerate()
                .peekable(),
        )
    });
    let mut sound_until = |writer: &mut Mp4Writer, time: f64| -> Result<()> {
        if let Some((rate, parts)) = &mut chunks {
            while let Some((n, part)) =
                parts.next_if(|(n, _)| *n as f64 * 1024. / *rate as f64 <= time)
            {
                writer.sound(part, n as f64 * 1024. / *rate as f64, *rate)?;
            }
        }
        Ok(())
    };
    let mut current = Some(first);
    let mut next = second;
    while let Some((time, picture)) = current {
        sound_until(&mut writer, time + 0.5)?;
        let duration = next.as_ref().map_or(step, |n| (n.0 - time).max(0.001));
        writer.frame(&picture, time, duration)?;
        current = next;
        next = if current.is_some() {
            decoder.next()?
        } else {
            None
        };
    }
    sound_until(&mut writer, f64::INFINITY)?;
    writer.finish()
}

/// 16-bit sound at twice its rate (22 050 to 44 100 Hz, which AAC takes):
/// the samples kept, those between them from a windowed sinc.
pub fn double_rate(pcm: &[i16], channels: usize) -> Vec<i16> {
    const TAPS: isize = 16;
    let channels = channels.max(1);
    let frames = pcm.len() / channels;
    let at = |i: isize, c: usize| -> f64 {
        if i < 0 || i as usize >= frames {
            0.
        } else {
            pcm[i as usize * channels + c] as f64
        }
    };
    // Weights of the samples around a point halfway between two of them.
    let weights: Vec<f64> = (-TAPS..TAPS)
        .map(|k| {
            let x = k as f64 + 0.5;
            let sinc = (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x);
            let window = 0.5 + 0.5 * (std::f64::consts::PI * x / (TAPS as f64 + 0.5)).cos();
            sinc * window
        })
        .collect();
    let mut out = Vec::with_capacity(pcm.len() * 2);
    for i in 0..frames as isize {
        for c in 0..channels {
            out.push(pcm[i as usize * channels + c]);
        }
        for c in 0..channels {
            let v: f64 = (-TAPS..TAPS)
                .zip(&weights)
                .map(|(k, w)| at(i + k + 1, c) * w)
                .sum();
            out.push(v.round().clamp(-32768., 32767.) as i16);
        }
    }
    out
}

/// What a [`Clip`] plays.
pub enum Source {
    Media(Vec<u8>, Kind),
    /// A DDV of the American release, decoded with the tables of the
    /// installation's `Tzar.exe`.
    Ddv(Vec<u8>, Arc<ddv::Tables>),
}

enum Decoder {
    Media(Media),
    Ddv(Box<ddv::Movie>, Arc<ddv::Tables>, usize),
}
impl Decoder {
    fn open(source: Source) -> Result<Self> {
        Ok(match source {
            Source::Media(bytes, kind) => Decoder::Media(Media::open(&bytes, kind)?),
            Source::Ddv(bytes, tables) => {
                Decoder::Ddv(Box::new(ddv::Movie::open(bytes)?), tables, 0)
            }
        })
    }
    fn header(&mut self) -> Header {
        match self {
            Decoder::Media(m) => Header {
                width: m.width,
                height: m.height,
                seconds: m.seconds,
                audio: m.audio.take(),
            },
            Decoder::Ddv(m, _, _) => Header {
                width: m.width,
                height: m.height,
                seconds: m.frames() as f64 / m.fps.max(1) as f64,
                audio: m.audio.take(),
            },
        }
    }
    fn next(&mut self) -> Result<Option<(f64, Bitmap)>> {
        match self {
            Decoder::Media(m) => m.next_frame(),
            Decoder::Ddv(m, tables, decoded) => {
                let time = *decoded as f64 / m.fps.max(1) as f64;
                *decoded += 1;
                Ok(m.next_frame(tables)?.map(|f| (time, f)))
            }
        }
    }
}
struct Header {
    width: u32,
    height: u32,
    seconds: f64,
    audio: Option<Pcm>,
}

/// A movie playing: its size, its sound, and its frames as they fall due.
pub struct Clip {
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    pub audio: Option<Pcm>,
    /// In a mutex only to be shared with the frontend's resources.
    frames: std::sync::Mutex<mpsc::Receiver<Result<(f64, Bitmap)>>>,
    next: Option<(f64, Bitmap)>,
    /// The time of the last frame given, none before the first.
    last: Option<f64>,
    exhausted: bool,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Clip {
    /// Opens a movie on its own thread; returns once its size, its sound
    /// and its first picture are known, so the picture shows at once.
    pub fn open(source: Source) -> Result<Self> {
        let (header_tx, header_rx) = mpsc::channel();
        let (tx, frames) = mpsc::sync_channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let worker = thread::Builder::new()
            .name("ReTzar filmato".into())
            .spawn(move || {
                let mut decoder = match Decoder::open(source) {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = header_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                let _ = header_tx.send(Ok(decoder.header()));
                while !halt.load(Ordering::Relaxed) {
                    match decoder.next() {
                        Ok(Some(frame)) => {
                            if tx.send(Ok(frame)).is_err() {
                                return;
                            }
                        }
                        Ok(None) => return,
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            })?;
        let header = header_rx
            .recv()
            .map_err(|_| invalid("Filmato interrotto"))?
            .map_err(invalid)?;
        let (next, exhausted) = match frames.recv() {
            Ok(frame) => (Some(frame?), false),
            Err(_) => (None, true),
        };
        Ok(Self {
            width: header.width,
            height: header.height,
            seconds: header.seconds,
            audio: header.audio,
            frames: std::sync::Mutex::new(frames),
            next,
            last: None,
            exhausted,
            stop,
            worker: Some(worker),
        })
    }
    /// The newest frame due at `time` seconds, if one is newer than the
    /// last given; the first one at once, whatever its time.
    pub fn frame_at(&mut self, time: f64) -> Result<Option<Bitmap>> {
        let mut shown = None;
        loop {
            if self.next.is_none() && !self.exhausted {
                match self.frames.get_mut().unwrap().try_recv() {
                    Ok(frame) => self.next = Some(frame?),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => self.exhausted = true,
                }
            }
            match self.next.take() {
                Some((at, frame)) if at <= time || self.last.is_none() => {
                    self.last = Some(at);
                    shown = Some(frame);
                }
                Some(later) => {
                    self.next = Some(later);
                    break;
                }
                None => break,
            }
        }
        Ok(shown)
    }
    /// Whether every frame has been shown and the movie's time is over.
    pub fn ended(&self, time: f64) -> bool {
        self.exhausted && self.next.is_none() && time >= self.seconds.max(self.last.unwrap_or(0.))
    }
}
impl Drop for Clip {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Unblock a worker waiting to hand over a frame.
        while self.frames.get_mut().unwrap().try_recv().is_ok() {}
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubling_the_rate_keeps_the_samples_and_fills_between() {
        let pcm: Vec<i16> = (0..64)
            .map(|i| ((i as f64 * 0.3).sin() * 10000.) as i16)
            .collect();
        let out = double_rate(&pcm, 1);
        assert_eq!(out.len(), 128);
        for i in 0..64 {
            assert_eq!(out[i * 2], pcm[i]);
        }
        // Between two samples of a slow wave, close to their middle.
        for i in 20..40 {
            let middle = (pcm[i] as f64 + pcm[i + 1] as f64) / 2.;
            assert!((out[i * 2 + 1] as f64 - middle).abs() < 400., "{i}");
        }
        let stereo = double_rate(&[1, -1, 2, -2], 2);
        assert_eq!(stereo.len(), 8);
        assert_eq!(&stereo[..2], &[1, -1]);
    }

    /// An MP4 written here reads back with its size, its pictures the right
    /// way up and its sound.
    #[cfg(windows)]
    #[test]
    fn an_mp4_written_reads_back() {
        let dir = std::env::temp_dir().join(format!("retzar-mp4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prova.mp4");
        let (w, h) = (320, 200);
        let mut writer =
            Mp4Writer::create(&path, w, h, (25, 1), (1_500_000, 0), Some((44100, 2))).unwrap();
        for i in 0..50 {
            // Red above, blue below.
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let k = ((y * w + x) * 4) as usize;
                    let top = y < h / 2;
                    rgba[k..k + 4].copy_from_slice(if top {
                        &[220, 30, 30, 255]
                    } else {
                        &[30, 30, 220, 255]
                    });
                    if x == (i * 4) as u32 % w {
                        rgba[k..k + 3].copy_from_slice(&[255, 255, 255]);
                    }
                }
            }
            let picture = Bitmap {
                width: w,
                height: h,
                rgba,
            };
            writer.frame(&picture, i as f64 / 25., 1. / 25.).unwrap();
        }
        let tone: Vec<i16> = (0..44100 * 2)
            .map(|i| {
                (((i / 2) as f64 * 440. * 2. * std::f64::consts::PI / 44100.).sin() * 8000.) as i16
            })
            .collect();
        for (n, chunk) in tone.chunks(2048).enumerate() {
            writer
                .sound(chunk, n as f64 * 1024. / 44100., 44100)
                .unwrap();
        }
        writer.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut media = Media::open(&bytes, Kind::Mp4).unwrap();
        assert_eq!((media.width, media.height), (w, h));
        assert!((media.seconds - 2.).abs() < 0.2, "{}", media.seconds);
        let audio = media.audio.take().unwrap();
        assert_eq!((audio.rate, audio.channels, audio.bits), (44100, 2, 16));
        assert!(audio.bytes.len() > 44100 * 2 * 2 / 2);
        let mut frames = 0;
        while let Some((time, picture)) = media.next_frame().unwrap() {
            let at = |x: u32, y: u32| {
                let k = ((y * w + x) * 4) as usize;
                [picture.rgba[k], picture.rgba[k + 1], picture.rgba[k + 2]]
            };
            let top = at(w - 10, 10);
            let bottom = at(w - 10, h - 10);
            assert!(top[0] > 150 && top[2] < 90, "{top:?}");
            assert!(bottom[2] > 150 && bottom[0] < 90, "{bottom:?}");
            assert!((0. ..2.1).contains(&time));
            frames += 1;
        }
        assert_eq!(frames, 50);
        // As a clip: the first picture at once, then the others in time,
        // and the end when the time is over.
        let mut clip = Clip::open(Source::Media(bytes, Kind::Mp4)).unwrap();
        assert!(clip.audio.is_some());
        let first = (0..400).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            clip.frame_at(0.).unwrap()
        });
        assert!(first.is_some());
        let mut time = 0.;
        let mut shown = 1;
        while !clip.ended(time) {
            if clip.frame_at(time).unwrap().is_some() {
                shown += 1;
            }
            time += 0.01;
            std::thread::sleep(std::time::Duration::from_millis(1));
            assert!(time < 10.);
        }
        assert!(shown > 25 && time >= 1.9, "{shown} {time}");
        // Of constant quality, black stays black from the first picture.
        let black = Bitmap {
            width: 64,
            height: 48,
            rgba: [0, 0, 0, 255].repeat(64 * 48),
        };
        let mut writer = Mp4Writer::create(&path, 64, 48, (15, 1), (4_000_000, 90), None).unwrap();
        for i in 0..5 {
            writer.frame(&black, i as f64 / 15., 1. / 15.).unwrap();
        }
        writer.finish().unwrap();
        let mut media = Media::open(&std::fs::read(&path).unwrap(), Kind::Mp4).unwrap();
        let (_, first) = media.next_frame().unwrap().unwrap();
        let brightest = first.rgba.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).max();
        assert!(brightest.unwrap() < 12, "{brightest:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The Italian launcher's MPEG intro reads through Media Foundation.
    #[cfg(windows)]
    #[test]
    #[ignore = "Requires this installation's original game data"]
    fn the_italian_intro_reads_from_memory() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bytes = std::fs::read(source.join("Avance/intro.MPG")).unwrap();
        let mut media = Media::open(&bytes, Kind::Mpeg).unwrap();
        println!(
            "intro {}×{} {:.1} s, sonoro {:?}",
            media.width,
            media.height,
            media.seconds,
            media
                .audio
                .as_ref()
                .map(|a| (a.rate, a.channels, a.bytes.len()))
        );
        assert!(media.width >= 320 && media.height >= 200 && media.seconds > 10.);
        assert!(media.audio.is_some());
        let mut frames = 0;
        let mut last = -1.;
        while let Some((time, _)) = media.next_frame().unwrap() {
            assert!(time > last);
            last = time;
            frames += 1;
            if frames == 100 {
                break;
            }
        }
        assert_eq!(frames, 100);
    }
}
