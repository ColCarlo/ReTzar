//! Original PCM WAVE and Standard MIDI files, with an isolated native audio worker.
mod ambience;
use crate::{Result, archive::Archive, assets::Ini, invalid, u32_at};
pub use ambience::{Catalog, Habitat, Moment, NatureContext, Scheduler};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
    pub bytes: Vec<u8>,
}
impl Pcm {
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WAVE") {
            return Err(invalid("Missing RIFF WAVE header"));
        }
        let end = 8usize
            .checked_add(u32_at(data, 4)? as usize)
            .ok_or("WAVE size overflow")?;
        if end > data.len() || end < 12 {
            return Err(invalid("Truncated RIFF"));
        }
        let mut at = 12;
        let mut format = None;
        let mut samples = None;
        while at < end {
            if at + 8 > end {
                return Err(invalid("Truncated WAVE chunk"));
            }
            let size = u32_at(data, at + 4)? as usize;
            let next = at.checked_add(8 + size).ok_or("WAVE chunk overflow")?;
            if next > end {
                return Err(invalid("Truncated WAVE chunk data"));
            }
            let body = &data[at + 8..next];
            match &data[at..at + 4] {
                b"fmt " => {
                    if body.len() < 16 {
                        return Err(invalid("Truncated WAVE format"));
                    }
                    let word = |n| u16::from_le_bytes(body[n..n + 2].try_into().unwrap());
                    let (tag, channels, rate, bits, align) =
                        (word(0), word(2), u32_at(body, 4)?, word(14), word(12));
                    if tag != 1
                        || !(1..=2).contains(&channels)
                        || ![8, 16].contains(&bits)
                        || !(8000..=192000).contains(&rate)
                        || align != channels * bits / 8
                        || u32_at(body, 8)? != rate * align as u32
                    {
                        return Err(invalid(format!(
                            "Unsupported WAVE: format {tag}, {channels} channels, {rate} Hz, {bits} bits"
                        )));
                    }
                    format = Some((channels, rate, bits, align));
                }
                b"data" => samples = Some(body),
                _ => {}
            }
            at = next + size % 2;
            if at > end {
                return Err(invalid("Missing WAVE padding"));
            }
        }
        let (channels, rate, bits, align) = format.ok_or("WAVE has no fmt chunk")?;
        let bytes = samples.ok_or("WAVE has no data chunk")?;
        if bytes.is_empty() || bytes.len() % align as usize != 0 {
            return Err(invalid("Invalid PCM sample alignment"));
        }
        Ok(Self {
            channels,
            rate,
            bits,
            bytes: bytes.to_vec(),
        })
    }
    pub fn seconds(&self) -> f64 {
        self.bytes.len() as f64 / (self.rate as f64 * self.channels as f64 * self.bits as f64 / 8.)
    }
}

#[derive(Clone, Debug)]
pub struct MidiEvent {
    pub seconds: f64,
    pub message: u32,
}
#[derive(Clone, Debug)]
pub struct Midi {
    pub events: Vec<MidiEvent>,
    pub seconds: f64,
}
fn be16(data: &[u8], n: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(
        data.get(n..n + 2).ok_or("Truncated MIDI u16")?.try_into()?,
    ))
}
fn be32(data: &[u8], n: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        data.get(n..n + 4).ok_or("Truncated MIDI u32")?.try_into()?,
    ))
}
fn byte(data: &[u8], at: &mut usize) -> Result<u8> {
    let b = *data.get(*at).ok_or("Truncated MIDI event")?;
    *at += 1;
    Ok(b)
}
fn vlq(data: &[u8], at: &mut usize) -> Result<u32> {
    let mut v = 0;
    for _ in 0..4 {
        let b = byte(data, at)?;
        v = (v << 7) | (b & 127) as u32;
        if b < 128 {
            return Ok(v);
        }
    }
    Err(invalid("MIDI variable length value exceeds four bytes"))
}
impl Midi {
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.get(..4) != Some(b"MThd") {
            return Err(invalid("Missing MIDI header"));
        }
        let header = be32(data, 4)? as usize;
        if header < 6 || data.len() < 8 + header {
            return Err(invalid("Truncated MIDI header"));
        }
        let (format, tracks, division) = (be16(data, 8)?, be16(data, 10)?, be16(data, 12)?);
        if format > 1
            || tracks == 0
            || tracks > 256
            || division == 0
            || division & 0x8000 != 0
            || (format == 0 && tracks != 1)
        {
            return Err(invalid("Unsupported MIDI format or time division"));
        }
        // Stable merge preserves track order for simultaneous tempo/program/note events.
        let mut raw: Vec<(u64, Option<u32>, Option<u32>)> = Vec::new();
        let mut at = 8 + header;
        let mut last_tick = 0;
        for _ in 0..tracks {
            if data.get(at..at + 4) != Some(b"MTrk") {
                return Err(invalid("Missing MIDI track"));
            }
            let size = be32(data, at + 4)? as usize;
            let body = data
                .get(at + 8..at + 8 + size)
                .ok_or("Truncated MIDI track")?;
            at += 8 + size;
            let (mut cursor, mut tick, mut running, mut ended) = (0, 0u64, 0u8, false);
            while cursor < body.len() {
                tick = tick
                    .checked_add(vlq(body, &mut cursor)? as u64)
                    .ok_or("MIDI tick overflow")?;
                if tick > division as u64 * 60 * 60 * 12 {
                    return Err(invalid("MIDI exceeds 12 hours"));
                }
                last_tick = last_tick.max(tick);
                let first = byte(body, &mut cursor)?;
                let status = if first < 128 {
                    if running == 0 {
                        return Err(invalid("MIDI running status without channel event"));
                    }
                    cursor -= 1;
                    running
                } else {
                    first
                };
                match status {
                    0x80..=0xef => {
                        running = status;
                        let a = byte(body, &mut cursor)?;
                        let b = if [0xc0, 0xd0].contains(&(status & 0xf0)) {
                            0
                        } else {
                            byte(body, &mut cursor)?
                        };
                        if a > 127 || b > 127 {
                            return Err(invalid("Invalid MIDI channel data"));
                        }
                        raw.push((
                            tick,
                            Some(status as u32 | ((a as u32) << 8) | ((b as u32) << 16)),
                            None,
                        ));
                    }
                    0xff => {
                        let kind = byte(body, &mut cursor)?;
                        let n = vlq(body, &mut cursor)? as usize;
                        let v = body
                            .get(cursor..cursor + n)
                            .ok_or("Truncated MIDI metadata")?;
                        cursor += n;
                        if kind == 0x51 {
                            if v.len() != 3 {
                                return Err(invalid("Invalid MIDI tempo"));
                            }
                            let tempo = ((v[0] as u32) << 16) | ((v[1] as u32) << 8) | v[2] as u32;
                            if tempo == 0 {
                                return Err(invalid("Zero MIDI tempo"));
                            }
                            raw.push((tick, None, Some(tempo)));
                        }
                        if kind == 0x2f {
                            if n != 0 {
                                return Err(invalid("Invalid MIDI end marker"));
                            }
                            ended = true;
                            break;
                        }
                    }
                    0xf0 | 0xf7 => {
                        running = 0;
                        let n = vlq(body, &mut cursor)? as usize;
                        body.get(cursor..cursor + n).ok_or("Truncated MIDI SysEx")?;
                        cursor += n;
                        // The original tracks use General MIDI; no hardware-specific SysEx is sent.
                    }
                    _ => return Err(invalid("Unsupported MIDI system event")),
                }
            }
            if !ended {
                return Err(invalid("MIDI track has no end marker"));
            }
        }
        raw.sort_by_key(|v| v.0);
        let (mut tick, mut time, mut tempo) = (0, 0., 500000u32);
        let mut events = Vec::new();
        for (next, msg, change) in raw {
            time += (next - tick) as f64 * tempo as f64 / (division as f64 * 1e6);
            tick = next;
            if let Some(t) = change {
                tempo = t;
            }
            if let Some(message) = msg {
                events.push(MidiEvent {
                    seconds: time,
                    message,
                });
            }
        }
        time += (last_tick - tick) as f64 * tempo as f64 / (division as f64 * 1e6);
        if events.is_empty() {
            return Err(invalid("MIDI has no channel events"));
        }
        Ok(Self {
            events,
            seconds: time.max(0.1),
        })
    }
}

pub struct AudioBank {
    archive: Archive,
    pcm: BTreeMap<String, Pcm>,
    midi: BTreeMap<String, Midi>,
    /// `[priority]` of SOUNDS.INI for each category (0x4cdd50 with
    /// 0x499660: a byte, 0 for a missing key).
    priorities: [u8; CATEGORIES.len()],
    music: Music,
}
/// Mission songs from `[SOUND]` of Tzar.ini.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Music {
    first: u8,
    last: u8,
    pause: Duration,
}
impl Music {
    fn read(ini: &crate::assets::Ini) -> Self {
        let value = |key: &str, default: u32| {
            ini.get("SOUND", key)
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(default)
        };
        let first = value("STARTSONG", 1).clamp(1, 99) as u8;
        Self {
            first,
            last: (value("ENDSONG", 10).clamp(1, 99) as u8).max(first),
            pause: Duration::from_millis(value("SONGPAUSE", 10_000) as u64),
        }
    }
}
impl AudioBank {
    pub fn open(source: &Path) -> Result<Self> {
        let archive = crate::assets::sounds(source)?;
        let mut midi = BTreeMap::new();
        for e in &archive.entries {
            let name = e.name.to_ascii_uppercase();
            if name.ends_with(".MID") {
                midi.insert(
                    name,
                    Midi::decode(&archive.read(e)?).map_err(|err| format!("{}: {err}", e.name))?,
                );
            }
        }
        let assets = crate::assets::GameAssets::open(source)?;
        let sounds = Ini::parse(&assets.get("DATA/SOUNDS.INI")?);
        let table: BTreeMap<String, u32> = sounds
            .entries("priority")
            .iter()
            .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.trim().parse().ok()?)))
            .collect();
        let priorities = CATEGORIES.map(|(key, _)| table.get(key).map_or(0, |&p| p as u8));
        let music = Music::read(&Ini::parse(
            &crate::vfs::read(&source.join("Tzar.ini")).unwrap_or_default(),
        ));
        Ok(Self {
            archive,
            pcm: BTreeMap::new(),
            midi,
            priorities,
            music,
        })
    }
    fn wave(&mut self, name: &str) -> Result<&Pcm> {
        let key = crate::archive::key(name);
        if !self.pcm.contains_key(&key) {
            let wave = Pcm::decode(&self.archive.get(&key)?)?;
            self.pcm.insert(key.clone(), wave);
        }
        Ok(&self.pcm[&key])
    }
}
pub fn audit(source: &Path) -> Result<serde_json::Value> {
    let archive = crate::assets::sounds(source)?;
    let mut waves = Vec::new();
    let mut music = Vec::new();
    for e in &archive.entries {
        if e.name.to_ascii_uppercase().ends_with(".WAV") {
            let p = Pcm::decode(&archive.read(e)?).map_err(|err| format!("{}: {err}", e.name))?;
            waves.push(serde_json::json!({"name":e.name,"rate":p.rate,"channels":p.channels,"bits":p.bits,"seconds":p.seconds()}));
        } else if e.name.to_ascii_uppercase().ends_with(".MID") {
            let m = Midi::decode(&archive.read(e)?).map_err(|err| format!("{}: {err}", e.name))?;
            music.push(
                serde_json::json!({"name":e.name,"events":m.events.len(),"seconds":m.seconds}),
            );
        }
    }
    Ok(
        serde_json::json!({"wav_count":waves.len(),"midi_count":music.len(),"waves":waves,"music":music}),
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct SoundState {
    pub sound: u8,
    pub music: u8,
    /// Nature calls and rain (PLAYNATURESOUNDS, checked by 0x4cf700).
    pub ambience: bool,
    pub focused: bool,
    /// Biome of the open mission and the generation of its load.
    pub scene: Option<(String, u32)>,
    /// A movie plays: it takes the music away (0x47f7cf), and the next song
    /// of the list follows it.
    pub movie: bool,
    /// The story window open, by the number [`Player::story`] gave it.
    pub story: Option<u32>,
    /// The music switched on (0xc97a94): off it stops the song and the
    /// check (0x47d5b7); on again it starts the next song at once (0x47d537).
    pub music_on: bool,
    /// The options page open over a game: opening it with no song of the
    /// list playing starts the next at once (0x47b916).
    pub options: bool,
}

/// Small xorshift generator for the worker's random choices.
struct Rng(u32);
impl Rng {
    fn seeded() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |t| t.subsec_nanos() ^ t.as_secs() as u32);
        Self(nanos | 1)
    }
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 % n.max(1)
    }
}

/// Mission songs STARTSONG..ENDSONG, shuffled once (Fisher-Yates) for the
/// whole session as the original does at start-up (0x44145d).
fn playlist(music: Music, rng: &mut Rng) -> Vec<u8> {
    let mut songs: Vec<u8> = (music.first..=music.last).collect();
    for i in (1..songs.len()).rev() {
        songs.swap(i, rng.below(i as u32 + 1) as usize);
    }
    songs
}
/// The short delay of the music check's messages (0x64).
const BEAT: Duration = Duration::from_millis(100);
/// How often the check looks again while the music is held (0x3e8).
const LOOK: Duration = Duration::from_millis(1000);

/// The mission screen's music check (0x4f3c70). The original drives it
/// with messages it posts to itself after a delay: 0x360bc02 looks whether
/// the music is free, every second while it is not, and once it is posts
/// 0x3f95282 after the pause, which starts the next song of the list. So
/// a song follows the end of what held the music (a song, a story window,
/// a movie) after the pause plus up to a second, as the look comes round.
/// A start that finds the music held is dropped, and with it the check:
/// then only the options page or the music switched on start a song.
#[derive(Clone, Debug, Default, PartialEq)]
struct MusicCheck {
    /// The look (0x360bc02) and the start (0x3f95282) posted, by when due.
    look: Option<Instant>,
    start: Option<Instant>,
    /// The pause before a start (0x58f5d8): 100 ms, SONGPAUSE after a
    /// song of the list ended by itself (0x4cd450).
    gap: Duration,
    /// The music held (0xc97aa0): by the song of the list playing, a story
    /// window or a movie.
    held: bool,
}
impl MusicCheck {
    /// A game begins: 0x441500 posts 0x3c52322 in 100 ms, which sets the
    /// pause to 100 ms and posts the look 100 ms later (0x4f3d09).
    fn begin(now: Instant) -> Self {
        Self {
            look: Some(now + BEAT * 2),
            start: None,
            gap: BEAT,
            held: false,
        }
    }
    /// The messages due by `now`; whether the next song starts.
    fn step(&mut self, now: Instant, focused: bool, on: bool) -> bool {
        let mut go = false;
        // A look (0x4f3d35): nothing with the music off; again in a second
        // while the music is held or the game is in the background (0x475370);
        // else the start after the pause.
        if let Some(t) = self.look.filter(|&t| t <= now) {
            self.look = None;
            if on {
                if self.held || !focused {
                    self.look = Some(t + LOOK);
                } else {
                    self.start = Some(t + self.gap);
                }
            }
        }
        if let Some(t) = self.start.filter(|&t| t <= now) {
            self.start = None;
            go = self.start_at(t, focused, on);
        }
        go
    }
    /// A start (0x4f3cc5), posted by a look or sent at once (0x4ce5a0):
    /// none while the music is held or off; in the background it looks again
    /// in a second; else the next song (0x4cd730), which holds the music
    /// and sets the pause back to 100 ms, and a look in a second.
    fn start_at(&mut self, t: Instant, focused: bool, on: bool) -> bool {
        if self.held || !on {
            return false;
        }
        self.look = Some(t + LOOK);
        if !focused {
            return false;
        }
        self.gap = BEAT;
        self.held = true;
        true
    }
    /// A song of the list ended by itself (0x4cd450 with 1): the music is
    /// free and the next one waits SONGPAUSE.
    fn ended(&mut self, pause: Duration) {
        self.held = false;
        self.gap = pause;
    }
}
/// Where a story's own song is kept among the songs.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
const STORY_SONG: &str = "STORY";

/// Categories of the original's sounds (0x4cdd50): the `[priority]` key of
/// SOUNDS.INI each takes, and how many effects of the category play at once
/// (0xc97ad8). Category 0 is nature and rain.
pub const CATEGORIES: [(&str, u8); 27] = [
    ("", 0),
    ("obj_select", 200),
    ("obj_order", 200),
    ("obj_create", 200),
    ("chop", 4),
    ("build", 4),
    ("dig", 2),
    ("fire", 3),
    ("attack_far", 6),
    ("attack_far", 6),
    ("attack_far", 6),
    ("attack_close", 5),
    ("attack_close", 5),
    ("attack_far", 5),
    ("explosion", 4),
    ("explosion", 4),
    ("explosion", 4),
    ("explosion", 4),
    ("explosion", 5),
    ("explosion", 4),
    ("mapedit_object", 5),
    ("mapedit_action", 5),
    ("death", 3),
    ("death", 3),
    ("obj_unit_ready", 200),
    ("obj_building_ready", 200),
    ("obj_taunt", 8),
];
/// The categories the game passes (vt+0x1c8 = 0x505710 and its callers).
pub mod category {
    /// Selection voices (0x4535e0).
    pub const SELECT: u8 = 1;
    /// Command voices, gates and gambling.
    pub const ORDER: u8 = 2;
    pub const CHOP: u8 = 4;
    pub const BUILD: u8 = 5;
    /// Digging and mining.
    pub const DIG: u8 = 6;
    pub const FIRE: u8 = 7;
    /// Bows, crossbows, mages, catapults and the messiah.
    pub const SHOT: u8 = 8;
    /// Longbows, ships and buildings that shoot (0x4dbbb0).
    pub const HEAVY_SHOT: u8 = 9;
    pub const DRAGON: u8 = 10;
    /// Blows (0x4df6e0).
    pub const BLOW: u8 = 11;
    /// Spells, potions and a priest's flame bolt.
    pub const SPELL: u8 = 13;
    /// Small explosions and meteors.
    pub const BLAST: u8 = 15;
    pub const SPLASH: u8 = 17;
    /// Lightning, turning to stone, flame balls and freezing.
    pub const BOLT: u8 = 19;
    pub const DEATH: u8 = 22;
    /// Buildings and walls falling.
    pub const COLLAPSE: u8 = 23;
    pub const UNIT_READY: u8 = 24;
    pub const BUILDING_READY: u8 = 25;
    /// The sounds of the map's objects placed with the editor (0x50299c),
    /// `mapedit_object` of `[priority]`.
    pub const MAP_OBJECT: u8 = 20;
}

/// The channels of the original with 32 hardware voices (0x4ceaac): 3 for
/// nature and rain, 19 for effects, 4 for the voices of units and 4 for
/// those of buildings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Nature,
    Effect,
    Unit,
    Building,
}
impl Channel {
    const ALL: [Channel; 4] = [
        Channel::Nature,
        Channel::Effect,
        Channel::Unit,
        Channel::Building,
    ];
    fn slots(self) -> usize {
        match self {
            Channel::Nature => 3,
            Channel::Effect => 19,
            Channel::Unit | Channel::Building => 4,
        }
    }
    /// The bridge's group of the channel's voices.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    fn group(self) -> i32 {
        self as i32 + 1
    }
}

/// Where a sound is and the centre cell of the view it is heard from
/// (0x4f8260), with the view's width in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub at: [i32; 2],
    pub centre: [i32; 2],
    pub width: u32,
}
impl Place {
    fn cells(self) -> u32 {
        (self.at[0] - self.centre[0]).unsigned_abs() + (self.at[1] - self.centre[1]).unsigned_abs()
    }
}
/// The widest view of the original, `LAYOUT1024H` of `LAYOUT.INI`. Wider
/// views of modern screens are heard as this one, so that a sound still
/// falls silent beyond 96 cells instead of across the whole map.
pub const WIDEST_VIEW: u32 = 1024;
/// Volume and pan in hundredths of a decibel (0x4d00e0): Manhattan distance
/// in cells `d`, volume `d·(−K)/(W·m/32)` with K = 2500 and m = 3 for
/// effects, K = 400 and m = 2 for the other channels, −10000 below −K; pan
/// `dx·51200/W` within ±10000.
fn attenuation(p: Place, effect: bool) -> (i32, i32) {
    let (k, m) = if effect { (2500, 3) } else { (400, 2) };
    let width = p.width.clamp(32, WIDEST_VIEW) as i64;
    let mut volume = -(p.cells().min(1 << 20) as i64) * k / (width * m / 32);
    if volume < -k {
        volume = -10000;
    }
    let dx = (p.at[0] - p.centre[0]).clamp(-(1 << 20), 1 << 20) as i64;
    let pan = (dx * 51200 / width).clamp(-10000, 10000);
    (volume as i32, pan as i32)
}
/// An effect starts only above −2500 (0x4cfc71).
pub fn placement(p: Place) -> Option<(f32, f32)> {
    let (volume, pan) = attenuation(p, true);
    (volume > -2500).then_some((volume as f32, pan as f32))
}
/// The voices of units and buildings are never quieter than −400 (0x4cef57).
fn voice_placement(p: Place) -> (i32, i32) {
    let (volume, pan) = attenuation(p, false);
    (volume.max(-400), pan)
}
fn amplitude(centibels: f32) -> f32 {
    10f32.powf(centibels / 2000.)
}
/// The clip of a weighted group for a roll of R(0, 1000) on the stream of
/// sounds and pictures (0x455210): the first whose range, from the weights
/// before it to those plus its own, holds the roll.
pub fn pick(candidates: &[(String, u32)], roll: i32) -> Option<&str> {
    let mut low = 0i64;
    for (name, weight) in candidates {
        let high = low + *weight as i64;
        if (low..=high).contains(&(roll as i64)) {
            return Some(name);
        }
        low = high;
    }
    None
}
/// A rain clip (0x4a8020, 0x4a8140): level 0–2 and `START`, `1`–`6` or `END`.
pub fn rain_clip(level: usize, clip: &str) -> String {
    let level = ["LIGHT", "MID", "HEAVY"][level.min(2)];
    format!("SOUND/RAIN/{level}/RAIN {clip}.WAV")
}

/// A sound to play.
#[derive(Clone, Debug)]
pub struct Cue {
    pub name: String,
    pub category: u8,
    pub channel: Channel,
    /// Where it is heard; none at the centre of the view, at 0 cB.
    pub place: Option<Place>,
    /// The object it follows when the view moves (0x4d01c0); none keeps it
    /// where it started.
    pub owner: Option<usize>,
    /// Played in a loop until stopped: the map's sounds in mode 1
    /// (0x50299c with the flag of +0x4e = 1).
    pub looped: bool,
}
/// The view moved (0x4d01c0): its centre cell and width, and where the
/// objects with sounds now are.
#[derive(Clone, Debug)]
pub struct Listener {
    pub centre: [i32; 2],
    pub width: u32,
    pub objects: Vec<(usize, [i32; 2])>,
}
/// Who a playing sound belongs to (0xc97ae0): nobody (nature, 0), a place
/// (0xfffe) or an object.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Nobody,
    Place,
    Object(usize),
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Voice {
    id: i32,
    category: u8,
    priority: u8,
    /// Where it was when it started (0xc97aec, 0xc97af0).
    at: [i32; 2],
    owner: Owner,
    /// Hundredths of a decibel.
    volume: i32,
    pan: i32,
}
/// The slot a new sound takes (0x4cfb50, 0x4ceea0, 0x4cf2d0, 0x4cf700), in
/// three passes over the slots in order: a free one; one of lower priority;
/// one of equal priority whose sound is no nearer to the centre of the view
/// than the new one. An effect whose category already plays as often as it
/// may takes only, in the last pass, a slot of its own category.
fn choose(slots: &[Option<Voice>], new: &Voice, at_limit: bool, centre: [i32; 2]) -> Option<usize> {
    let distance =
        |p: [i32; 2]| (centre[0] - p[0]).unsigned_abs() + (centre[1] - p[1]).unsigned_abs();
    let near = distance(new.at);
    for pass in if at_limit { 2 } else { 0 }..3 {
        for (k, slot) in slots.iter().enumerate() {
            let take = match slot {
                None => !at_limit,
                Some(v) if pass == 1 => new.priority > v.priority,
                Some(v) if pass == 2 && at_limit => {
                    new.category != 0 && v.category == new.category && near <= distance(v.at)
                }
                Some(v) if pass == 2 => v.priority == new.priority && near <= distance(v.at),
                Some(_) => false,
            };
            if take {
                return Some(k);
            }
        }
    }
    None
}
/// An effect's priority: its category's, divided by one more than the
/// effects of the category playing, unless it is 255 (0x4cfc87).
fn effect_priority(base: u8, playing: usize) -> u8 {
    if base == u8::MAX {
        base
    } else {
        (base as usize / (playing + 1)) as u8
    }
}
/// The slots of every channel.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
struct Mixer {
    slots: [Vec<Option<Voice>>; 4],
}
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
impl Mixer {
    fn new() -> Self {
        Self {
            slots: Channel::ALL.map(|c| vec![None; c.slots()]),
        }
    }
    fn voices(&self) -> impl Iterator<Item = (Channel, &Voice)> {
        Channel::ALL
            .into_iter()
            .flat_map(move |c| self.slots[c as usize].iter().flatten().map(move |v| (c, v)))
    }
    /// The objects with sounds, and those speaking with a voice of units or
    /// buildings (flag 0x800000).
    fn sounding(&self) -> Sounding {
        let mut s = Sounding::default();
        for (channel, v) in self.voices() {
            if let Owner::Object(o) = v.owner {
                s.owners.push(o);
                if matches!(channel, Channel::Unit | Channel::Building) {
                    s.speaking.push(o);
                }
            }
        }
        s.owners.sort_unstable();
        s.owners.dedup();
        s
    }
}
/// What the frontend may ask about the voices playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sounding {
    pub owners: Vec<usize>,
    pub speaking: Vec<usize>,
    /// A movie's sound is playing.
    pub movie: bool,
    /// The song playing: `MUSIC/….MID` of the list, or the story's own.
    pub song: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub pcm_backend: String,
    pub midi_backend: String,
    pub errors: Vec<String>,
}
impl Status {
    /// Keeps the first errors only: a failing device must not grow the list.
    fn fail(&mut self, error: impl ToString) {
        if self.errors.len() < 16 {
            self.errors.push(error.to_string());
        }
    }
}
enum Command {
    State(SoundState),
    Play(Cue),
    /// Silence an object (0x4d05b0): before a new order or its death.
    StopObject(usize),
    Listener(Listener),
    /// A story's narration or a conversation's line (0x4d0a80), or none.
    Speech(Option<Pcm>),
    /// A DDV movie's sound, or none. The game's sounds are silent while a
    /// movie plays, as when the window is inactive, so this one does not
    /// ask for the focus: the movie halts and silences itself.
    Movie(Option<Pcm>),
    /// A story window opens, with the music it names (a MIDI of the map's
    /// files), if any.
    Story(u32, Option<Vec<u8>>),
    Stop,
}
pub struct Player {
    tx: mpsc::Sender<Command>,
    worker: Option<thread::JoinHandle<()>>,
    pub status: Arc<Mutex<Status>>,
    sounding: Arc<Mutex<Sounding>>,
    /// Nature calls of each biome, habitat and time of day.
    pub nature: ambience::Catalog,
    /// Each rain clip's length in ms (0x4a81d0).
    pub rain_ms: BTreeMap<String, u64>,
}
impl Player {
    pub fn start(source: &Path) -> Result<Self> {
        let mut bank = AudioBank::open(source)?;
        let assets = crate::assets::GameAssets::open(source)?;
        let nature = ambience::Catalog::load(&assets)?;
        let mut rain_ms = BTreeMap::new();
        for level in 0..3 {
            for clip in ["START", "1", "2", "3", "4", "5", "6", "END"] {
                let name = rain_clip(level, clip);
                if let Ok(p) = bank.wave(&name) {
                    let rate = p.rate as u64 * p.channels as u64 * p.bits as u64 / 8;
                    rain_ms.insert(name, p.bytes.len() as u64 * 1000 / rate.max(1));
                }
            }
        }
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let sounding = Arc::new(Mutex::new(Sounding::default()));
        let (shared, voices) = (status.clone(), sounding.clone());
        let worker = thread::Builder::new()
            .name("Tzar audio".into())
            .spawn(move || worker(bank, rx, shared, voices))?;
        Ok(Self {
            tx,
            worker: Some(worker),
            status,
            sounding,
            nature,
            rain_ms,
        })
    }
    pub fn state(&self, s: SoundState) {
        let _ = self.tx.send(Command::State(s));
    }
    pub fn play(&self, cue: Cue) {
        let _ = self.tx.send(Command::Play(cue));
    }
    pub fn stop_object(&self, object: usize) {
        let _ = self.tx.send(Command::StopObject(object));
    }
    pub fn listener(&self, l: Listener) {
        let _ = self.tx.send(Command::Listener(l));
    }
    /// The narration of a story or a conversation's line (0x4d0a80): the
    /// sound effects stop first (0x4d0a00) and the voice before goes; none
    /// silences it.
    pub fn speech(&self, pcm: Option<Pcm>) {
        let _ = self.tx.send(Command::Speech(pcm));
    }
    /// The sound of a movie decoded by ReTzar, or none.
    pub fn movie(&self, pcm: Option<Pcm>) {
        let _ = self.tx.send(Command::Movie(pcm));
    }
    /// The story window `id` opens (0x4c34c0). It takes the game's music
    /// away while it stays open (0x4c389e: the song ends, 0xc97aa0 = 1)
    /// and plays its own once, `song` (the story's or scroll's music, a
    /// file of the map); closing it ends that song and the list goes on
    /// (0x4c3ad2). The window is then reported open with
    /// [`SoundState::story`] = `id`.
    pub fn story(&self, id: u32, song: Option<Vec<u8>>) {
        let _ = self.tx.send(Command::Story(id, song));
    }
    /// Whether a movie's sound is playing.
    pub fn movie_playing(&self) -> bool {
        self.sounding.lock().is_ok_and(|s| s.movie)
    }
    /// The song playing, if any (a story's own is `STORY`).
    pub fn song(&self) -> Option<String> {
        self.sounding.lock().ok().and_then(|s| s.song.clone())
    }
    /// The objects with sounds playing.
    pub fn owners(&self) -> Vec<usize> {
        self.sounding
            .lock()
            .map_or(Vec::new(), |s| s.owners.clone())
    }
    /// An object already speaking takes no other voice (0x505720).
    pub fn speaking(&self, object: usize) -> bool {
        self.sounding
            .lock()
            .is_ok_and(|s| s.speaking.contains(&object))
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Stop);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::*;
    use std::ffi::c_void;
    #[cfg(windows)]
    #[link(name = "winmm")]
    unsafe extern "system" {
        fn midiOutOpen(
            handle: *mut *mut c_void,
            device: u32,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        fn midiOutShortMsg(handle: *mut c_void, message: u32) -> u32;
        fn midiOutReset(handle: *mut c_void) -> u32;
        fn midiOutClose(handle: *mut c_void) -> u32;
    }
    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        #[link_name = "tz_midi_open"]
        fn midiOutOpen(
            handle: *mut *mut c_void,
            device: u32,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        #[link_name = "tz_midi_message"]
        fn midiOutShortMsg(handle: *mut c_void, message: u32) -> u32;
        #[link_name = "tz_midi_reset"]
        fn midiOutReset(handle: *mut c_void) -> u32;
        #[link_name = "tz_midi_close"]
        fn midiOutClose(handle: *mut c_void) -> u32;
    }
    unsafe extern "C" {
        fn tz_audio_create(error: *mut i32) -> *mut c_void;
        fn tz_audio_destroy(engine: *mut c_void);
        fn tz_audio_voice_active(engine: *mut c_void, id: i32) -> i32;
        fn tz_audio_stop_voice(engine: *mut c_void, id: i32);
        fn tz_audio_stop(engine: *mut c_void, group: i32);
        fn tz_audio_set(engine: *mut c_void, id: i32, gain: f32, pan: f32);
        fn tz_audio_play(
            engine: *mut c_void,
            bytes: *const u8,
            size: u32,
            rate: u32,
            channels: u16,
            bits: u16,
            group: i32,
            looped: i32,
            gain: f32,
            pan: f32,
        ) -> i32;
    }
    pub struct Output {
        pcm: *mut c_void,
        midi: *mut c_void,
        pub volumes: [u8; 16],
    }
    impl Output {
        pub fn open(status: &mut Status) -> Self {
            let mut error = 0;
            let pcm = unsafe { tz_audio_create(&mut error) };
            if pcm.is_null() {
                status.fail(format!("PCM backend: error {error:#010x}"));
            } else {
                #[cfg(windows)]
                {
                    status.pcm_backend = "XAudio2 2.9 / dispositivo predefinito".into();
                }
                #[cfg(target_os = "macos")]
                {
                    status.pcm_backend = "Core Audio / default output device".into();
                }
            }
            let mut midi = std::ptr::null_mut();
            let code = unsafe { midiOutOpen(&mut midi, u32::MAX, 0, 0, 0) };
            if code != 0 {
                status.fail(format!("MIDI backend: error {code:#010x}"));
            } else {
                #[cfg(windows)]
                {
                    status.midi_backend = "General MIDI / sintetizzatore Windows".into();
                }
                #[cfg(target_os = "macos")]
                {
                    status.midi_backend = "General MIDI / Apple DLS synthesizer".into();
                }
            }
            Self {
                pcm,
                midi,
                volumes: [100; 16],
            }
        }
        /// The id of the started voice.
        pub fn play(
            &mut self,
            p: &Pcm,
            group: i32,
            looped: bool,
            gain: f32,
            pan: f32,
            status: &mut Status,
        ) -> Option<i32> {
            if self.pcm.is_null() || gain <= 0. {
                return None;
            }
            let result = unsafe {
                tz_audio_play(
                    self.pcm,
                    p.bytes.as_ptr(),
                    p.bytes.len() as u32,
                    p.rate,
                    p.channels,
                    p.bits,
                    group,
                    looped as i32,
                    gain,
                    pan,
                )
            };
            if result < 0 {
                status.fail(format!("PCM: HRESULT {result:#010x}"));
                None
            } else {
                Some(result)
            }
        }
        pub fn voice_active(&mut self, id: i32) -> bool {
            !self.pcm.is_null() && unsafe { tz_audio_voice_active(self.pcm, id) } != 0
        }
        pub fn stop_voice(&mut self, id: i32) {
            if !self.pcm.is_null() {
                unsafe { tz_audio_stop_voice(self.pcm, id) }
            }
        }
        pub fn stop_group(&mut self, group: i32) {
            if !self.pcm.is_null() {
                unsafe {
                    tz_audio_stop(self.pcm, group);
                }
            }
        }
        /// A playing voice's new gain and pan.
        pub fn set(&mut self, id: i32, gain: f32, pan: f32) {
            if !self.pcm.is_null() {
                unsafe { tz_audio_set(self.pcm, id, gain, pan) }
            }
        }
        pub fn message(&mut self, mut msg: u32, gain: u8, status: &mut Status) {
            let channel = (msg & 15) as usize;
            if msg & 0xf0 == 0xb0 && (msg >> 8) & 127 == 7 {
                self.volumes[channel] = ((msg >> 16) & 127) as u8;
                msg = (msg & 0xffff) | ((self.volumes[channel] as u32 * gain as u32 / 100) << 16);
            }
            if self.midi.is_null() {
                return;
            }
            let code = unsafe { midiOutShortMsg(self.midi, msg) };
            if code != 0 {
                status.fail(format!("MIDI message: {code}"));
            }
        }
        pub fn volume(&mut self, gain: u8, status: &mut Status) {
            // Preserve each channel's original mix while changing only this MIDI stream.
            for ch in 0..16 {
                let v = self.volumes[ch];
                self.message(
                    0xb0 | ch as u32 | (7 << 8) | ((v as u32) << 16),
                    gain,
                    status,
                );
            }
        }
        pub fn reset(&mut self, status: &mut Status) {
            if !self.midi.is_null() {
                unsafe {
                    midiOutReset(self.midi);
                }
            }
            self.volumes = [100; 16];
            for ch in 0..16 {
                // Restore controllers, program and pitch for the next General MIDI track.
                for msg in [0xb0 | ch | (121 << 8), 0xc0 | ch, 0xe0 | ch | (64 << 16)] {
                    self.message(msg, 100, status);
                }
            }
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            unsafe {
                if !self.midi.is_null() {
                    midiOutReset(self.midi);
                    midiOutClose(self.midi);
                }
                if !self.pcm.is_null() {
                    tz_audio_destroy(self.pcm);
                }
            }
        }
    }
}
/// Longest sleep of the audio thread: commands are answered within this time.
const AUDIO_IDLE: Duration = Duration::from_millis(5);

/// Time until the next due MIDI event, capped by [`AUDIO_IDLE`]. Waiting for
/// the exact event time keeps notes on beat: a fixed 8 ms poll measured
/// 12–18 ms on Windows, late enough to blur fast passages.
fn audio_wait(next_event: Option<f64>, elapsed: f64) -> Duration {
    match next_event {
        Some(at) => Duration::from_secs_f64((at - elapsed).clamp(0., AUDIO_IDLE.as_secs_f64())),
        None => AUDIO_IDLE,
    }
}

/// Channel state (programs, controllers, pitch bends) that precedes `cursor`,
/// in order, so a paused song resumes with the right instruments and volume.
fn chase(midi: &Midi, cursor: usize) -> impl Iterator<Item = u32> + '_ {
    midi.events[..cursor.min(midi.events.len())]
        .iter()
        .map(|e| e.message)
        .filter(|m| matches!(m & 0xf0, 0xb0 | 0xc0 | 0xe0))
}

/// The song being played and where it is.
#[cfg(any(windows, target_os = "macos"))]
struct Song {
    track: String,
    started: Instant,
    cursor: usize,
    /// Position kept while the window is in the background.
    paused_at: Option<Duration>,
}
#[cfg(any(windows, target_os = "macos"))]
impl Song {
    /// A song started in the background waits at its beginning, so the
    /// return does not play every note it would have played meanwhile.
    fn start(track: String, focused: bool) -> Self {
        Self {
            track,
            started: Instant::now(),
            cursor: 0,
            paused_at: (!focused).then_some(Duration::ZERO),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl Mixer {
    /// Frees the slots whose sound ended.
    fn refresh(&mut self, output: &mut native::Output) {
        for slot in self.slots.iter_mut().flatten() {
            if slot.is_some_and(|v| !output.voice_active(v.id)) {
                *slot = None;
            }
        }
    }
    fn stop(&mut self, output: &mut native::Output, channels: &[Channel]) {
        for &c in channels {
            output.stop_group(c.group());
            self.slots[c as usize].fill(None);
        }
    }
}
/// The bridge's group of the narration, after the four channels.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
const SPEECH_GROUP: i32 = 5;
/// The group of a movie's sound.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
const MOVIE_GROUP: i32 = 6;
#[cfg(any(windows, target_os = "macos"))]
fn gain(sound: u8, volume: i32) -> f32 {
    sound as f32 / 100. * amplitude(volume as f32)
}
/// Starts a sound in the slot its channel gives it (0x4cfb50, 0x4ceea0,
/// 0x4cf2d0, 0x4cf700); the voice it displaces stops first.
#[cfg(any(windows, target_os = "macos"))]
fn play(
    bank: &mut AudioBank,
    output: &mut native::Output,
    mixer: &mut Mixer,
    state: &SoundState,
    cue: Cue,
    status: &mut Status,
) {
    let channel = cue.channel;
    let (volume, pan) = match cue.place {
        None => (0, 0),
        Some(p) if channel == Channel::Effect => match attenuation(p, true) {
            (volume, _) if volume <= -2500 => return,
            heard => heard,
        },
        Some(p) => voice_placement(p),
    };
    let centre = cue.place.map_or([0, 0], |p| p.centre);
    let base = bank
        .priorities
        .get(cue.category as usize)
        .copied()
        .unwrap_or(0);
    let slots = &mut mixer.slots[channel as usize];
    let (priority, at_limit) = if channel == Channel::Effect {
        let playing = slots
            .iter()
            .flatten()
            .filter(|v| v.category == cue.category)
            .count();
        let most = CATEGORIES
            .get(cue.category as usize)
            .map_or(0, |c| c.1 as usize);
        (effect_priority(base, playing), playing >= most)
    } else {
        (base, false)
    };
    let mut voice = Voice {
        id: 0,
        category: cue.category,
        priority,
        at: cue.place.map_or(centre, |p| p.at),
        owner: match (cue.place, cue.owner) {
            (None, _) => Owner::Nobody,
            (_, Some(o)) => Owner::Object(o),
            (_, None) => Owner::Place,
        },
        volume,
        pan,
    };
    let Some(k) = choose(slots, &voice, at_limit, centre) else {
        return;
    };
    if let Some(old) = slots[k].take() {
        output.stop_voice(old.id);
    }
    match bank.wave(&cue.name) {
        Ok(p) => {
            let g = gain(state.sound, volume);
            if let Some(id) = output.play(p, channel.group(), cue.looped, g, pan as f32, status) {
                voice.id = id;
                slots[k] = Some(voice);
            }
        }
        Err(e) => status.fail(e),
    }
}
/// The view moved more than three cells (0x4d01c0): the sounds of places
/// and objects are heard again from the new centre; effects falling below
/// −2500 stop, the voices of units and buildings keep −400 at least.
#[cfg(any(windows, target_os = "macos"))]
fn relocate(output: &mut native::Output, mixer: &mut Mixer, sound: u8, l: &Listener) {
    for channel in [Channel::Effect, Channel::Unit, Channel::Building] {
        for slot in mixer.slots[channel as usize].iter_mut() {
            let Some(mut v) = *slot else { continue };
            let at = match v.owner {
                Owner::Nobody => continue,
                Owner::Place => v.at,
                Owner::Object(o) => match l.objects.iter().find(|(j, _)| *j == o) {
                    Some(&(_, at)) => at,
                    None => continue,
                },
            };
            let place = Place {
                at,
                centre: l.centre,
                width: l.width,
            };
            let (volume, pan) = if channel == Channel::Effect {
                let (volume, pan) = attenuation(place, true);
                if volume < -2500 {
                    output.stop_voice(v.id);
                    *slot = None;
                    continue;
                }
                (volume, pan)
            } else {
                voice_placement(place)
            };
            (v.volume, v.pan) = (volume, pan);
            output.set(v.id, gain(sound, volume), pan as f32);
            *slot = Some(v);
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn worker(
    mut bank: AudioBank,
    rx: mpsc::Receiver<Command>,
    shared: Arc<Mutex<Status>>,
    sounding: Arc<Mutex<Sounding>>,
) {
    let mut status = Status::default();
    let mut output = native::Output::open(&mut status);
    publish(&shared, &status);
    let mut state = SoundState {
        sound: 75,
        music: 50,
        ambience: true,
        focused: false,
        scene: None,
        movie: false,
        story: None,
        music_on: true,
        options: false,
    };
    let mut rng = Rng::seeded();
    let songs = playlist(bank.music, &mut rng);
    let mut next_song = 0usize;
    let mut song: Option<Song> = None;
    // The mission's music check; idle outside a game.
    let mut check = MusicCheck::default();
    // The song of the story window about to open.
    let mut story_song: Option<(u32, Option<Midi>)> = None;
    let mut mixer = Mixer::new();
    let mut heard = Sounding::default();
    // The narration's voice, alone in its group.
    let mut speech: Option<i32> = None;
    let mut movie: Option<i32> = None;
    let mut stop = false;
    while !stop {
        let next_event = song.as_ref().filter(|_| state.focused).and_then(|s| {
            let m = bank.midi.get(&s.track)?;
            Some(m.events.get(s.cursor).map_or(m.seconds, |e| e.seconds))
        });
        let elapsed = song
            .as_ref()
            .map_or(0., |s| s.started.elapsed().as_secs_f64());
        let wait = audio_wait(next_event, elapsed);
        if !wait.is_zero() {
            thread::sleep(wait);
        }
        let mut commands = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(c) => commands.push(c),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    stop = true;
                    break;
                }
            }
        }
        mixer.refresh(&mut output);
        for command in commands {
            match command {
                Command::Stop => {
                    stop = true;
                    break;
                }
                Command::Play(cue) => {
                    if !state.focused
                        || state.sound == 0
                        || state.scene.is_none()
                        || (cue.channel == Channel::Nature && !state.ambience)
                    {
                        continue;
                    }
                    play(&mut bank, &mut output, &mut mixer, &state, cue, &mut status);
                }
                Command::StopObject(o) => {
                    for slot in mixer.slots.iter_mut().flatten() {
                        if let Some(v) = slot
                            && v.owner == Owner::Object(o)
                        {
                            output.stop_voice(v.id);
                            *slot = None;
                        }
                    }
                }
                Command::Listener(l) => relocate(&mut output, &mut mixer, state.sound, &l),
                Command::Speech(pcm) => {
                    if let Some(id) = speech.take() {
                        output.stop_voice(id);
                    }
                    if let Some(p) = pcm.filter(|_| state.focused && state.sound > 0) {
                        mixer.stop(
                            &mut output,
                            &[Channel::Effect, Channel::Unit, Channel::Building],
                        );
                        speech = output.play(
                            &p,
                            SPEECH_GROUP,
                            false,
                            gain(state.sound, 0),
                            0.,
                            &mut status,
                        );
                    }
                }
                Command::Story(id, song) => {
                    let midi = song.and_then(|bytes| match Midi::decode(&bytes) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            status.fail(format!("Musica della storia: {e}"));
                            None
                        }
                    });
                    story_song = Some((id, midi));
                }
                Command::Movie(pcm) => {
                    if let Some(id) = movie.take() {
                        output.stop_voice(id);
                    }
                    if let Some(p) = pcm.filter(|_| state.sound > 0) {
                        movie = output.play(
                            &p,
                            MOVIE_GROUP,
                            false,
                            gain(state.sound, 0),
                            0.,
                            &mut status,
                        );
                    }
                }
                Command::State(next) => {
                    let context_changed = next.scene != state.scene;
                    // A movie's sound ends with the movie.
                    if !next.movie
                        && let Some(id) = movie.take()
                    {
                        output.stop_voice(id);
                    }
                    if !next.focused || context_changed {
                        mixer.stop(
                            &mut output,
                            &[Channel::Effect, Channel::Unit, Channel::Building],
                        );
                        if let Some(id) = speech.take() {
                            output.stop_voice(id);
                        }
                    }
                    if !next.focused || context_changed || !next.ambience || next.sound == 0 {
                        mixer.stop(&mut output, &[Channel::Nature]);
                    }
                    if context_changed {
                        output.reset(&mut status);
                        song = None;
                        bank.midi.remove(STORY_SONG);
                        if next.scene.is_some() {
                            check = MusicCheck::begin(Instant::now());
                        } else {
                            check = MusicCheck::default();
                            song = Some(Song::start("MUSIC/INTRO.MID".into(), next.focused));
                        }
                    } else if next.movie != state.movie {
                        // 0x47f7cf: a movie ends the song and holds the
                        // music; 0x47f600: its end lets the music go, a
                        // story window open or not.
                        output.reset(&mut status);
                        song = None;
                        check.held = next.movie;
                    } else if next.focused != state.focused
                        && let Some(s) = &mut song
                    {
                        // Leaving: silence held notes and remember the position.
                        // Returning: restore instruments, then continue from there.
                        output.reset(&mut status);
                        if !next.focused {
                            s.paused_at.get_or_insert(s.started.elapsed());
                        } else if let Some(position) = s.paused_at.take() {
                            s.started = Instant::now() - position;
                            if let Some(midi) = bank.midi.get(&s.track) {
                                for message in chase(midi, s.cursor) {
                                    output.message(message, next.music, &mut status);
                                }
                            }
                        }
                    }
                    // 0x4c389e / 0x4c3ad2: a story window opening or
                    // closing ends the song playing, also one opening with
                    // its map; open it holds the music (0x4c38c9), closed
                    // it lets it go (0x4c3af8) for the check to find; one
                    // left open with the map gives the music back with it.
                    let opened = next.story.is_some() && next.story != state.story;
                    let closed = next.story.is_none() && state.story.is_some() && !context_changed;
                    if opened || closed {
                        output.reset(&mut status);
                        song = None;
                        bank.midi.remove(STORY_SONG);
                        check.held = opened;
                        if let Some(id) = next.story
                            && let Some((_, Some(midi))) =
                                story_song.take().filter(|(n, _)| *n == id)
                        {
                            bank.midi.insert(STORY_SONG.into(), midi);
                            song = Some(Song::start(STORY_SONG.into(), next.focused));
                        }
                    }
                    // Over a game, the music switched off ends the song of
                    // the list and lets the music go (0x47d5b7); switched on
                    // again, the next song starts at once (0x47d537), as on
                    // opening the options page with none playing (0x47b916).
                    if next.scene.is_some() && !context_changed {
                        if !next.music_on && state.music_on {
                            if song.as_ref().is_some_and(|s| s.track != STORY_SONG) {
                                output.reset(&mut status);
                                song = None;
                            }
                            check.held = false;
                        }
                        let now = (next.music_on && !state.music_on)
                            || (next.options && !state.options && song.is_none());
                        if now
                            && check.start_at(Instant::now(), next.focused, next.music_on)
                            && let Some(n) = songs.get(next_song % songs.len().max(1))
                        {
                            next_song += 1;
                            song = Some(Song::start(format!("MUSIC/{n}.MID"), true));
                        }
                    }
                    if next.sound != state.sound {
                        for (_, v) in mixer.voices() {
                            output.set(v.id, gain(next.sound, v.volume), v.pan as f32);
                        }
                        if let Some(id) = speech {
                            output.set(id, gain(next.sound, 0), 0.);
                        }
                        if let Some(id) = movie {
                            output.set(id, gain(next.sound, 0), 0.);
                        }
                    }
                    output.volume(if next.focused { next.music } else { 0 }, &mut status);
                    state = next;
                }
            }
        }
        let mut now = mixer.sounding();
        now.movie = movie.is_some_and(|id| output.voice_active(id));
        now.song = song.as_ref().map(|s| s.track.clone());
        if now != heard {
            if let Ok(mut s) = sounding.lock() {
                s.clone_from(&now);
            }
            heard = now;
        }
        if state.focused
            && let Some(s) = &mut song
            && let Some(midi) = bank.midi.get(&s.track)
        {
            let elapsed = s.started.elapsed().as_secs_f64();
            while s.cursor < midi.events.len() && midi.events[s.cursor].seconds <= elapsed {
                output.message(midi.events[s.cursor].message, state.music, &mut status);
                s.cursor += 1;
            }
            if elapsed >= midi.seconds {
                output.reset(&mut status);
                output.volume(state.music, &mut status);
                if s.track == STORY_SONG {
                    // A story's song plays once: its window still holds
                    // the music, so no other follows (0x4f3cc5).
                    song = None;
                } else if state.scene.is_some() {
                    // A mission song ended by itself: the music is free,
                    // and the check starts the next after SONGPAUSE.
                    check.ended(bank.music.pause);
                    song = None;
                } else {
                    *s = Song::start(std::mem::take(&mut s.track), true);
                }
            }
        }
        if check.step(Instant::now(), state.focused, state.music_on)
            && let Some(n) = songs.get(next_song % songs.len().max(1))
        {
            next_song += 1;
            song = Some(Song::start(format!("MUSIC/{n}.MID"), true));
        }
        if let Ok(s) = shared.lock()
            && s.errors.len() != status.errors.len()
        {
            drop(s);
            publish(&shared, &status);
        }
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
fn worker(
    _bank: AudioBank,
    _rx: mpsc::Receiver<Command>,
    shared: Arc<Mutex<Status>>,
    _sounding: Arc<Mutex<Sounding>>,
) {
    let mut status = Status::default();
    status.fail("Native playback currently requires Windows or macOS");
    publish(&shared, &status);
}
fn publish(shared: &Arc<Mutex<Status>>, status: &Status) {
    if let Ok(mut s) = shared.lock() {
        *s = status.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The tests that open the device take turns: the MIDI output admits one
    /// owner at a time.
    #[cfg(any(windows, target_os = "macos"))]
    static DEVICE: Mutex<()> = Mutex::new(());
    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    #[ignore = "Requires the default audio device; plays only silence"]
    fn device_voices_are_identified_panned_and_stopped_one_by_one() {
        let _device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
        let mut status = Status::default();
        let mut output = native::Output::open(&mut status);
        assert!(!status.pcm_backend.is_empty(), "{:?}", status.errors);
        let silence = Pcm {
            rate: 22050,
            channels: 1,
            bits: 16,
            bytes: vec![0; 22050 * 2],
        };
        let a = output
            .play(&silence, 4, true, 0.01, -10000., &mut status)
            .unwrap();
        let b = output
            .play(&silence, 4, true, 0.01, 2500., &mut status)
            .unwrap();
        assert_ne!(a, b);
        assert!(output.voice_active(a) && output.voice_active(b));
        output.stop_voice(a);
        assert!(!output.voice_active(a) && output.voice_active(b));
        output.set(b, 0.02, 0.);
        assert!(output.voice_active(b));
        output.stop_group(4);
        assert!(!output.voice_active(b));
        assert!(!status.midi_backend.is_empty(), "{:?}", status.errors);
        output.reset(&mut status);
        output.volume(0, &mut status);
        // Both original WAVE depths, mono/stereo, finish after resampling.
        for bits in [8, 16] {
            for channels in [1, 2] {
                let pcm = Pcm {
                    rate: 22050,
                    channels,
                    bits,
                    bytes: vec![
                        if bits == 8 { 128 } else { 0 };
                        441 * channels as usize * (bits as usize / 8)
                    ],
                };
                let id = output.play(&pcm, 6, false, 0.01, 0., &mut status).unwrap();
                let started = Instant::now();
                while output.voice_active(id) {
                    assert!(started.elapsed() < Duration::from_secs(3));
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        assert!(status.errors.is_empty(), "{:?}", status.errors);
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "Requires the installed sounds.wdt and the default audio device; plays only silence"]
    fn a_movie_is_heard_while_the_game_is_silenced_for_it() {
        let _device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let player = Player::start(source).unwrap();
        let state = |movie| SoundState {
            sound: 1,
            music: 0,
            ambience: false,
            // The frontend silences the game while a movie plays.
            focused: false,
            scene: None,
            movie,
            story: None,
            music_on: true,
            options: false,
        };
        let silence = || Pcm {
            rate: 22050,
            channels: 2,
            bits: 16,
            bytes: vec![0; 22050 * 4 * 2],
        };
        let wait = |playing: bool| {
            let start = std::time::Instant::now();
            while player.movie_playing() != playing {
                assert!(start.elapsed() < Duration::from_secs(3), "{playing}");
                thread::sleep(Duration::from_millis(10));
            }
        };
        player.state(state(true));
        player.movie(Some(silence()));
        wait(true);
        player.movie(None);
        wait(false);
        // The movie's end silences it too.
        player.movie(Some(silence()));
        wait(true);
        player.state(state(false));
        wait(false);
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "Requires the installed sounds.wdt and the default audio device; music at volume 0"]
    fn a_story_takes_the_music_away_and_plays_its_own() {
        let _device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let player = Player::start(source).unwrap();
        let state = |story, movie| SoundState {
            sound: 0,
            music: 0,
            ambience: false,
            focused: true,
            scene: Some(("GRASS".into(), 1)),
            movie,
            story,
            music_on: true,
            options: false,
        };
        let wait = |want: &dyn Fn(Option<String>) -> bool| {
            let start = std::time::Instant::now();
            while !want(player.song()) {
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "{:?}",
                    player.song()
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        let listed = |s: Option<String>| s.is_some_and(|s| s.starts_with("MUSIC/"));
        player.state(state(None, false));
        wait(&listed);
        // 0x4c389e: the window ends the song and plays the story's.
        let midi = Archive::open(&source.join("Packs/sounds.wdt"))
            .unwrap()
            .get("MUSIC/INTRO.MID")
            .unwrap();
        player.story(1, Some(midi.clone()));
        player.state(state(Some(1), false));
        wait(&|s| s.as_deref() == Some(STORY_SONG));
        // Without music of its own the window only silences the list.
        player.story(2, None);
        player.state(state(Some(2), false));
        wait(&|s| s.is_none());
        thread::sleep(BEAT * 3);
        assert_eq!(player.song(), None);
        // Closing it, the list goes on.
        player.state(state(None, false));
        wait(&listed);
        // A movie's end gives the music back even under a story window.
        player.story(3, None);
        player.state(state(Some(3), false));
        wait(&|s| s.is_none());
        player.state(state(Some(3), true));
        player.state(state(Some(3), false));
        wait(&listed);
        // A story opening with its map plays its own from the start.
        player.story(4, Some(midi));
        player.state(SoundState {
            scene: Some(("GRASS".into(), 2)),
            ..state(Some(4), false)
        });
        wait(&|s| s.as_deref() == Some(STORY_SONG));
        // A map left with the story open gives the music back with the next.
        player.state(SoundState {
            scene: Some(("GRASS".into(), 3)),
            ..state(None, false)
        });
        wait(&listed);
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "Requires the installed sounds.wdt and the default audio device; plays at volume 1"]
    fn a_looped_sound_of_the_map_plays_until_its_object_is_silenced() {
        let _device = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let player = Player::start(source).unwrap();
        player.state(SoundState {
            sound: 1,
            music: 0,
            ambience: false,
            focused: true,
            scene: Some(("GRASS".into(), 1)),
            movie: false,
            story: None,
            music_on: true,
            options: false,
        });
        let cue = |owner, looped| Cue {
            name: "SOUND/work/chop 1.wav".into(),
            category: category::MAP_OBJECT,
            channel: Channel::Effect,
            place: Some(Place {
                at: [10, 10],
                centre: [10, 10],
                width: 640,
            }),
            owner: Some(owner),
            looped,
        };
        let wait = |want: &dyn Fn(&[usize]) -> bool| {
            let start = std::time::Instant::now();
            while !want(&player.owners()) {
                assert!(
                    start.elapsed() < Duration::from_secs(3),
                    "{:?}",
                    player.owners()
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        player.play(cue(7, true));
        player.play(cue(8, false));
        wait(&|o| o.contains(&7) && o.contains(&8));
        // Well after the clip's end the loop still sounds, the other not.
        thread::sleep(Duration::from_secs(2));
        wait(&|o| o.contains(&7) && !o.contains(&8));
        player.stop_object(7);
        wait(&|o| !o.contains(&7));
    }
    #[test]
    fn the_music_check_starts_songs_when_the_original_does() {
        let t0 = Instant::now();
        let at = |n: u64| t0 + Duration::from_millis(n);
        // The check stepped every 10 ms: when the first song starts.
        let run = |c: &mut MusicCheck, from: u64, to: u64| {
            (from..=to).step_by(10).find(|&n| c.step(at(n), true, true))
        };
        // A game: 0x3c52322 in 100 ms, the look 100 ms later, the song after
        // the 100 ms pause; then a look every second while it plays.
        let mut playing = MusicCheck::begin(t0);
        assert_eq!(run(&mut playing, 0, 1000), Some(300));
        assert_eq!(playing.look, Some(at(1300)));
        // A story window opened at 1500 holds the music until it closes:
        // the next song comes 100 ms after the first look that finds the
        // music free, 0.1 to 1.1 s after the window closes.
        for (close, start) in [(5050, 5400), (5250, 5400), (5310, 6400), (5350, 6400)] {
            let mut c = playing.clone();
            assert_eq!(run(&mut c, 310, 1490), None);
            c.held = true;
            assert_eq!(run(&mut c, 1500, close - 10), None);
            c.held = false;
            assert_eq!(run(&mut c, close, 9000), Some(start), "{close}");
        }
        // A song ending by itself at 2500: SONGPAUSE after the next look.
        let mut ended = playing.clone();
        assert_eq!(run(&mut ended, 310, 2490), None);
        ended.ended(Duration::from_secs(10));
        let mut c = ended.clone();
        assert_eq!(run(&mut c, 2500, 20_000), Some(13_300));
        // The pause is back to 100 ms once a song starts.
        assert_eq!(c.gap, BEAT);
        // A story window still open when that start comes drops it, and the
        // check with it: closed, no song follows...
        let mut c = ended.clone();
        assert_eq!(run(&mut c, 2500, 3990), None);
        c.held = true;
        assert_eq!(run(&mut c, 4000, 13_990), None);
        c.held = false;
        assert_eq!(run(&mut c, 14_000, 60_000), None);
        assert_eq!((c.look, c.start), (None, None));
        // ...until the options page or the music switched on start one at
        // once (0x4ce5a0), and the check goes on.
        assert!(c.start_at(at(60_000), true, true));
        assert_eq!(c.look, Some(at(61_000)));
        // In the background the look comes round every second and the song
        // waits for the window's return.
        let mut c = MusicCheck::begin(t0);
        assert!((0..=3000).step_by(10).all(|n| !c.step(at(n), false, true)));
        assert_eq!(run(&mut c, 3010, 9000), Some(3300));
        // With the music off the check ends.
        let mut c = MusicCheck::begin(t0);
        assert!((0..=3000).step_by(10).all(|n| !c.step(at(n), true, false)));
        assert_eq!((c.look, c.start), (None, None));
    }
    #[test]
    fn mission_songs_are_shuffled_once_from_the_configured_range() {
        let music = Music {
            first: 1,
            last: 10,
            pause: Duration::from_secs(10),
        };
        let mut a = playlist(music, &mut Rng(7));
        assert_ne!(a, (1..=10).collect::<Vec<u8>>());
        assert_eq!(a, playlist(music, &mut Rng(7)));
        a.sort();
        assert_eq!(a, (1..=10).collect::<Vec<u8>>());
        let ini = Ini::parse(b"[SOUND]\nSTARTSONG=3\nENDSONG=5\nSONGPAUSE=2500\n");
        assert_eq!(
            Music::read(&ini),
            Music {
                first: 3,
                last: 5,
                pause: Duration::from_millis(2500)
            }
        );
        assert_eq!(Music::read(&Ini::parse(b"")), music);
    }
    #[test]
    fn effects_and_voices_fade_with_distance_and_pan_like_the_original() {
        // A 640-pixel view: 20 cells wide, effects silent from 60 cells.
        let at = |x, y, width| Place {
            at: [x, y],
            centre: [100, 100],
            width,
        };
        assert_eq!(placement(at(100, 100, 640)), Some((0., 0.)));
        assert_eq!(placement(at(100, 130, 640)), Some((-1250., 0.)));
        assert_eq!(
            placement(at(100, 159, 640)).map(|p| p.0 > -2500.),
            Some(true)
        );
        assert_eq!(placement(at(100, 160, 640)), None);
        // The offset is in cells (0x420b70 and 0x4f8260 both give cells):
        // 2 cells right of the centre pan by 160, 125 cells left fully.
        assert_eq!(placement(at(102, 100, 640)).unwrap().1, 160.);
        assert_eq!(attenuation(at(-25, 100, 640), true).1, -10000);
        // A 2560-pixel view is heard as the original's widest, 1024 pixels:
        // silent from 96 cells, not from 240.
        assert_eq!(
            placement(at(100, 195, 2560)).map(|p| p.0 > -2500.),
            Some(true)
        );
        assert_eq!(placement(at(100, 196, 2560)), None);
        assert_eq!(placement(at(116, 94, 2560)).unwrap().1, 800.);
        // 0x4cef57: voices of units and buildings, K = 400 over two widths,
        // never below −400.
        assert_eq!(voice_placement(at(100, 120, 640)), (-200, 0));
        assert_eq!(voice_placement(at(100, 140, 640)), (-400, 0));
        assert_eq!(voice_placement(at(100, 400, 640)), (-400, 0));
        assert!((amplitude(-2000.) - 0.1).abs() < 1e-6);
    }
    #[test]
    fn full_channels_give_way_like_the_original() {
        let voice = |category, priority, at| Voice {
            id: 0,
            category,
            priority,
            at,
            owner: Owner::Place,
            volume: 0,
            pan: 0,
        };
        let centre = [0, 0];
        let busy = [
            Some(voice(4, 120, [4, 0])),
            Some(voice(11, 170, [9, 0])),
            Some(voice(4, 60, [2, 0])),
            Some(voice(22, 200, [12, 0])),
        ];
        // A free slot first, in order.
        let mut free = busy;
        free[2] = None;
        assert_eq!(choose(&free, &voice(4, 10, [0, 0]), false, centre), Some(2));
        // Then the first slot of lower priority, not the lowest (0x4cfd29).
        assert_eq!(
            choose(&busy, &voice(5, 170, [0, 0]), false, centre),
            Some(0)
        );
        // Then the first of equal priority no nearer than the new sound.
        assert_eq!(choose(&busy, &voice(5, 60, [1, 0]), false, centre), Some(2));
        assert_eq!(choose(&busy, &voice(5, 60, [3, 0]), false, centre), None);
        assert_eq!(choose(&busy, &voice(5, 50, [0, 0]), false, centre), None);
        // A category at its most (0xc97ad8) takes only one of its own, even
        // of higher priority, and never a free slot.
        assert_eq!(
            choose(&free, &voice(22, 250, [0, 0]), true, centre),
            Some(3)
        );
        assert_eq!(choose(&free, &voice(22, 250, [13, 0]), true, centre), None);
        // Repeated categories lose weight; 255 never does.
        assert_eq!(effect_priority(120, 2), 40);
        assert_eq!(effect_priority(170, 1), 85);
        assert_eq!(effect_priority(255, 4), 255);
        assert_eq!(CATEGORIES[13], ("attack_far", 5));
        assert_eq!(CATEGORIES[19], ("explosion", 4));
    }
    #[test]
    fn weighted_groups_pick_by_a_roll_of_a_thousand() {
        // [mine] of SOUNDS.INI: ranges 0–500, 500–750, 750–1000; a roll on
        // a boundary goes to the first range that holds it (0x455210).
        let mine: Vec<(String, u32)> = [("a", 500), ("b", 250), ("c", 250)]
            .map(|(n, w)| (n.to_owned(), w))
            .to_vec();
        assert_eq!(pick(&mine, 0), Some("a"));
        assert_eq!(pick(&mine, 500), Some("a"));
        assert_eq!(pick(&mine, 501), Some("b"));
        assert_eq!(pick(&mine, 1000), Some("c"));
        // A group short of a thousand stays silent above its sum.
        assert_eq!(pick(&mine[..2], 751), None);
        assert_eq!(rain_clip(1, "END"), "SOUND/RAIN/MID/RAIN END.WAV");
    }
    #[test]
    fn wave_chunks_padding_and_alignment() {
        let mut bytes = b"RIFF\0\0\0\0WAVEJUNK\x01\0\0\0x\0fmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\x04\0\0\0\0\0\x10\0".to_vec();
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let pcm = Pcm::decode(&bytes).unwrap();
        assert_eq!((pcm.rate, pcm.bits, pcm.bytes.len()), (44100, 16, 4));
        bytes.pop();
        assert!(Pcm::decode(&bytes).is_err());
    }
    #[test]
    fn midi_tempo_and_running_status() {
        let body = [
            0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0, 0x90, 60, 100, 96, 60, 0, 0, 0xff, 0x51, 3,
            0x0f, 0x42, 0x40, 96, 0x90, 62, 90, 96, 62, 0, 0, 0xff, 0x2f, 0,
        ];
        let mut data = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk".to_vec();
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&body);
        let m = Midi::decode(&data).unwrap();
        assert_eq!(m.events.len(), 4);
        assert_eq!(m.events[1].seconds, 0.5);
        assert_eq!(m.events[2].seconds, 1.5);
        assert_eq!(m.seconds, 2.5);
        data.pop();
        assert!(Midi::decode(&data).is_err());
    }
    #[test]
    fn malformed_midi_does_not_panic() {
        for bytes in [
            vec![],
            b"MThd\xff\xff\xff\xff".to_vec(),
            b"MThd\0\0\0\x06\0\0\0\x01\0\0".to_vec(),
        ] {
            assert!(Midi::decode(&bytes).is_err());
        }
        let mut at = 0;
        assert!(vlq(&[0xff; 4], &mut at).is_err());
    }
    #[test]
    fn midi_waits_for_the_next_event_and_resumes_with_channel_state() {
        let wait = audio_wait(Some(1.0), 0.9985).as_secs_f64();
        assert!((wait - 0.0015).abs() < 1e-6);
        assert_eq!(audio_wait(Some(1.0), 1.2), Duration::ZERO);
        assert_eq!(audio_wait(Some(9.0), 0.0), AUDIO_IDLE);
        assert_eq!(audio_wait(None, 0.0), AUDIO_IDLE);
        // Program 5, note on, volume 80, pitch bend centre, note off, program 7.
        let midi = Midi {
            events: [
                0x0000_05c1,
                0x0064_3c91,
                0x0050_07b1,
                0x0040_00e1,
                0x0000_3c81,
                0x0000_07c1,
            ]
            .iter()
            .enumerate()
            .map(|(i, &message)| MidiEvent {
                seconds: i as f64,
                message,
            })
            .collect(),
            seconds: 6.,
        };
        // Notes are never replayed; program, volume and bend are, in order.
        assert_eq!(
            chase(&midi, 5).collect::<Vec<_>>(),
            [0x0000_05c1, 0x0050_07b1, 0x0040_00e1]
        );
        assert_eq!(chase(&midi, 99).count(), 4);
    }
    #[test]
    #[ignore = "requires original data"]
    fn original_audio_decodes() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let report = audit(source).unwrap();
        assert_eq!(report["midi_count"], 14);
        assert!(report["wav_count"].as_u64().unwrap() > 400);
        AudioBank::open(source).unwrap();
    }
}
