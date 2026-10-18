use crate::{
    Result,
    assets::{Ini, legacy_text},
    hpfs::Container,
    invalid,
    map::MapGrid,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub title: String,
    pub description: String,
    pub file: PathBuf,
    pub nested: Option<String>,
    pub header: String,
    pub side: usize,
    /// Fewest and most kingdoms the map is made for (header bytes 0x4C, 0x4D).
    pub players: [u8; 2],
    /// Icon of the map list, a row of `COMMONMAP TYPES.BMP` (header byte 0x138).
    pub map_type: u8,
    pub initially_available: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Campaign {
    pub title: String,
    pub description: String,
    pub missions: Vec<Scenario>,
}
#[derive(Default, Clone, Debug, Serialize)]
pub struct ScenarioCatalog {
    pub campaigns: Vec<Campaign>,
    pub missions: Vec<Scenario>,
}
/// The American release's campaign, «The Burden of the Crown», which the
/// Italian edition lacks: `Campaign\Tzar.wcm` of the install, or of its CD
/// copied into `USA\CD`.
pub fn american_campaign(source: &Path) -> Option<PathBuf> {
    [
        source.join("Campaign/Tzar.wcm"),
        source.join("USA/CD/SETUP/Tzar/Campaign/Tzar.wcm"),
    ]
    .into_iter()
    .find(|p| crate::vfs::is_file(p))
}
/// The campaigns of FX's Spanish edition that the Italian one lacks, in
/// Spanish.
pub const SPANISH_CAMPAIGNS: [&str; 1] = ["Los Dominios de la Magia.wcm"];
/// The Spanish edition's own campaigns ([`SPANISH_CAMPAIGNS`]):
/// the `Campaign` folder of the install, or of the Spanish edition copied
/// into `Anthology`.
pub fn spanish_campaigns(source: &Path) -> Vec<PathBuf> {
    SPANISH_CAMPAIGNS
        .iter()
        .filter_map(|name| {
            [
                source.join("Campaign").join(name),
                source.join("Anthology/Campaign").join(name),
            ]
            .into_iter()
            .find(|p| crate::vfs::is_file(p))
        })
        .collect()
}
/// Whether a file is one of the Spanish edition's own campaigns.
pub fn is_spanish(file: &Path) -> bool {
    file.file_name()
        .is_some_and(|n| SPANISH_CAMPAIGNS.iter().any(|s| n.eq_ignore_ascii_case(s)))
}
/// Whether a file is the American campaign, whose texts are in English.
pub fn is_american(file: &Path) -> bool {
    file.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("Tzar.wcm"))
}
/// Where a map's LoadMap actions lead, as its campaign names its maps
/// (`maps/….wmp`, lower case).
fn next_maps(c: &Container, header: &str) -> Vec<String> {
    let base = &header[..header.len().saturating_sub(".header".len())];
    let entry = |suffix: &str| c.get(&format!("{base}.{suffix}")).unwrap_or_default();
    let Ok(bytes) = c.get(header) else {
        return Vec::new();
    };
    crate::script::Script::parse(
        crate::script::counts(&bytes),
        &entry("events"),
        &entry("actions"),
        &entry("rules"),
    )
    .map(|s| {
        s.actions
            .iter()
            .filter_map(|a| match &a.value {
                crate::script::Action::LoadMap(t) => {
                    Some(t.replace('\\', "/").to_ascii_lowercase())
                }
                _ => None,
            })
            .collect()
    })
    .unwrap_or_default()
}
/// The story order of a campaign whose map names carry none: the missions
/// MAPS\Maps.ini opens, then those their LoadMap actions lead to, as met;
/// any other after them, as Maps.ini lists them. The original lists only
/// the first (0x4225e3); the order shows in ReTzar's list of the missions
/// reached.
fn story_order(missions: Vec<Scenario>, next: &[Vec<String>]) -> Vec<Scenario> {
    let key = |m: &Scenario| m.nested.as_deref().unwrap_or("").to_ascii_lowercase();
    let mut order: Vec<usize> = (0..missions.len())
        .filter(|&i| missions[i].initially_available)
        .collect();
    let mut k = 0;
    while k < order.len() {
        for target in &next[order[k]] {
            if let Some(j) = missions.iter().position(|m| key(m) == *target)
                && !order.contains(&j)
            {
                order.push(j);
            }
        }
        k += 1;
    }
    let rest: Vec<usize> = (0..missions.len()).filter(|i| !order.contains(i)).collect();
    order.extend(rest);
    let mut slots: Vec<Option<Scenario>> = missions.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}
/// A text of the map files, in the language of the moment.
fn description(c: &Container, name: &str) -> String {
    c.get(name)
        .map(|d| crate::lang::mission_text(legacy_text(&d).trim_end_matches('\0').trim()))
        .unwrap_or_default()
}
fn scenario(
    c: &Container,
    file: &Path,
    nested: Option<String>,
    available: bool,
) -> Result<Scenario> {
    let header = &c
        .entries
        .iter()
        .find(|e| e.name.to_ascii_lowercase().ends_with(".header"))
        .ok_or_else(|| invalid("Scenario header not found"))?
        .name;
    let grid = MapGrid::read(c, header)?;
    let bytes = c.get(header)?;
    let title = legacy_text(
        bytes
            .get(0x56..0x96)
            .unwrap_or(&[])
            .split(|b| *b == 0)
            .next()
            .unwrap_or(&[]),
    )
    .trim()
    .to_owned();
    let title = if title.is_empty() {
        grid.name.rsplit('/').next().unwrap().to_owned()
    } else {
        crate::lang::mission_text(&title)
    };
    Ok(Scenario {
        id: format!(
            "{}::{}",
            file.file_name().unwrap().to_string_lossy(),
            grid.name
        )
        .to_ascii_lowercase(),
        title,
        description: description(c, &format!("{}.description", grid.name)),
        file: file.to_path_buf(),
        nested,
        header: header.clone(),
        side: grid.side,
        players: [
            bytes.get(0x4c).copied().unwrap_or(0),
            bytes.get(0x4d).copied().unwrap_or(0),
        ],
        map_type: bytes.get(0x138).copied().unwrap_or(0),
        initially_available: available,
    })
}
impl Scenario {
    /// Opens only the blocks of this scenario, even inside a large campaign.
    pub fn container(&self) -> Result<Container> {
        let c = Container::open(&self.file)?;
        if let Some(n) = &self.nested {
            c.open_nested(n)
        } else {
            Ok(c)
        }
    }
    pub fn grid(&self) -> Result<MapGrid> {
        MapGrid::read(&self.container()?, &self.header)
    }
    /// The recorded voices of FX's Spanish edition for this campaign
    /// mission: the WAV files of the same map in its Spanish campaign, when
    /// the pack holds it ([`crate::packing::SPANISH`]); none otherwise.
    pub fn spanish_voices(&self) -> Vec<(String, Vec<u8>)> {
        let (Some(nested), Some(root)) = (&self.nested, self.file.parent().and_then(Path::parent))
        else {
            return Vec::new();
        };
        let campaigns = root.join(crate::packing::SPANISH).join("Campaign");
        for path in crate::vfs::list(&campaigns).unwrap_or_default() {
            let Ok(c) = Container::open(&path) else {
                continue;
            };
            let Some(e) = c
                .entries
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(nested))
            else {
                continue;
            };
            let Ok(map) = c.open_nested(&e.name) else {
                return Vec::new();
            };
            return map
                .entries
                .iter()
                .filter(|x| x.name.to_ascii_lowercase().ends_with(".wav"))
                .filter_map(|x| Some((x.name.clone(), map.read(x.inode).ok()?)))
                .collect();
        }
        Vec::new()
    }
}
impl ScenarioCatalog {
    /// Find a scenario by its stable id: (campaign index, mission index).
    /// Indices change when maps are added; ids do not.
    pub fn locate(&self, id: &str) -> Option<(Option<usize>, usize)> {
        if let Some(i) = self.missions.iter().position(|m| m.id == id) {
            return Some((None, i));
        }
        self.campaigns.iter().enumerate().find_map(|(c, campaign)| {
            campaign
                .missions
                .iter()
                .position(|m| m.id == id)
                .map(|i| (Some(c), i))
        })
    }
    pub fn load(source: &Path) -> Result<Self> {
        let mut result = Self::default();
        let mut files: Vec<_> = crate::vfs::list(&source.join("Maps"))?
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wmp")))
            .collect();
        files.sort();
        for f in files {
            result
                .missions
                .push(scenario(&Container::open(&f)?, &f, None, true)?);
        }
        // An installation may lack the campaigns' folder.
        let mut files: Vec<_> = crate::vfs::list(&source.join("Campaign"))
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wcm")))
            .collect();
        if let Some(f) = american_campaign(source)
            && !files.iter().any(|p| is_american(p))
        {
            files.push(f);
        }
        for f in spanish_campaigns(source) {
            if !files.iter().any(|p| p.file_name() == f.file_name()) {
                files.push(f);
            }
        }
        files.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
        for f in files {
            let c = Container::open(&f)?;
            let description = description(&c, "MAPS/campaign description.ini");
            let title = description.lines().next().unwrap_or("").trim().to_owned();
            let mut campaign = Campaign {
                title,
                description,
                missions: Vec::new(),
            };
            let list = Ini::parse(&c.get("MAPS/maps.ini")?);
            let mut next = Vec::new();
            for (name, availability) in list.entries("maps") {
                let nested = c
                    .entries
                    .iter()
                    .find(|e| e.name.eq_ignore_ascii_case(&format!("MAPS/{name}.wmp")))
                    .ok_or_else(|| invalid(format!("Missing campaign scenario {name}")))?;
                let map = c.open_nested(&nested.name)?;
                let mission = scenario(
                    &map,
                    &f,
                    Some(nested.name.clone()),
                    availability.eq_ignore_ascii_case("yes"),
                )?;
                next.push(next_maps(&map, &mission.header));
                campaign.missions.push(mission);
            }
            // The expansion's directory order includes ending branches first.
            // Numbered scenario names are its stable story order.
            if campaign.missions.iter().any(|m| {
                m.nested
                    .as_deref()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            }) {
                campaign.missions.sort_by_key(|m| {
                    let internal = m
                        .nested
                        .as_deref()
                        .unwrap_or("")
                        .rsplit('/')
                        .next()
                        .unwrap_or("");
                    let n: String = internal
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    (n.parse::<u32>().unwrap_or(0), internal.to_owned())
                });
            } else {
                campaign.missions = story_order(campaign.missions, &next);
            }
            result.campaigns.push(campaign);
        }
        Ok(result)
    }
}
