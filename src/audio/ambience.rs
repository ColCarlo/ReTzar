//! Original terrain INI sound lists, and the view's timers of nature calls
//! (0x4f423e) and rain (0x4a8020).
use crate::{
    Result,
    assets::{GameAssets, Ini},
    simulation::NativeRng,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habitat {
    Marine,
    Forest,
    Plains,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NatureContext {
    pub habitat: Habitat,
    pub night: bool,
}
impl NatureContext {
    fn index(self) -> usize {
        self.habitat as usize + if self.night { 3 } else { 0 }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Catalog(BTreeMap<String, [Vec<String>; 6]>);
impl Catalog {
    pub fn load(assets: &GameAssets) -> Result<Self> {
        let mut lists = BTreeMap::new();
        for biome in ["grass", "summer", "snow", "desert"] {
            let ini = Ini::parse(&assets.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))?);
            lists.insert(biome.to_owned(), parse(&ini));
        }
        Ok(Self(lists))
    }
    pub fn clips(&self, biome: &str, context: NatureContext) -> &[String] {
        self.0
            .get(biome)
            .map_or(&[], |lists| lists[context.index()].as_slice())
    }
}
fn parse(ini: &Ini) -> [Vec<String>; 6] {
    let mut lists: [Vec<String>; 6] = Default::default();
    for (path, tags) in ini.entries("sounds") {
        let tags = tags.to_ascii_lowercase();
        let any_time = !["day", "giorno", "night", "notte"]
            .iter()
            .any(|s| tags.contains(s));
        for (habitat, labels) in [
            (Habitat::Marine, ["marine", "mare"]),
            (Habitat::Forest, ["forest", "foresta"]),
            (Habitat::Plains, ["plains", "pianura"]),
        ] {
            if !labels.iter().any(|s| tags.contains(s)) {
                continue;
            }
            for (night, labels) in [(false, ["day", "giorno"]), (true, ["night", "notte"])] {
                if any_time || labels.iter().any(|s| tags.contains(s)) {
                    lists[NatureContext { habitat, night }.index()].push(crate::archive::key(path));
                }
            }
        }
    }
    lists
}

/// The view's timers for nature calls (0x4f423e) and rain (0x4a8140,
/// 0x4a8020), in milliseconds of the mission, drawing from the stream of
/// sounds and pictures (0x5cf588).
#[derive(Clone, Debug, Default)]
pub struct Scheduler {
    nature_due: Option<u64>,
    rain: Option<Rain>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rain {
    /// Level 0–2 of the shower (0x4e), kept while it ends (0x4f).
    level: usize,
    ending: bool,
    due: u64,
}
/// What the view sees at a moment.
pub struct Moment<'a> {
    pub now: u64,
    /// The game is paused (0x5cf6f0 & 2).
    pub paused: bool,
    /// Habitat and time of day of the explored cell at the centre (0x455680).
    pub nature: Option<NatureContext>,
    pub raining: bool,
    pub biome: &'a str,
}
impl Scheduler {
    /// The clips to play now. A nature call comes every R(10000, 25000) ms,
    /// drawn before anything else, one of the clips of the habitat at
    /// R(0, n − 1) unless the game is paused; rain starts at a level of
    /// R(0, 2) with its START clip, goes on with clip R(1, 6) when the
    /// previous one is 700 ms from its end (1000 ms later while paused),
    /// and ends with END in place of the next clip.
    pub fn tick(
        &mut self,
        m: &Moment,
        rng: &mut NativeRng,
        catalog: &Catalog,
        length: impl Fn(&str) -> u64,
    ) -> Vec<String> {
        let mut clips = Vec::new();
        let due = *self
            .nature_due
            .get_or_insert_with(|| m.now + rng.range(10_000, 25_000) as u64);
        if m.now >= due {
            self.nature_due = Some(m.now + rng.range(10_000, 25_000) as u64);
            if !m.paused
                && let Some(context) = m.nature
            {
                let list = catalog.clips(m.biome, context);
                if !list.is_empty() {
                    clips.push(list[rng.range(0, list.len() as i32 - 1) as usize].clone());
                }
            }
        }
        // 0x4a81d0: the next clip starts 700 ms before this one ends.
        let next = |name: &str| {
            let ms = length(name);
            if ms > 700 { ms - 700 } else { ms }
        };
        // 0x4a82b0 ends the shower; one starting while the last ends begins
        // afresh with a new level.
        if self.rain.is_some_and(|r| r.ending) && m.raining {
            self.rain = None;
        }
        match &mut self.rain {
            None if m.raining => {
                let level = rng.range(0, 2) as usize;
                let name = super::rain_clip(level, "START");
                self.rain = Some(Rain {
                    level,
                    ending: false,
                    due: m.now + next(&name),
                });
                clips.push(name);
            }
            Some(r) if !m.raining => r.ending = true,
            _ => {}
        }
        if let Some(r) = &mut self.rain
            && m.now >= r.due
        {
            if m.paused {
                r.due = m.now + 1000;
            } else if r.ending {
                clips.push(super::rain_clip(r.level, "END"));
                self.rain = None;
            } else {
                let name = super::rain_clip(r.level, &rng.range(1, 6).to_string());
                r.due = m.now + next(&name);
                clips.push(name);
            }
        }
        clips
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires installed original archives"]
    fn nature_sounds_follow_original_biome_habitat_and_time_lists() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        let catalog = Catalog::load(&assets).unwrap();
        let mut bank = super::super::AudioBank::open(std::path::Path::new("..")).unwrap();
        for biome in ["grass", "summer", "snow", "desert"] {
            for habitat in [Habitat::Marine, Habitat::Forest, Habitat::Plains] {
                for night in [false, true] {
                    let clips = catalog.clips(biome, NatureContext { habitat, night });
                    assert!(!clips.is_empty(), "{biome}/{habitat:?}/{night}");
                    for name in clips {
                        assert!(bank.wave(name).unwrap().seconds() > 0., "{name}");
                    }
                }
            }
        }
        let day = catalog.clips(
            "grass",
            NatureContext {
                habitat: Habitat::Plains,
                night: false,
            },
        );
        let night = catalog.clips(
            "grass",
            NatureContext {
                habitat: Habitat::Plains,
                night: true,
            },
        );
        assert!(day.iter().any(|s| s.ends_with("FIELD01.WAV")));
        assert!(!night.iter().any(|s| s.ends_with("FIELD01.WAV")));
    }
    #[test]
    fn nature_and_rain_draw_from_the_stream_of_sounds_like_the_view_timers() {
        let mut lists: [Vec<String>; 6] = Default::default();
        let context = NatureContext {
            habitat: Habitat::Plains,
            night: false,
        };
        lists[context.index()] = vec!["A".into(), "B".into(), "C".into()];
        let catalog = Catalog(BTreeMap::from([("grass".to_owned(), lists)]));
        let at = |now, paused, raining| Moment {
            now,
            paused,
            nature: Some(context),
            raining,
            biome: "grass",
        };
        let length = |_: &str| 2000;
        let mut s = Scheduler::default();
        let mut rng = NativeRng(666);
        let mut expect = NativeRng(666);
        // 0x4f423e: the interval first, then the clip, on the same stream.
        assert!(
            s.tick(&at(0, false, false), &mut rng, &catalog, length)
                .is_empty()
        );
        let first = expect.range(10_000, 25_000) as u64;
        assert!(
            s.tick(&at(first - 1, false, false), &mut rng, &catalog, length)
                .is_empty()
        );
        let clips = s.tick(&at(first, false, false), &mut rng, &catalog, length);
        let second = first + expect.range(10_000, 25_000) as u64;
        assert_eq!(clips, [["A", "B", "C"][expect.range(0, 2) as usize]]);
        // Paused, the interval is still drawn but no call is made.
        assert!(
            s.tick(&at(second, true, false), &mut rng, &catalog, length)
                .is_empty()
        );
        expect.range(10_000, 25_000);
        assert_eq!(rng, expect);
        // Rain: a level of R(0, 2) and its START; the next clip 700 ms
        // before the end, R(1, 6); END once the shower is over.
        let t = second + 1;
        let level = expect.range(0, 2) as usize;
        let start = s.tick(&at(t, false, true), &mut rng, &catalog, length);
        assert_eq!(start, [super::super::rain_clip(level, "START")]);
        assert!(
            s.tick(&at(t + 1299, false, true), &mut rng, &catalog, length)
                .is_empty()
        );
        let clip = expect.range(1, 6).to_string();
        let next = s.tick(&at(t + 1300, false, true), &mut rng, &catalog, length);
        assert_eq!(next, [super::super::rain_clip(level, &clip)]);
        assert!(
            s.tick(&at(t + 1400, false, false), &mut rng, &catalog, length)
                .is_empty()
        );
        // Paused when due: one second later.
        assert!(
            s.tick(&at(t + 2600, true, false), &mut rng, &catalog, length)
                .is_empty()
        );
        let end = s.tick(&at(t + 3600, false, false), &mut rng, &catalog, length);
        assert_eq!(end, [super::super::rain_clip(level, "END")]);
        assert_eq!(rng, expect);
        assert!(
            s.tick(&at(t + 9000, false, false), &mut rng, &catalog, length)
                .is_empty()
        );
    }
}
