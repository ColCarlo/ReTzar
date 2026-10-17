//! Random-map preparation, without AI or production. Native references:
//! 0x4b4c80 (generation), 0x401610 (plasma), 0x4b3c90 (starting shapes),
//! 0x4b2760 (corner encoding), 0x4b1560/0x4b1880 (object placement). The RNG
//! is modernized (SplitMix64 replaces the old global stream); geometry,
//! field kernels and placement are translated, the kernels compared with
//! machine-code fixtures.
use crate::{
    Result,
    assets::{GameAssets, Ini},
    bitmap::Bitmap,
    invalid,
    map::MapGrid,
    world::{
        GameRules, GameSetup, Kingdom, MapObject, MissionWorld, ObjectLibrary, class,
        starting::new_object,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SIDES: [usize; 7] = [64, 96, 128, 192, 256, 384, 512];
pub const TYPES: [&str; 6] = [
    "Casuale",
    "Arcipelaghi",
    "Isole",
    "Continente",
    "Laghi",
    "Terra",
];
// Original 0x58e7d4: water threshold, unused byte, plasma roughness, divisor.
const PARAMETERS: [[u8; 4]; 6] = [
    [0, 0, 0, 0],
    [190, 0, 5, 2],
    [160, 0, 3, 2],
    [130, 0, 1, 1],
    [120, 0, 3, 1],
    [0, 0, 1, 1],
];
const SHAPES: [u16; 16] = [15, 6, 10, 2, 12, 4, 8, 0, 13, 5, 9, 1, 11, 3, 7, 15];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Original combo indices, including zero = choose a map type.
    pub kind: usize,
    pub map_resources: usize,
    pub guarded: bool,
    pub animals: bool,
    pub items: bool,
    pub cliffs: bool,
    pub gateways: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            kind: 0,
            map_resources: 1,
            guarded: true,
            animals: true,
            items: true,
            cliffs: false,
            gateways: true,
        }
    }
}
impl Options {
    pub fn normalize(&mut self) {
        self.kind = self.kind.min(5);
        self.map_resources = self.map_resources.min(2);
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    /// The original's R(min, max), both ends included.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi.saturating_sub(lo) + 1)
    }
}

/// Translation of 0x401000/0x401610: recursive edges then centre,
/// TL/TR/BR/BL traversal, inclusive random displacement, u16 saturation.
/// The old code compares unsigned words: negative displacement saturates to
/// 255, not to zero. Changing this changes coastlines and forest density.
fn native_plasma(size: usize, seed: u64, roughness: u8, divisor: u8) -> Vec<u8> {
    struct Field {
        n: usize,
        a: Vec<u16>,
        rng: Rng,
        rough: usize,
        div: usize,
    }
    impl Field {
        fn set(&mut self, x: usize, y: usize, endpoints: &[(usize, usize)], amplitude: usize) {
            let i = y * self.n + x;
            if self.a[i] & 0x8000 != 0 {
                return;
            }
            let mean = endpoints
                .iter()
                .map(|&(x, y)| (self.a[y * self.n + x] & 0x7fff) as i32)
                .sum::<i32>()
                / endpoints.len() as i32;
            let value = (mean + self.rng.below(amplitude * 2 + 1) as i32 - amplitude as i32) as u16;
            self.a[i] = value.min(255) | 0x8000;
        }
        fn split(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
            let dx = x1 - x0;
            let dy = y1 - y0;
            if dx == 1 {
                if dy == 1 {
                    return;
                }
                let amp = 2 * self.rough / self.div;
                self.set(x0, y0 + 1, &[(x0, y0), (x0, y0 + 2)], amp);
                self.set(x1, y0 + 1, &[(x1, y0), (x1, y0 + 2)], amp);
                return;
            }
            let x = x0 + dx / 2;
            let y = y0 + dy / 2;
            let ax = dx * self.rough / self.div;
            let ay = dy * self.rough / self.div;
            self.set(x1, y, &[(x1, y0), (x1, y1)], ay);
            self.set(x0, y, &[(x0, y0), (x0, y1)], ay);
            self.set(x, y0, &[(x0, y0), (x1, y0)], ax);
            self.set(x, y1, &[(x0, y1), (x1, y1)], ax);
            self.set(x, y, &[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], ax);
            self.split(x0, y0, x, y);
            self.split(x, y0, x1, y);
            self.split(x, y, x1, y1);
            self.split(x0, y, x, y1);
        }
    }
    let mut f = Field {
        n: size,
        a: vec![0; size * size],
        rng: Rng(seed),
        rough: roughness as usize,
        div: divisor.max(1) as usize,
    };
    for i in [0, size - 1, size * size - 1, (size - 1) * size] {
        f.a[i] = f.rng.below(256) as u16;
    }
    f.split(0, 0, size - 1, size - 1);
    f.a.into_iter().map(|v| v as u8).collect()
}
fn plasma(seed: u64, roughness: u8, divisor: u8) -> Vec<u8> {
    native_plasma(512, seed, roughness, divisor)
}
fn sample(field: &[u8], side: usize, x: usize, y: usize) -> u8 {
    // Keep the original stride at exact divisors; fix its cropping at 96/192/384.
    field[(y.min(side - 1) * 512 / side) * 512 + x.min(side - 1) * 512 / side]
}
/// Original sectors (0x4b06f0), including the centred third/fifth kingdom.
fn sectors(side: usize, players: usize) -> Vec<[usize; 4]> {
    let h = side / 2;
    let q = side / 4;
    let t = side / 3;
    match players {
        2 => vec![[0, 0, side - 1, h - 1], [0, h, side - 1, side - 1]],
        3 => vec![
            [0, 0, h - 1, h - 1],
            [h, 0, side - 1, h - 1],
            [q, h, q + h - 1, side - 1],
        ],
        4 => vec![
            [0, 0, h - 1, h - 1],
            [h, 0, side - 1, h - 1],
            [h, h, side - 1, side - 1],
            [0, h, h - 1, side - 1],
        ],
        5 => vec![
            [0, 0, t - 1, h - 1],
            [t, 0, 2 * t - 1, h - 1],
            [2 * t, 0, 3 * t - 1, h - 1],
            [h, h, h + t - 1, side - 1],
            [side / 6, h, side / 6 + t - 1, side - 1],
        ],
        6 => vec![
            [0, 0, t - 1, h - 1],
            [t, 0, 2 * t - 1, h - 1],
            [2 * t, 0, 3 * t - 1, h - 1],
            [2 * t, h, 3 * t - 1, side - 1],
            [t, h, 2 * t - 1, side - 1],
            [0, h, t - 1, h + t - 1],
        ],
        7 => vec![
            [0, 0, q - 1, h - 1],
            [q, 0, 2 * q - 1, h - 1],
            [2 * q, 0, 3 * q - 1, h - 1],
            [3 * q, 0, side - 1, h - 1],
            [2 * t, h, 3 * t - 1, side - 1],
            [t, h, 2 * t - 1, side - 1],
            [0, h, t - 1, h + t - 1],
        ],
        _ => vec![
            [0, 0, q - 1, h - 1],
            [q, 0, 2 * q - 1, h - 1],
            [2 * q, 0, 3 * q - 1, h - 1],
            [3 * q, 0, side - 1, h - 1],
            [3 * q, h, side - 1, side - 1],
            [2 * q, h, 3 * q - 1, side - 1],
            [q, h, 2 * q - 1, side - 1],
            [0, h, q - 1, side - 1],
        ],
    }
}
/// Pixels of a starting shape (0x4b3f57–0x4b3fd9): white is skipped, green
/// (`0x3e0`) and black (`0`) open their layer and make land, every other
/// colour (the blue outer ring `0x3f` of the 40 and 60 families) only
/// makes land.
const WHITE: u8 = 0;
const GREEN: u8 = 1;
const BLACK: u8 = 2;
const LAND: u8 = 3;
#[derive(Clone)]
struct StartShape {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    pixels: Vec<u8>,
    sector: [usize; 4],
}
impl StartShape {
    fn pixel(&self, x: usize, y: usize) -> u8 {
        if x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h {
            self.pixels[(y - self.y) * self.w + x - self.x]
        } else {
            WHITE
        }
    }
    /// The shape's own cells of one colour, the layers of 0xc8b0b0[p].
    fn layer(&self, colour: u8) -> Vec<bool> {
        self.pixels.iter().map(|&p| p == colour).collect()
    }
}
/// 0x4b04a0 groups players by team, shuffles the groups, then their members.
/// Its four group slots include empty groups, which also consume random draws.
fn team_order(teams: &[u8], rng: &mut Rng) -> Vec<usize> {
    let mut by_team = std::collections::BTreeMap::<u8, Vec<usize>>::new();
    for (player, &team) in teams.iter().enumerate() {
        by_team.entry(team).or_default().push(player);
    }
    let mut groups: Vec<_> = by_team.into_values().collect();
    groups.resize(groups.len().max(4), Vec::new());
    let mut order = Vec::new();
    while !groups.is_empty() {
        let g = rng.below(groups.len());
        let mut members = groups.remove(g);
        while !members.is_empty() {
            let index = rng.below(members.len());
            order.push(members.remove(index));
        }
    }
    order
}
fn starting_shapes(
    assets: &GameAssets,
    side: usize,
    players: usize,
    seed: u64,
    teams: &[u8],
) -> Result<Vec<StartShape>> {
    let mut rng = Rng(seed ^ 0x5348415045);
    let area = side * side / players / 512;
    let family = if area < 35 {
        30
    } else if area < 50 {
        40
    } else {
        60
    };
    let sectors = sectors(side, players);
    let mut regions = vec![[0; 4]; players];
    for (sector, player) in team_order(teams, &mut rng).into_iter().enumerate() {
        regions[player] = sectors[sector];
    }
    let mut out = Vec::new();
    for sector in regions {
        let im = Bitmap::decode(
            &assets.get(&format!(
                "IMAGES/RANDOMMAP/{family}_RND_SHAPE{}.BMP",
                rng.below(3) + 1
            ))?,
            false,
        )?;
        let [x0, y0, x1, y1] = sector;
        // Sector endpoints are inclusive: a 0..31 sector has 32 cells,
        // leaving exactly 30 cells after its one-cell margins.
        let w = (im.width as usize).min(x1 - x0 - 1);
        let h = (im.height as usize).min(y1 - y0 - 1);
        let x = x0 + 1 + rng.below((x1 - x0 - w).max(1));
        let y = y0 + 1 + rng.below((y1 - y0 - h).max(1));
        let mut pixels = Vec::with_capacity(w * h);
        for yy in 0..h {
            for xx in 0..w {
                let i = ((yy * im.height as usize / h) * im.width as usize
                    + xx * im.width as usize / w)
                    * 4;
                // 0x4b3f57: the 15-bit colour of the pixel (0x517550).
                pixels.push(match [0, 1, 2].map(|c| im.rgba[i + c] >> 3) {
                    [0, 31, 0] => GREEN,
                    [0, 0, 0] => BLACK,
                    [31, 31, 31] => WHITE,
                    _ => LAND,
                });
            }
        }
        out.push(StartShape {
            x,
            y,
            w,
            h,
            pixels,
            sector,
        });
    }
    Ok(out)
}

fn encode(corners: [u8; 4]) -> u16 {
    let a = *corners.iter().min().unwrap() as u16;
    let b = *corners.iter().max().unwrap() as u16;
    if a == b {
        return (a << 7) | (b << 4) | SHAPES[15];
    }
    let mask = corners
        .iter()
        .enumerate()
        .fold(0, |m, (i, &v)| m | (((v as u16 == b) as usize) << (3 - i)));
    (a << 7) | (b << 4) | SHAPES[mask]
}

#[cfg(test)]
fn vertices(side: usize, seed: u64, kind: usize, _points: &[[u16; 2]]) -> Vec<u8> {
    vertices_with_parameters(side, seed, PARAMETERS[kind], kind == 2)
}
fn vertices_with_parameters(side: usize, seed: u64, params: [u8; 4], inverted: bool) -> Vec<u8> {
    let field = plasma(seed, params[2], params[3]);
    let mut v = vec![0; (side + 1) * (side + 1)];
    for y in 0..=side {
        for x in 0..=side {
            let h = sample(&field, side, x, y);
            // 0x4b28d0: islands invert the threshold; other map types use <=.
            v[y * (side + 1) + x] = u8::from(if inverted {
                h <= params[0]
            } else {
                h > params[0]
            });
        }
    }
    v
}

#[cfg(test)]
fn terrain_grid(side: usize, seed: u64) -> Result<MapGrid> {
    if !SIDES.contains(&side) {
        return Err(invalid("Dimensione della mappa casuale non valida"));
    }
    let v = vertices(side, seed, 1, &[]);
    Ok(grid_from_vertices(side, seed, &v))
}
fn grid_from_vertices(side: usize, seed: u64, v: &[u8]) -> MapGrid {
    let n = side + 1;
    let mut cells = Vec::with_capacity(side * side);
    for y in 0..side {
        for x in 0..side {
            cells.push(encode([
                v[y * n + x],
                v[y * n + x + 1],
                v[(y + 1) * n + x + 1],
                v[(y + 1) * n + x],
            ]));
        }
    }
    MapGrid {
        name: format!("Mappa casuale {seed}"),
        side,
        cells,
    }
}

pub struct Generated {
    pub grid: MapGrid,
    pub mission: MissionWorld,
    pub biome: String,
    pub kind: usize,
}
pub fn generate(
    assets: &GameAssets,
    side: usize,
    seed: u64,
    setup: Option<&GameSetup>,
) -> Result<Generated> {
    if !SIDES.contains(&side) {
        return Err(invalid("Dimensione della mappa casuale non valida"));
    }
    let setup = setup.cloned().unwrap_or_default();
    let opts = setup.random_generation.clone().unwrap_or_default();
    if opts.kind > 5 || opts.map_resources > 2 {
        return Err(invalid("Impostazioni della mappa casuale non valide"));
    }
    let players = setup.players.unwrap_or(2);
    if !(2..=8).contains(&players) || setup.human.unwrap_or(0) >= players {
        return Err(invalid("Numero di regni o giocatore umano non valido"));
    }
    let biome = setup.random_biome.clone().unwrap_or_else(|| "grass".into());
    if !["grass", "summer", "snow", "desert"].contains(&biome.as_str()) {
        return Err(invalid("Terreno casuale non valido"));
    }
    let ini = Ini::parse(&assets.get(&format!("IMAGES/TERRAIN/{biome}.TERRAIN.INI"))?);
    let res = Ini::parse(&assets.get("DATA/RES.INI")?);
    let lib = ObjectLibrary::load(assets, &biome)?;
    let kind = opts.kind;
    let mut teams = vec![0; players];
    for choice in &setup.kingdoms {
        if choice.record < players {
            teams[choice.record] = choice.team;
        }
    }
    let mut shapes = starting_shapes(assets, side, players, seed, &teams)?;
    // 0x4b4d07 skips preset overrides for type zero. The first INI terrain
    // layer supplies its own threshold and fractal parameters (0x4b40fa).
    // The mountains read the same field (0x4b435e passes it to 0x4b33b0).
    let params = if kind == 0 {
        let values = ini
            .entries("randomterrainset")
            .first()
            .map(|(key, _)| ini.numbers("randomterrainset", key))
            .filter(|v| v.len() >= 4)
            .ok_or_else(|| invalid("Parametri originali del terreno casuale mancanti"))?;
        std::array::from_fn(|i| values[i].clamp(0, 255) as u8)
    } else {
        PARAMETERS[kind]
    };
    let mut v = vertices_with_parameters(side, seed, params, kind == 2);
    let n = side + 1;
    // 0x4b5a18 counts the fish from the water vertices of this first
    // classification (0x4b28d0, 0xc8b118), before the shapes and layers.
    let water_vertices = (0..side)
        .flat_map(|y| (0..side).map(move |x| y * n + x))
        .filter(|&i| v[i] == 0)
        .count();
    let castle = cells(&lib, "e_castle").map(|c| c + 2);
    for shape in &mut shapes {
        castle_room(shape, castle);
        for y in 0..shape.h {
            for x in 0..shape.w {
                if shape.pixels[y * shape.w + x] != WHITE {
                    v[(shape.y + y) * n + shape.x + x] = 1;
                }
            }
        }
    }

    // Original layer passes operate on native 0/1 vertices.
    let mut layer_rng = seed ^ 0x4c4159455253;
    for (key, _) in ini.entries("randomterrainset").iter().skip(1) {
        let Some((paint, base)) = layer_pair(key) else {
            continue;
        };
        let values = ini.numbers("randomterrainset", key);
        if values.len() < 4 {
            continue;
        }
        let field = plasma(
            layer_rng,
            values[2].clamp(1, 255) as u8,
            values[3].clamp(1, 255) as u8,
        );
        layer_rng = layer_rng.wrapping_add(1);
        let heights: Vec<_> = (0..side)
            .flat_map(|y| {
                (0..side).map({
                    let field = &field;
                    move |x| sample(field, side, x, y)
                })
            })
            .collect();
        layer_pass(&mut v, side, &heights, paint, base, values[0], values[1]);
    }
    let mut grid = grid_from_vertices(side, seed, &v);
    // The forests leave the green and black of the shapes alone.
    let mut reserved = vec![false; side * side];
    for shape in &shapes {
        for y in shape.y..shape.y + shape.h {
            for x in shape.x..shape.x + shape.w {
                reserved[y * side + x] = matches!(shape.pixel(x, y), GREEN | BLACK);
            }
        }
    }
    if opts.cliffs {
        mountains(&mut grid, seed, params, &shapes);
    }
    nature(&mut grid, &ini, &reserved, seed);
    let rules = setup.options.clone().unwrap_or_else(|| GameRules {
        resource_level: 1,
        ..Default::default()
    });
    let section = ["Low", "Medium", "High"][rules.resource_level.min(2)];
    let resources = setup.resources.unwrap_or_else(|| {
        ["gold", "stone", "food", "wood"].map(|key| {
            res.numbers(section, key)
                .first()
                .copied()
                .unwrap_or(0)
                .max(0) as u32
        })
    });
    let mut kingdom_rng = Rng(seed ^ 0x4b494e47444f4d);
    let mut mission = MissionWorld {
        game_rules: rules,
        diplomacy: (0..64)
            .map(|i| if i / 8 == i % 8 { 0x40 } else { 0x10 })
            .collect(),
        heroes_taken: vec![true; setup.heroes_taken],
        ..Default::default()
    };
    // 0x4b4fb5: nine records named «Regno» (`misc` kingdom3) and their
    // index from 0, each led by the next hero of the table for the row's
    // civilization (any while it is still random), team and colour i + 1.
    // The screen's rows then give the kingdoms their names.
    let label = lib.locale.get("misc", "kingdom3").unwrap_or("Regno").trim();
    let heroes = crate::gameplay::hero_table(&crate::assets::legacy_text(
        &assets.get("DATA/HEROES.INI").unwrap_or_default(),
    ));
    for i in 0..9 {
        let civ = setup
            .kingdoms
            .iter()
            .find(|k| k.record == i)
            .and_then(|k| k.civilization)
            .unwrap_or(0);
        let leader = crate::gameplay::take_hero(&heroes, &mut mission.heroes_taken, civ)
            .unwrap_or_else(|| "Eroe".into());
        if i == 8 {
            break;
        }
        mission.kingdoms.push(Kingdom {
            name: format!("{label}{i}"),
            leader,
            active: i < players,
            color: i as u8 + 1,
            team: i as u8 + 1,
            civilization: 1 + kingdom_rng.below(3) as u8,
            resources,
            computer: i != setup.human.unwrap_or(0),
            ..Default::default()
        });
    }
    let mut resolved = setup.clone();
    for k in &mut resolved.kingdoms {
        if k.civilization == Some(0) {
            k.civilization = Some(1 + kingdom_rng.below(3) as u8);
        }
    }
    mission.apply_setup(&resolved);
    populate(
        &mut grid,
        &mut mission,
        &lib,
        &ini,
        &opts,
        &shapes,
        water_vertices,
        seed,
    )?;
    mission.records = mission.objects.len();
    Ok(Generated {
        grid,
        mission,
        biome,
        kind,
    })
}

fn layer_pair(key: &str) -> Option<(u8, u8)> {
    let b = key.as_bytes();
    (b.len() == 2 && b.iter().all(u8::is_ascii_digit)).then(|| (b[0] - b'0', b[1] - b'0'))
}

/// Tree images (0-based records in `[treename]` order) allowed on each
/// terrain by `[treeterrain]` (0x43cb0e). Unknown species are ignored.
fn tree_records(ini: &Ini) -> [Vec<u16>; 8] {
    let mut first = std::collections::BTreeMap::new();
    let mut next = 0u16;
    for (name, count) in ini.entries("treename") {
        let count = count.trim().parse::<u16>().unwrap_or(0);
        first.insert(name.to_ascii_lowercase(), (next, count));
        next += count;
    }
    let mut records: [Vec<u16>; 8] = Default::default();
    for (name, terrains) in ini.entries("treeterrain") {
        let Some(&(start, count)) = first.get(&name.to_ascii_lowercase()) else {
            continue;
        };
        for t in terrains
            .split(',')
            .filter_map(|t| t.trim().parse::<usize>().ok())
        {
            if let Some(list) = records.get_mut(t) {
                list.extend(start..start + count);
            }
        }
    }
    for list in &mut records {
        list.sort_unstable();
        list.dedup();
    }
    records
}
/// Cell bits of a generated tree (0x43c460, 0x4b2e2f): the `r`-th image
/// allowed on the terrain, or 0xffff past them. The 0-based record lands in
/// the 1-based tree field, so the original shows the image before the one it
/// picked, nothing for the first and the last image (31) for 0xffff.
fn tree_bits(records: &[Vec<u16>; 8], terrain: usize, r: usize) -> u16 {
    let record = records[terrain].get(r).copied().unwrap_or(0xffff);
    (record & 0x1f) << 11
}
/// The two terrains of a cell, their tree counts and the terrain that
/// chooses the species (the one with more images; the second on a tie).
fn tree_terrain(records: &[Vec<u16>; 8], cell: u16) -> ([usize; 2], usize, usize) {
    let t = [(cell >> 7 & 7) as usize, (cell >> 4 & 7) as usize];
    let n = [records[t[0]].len(), records[t[1]].len()];
    (t, n[0].max(n[1]), if n[0] > n[1] { t[0] } else { t[1] })
}
/// 0x4b2ab0: trees on land cells whose ring of cells two steps away is land
/// too. Inside the forest field the chance is `rand(0..=n·k) ≤ n` with
/// `k = 100 / divisor` (dense above the cutoff), outside a stray needs
/// `rand(0..=10000) ≤ setting 4` first; `n` is the number of tree images
/// allowed on the cell's terrains, and the same draw picks the species.
fn nature(grid: &mut MapGrid, ini: &Ini, reserved: &[bool], seed: u64) {
    let side = grid.side;
    let settings = ini.numbers("randomtreeset", &side.to_string());
    let setting = |k: usize, default: i32| settings.get(k).copied().unwrap_or(default);
    let water = ini
        .get("terrain", "water")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1);
    let records = tree_records(ini);
    let forest = plasma(
        seed ^ 0x464f52455354,
        setting(2, 11).clamp(1, 255) as u8,
        setting(3, 5).clamp(1, 255) as u8,
    );
    let mut rng = Rng(seed ^ 0x5452454553);
    let land = |c: u16| (c >> 7 & 7) as usize >= water && (c >> 4 & 7) as usize >= water;
    for y in 0..side {
        for x in 0..side {
            let i = y * side + x;
            let cell = grid.cells[i];
            let ring = [0, 2, 4].iter().all(|&dy| {
                [0, 2, 4].iter().all(|&dx| {
                    let (xx, yy) = ((x + dx).wrapping_sub(2), (y + dy).wrapping_sub(2));
                    (dx, dy) == (2, 2)
                        || xx >= side
                        || yy >= side
                        || land(grid.cells[yy * side + xx])
                })
            });
            let (_, n, terrain) = tree_terrain(&records, cell);
            if reserved[i] || !ring || cell & 0xfc00 != 0 || !land(cell) || n == 0 {
                continue;
            }
            let h = sample(&forest, side, x, y) as i32;
            let k = if h >= setting(0, 1) && h <= setting(1, 255) {
                100 / if h > setting(7, 235) {
                    setting(6, 40)
                } else {
                    setting(5, 1)
                }
                .max(1)
            } else if rng.below(10001) as i32 <= setting(4, 1) {
                1
            } else {
                continue;
            };
            let r = rng.below(n * k.max(0) as usize + 1);
            if r <= n {
                grid.cells[i] |= tree_bits(&records, terrain, r);
            }
        }
    }
}

/// 0x4b23c0: row-major, in-place painting, rejecting a third surface in
/// the eight neighbouring vertices. Boundary copies retain their base type.
fn layer_pass(v: &mut [u8], side: usize, field: &[u8], paint: u8, base: u8, min: i32, max: i32) {
    let stride = side + 1;
    for y in 0..side {
        for x in 0..side {
            let i = y * stride + x;
            let h = field[y * side + x] as i32;
            if v[i] != base || h < min || h > max {
                continue;
            }
            if (y.saturating_sub(1)..=y + 1).all(|yy| {
                (x.saturating_sub(1)..=x + 1).all(|xx| [base, paint].contains(&v[yy * stride + xx]))
            }) {
                v[i] = paint;
            }
        }
    }
}

/// 0x4b3090: height derivatives bend an initially straight cliff path.
fn cliff_path(field: &[u8], side: usize, length: usize, rng: &mut Rng) -> Vec<[i32; 2]> {
    let mut column = rng.below(side - length + 1);
    let row = 10 + rng.below(side - 19);
    let mut path = vec![[0, 0]];
    let mut progress = 0;
    while path.len() < length {
        let delta = if path.len() <= 2 {
            0
        } else {
            (field[row * side + column + 1] as i32 - field[row * side + column] as i32).signum()
        };
        column += 1;
        progress += 1;
        if delta != 0 {
            path.push([progress - 1, path.last().unwrap()[1] + delta]);
        }
        // Native writes the extra terminal point if a bend straddles length.
        path.push([progress, path.last().unwrap()[1]]);
    }
    path.truncate(length);
    path
}
fn cliff_turn(points: [[i32; 2]; 3], previous: u8, side: &mut bool) -> u8 {
    let [[x0, y0], [x1, y1], [x2, y2]] = points;
    let kind = if x0 == x1 && x1 == x2 {
        0
    } else if y0 == y1 && y1 == y2 {
        1
    } else if x0 < x2 {
        if x0 == x1 {
            2 + u8::from(y0 <= y2)
        } else {
            4 + u8::from(y0 > y2)
        }
    } else if x0 == x1 {
        4 + u8::from(y0 <= y2)
    } else {
        2 + u8::from(y0 > y2)
    };
    let flip = match previous {
        0 => {
            if *side {
                ![0, 2, 3].contains(&kind)
            } else {
                ![0, 4, 5].contains(&kind)
            }
        }
        1 => ![1, 2, 4].contains(&kind),
        2 => [0, 5].contains(&kind),
        3 => [0, 1, 4].contains(&kind),
        4 => kind == 3,
        5 => [1, 2].contains(&kind),
        _ => false,
    };
    if flip {
        *side = !*side;
    }
    kind
}
fn cliff_cap(points: [[i32; 2]; 2], start: bool, previous: u8, side: &mut bool) -> (u8, bool) {
    let [[x0, y0], [x1, y1]] = points;
    let kind = if y0 == y1 { 1 } else { 0 };
    let mut reverse = if kind == 1 { x1 < x0 } else { y1 < y0 };
    if !start {
        reverse = !reverse;
        if (previous == 2 && kind == 0 && reverse)
            || previous == 3
            || (previous == 5 && kind == 1 && reverse)
        {
            *side = !*side;
        }
    }
    (kind, reverse)
}
/// Six turns × two faces, and four cap orientations × two faces. Coordinates
/// are the original atlas table at 0x58e7fc / 0x58e814, in 32px tiles.
const CLIFF_TURNS: [[u16; 2]; 12] = [
    [4, 2],
    [0, 2],
    [2, 0],
    [2, 4],
    [0, 0],
    [0, 6],
    [0, 4],
    [0, 8],
    [4, 0],
    [2, 6],
    [4, 4],
    [2, 8],
];
const CLIFF_CAPS: [[u16; 2]; 8] = [
    [4, 6],
    [4, 8],
    [4, 10],
    [4, 12],
    [0, 10],
    [2, 10],
    [0, 12],
    [2, 12],
];
fn cliff_pieces(path: &[[i32; 2]], origin: [i32; 2], face: &mut bool) -> Vec<([i32; 2], [u16; 2])> {
    let mut previous = 255;
    let mut cell = origin;
    let mut orientation = false;
    let mut pieces = Vec::new();
    for i in 0..path.len() - 1 {
        let cap = i == 0 || i == path.len() - 2;
        let kind = if cap {
            let (k, o) = cliff_cap([path[i], path[i + 1]], i == 0, previous, face);
            orientation = o;
            k
        } else {
            cliff_turn([path[i], path[i + 1], path[i + 2]], previous, face)
        };
        previous = kind;
        let atlas = if cap {
            CLIFF_CAPS[orientation as usize + 2 * (*face as usize + 2 * kind as usize)]
        } else {
            CLIFF_TURNS[kind as usize * 2 + *face as usize]
        };
        pieces.push((cell, atlas));
        match kind {
            0 => cell[1] += if orientation { -2 } else { 2 },
            1 => cell[0] += if orientation { -2 } else { 2 },
            2 => {
                if path[i][0] < path[i + 2][0] {
                    cell[0] += 2;
                }
                if path[i][1] < path[i + 2][1] {
                    cell[1] += 2;
                }
            }
            3 => {
                if path[i][0] < path[i + 2][0] {
                    cell[0] += 2;
                }
                if path[i][1] >= path[i + 2][1] {
                    cell[1] -= 2;
                }
            }
            4 => {
                if path[i][0] >= path[i + 2][0] {
                    cell[0] -= 2;
                }
                if path[i][1] < path[i + 2][1] {
                    cell[1] += 2;
                }
            }
            5 => {
                if path[i][0] >= path[i + 2][0] {
                    cell[0] -= 2;
                }
                if path[i][1] >= path[i + 2][1] {
                    cell[1] -= 2;
                }
            }
            _ => unreachable!(),
        }
    }
    pieces
}
/// 0x4b33b0: connected 2×2 pieces, length and count scaled by map size.
fn mountains(grid: &mut MapGrid, seed: u64, params: [u8; 4], shapes: &[StartShape]) {
    let side = grid.side;
    // Native's 24..size-35 origin requires at least 64 cells.
    let field = plasma(seed, params[2], params[3]);
    let heights: Vec<_> = (0..side)
        .flat_map(|y| {
            (0..side).map({
                let field = &field;
                move |x| sample(field, side, x, y)
            })
        })
        .collect();
    let count = (grid
        .cells
        .iter()
        .filter(|&&c| c >> 7 & 7 != 0 && c >> 4 & 7 != 0)
        .count() as f64)
        .sqrt() as usize
        / 16;
    native_cliffs(
        grid,
        &heights,
        shapes,
        &mut Rng(seed ^ 0x434c49464653),
        count,
    );
}
fn native_cliffs(
    grid: &mut MapGrid,
    heights: &[u8],
    shapes: &[StartShape],
    rng: &mut Rng,
    count: usize,
) {
    let side = grid.side;
    let min = 13 - (512 - side) * 10 / 512;
    let max = 21 - (512 - side) * 18 / 512;
    let mut placed = 0;
    for _ in 0..500 {
        if placed >= count {
            break;
        }
        let origin = [
            ((24 + rng.below(side - 58)) & !1) as i32,
            ((24 + rng.below(side - 58)) & !1) as i32,
        ];
        let length = min + rng.below(max - min + 1);
        let path = cliff_path(heights, side, length, rng);
        let mut face = rng.below(2) != 0;
        let pieces = cliff_pieces(&path, origin, &mut face);
        if !pieces.iter().all(|&(cell, _)| {
            let [x, y] = cell;
            x >= 1
                && y >= 1
                && x + 2 < side as i32
                && y + 2 < side as i32
                && (y..y + 2).all(|yy| {
                    (x..x + 2).all(|xx| {
                        let c = grid.cells[yy as usize * side + xx as usize];
                        c & 0xfc00 == 0
                            && c >> 7 & 7 != 0
                            && c >> 7 & 7 == c >> 4 & 7
                            && !shapes.iter().any(|s| {
                                xx >= s.x as i32
                                    && yy >= s.y as i32
                                    && xx < (s.x + s.w) as i32
                                    && yy < (s.y + s.h) as i32
                            })
                    })
                })
        }) {
            continue;
        }
        // Native validation advances the facing state; its second traversal
        // keeps that final face before stamping the atlas pieces.
        for (cell, atlas) in cliff_pieces(&path, origin, &mut face) {
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = (cell[1] as usize + dy) * side + cell[0] as usize + dx;
                    let ground = grid.cells[i] >> 7 & 7;
                    grid.cells[i] = 0x400
                        | (ground << 7)
                        | ((atlas[0] + dx as u16) << 4)
                        | (atlas[1] + dy as u16);
                }
            }
        }
        placed += 1;
    }
}

/// The cells of a picture as the image loader keeps them (0x498373,
/// 0x498d34; `ObjectLibrary::picture_cells`).
fn cells(lib: &ObjectLibrary, picture: &str) -> [usize; 2] {
    lib.picture_cells(picture)
}
/// The cells of the picture an object of the class is created with.
fn class_cells(lib: &ObjectLibrary, o: &MapObject) -> [usize; 2] {
    lib.final_image(o)
        .map_or([1, 1], |picture| cells(lib, &picture))
}
/// Past the menu's limits (more kingdoms at 64 and 96, development settings
/// only) a bitmap is squeezed into a sector narrower than it; a squeezed
/// shape left without a green block for the castle gets one in its centre.
fn castle_room(shape: &mut StartShape, room: [usize; 2]) {
    if !corners(&shape.layer(GREEN), shape.w, shape.h, room).is_empty() {
        return;
    }
    let [w, h] = [room[0].min(shape.w), room[1].min(shape.h)];
    let (x0, y0) = ((shape.w - w) / 2, (shape.h - h) / 2);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            shape.pixels[y * shape.w + x] = GREEN;
        }
    }
}
/// 0x4b1120: a block of `size` at (x, y) wholly inside the layer and set.
fn block(layer: &[bool], w: usize, h: usize, x: usize, y: usize, size: [usize; 2]) -> bool {
    x + size[0] <= w
        && y + size[1] <= h
        && (y..y + size[1]).all(|yy| (x..x + size[0]).all(|xx| layer[yy * w + xx]))
}
/// 0x4b11f0: the corners of those blocks, row by row.
fn corners(layer: &[bool], w: usize, h: usize, size: [usize; 2]) -> Vec<[usize; 2]> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| [x, y]))
        .filter(|&[x, y]| block(layer, w, h, x, y, size))
        .collect()
}
/// 0x4b1280: a block leaves the layer and the candidates whose corner lies
/// in it leave the list; candidates that only overlap it stay.
fn clear(
    layer: &mut [bool],
    w: usize,
    list: &mut Vec<[usize; 2]>,
    at: [usize; 2],
    size: [usize; 2],
) {
    for y in at[1]..at[1] + size[1] {
        for x in at[0]..at[0] + size[0] {
            layer[y * w + x] = false;
        }
    }
    list.retain(|&[x, y]| {
        !(x >= at[0] && x < at[0] + size[0] && y >= at[1] && y < at[1] + size[1])
    });
}
/// 0x4b1dd0: three wooden towers on the four arms of the castle's axes.
/// The candidates are the corners of black blocks three cells larger than
/// the tower: in the band of ten columns through the shape's centre, north
/// or south of its centre row; else in the band of ten rows, west or east
/// of its centre column. A random arm opens, then every arm in turn gives
/// a random corner of its own, the tower standing one cell inside. Only the
/// tower's cells leave the black and only corners inside them leave the
/// arm, so an arm visited twice (the others empty) may give the same
/// corner again. No test of terrain or reach.
fn towers(
    shape: &StartShape,
    black: &mut [bool],
    tower: [usize; 2],
    rng: &mut Rng,
) -> Vec<[usize; 2]> {
    let (w, h) = (shape.w as i32, shape.h as i32);
    let (x0, y0) = ((w - 10) / 2, (h - 10) / 2);
    let (cx, cy) = ((w - 1) / 2, (2 * y0 + 9) / 2);
    let size = tower.map(|n| n + 3);
    let mut arms: [Vec<[usize; 2]>; 4] = Default::default();
    for y in 0..h {
        for x in 0..w {
            let corner = [x as usize, y as usize];
            if !block(black, shape.w, shape.h, corner[0], corner[1], size) {
                continue;
            }
            if (x0..=x0 + 9).contains(&x) {
                arms[if y < cy { 1 } else { 3 }].push(corner);
            } else if (y0..=y0 + 9).contains(&y) {
                arms[if x < cx { 0 } else { 2 }].push(corner);
            }
        }
    }
    let n = arms.iter().map(Vec::len).sum::<usize>().min(3);
    let mut arm = rng.below(4);
    let mut out = Vec::new();
    while out.len() < n {
        let list = &mut arms[arm];
        if !list.is_empty() {
            let [x, y] = list[rng.below(list.len())];
            let at = [x + 1, y + 1];
            clear(black, shape.w, list, at, tower);
            out.push([shape.x + at[0], shape.y + at[1]]);
        }
        arm = (arm + 1) % 4;
    }
    out
}
/// 0x4b4460 for a kingdom's first deposit: a random corner of the black
/// blocks two cells larger than the deposit, which stands one cell inside;
/// the whole block leaves the black.
fn first_deposit(
    shape: &StartShape,
    black: &mut [bool],
    size: [usize; 2],
    rng: &mut Rng,
) -> Option<[usize; 2]> {
    let room = size.map(|n| n + 2);
    let mut list = corners(black, shape.w, shape.h, room);
    // The original would read a missing candidate; a squeezed shape (see
    // `castle_room`) can lack the block.
    if list.is_empty() {
        return None;
    }
    let [x, y] = list[rng.below(list.len())];
    clear(black, shape.w, &mut list, [x, y], room);
    Some([shape.x + x + 1, shape.y + y + 1])
}
/// The terrains a picture may lie on (0x4985ac): `water` the water kinds,
/// `land` the others, else a list of kinds.
fn terrain_mask(lib: &ObjectLibrary, picture: &str, water: usize, kinds: usize) -> u8 {
    let span = |r: std::ops::Range<usize>| r.filter(|&k| k < 8).fold(0u8, |m, k| m | 1 << k);
    match lib.images.get(picture, "terrains").map(str::trim) {
        None => 0,
        Some(t) if t.eq_ignore_ascii_case("water") => span(0..water),
        Some(t) if t.eq_ignore_ascii_case("land") => span(water..kinds),
        Some(_) => lib
            .images
            .numbers(picture, "terrains")
            .into_iter()
            .take(9)
            .filter(|k| (0..8).contains(k))
            .fold(0, |m, k| m | 1 << k),
    }
}
/// The layers objects take in the generator's cells (0xc8b110).
const AIR: u32 = 0x800000;
const ON_LAND: u32 = 0x1000000;
const ON_WATER: u32 = 0x2000000;
/// Set with the land layer by the objects of the shapes, the deposits and
/// the ground objects.
const TAKEN: u32 = 0x10000;
/// The generator's cells: the terrain word with its trees and cliffs, and
/// the layers the objects take.
struct Ground<'a> {
    grid: &'a mut MapGrid,
    marks: Vec<u32>,
    water: usize,
}
impl Ground<'_> {
    fn side(&self) -> usize {
        self.grid.side
    }
    fn word(&self, x: usize, y: usize) -> u32 {
        let i = y * self.side() + x;
        self.grid.cells[i] as u32 | self.marks[i]
    }
    fn land(&self, word: u32) -> bool {
        (word >> 7 & 7) as usize >= self.water && (word >> 4 & 7) as usize >= self.water
    }
    fn mark(&mut self, at: [usize; 2], size: [usize; 2], bits: u32) {
        let side = self.side();
        for y in at[1]..(at[1] + size[1]).min(side) {
            for x in at[0]..(at[0] + size[0]).min(side) {
                self.marks[y * side + x] |= bits;
            }
        }
    }
    /// 0x4b1360: an object of the shapes or a deposit takes its cells.
    fn take(&mut self, at: [usize; 2], size: [usize; 2]) {
        self.mark(at, size, ON_LAND | TAKEN);
    }
    /// 0x4b1420: a square of `size` free in the layer: in the air nothing
    /// flying, on land free land without trees or cliffs, on water free
    /// water. The other layers pass.
    fn fits(&self, layer: usize, at: [usize; 2], size: usize) -> bool {
        (at[1]..at[1] + size).all(|y| {
            (at[0]..at[0] + size).all(|x| {
                let c = self.word(x, y);
                match layer {
                    0 => c & AIR == 0,
                    1 => self.land(c) && c & 0x108fc00 == 0,
                    2 => {
                        (c >> 7 & 7) < self.water as u32
                            && (c >> 4 & 7) < self.water as u32
                            && c & ON_WATER == 0
                    }
                    _ => true,
                }
            })
        })
    }
    /// 0x4b1560: up to `count` objects on random cells of the whole map, at
    /// least `margin` from its edges (16 for birds and eagles), in 6999
    /// draws for the whole series. The test covers a square of the given
    /// margin at the cell, in the class's layer (`teren`); the object then
    /// takes its picture's cells there. Cows are never placed. The count of
    /// placed objects is a byte: past 255 it starts again from the first
    /// slot, and only the slots below it are returned.
    #[allow(clippy::too_many_arguments)]
    fn scatter(
        &mut self,
        rng: &mut Rng,
        class: usize,
        layer: usize,
        size: [usize; 2],
        count: usize,
        margin: usize,
    ) -> Vec<[usize; 2]> {
        let mut slots: Vec<[usize; 2]> = Vec::new();
        let mut placed = 0u8;
        let side = self.side();
        let edge = if matches!(class, class::BIRD | class::EAGLE) {
            16
        } else {
            margin
        };
        if class == class::COW || count == 0 || side < 2 * edge.max(margin) + 1 {
            return slots;
        }
        for _ in 1..7000 {
            let at = [0; 2].map(|_| rng.range(edge, side - edge - 1));
            if layer <= 2 && self.fits(layer, at, margin) {
                self.mark(at, size, [AIR, ON_LAND, ON_WATER][layer]);
                match slots.get_mut(placed as usize) {
                    Some(slot) => *slot = at,
                    None => slots.push(at),
                }
                placed = placed.wrapping_add(1);
            }
            if placed as usize >= count {
                break;
            }
        }
        slots.truncate(placed as usize);
        slots
    }
    /// 0x4b1880: up to `count` copies of a picture, each with 7000 random
    /// corners on the whole map; the first copy that finds none ends the
    /// series. Every cell of the picture and of the ring around it inside
    /// the map must have both terrains in the picture's mask, and the ring's
    /// bottom row must reach into the map. A ground object also keeps away
    /// from the rectangles of the starting shapes (its corner one cell
    /// further left and up), needs cells without trees, objects or deposits
    /// and takes them, ring included; a ground patch takes nothing.
    fn decorate(
        &mut self,
        rng: &mut Rng,
        object: bool,
        size: [usize; 2],
        mask: u8,
        count: usize,
        shapes: &[StartShape],
    ) -> Vec<[usize; 2]> {
        let side = self.side();
        let mut out = Vec::new();
        if size[0] >= side || size[1] >= side {
            return out;
        }
        for _ in 0..count {
            let found = (0..7000).find_map(|_| {
                let x = rng.range(0, side - size[0] - 1);
                let y = rng.range(0, side - size[1] - 1);
                let shaped = shapes.iter().any(|s| {
                    x + size[0] >= s.x && x < s.x + s.w && y + size[1] >= s.y && y < s.y + s.h
                });
                (!(object && shaped) && self.ring_fits([x, y], size, mask, object))
                    .then_some([x, y])
            });
            let Some(at) = found else {
                break;
            };
            if object {
                for y in at[1].saturating_sub(1)..(at[1] + size[1] + 1).min(side) {
                    for x in at[0].saturating_sub(1)..(at[0] + size[0] + 1).min(side) {
                        self.marks[y * side + x] |= ON_LAND | TAKEN;
                    }
                }
            }
            out.push(at);
        }
        out
    }
    /// The cell test of 0x4b1880. Each row of the ring passes on its last
    /// cell, so a row wholly outside the map (the top one at y = 0) fails.
    fn ring_fits(&self, at: [usize; 2], size: [usize; 2], mask: u8, object: bool) -> bool {
        let side = self.side() as i32;
        (-1..=size[1] as i32).all(|dy| {
            let mut ok = false;
            for dx in -1..=size[0] as i32 {
                let (x, y) = (at[0] as i32 + dx, at[1] as i32 + dy);
                ok = false;
                if x < 0 || y < 0 || x >= side || y >= side {
                    continue;
                }
                let c = self.word(x as usize, y as usize);
                if (1u8 << (c >> 7 & 7) | 1u8 << (c >> 4 & 7) | mask) != mask
                    || object && c & 0x109f800 != 0
                {
                    return false;
                }
                ok = true;
            }
            ok
        })
    }
}
fn word(o: &mut MapObject, at: usize, n: u16) {
    if o.data.len() < at + 2 {
        o.data.resize(at + 2, 0);
    }
    o.data[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn amount(o: &mut MapObject, n: u32) {
    let (at, len) = if o.class == class::FISHPATCH {
        (56, 60)
    } else {
        (58, 64)
    };
    o.data.resize(len, 0);
    o.data[at..at + 4].copy_from_slice(&n.to_le_bytes());
    word(o, 0, len as u16);
}
/// A new object of the generator, added to the mission.
fn add(
    mission: &mut MissionWorld,
    ids: &mut BTreeSet<u16>,
    lib: &ObjectLibrary,
    class: usize,
    owner: u8,
    civ: u8,
    at: [usize; 2],
) -> Result<usize> {
    let o = new_object(ids, 1, class, owner, civ, at.map(|n| n as u16), lib)?;
    mission.objects.push(o);
    Ok(mission.objects.len() - 1)
}
/// The picture's name in the record (+0x38), as ruins and items keep it.
fn picture(o: &mut MapObject, name: &str, len: usize) {
    o.data.resize(len, 0);
    let n = name.len().min(31);
    o.data[56..56 + n].copy_from_slice(&name.as_bytes()[..n]);
    o.custom_image = name[..n].into();
    word(o, 0, len as u16);
}
/// 0x58e788 and the six descriptors at 0x58e798: the random spells, the
/// pictures and the bonus ranges of the items.
const SPELLS: [u8; 7] = [9, 17, 14, 1, 12, 5, 2];
/// A random item of the class (0x4b5c50–0x4b6150).
fn item(o: &mut MapObject, rng: &mut Rng) {
    let (image, at, bonus) = match o.class {
        // Books: a picture, a spell, five charges at most, 1–3 uses.
        class::ITEM_MAGIC => (
            format!("item_book{}", rng.below(3) + 1),
            102,
            1 + rng.below(3) as u16,
        ),
        // R(2, 4): the bow, the shield or the sword.
        class::ITEM_PLUSPLUS => match rng.range(2, 4) {
            2 => (
                format!("item_bow{}", rng.below(3) + 1),
                22,
                (rng.range(5, 15) as u16) << 8,
            ),
            3 => (
                format!("item_shield{}", rng.below(3) + 1),
                24,
                rng.range(5, 15) as u16,
            ),
            _ => (
                ["item_sword", "item_sword1", "item_sword2", "item_sword3"][rng.below(4)].into(),
                22,
                rng.range(5, 15) as u16,
            ),
        },
        // R(0, 1) chooses the descriptor: its first two potions restore
        // 10–50, the last three of the other 2–8.
        _ => {
            let kind = if rng.below(2) == 1 {
                rng.range(2, 4)
            } else {
                rng.below(2)
            };
            let (low, high) = if kind < 2 { (10, 50) } else { (2, 8) };
            (
                [
                    "item_potion2",
                    "item_potion4",
                    "item_potion3",
                    "item_potion7",
                    "item_potion6",
                ][kind]
                    .into(),
                [18, 36, 28, 22, 24][kind],
                rng.range(low, high) as u16,
            )
        }
    };
    picture(o, &image, 106);
    for at in [18, 20, 22, 24, 28, 36] {
        word(o, at, 0);
    }
    o.health = 0;
    o.max_health = 0;
    word(o, at, bonus);
    if o.class == class::ITEM_MAGIC {
        o.data[96] = SPELLS[rng.below(SPELLS.len())];
        o.data[98] = 1;
        o.data[99] = 1;
        word(o, 100, rng.range(1, 5) as u16);
    } else if o.class == class::ITEM_PLUSPLUS {
        word(
            o,
            96,
            if image.starts_with("item_shield") {
                2
            } else {
                1
            },
        );
    }
}

/// 0x4b4c80 after the terrain: the objects in the original's order.
#[allow(clippy::too_many_arguments)]
fn populate(
    grid: &mut MapGrid,
    mission: &mut MissionWorld,
    lib: &ObjectLibrary,
    terrain: &Ini,
    opts: &Options,
    shapes: &[StartShape],
    water_vertices: usize,
    seed: u64,
) -> Result<()> {
    let mut rng = Rng(seed ^ 0x5245534f55524345);
    let side = grid.side;
    let players = shapes.len();
    let count = |key: &str, default: usize| {
        terrain
            .get("terrain", key)
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(default)
    };
    let water = count("water", 1);
    let kinds = water + count("ground", 7);
    let standard = mission.game_rules.game_type == 0;
    let civs: Vec<u8> = (0..players)
        .map(|p| mission.kingdoms[p].civilization)
        .collect();
    let mut ids = BTreeSet::new();
    let mut green: Vec<_> = shapes.iter().map(|s| s.layer(GREEN)).collect();
    let mut black: Vec<_> = shapes.iter().map(|s| s.layer(BLACK)).collect();
    let mut ground = Ground {
        grid,
        marks: vec![0; side * side],
        water,
    };
    if opts.guarded {
        let tower = cells(lib, "e_wood_tower");
        for (p, shape) in shapes.iter().enumerate() {
            for at in towers(shape, &mut black[p], tower, &mut rng) {
                ground.take(at, tower);
                add(
                    mission,
                    &mut ids,
                    lib,
                    class::TOWER_WOOD,
                    p as u8 + 1,
                    civs[p],
                    at,
                )?;
            }
        }
    }
    // 0x4b4950: gold, then stone; every kingdom's first deposit, then in
    // standard games the others, with up to 40 draws each; the first that
    // fails ends the kingdom's series. Amounts are R(min, max) rounded down
    // to hundreds.
    let first = [2300, 3700, 5200][opts.map_resources];
    let (others, low, high) =
        [(2, 2300, 3700), (2, 3700, 5200), (3, 5200, 7500)][opts.map_resources];
    let mut deposits = vec![Vec::new(); players];
    for (class, name) in [
        (class::RES_GOLD, "res_gold"),
        (class::RES_STONE, "res_stone"),
    ] {
        let size = cells(lib, name);
        for (p, shape) in shapes.iter().enumerate() {
            if let Some(at) = first_deposit(shape, &mut black[p], size, &mut rng) {
                ground.take(at, size);
                deposits[p].push((class, at, first));
            }
        }
        if !standard {
            continue;
        }
        for (p, shape) in shapes.iter().enumerate() {
            for _ in 0..others {
                let Some(at) = ground.deposit_site(shape, size, &mut rng) else {
                    break;
                };
                ground.take(at, size);
                deposits[p].push((class, at, rng.range(low, high) as u32 / 100 * 100));
            }
        }
    }
    // 0x4b21d0: the castle (`e_castle`, 7 × 7 for every civilization) on a
    // random corner of the green blocks two cells larger, one cell inside;
    // only its 7 × 7 leaves the green. No test of terrain.
    let castle = cells(lib, "e_castle");
    for (p, shape) in shapes.iter().enumerate() {
        let room = castle.map(|n| n + 2);
        let mut list = corners(&green[p], shape.w, shape.h, room);
        if list.is_empty() {
            return Err(invalid("Spazio insufficiente per il castello"));
        }
        let [x, y] = list[rng.below(list.len())];
        clear(&mut green[p], shape.w, &mut list, [x + 1, y + 1], castle);
        let at = [shape.x + x + 1, shape.y + y + 1];
        ground.take(at, castle);
        add(
            mission,
            &mut ids,
            lib,
            class::CASTLE,
            p as u8 + 1,
            civs[p],
            at,
        )?;
        mission.starting_positions[p] = Some(at.map(|n| n as u16));
    }
    // 0x4b5249: each kingdom's gold, then its stone, with half as much gold
    // again. The look R(0, 2) is drawn again at loading (0x4afeae).
    for list in &deposits {
        for &(class, at, ore) in list {
            let i = add(mission, &mut ids, lib, class, 0, 0, at)?;
            let ore = if class == class::RES_GOLD {
                ore * 150 / 100
            } else {
                ore
            };
            amount(&mut mission.objects[i], ore);
        }
    }
    // 0x4b1c80: three peasants on random green cells left by the castle,
    // with no test: under an 8 × 8 castle one can stand on its last row.
    for (p, shape) in shapes.iter().enumerate() {
        let mut list = corners(&green[p], shape.w, shape.h, [1, 1]);
        for _ in 0..3 {
            if list.is_empty() {
                break;
            }
            let [x, y] = list[rng.below(list.len())];
            clear(&mut green[p], shape.w, &mut list, [x, y], [1, 1]);
            let at = [shape.x + x, shape.y + y];
            ground.take(at, [1, 1]);
            add(
                mission,
                &mut ids,
                lib,
                class::PEON,
                p as u8 + 1,
                civs[p],
                at,
            )?;
        }
    }
    // 0x4b2e50: thirty trees on random black 3 × 3 blocks left by the
    // towers and deposits. Only the centre is tested (land, no tree,
    // cliff or object, a terrain with trees) and the tree grows there; the
    // 3 × 3 leaves the black and the corners inside it leave the list, so
    // trees can stand side by side. The species is `rand(0..=n)` as in the
    // forests.
    let records = tree_records(terrain);
    for (p, shape) in shapes.iter().enumerate() {
        let mut list = corners(&black[p], shape.w, shape.h, [3, 3]);
        let mut planted = 0;
        while planted < 30 && !list.is_empty() {
            let index = rng.below(list.len());
            let [x, y] = list[index];
            let (cx, cy) = (shape.x + x + 1, shape.y + y + 1);
            let c = ground.word(cx, cy);
            let (_, n, species) = tree_terrain(&records, c as u16);
            if !ground.land(c) || c & 0x108fc00 != 0 || n == 0 {
                // The original keeps the corner and draws it again: the
                // outcome is the same, but it never ends once none is left.
                list.swap_remove(index);
                continue;
            }
            clear(&mut black[p], shape.w, &mut list, [x, y], [3, 3]);
            ground.grid.cells[cy * side + cx] |= tree_bits(&records, species, rng.below(n + 1));
            planted += 1;
        }
    }
    // 0x4b54e9: half as many magic gateways as kingdoms, the margin being
    // the picture's width plus two.
    if opts.gateways {
        let probe = new_object(
            &mut BTreeSet::new(),
            1,
            class::MAGIC_GATEWAY,
            10,
            0,
            [0; 2],
            lib,
        )?;
        let size = class_cells(lib, &probe);
        let layer = lib.layer(&probe);
        for at in ground.scatter(
            &mut rng,
            class::MAGIC_GATEWAY,
            layer,
            size,
            players / 2,
            size[0] + 2,
        ) {
            add(mission, &mut ids, lib, class::MAGIC_GATEWAY, 10, 0, at)?;
        }
    }
    // 0x4b55ab: only in King of the Hill, a ruin per kingdom and one more,
    // each a random `gr_` picture that is not a shadow.
    if mission.game_rules.game_type == 1 {
        let ruins: Vec<_> = lib
            .images
            .section_names()
            .filter(|key| {
                let key = key.to_ascii_lowercase();
                key.starts_with("gr_") && !key.ends_with("_s")
            })
            .collect();
        let probe = new_object(&mut BTreeSet::new(), 1, class::RUIN, 10, 0, [0; 2], lib)?;
        let layer = lib.layer(&probe);
        for _ in 0..players + 1 {
            if ruins.is_empty() {
                break;
            }
            let name = ruins[rng.below(ruins.len())];
            let size = cells(lib, name);
            for at in ground.scatter(&mut rng, class::RUIN, layer, size, 1, size[0] + 2) {
                let i = add(mission, &mut ids, lib, class::RUIN, 10, 0, at)?;
                picture(&mut mission.objects[i], name, 88);
            }
        }
    }
    // 0x4b5764: every picture with a display name, in the catalogue's
    // order: R(0, limit) copies, the limit by map size for patches and 1,
    // 2 or 3 for ground objects. The record (0x497d40) belongs to nobody,
    // with the picture's own frame rate and draw priority 1 or 100.
    let limit = [4, 6, 8, 10, 16, 24, 32][SIDES.iter().position(|&n| n == side).unwrap_or(0)];
    let object_limit = match side {
        512 => 3,
        384 => 2,
        _ => 1,
    };
    for name in lib.images.section_names() {
        let key = name.to_ascii_lowercase();
        let (class, most) = if key.starts_with("gp_") {
            (class::GROUND_PATCH, limit)
        } else if key.starts_with("go_") {
            (class::GROUND_OBJECT, object_limit)
        } else {
            continue;
        };
        if lib
            .images
            .get(name, "displayname")
            .is_none_or(|n| n.trim().is_empty())
        {
            continue;
        }
        let copies = rng.range(0, most);
        if copies == 0 {
            continue;
        }
        let size = cells(lib, name);
        let mask = terrain_mask(lib, name, water, kinds);
        let object = class == class::GROUND_OBJECT;
        for at in ground.decorate(&mut rng, object, size, mask, copies, shapes) {
            let i = add(mission, &mut ids, lib, class, 0, 0, at)?;
            let o = &mut mission.objects[i];
            picture(o, name, 116);
            let number = |k: &str| {
                lib.images
                    .get(name, k)
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(0)
            };
            word(o, 0x58, number("fps") as u16 & 0xff);
            let animated = if number("rows") > 1 { u32::MAX } else { 0 };
            o.data[0x5a..0x5e].copy_from_slice(&animated.to_le_bytes());
            word(o, 0x5e, if object { 100 } else { 1 });
            o.data[0x64] = 1;
            word(o, 0x66, at[0] as u16 * 32);
            word(o, 0x68, at[1] as u16 * 32);
        }
    }
    // 0x4b5a18: fish patches, between a 361st and four 361sts of the
    // water, four cells from the edges on free 4 × 4 water.
    let fish = rng.range(water_vertices / 361, water_vertices * 4 / 361);
    let probe = new_object(&mut BTreeSet::new(), 1, class::FISHPATCH, 0, 0, [0; 2], lib)?;
    let (size, layer) = (class_cells(lib, &probe), lib.layer(&probe));
    for at in ground.scatter(&mut rng, class::FISHPATCH, layer, size, fish, 4) {
        let i = add(mission, &mut ids, lib, class::FISHPATCH, 0, 0, at)?;
        amount(&mut mission.objects[i], 1000);
    }
    // 0x4b5b14: one count for every animal class, R(1, 5) + side / 64,
    // another for every item class, R(1, 10) + side / 32, then the classes
    // in their table's order.
    if opts.animals || opts.items {
        let animals = rng.range(1, 5) + side / 64;
        let items = rng.range(1, 10) + side / 32;
        for class in [
            26,
            class::ITEM_PLUSPLUS,
            class::ITEM_MAGIC,
            class::ITEM_HEALTH,
            class::COW,
            class::BIRD,
            class::EAGLE,
        ] {
            let animal = !matches!(
                class,
                class::ITEM_PLUSPLUS | class::ITEM_MAGIC | class::ITEM_HEALTH
            );
            if !(if animal { opts.animals } else { opts.items }) {
                continue;
            }
            let owner = if animal { 10 } else { 0 };
            let probe = new_object(&mut BTreeSet::new(), 1, class, owner, 0, [0; 2], lib)?;
            let (size, layer) = (class_cells(lib, &probe), lib.layer(&probe));
            let count = if animal { animals } else { items };
            for at in ground.scatter(&mut rng, class, layer, size, count, 1) {
                let i = add(mission, &mut ids, lib, class, owner, 0, at)?;
                if !animal {
                    item(&mut mission.objects[i], &mut rng);
                }
            }
        }
    }
    Ok(())
}
impl Ground<'_> {
    /// 0x4b4460 for the other deposits: a random corner in the kingdom's
    /// sector, not touching its shape's rectangle, on land free of objects,
    /// trees and cliffs; up to 40 draws. No test of reach.
    fn deposit_site(
        &self,
        shape: &StartShape,
        size: [usize; 2],
        rng: &mut Rng,
    ) -> Option<[usize; 2]> {
        let [x0, y0, x1, y1] = shape.sector;
        (0..40).find_map(|_| {
            let x = rng.range(x0, (x1 + 1).saturating_sub(size[0] + 2));
            let y = rng.range(y0, (y1 + 1).saturating_sub(size[1] + 2));
            let shaped = x + size[0] >= shape.x
                && x < shape.x + shape.w
                && y + size[1] >= shape.y
                && y < shape.y + shape.h;
            (!shaped
                && (y..y + size[1]).all(|yy| {
                    (x..x + size[0]).all(|xx| {
                        let c = self.word(xx, yy);
                        self.land(c) && c & 0x101fc00 == 0
                    })
                }))
            .then_some([x, y])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_trees_follow_treeterrain_with_the_native_offset() {
        let ini = Ini::parse(
            b"[treename]\nTronco = 3\nArbusto = 2\nPino = 5\nOlmo = 21\n\
              [treeterrain]\nTronco = 1,2,3,4\nOlmo = 1,2,3,4\nPino = 1,2,3,4\nFaggio = 1\n",
        );
        let records = tree_records(&ini);
        // Shrubs (records 3-4) are not listed; terrain 5 allows nothing.
        assert_eq!(records[1].len(), 29);
        assert!(!records[1].contains(&3) && !records[1].contains(&4));
        assert!(records[0].is_empty() && records[5].is_empty());
        // The record lands in the 1-based field: the first pick is no tree,
        // the fourth shows the second shrub, a draw past the list image 31.
        assert_eq!(tree_bits(&records, 1, 0), 0);
        assert_eq!(tree_bits(&records, 1, 3) >> 11, 5);
        assert_eq!(tree_bits(&records, 1, 29) >> 11, 31);
        // The species comes from the terrain with more images.
        assert_eq!(tree_terrain(&records, 1 << 7 | 5 << 4), ([1, 5], 29, 1));
        assert_eq!(tree_terrain(&records, 2 << 7 | 3 << 4).2, 3);
    }
    #[test]
    fn random_type_uses_native_biome_parameters_instead_of_another_preset() {
        let native = include_bytes!("../tests/fixtures/random-native/default-plasma-512-741.bin");
        let actual = vertices_with_parameters(128, 741, [180, 0, 1, 1], false);
        for y in 0..=128 {
            for x in 0..=128 {
                let height = native[(y.min(127) * 4) * 512 + x.min(127) * 4];
                assert_eq!(actual[y * 129 + x], u8::from(height > 180));
            }
        }
        // The previous implementation picked a named type based on the seed.
        assert!((1..=5).all(|kind| actual != vertices(128, 741, kind, &[])));
    }
    #[test]
    fn team_sector_assignment_matches_original_machine_code() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/random-native/teams.json"))
                .unwrap();
        for f in fixtures.as_array().unwrap() {
            let teams: Vec<_> = f["teams"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t.as_u64().unwrap() as u8)
                .collect();
            let order = team_order(&teams, &mut Rng(f["seed"].as_u64().unwrap()));
            assert_eq!(serde_json::to_value(order).unwrap(), f["order"], "{f}");
        }
    }
    #[test]
    fn complete_cliff_placement_matches_original_machine_code() {
        let mut grid = MapGrid {
            name: "native cliff fixture".into(),
            side: 128,
            cells: vec![0x9f; 128 * 128],
        };
        native_cliffs(
            &mut grid,
            include_bytes!("../tests/fixtures/random-native/cliffs-height.bin"),
            &[],
            &mut Rng(7),
            4,
        );
        let native: Vec<u16> = include_bytes!("../tests/fixtures/random-native/cliffs-native.bin")
            .chunks_exact(2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
            .collect();
        let differences: Vec<_> = grid
            .cells
            .iter()
            .zip(&native)
            .enumerate()
            .filter(|&(_, (a, b))| a != b)
            .map(|(i, (a, b))| (i % 128, i / 128, *a, *b))
            .collect();
        assert!(
            differences.is_empty(),
            "{} different cells: {:?}",
            differences.len(),
            &differences[..differences.len().min(15)]
        );
    }
    #[test]
    fn terrain_layers_and_cliff_geometry_match_original_instructions() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/random-native/geometry.json"
        ))
        .unwrap();
        let bytevec = |v: &serde_json::Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect::<Vec<_>>()
        };
        let l = &fixtures["layers"];
        let mut vertices = bytevec(&l["vertices"]);
        layer_pass(
            &mut vertices,
            l["side"].as_u64().unwrap() as usize,
            &bytevec(&l["field"]),
            2,
            1,
            90,
            255,
        );
        assert_eq!(vertices, bytevec(&l["expected"]));
        for c in fixtures["corners"].as_array().unwrap() {
            let a = c["points"].as_array().unwrap();
            let mut side = c["side"].as_u64().unwrap() != 0;
            let pts = std::array::from_fn(|i| {
                [
                    a[2 * i].as_i64().unwrap() as i32,
                    a[2 * i + 1].as_i64().unwrap() as i32,
                ]
            });
            assert_eq!(
                cliff_turn(pts, c["previous"].as_u64().unwrap() as u8, &mut side),
                c["kind"].as_u64().unwrap() as u8,
                "{c}"
            );
            assert_eq!(side, c["next_side"].as_u64().unwrap() != 0, "{c}");
        }
        for c in fixtures["endpoints"].as_array().unwrap() {
            let a = c["points"].as_array().unwrap();
            let mut side = c["side"].as_u64().unwrap() != 0;
            let pts = std::array::from_fn(|i| {
                [
                    a[2 * i].as_i64().unwrap() as i32,
                    a[2 * i + 1].as_i64().unwrap() as i32,
                ]
            });
            let (kind, reverse) = cliff_cap(
                pts,
                c["start"].as_u64().unwrap() != 0,
                c["previous"].as_u64().unwrap() as u8,
                &mut side,
            );
            assert_eq!(
                kind as u32 | ((reverse as u32) << 16),
                c["code"].as_u64().unwrap() as u32,
                "{c}"
            );
            assert_eq!(side, c["next_side"].as_u64().unwrap() != 0, "{c}");
        }
        for p in fixtures["paths"].as_array().unwrap() {
            let mut rng = Rng(p["rng_seed"].as_u64().unwrap());
            let path = cliff_path(
                &bytevec(&p["field"]),
                p["side"].as_u64().unwrap() as usize,
                p["length"].as_u64().unwrap() as usize,
                &mut rng,
            );
            for (i, &[x, y]) in path.iter().enumerate() {
                assert_eq!(
                    (x, y),
                    (
                        p["x"][i].as_i64().unwrap() as i32,
                        p["y"][i].as_i64().unwrap() as i32
                    ),
                    "path seed {} at {i}",
                    p["seed"]
                );
            }
        }
    }
    #[test]
    fn recursive_plasma_matches_original_machine_instructions() {
        for (side, rough, div, seed, expected) in [
            (
                8,
                1,
                1,
                0,
                include_bytes!("../tests/fixtures/random-native/plasma-8-1-1-0.bin").as_slice(),
            ),
            (
                16,
                5,
                2,
                17,
                include_bytes!("../tests/fixtures/random-native/plasma-16-5-2-17.bin").as_slice(),
            ),
            (
                32,
                19,
                5,
                846,
                include_bytes!("../tests/fixtures/random-native/plasma-32-19-5-846.bin").as_slice(),
            ),
            (
                512,
                11,
                5,
                1,
                include_bytes!("../tests/fixtures/random-native/plasma-512-11-5-1.bin").as_slice(),
            ),
        ] {
            assert_eq!(
                native_plasma(side, seed, rough, div),
                expected,
                "{side}/{rough}/{div}/{seed}"
            );
        }
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/random-native/native-fixtures.json"
        ))
        .unwrap();
        for players in 2..=8 {
            assert_eq!(
                serde_json::to_value(sectors(256, players)).unwrap(),
                oracle["sectors"][players.to_string()],
                "players {players}"
            );
        }
    }
    #[test]
    fn native_corner_codes_are_rotated_and_uniform_tiles_are_unambiguous() {
        assert_eq!(encode([1, 0, 0, 0]), 0x1d);
        assert_eq!(encode([0, 1, 0, 0]), 0x1c);
        assert_eq!(encode([0, 0, 1, 0]), 0x1a);
        assert_eq!(encode([0, 0, 0, 1]), 0x16);
        assert_eq!(encode([2; 4]), 0x12f);
        for mask in 1..15 {
            let corners = std::array::from_fn(|i| u8::from(mask & (1 << (3 - i)) != 0));
            assert!((encode(corners) & 15) < 14);
        }
    }
    #[test]
    fn seeds_are_stable_and_all_seven_sizes_use_complete_native_cells() {
        for side in SIDES {
            let a = terrain_grid(side, 1).unwrap();
            let b = terrain_grid(side, 1).unwrap();
            assert_eq!(a.cells, b.cells);
            assert_eq!(a.cells.len(), side * side);
            assert_ne!(a.cells, terrain_grid(side, u64::MAX).unwrap().cells);
        }
        assert!(terrain_grid(999, 0).is_err());
    }
    #[test]
    #[ignore = "Requires installed original archives"]
    fn kingdom_records_take_the_heroes_left_by_the_screen() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        let setup = GameSetup {
            players: Some(3),
            heroes_taken: 7,
            kingdoms: vec![crate::world::KingdomChoice {
                record: 1,
                color: 2,
                team: 1,
                civilization: Some(2),
                name: Some("Mario".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let g = generate(&assets, 128, 5, Some(&setup)).unwrap();
        let heroes = crate::gameplay::hero_table(&crate::assets::legacy_text(
            &assets.get("DATA/HEROES.INI").unwrap(),
        ));
        let named = |civs: u16| -> Vec<&str> {
            heroes
                .iter()
                .filter(|h| h.1 == civs)
                .map(|h| h.0.as_str())
                .collect()
        };
        let (euro, arab) = (named(2), named(4));
        let k = &g.mission.kingdoms;
        // 0x4b4fb5: «Regno» and the index; the next free hero, any while
        // the civilization is random; team and colour i + 1.
        assert_eq!(
            (k[0].name.as_str(), k[0].leader.as_str()),
            ("Regno0", euro[7])
        );
        assert_eq!(
            (k[2].name.as_str(), k[2].leader.as_str()),
            ("Regno2", euro[8])
        );
        assert_eq!((k[2].team, k[2].color), (3, 3));
        // The Arab row took the first Arab hero; its name is the row's.
        assert_eq!((k[1].name.as_str(), k[1].leader.as_str()), ("Mario", ""));
        let arab0 = heroes.iter().position(|h| h.0 == arab[0]).unwrap();
        assert!(g.mission.heroes_taken[arab0]);
        // Seven names of the screen, nine records.
        assert_eq!(g.mission.heroes_taken.iter().filter(|&&t| t).count(), 16);
    }
    #[test]
    #[ignore = "Requires installed original archives"]
    fn options_seed_geography_and_item_attributes_reach_the_generated_world() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        let mut setup = GameSetup {
            players: Some(4),
            human: Some(2),
            random_generation: Some(Options {
                kind: 5,
                guarded: false,
                animals: false,
                items: false,
                cliffs: false,
                gateways: false,
                map_resources: 0,
            }),
            options: Some(GameRules {
                resource_level: 0,
                population: 50,
                ..Default::default()
            }),
            kingdoms: vec![
                crate::world::KingdomChoice {
                    record: 0,
                    color: 2,
                    team: 1,
                    civilization: Some(2),
                    ..Default::default()
                },
                crate::world::KingdomChoice {
                    record: 1,
                    color: 1,
                    team: 1,
                    civilization: Some(3),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let a = generate(&assets, 192, 741, Some(&setup)).unwrap();
        let b = generate(&assets, 192, 741, Some(&setup)).unwrap();
        assert_eq!(a.grid.cells, b.grid.cells);
        assert_eq!(
            serde_json::to_vec(&a.mission).unwrap(),
            serde_json::to_vec(&b.mission).unwrap()
        );
        assert_eq!(a.mission.player(), 3);
        assert_eq!(a.mission.kingdoms[0].civilization, 2);
        assert_eq!(a.mission.kingdoms[1].civilization, 3);
        assert_eq!(a.mission.kingdoms[0].color, 2);
        assert_eq!(a.mission.diplomacy[1] & 0x70, 0x40);
        assert_eq!(a.mission.diplomacy[2] & 0x70, 0x10);
        assert_eq!(a.mission.kingdoms[0].resources, [0, 0, 300, 300]);
        assert_eq!(a.mission.game_rules.population, 50);
        assert!(
            a.grid
                .cells
                .iter()
                .all(|&c| c >> 7 & 7 != 0 && c >> 4 & 7 != 0)
        );
        assert!(a.mission.objects.iter().all(|o| o.class != 40
            && !o.item()
            && (o.owner != 10
                || [class::RUIN, class::GROUND_OBJECT, class::GROUND_PATCH].contains(&o.class))));
        assert!(!a.grid.cells.iter().any(|c| c & 0x400 != 0));
        let o = setup.random_generation.as_mut().unwrap();
        o.map_resources = 2;
        o.guarded = true;
        o.animals = true;
        o.items = true;
        o.cliffs = true;
        o.gateways = true;
        setup.options.as_mut().unwrap().resource_level = 2;
        let b = generate(&assets, 192, 741, Some(&setup)).unwrap();
        assert_eq!(b.mission.kingdoms[0].resources, [1500; 4]);
        for class in [class::RES_GOLD, class::RES_STONE] {
            assert!(
                b.mission
                    .objects
                    .iter()
                    .filter(|o| o.class == class)
                    .count()
                    > a.mission
                        .objects
                        .iter()
                        .filter(|o| o.class == class)
                        .count()
            );
        }
        assert_eq!(
            b.mission.objects.iter().filter(|o| o.class == 40).count(),
            12
        );
        assert!(b.grid.cells.iter().any(|c| c & 0x400 != 0));
        assert!(b.mission.objects.iter().any(|o| o.class == 72));
        let lib = ObjectLibrary::load(&assets, "grass").unwrap();
        for o in b.mission.objects.iter().filter(|o| o.item()) {
            assert!(lib.icon_for(o).is_some());
            assert!(!lib.item_description(o).is_empty());
            match o.class {
                class::ITEM_MAGIC => {
                    assert!(o.spell_key().is_some());
                    assert!((1..=3).contains(&o.word(102)));
                    assert!((1..=5).contains(&o.word(100)));
                    assert_eq!(o.word(104), 0);
                }
                class::ITEM_PLUSPLUS => assert!(
                    (5..=15).contains(&(o.byte(22) as u16).max(o.byte(23) as u16).max(o.word(24)))
                ),
                class::ITEM_HEALTH => assert!([18, 22, 24, 28, 36].iter().any(|&i| o.word(i) > 0)),
                _ => panic!("unexpected random item"),
            }
        }
        assert!(
            generate(
                &assets,
                64,
                0,
                Some(&GameSetup {
                    players: Some(9),
                    ..Default::default()
                })
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "Requires installed original archives"]
    fn geography_examples_and_many_seeds_have_safe_starting_resources() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        for biome in ["grass", "snow", "desert", "summer"] {
            for kind in 1..=5 {
                let setup = GameSetup {
                    random_biome: Some(biome.into()),
                    players: Some(4),
                    random_generation: Some(Options {
                        kind,
                        cliffs: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let g = generate(&assets, 256, 846, Some(&setup)).unwrap();
                assert_eq!(
                    g.mission
                        .objects
                        .iter()
                        .filter(|o| o.class == class::CASTLE)
                        .count(),
                    4
                );
            }
        }
        for side in [64, 96] {
            for kind in 1..=5 {
                for seed in [0, u64::MAX] {
                    generate(
                        &assets,
                        side,
                        seed,
                        Some(&GameSetup {
                            players: Some(8),
                            random_generation: Some(Options {
                                kind,
                                map_resources: 2,
                                cliffs: true,
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                    )
                    .unwrap_or_else(|e| panic!("small map {side}, type {kind}, seed {seed}: {e}"));
                }
            }
        }
        for seed in 0..32 {
            for kind in [1, 2, 4] {
                let g = generate(
                    &assets,
                    128,
                    seed,
                    Some(&GameSetup {
                        players: Some(8),
                        random_generation: Some(Options {
                            kind,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                )
                .unwrap_or_else(|e| panic!("seed {seed}, type {kind}: {e}"));
                for castle in g
                    .mission
                    .objects
                    .iter()
                    .filter(|o| o.class == class::CASTLE)
                {
                    assert!(g.grid.cells.iter().enumerate().any(|(i, &c)| c >> 11 != 0
                        && (i % 128).abs_diff(castle.x as usize) < 24
                        && (i / 128).abs_diff(castle.y as usize) < 24));
                }
            }
        }
    }
    #[test]
    #[ignore = "Requires installed original archives"]
    fn native_scenery_catalog_and_gateways_are_distributed_across_the_map() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        for biome in ["grass", "summer", "snow", "desert"] {
            let setup = GameSetup {
                players: Some(4),
                random_biome: Some(biome.into()),
                random_generation: Some(Options {
                    kind: 1,
                    gateways: true,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let g = generate(&assets, 256, 846, Some(&setup)).unwrap();
            let lib = ObjectLibrary::load(&assets, biome).unwrap();
            let mut pictures = std::collections::BTreeMap::<&str, usize>::new();
            let mut sea = 0;
            for o in &g.mission.objects {
                if [class::GROUND_OBJECT, class::GROUND_PATCH].contains(&o.class) {
                    assert_eq!(o.owner, 0);
                    *pictures.entry(&o.custom_image).or_default() += 1;
                    sea +=
                        usize::from(lib.images.get(&o.custom_image, "terrains") == Some("water"));
                    assert_eq!(o.word(0), o.data.len() as u16);
                }
            }
            assert!(
                pictures.len() >= 20,
                "{biome}: {} picture types",
                pictures.len()
            );
            assert!(sea > 0, "{biome}: water scenery missing");
            for (picture, count) in pictures {
                assert!(count <= if picture.starts_with("gp_") { 16 } else { 1 });
            }
            // Ruins only in King of the Hill.
            assert!(!g.mission.objects.iter().any(|o| o.class == class::RUIN));
            let gates: Vec<_> = g.mission.objects.iter().filter(|o| o.class == 72).collect();
            assert_eq!(gates.len(), 2);
            let castles: Vec<_> = g
                .mission
                .objects
                .iter()
                .filter(|o| o.class == class::CASTLE)
                .collect();
            assert!(
                gates.iter().any(|gate| castles.iter().all(|c| gate
                    .x
                    .abs_diff(c.x)
                    .max(gate.y.abs_diff(c.y))
                    > 30)),
                "{biome}: gates still follow castles"
            );
            let disabled = generate(
                &assets,
                128,
                846,
                Some(&GameSetup {
                    players: Some(4),
                    random_generation: Some(Options {
                        gateways: false,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            )
            .unwrap();
            assert!(!disabled.mission.objects.iter().any(|o| o.class == 72));
        }
    }
    #[test]
    #[ignore = "Requires installed original archives"]
    fn generated_worlds_cover_every_biome_type_size_and_player_count() {
        let assets = GameAssets::open(std::path::Path::new("..")).unwrap();
        let shapes = starting_shapes(&assets, 128, 8, 1, &[0; 8]).unwrap();
        assert!(shapes.iter().all(|shape| shape.w == 30 && shape.h == 30));
        let mut cases = Vec::new();
        for biome in ["grass", "summer", "snow", "desert"] {
            for kind in 0..=5 {
                cases.push((128, 8, biome, kind));
            }
        }
        for side in SIDES {
            for players in 2..=8 {
                cases.push((side, players, "grass", 3));
            }
        }
        for (side, players, biome, kind) in cases {
            for seed in [1, 17] {
                let setup = GameSetup {
                    players: Some(players),
                    random_biome: Some(biome.into()),
                    random_generation: Some(Options {
                        kind,
                        cliffs: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let generated = generate(&assets, side, seed, Some(&setup))
                    .unwrap_or_else(|e| panic!("{side}/{players}/{biome}/{kind}/{seed}: {e}"));
                let m = &generated.mission;
                let g = &generated.grid;
                let lib = ObjectLibrary::load(&assets, biome).unwrap();
                assert_eq!(m.active_kingdoms().count(), players);
                assert_eq!(
                    m.objects
                        .iter()
                        .filter(|o| o.class == class::CASTLE)
                        .count(),
                    players
                );
                assert_eq!(
                    m.objects.iter().filter(|o| o.class == class::PEON).count(),
                    players * 3
                );
                assert!(m.objects.iter().filter(|o| o.class == 40).count() <= players * 3);
                assert!(!m.objects.iter().any(|o| o.class == class::COW));
                for class in [26, class::BIRD, class::EAGLE] {
                    assert!(m.objects.iter().filter(|o| o.class == class).count() <= 5 + side / 64);
                }
                let mut shapes =
                    starting_shapes(&assets, side, players, seed, &vec![0; players]).unwrap();
                let castle = cells(&lib, "e_castle").map(|c| c + 2);
                for shape in &mut shapes {
                    castle_room(shape, castle);
                }
                // Every pixel of a block of the shape is of one colour.
                let all = |shape: &StartShape, at: [usize; 2], size: usize, colour: u8| {
                    (at[1]..at[1] + size)
                        .all(|y| (at[0]..at[0] + size).all(|x| shape.pixel(x, y) == colour))
                };
                let label = format!("{side}/{players}/{biome}/{kind}/{seed}");
                let mut ids = BTreeSet::new();
                for o in &m.objects {
                    assert!(ids.insert(o.id));
                    let at = [o.x as usize, o.y as usize];
                    let c = g.cells[at[1] * side + at[0]];
                    let land = c >> 7 & 7 != 0 && c >> 4 & 7 != 0;
                    match o.class {
                        class::CASTLE => {
                            let shape = &shapes[o.owner as usize - 1];
                            assert!(
                                all(shape, [at[0] - 1, at[1] - 1], 9, GREEN),
                                "{label}: castle"
                            );
                        }
                        class::PEON => {
                            assert_eq!(shapes[o.owner as usize - 1].pixel(at[0], at[1]), GREEN);
                        }
                        40 => {
                            let shape = &shapes[o.owner as usize - 1];
                            assert!(
                                all(shape, [at[0] - 1, at[1] - 1], 5, BLACK),
                                "{label}: tower"
                            );
                            let x = (at[0] - 1 - shape.x) as i32;
                            let y = (at[1] - 1 - shape.y) as i32;
                            let (x0, y0) = ((shape.w as i32 - 10) / 2, (shape.h as i32 - 10) / 2);
                            assert!(
                                (x0..=x0 + 9).contains(&x) || (y0..=y0 + 9).contains(&y),
                                "{label}: tower off the axes"
                            );
                        }
                        class::RES_GOLD | class::RES_STONE => {
                            // The first deposit in the black of its shape,
                            // the others away from it, on free land: from
                            // the kingdom's sector, the map's edge included
                            // (0x4b4950).
                            let first = (at[0] > 0 && at[1] > 0)
                                .then(|| {
                                    shapes
                                        .iter()
                                        .find(|s| all(s, [at[0] - 1, at[1] - 1], 6, BLACK))
                                })
                                .flatten();
                            assert!(
                                first.is_some()
                                    || !shapes.iter().any(|s| at[0] + 4 >= s.x
                                        && at[0] < s.x + s.w
                                        && at[1] + 4 >= s.y
                                        && at[1] < s.y + s.h)
                                        && land
                                        && c & 0xfc00 == 0,
                                "{label}: deposit"
                            );
                        }
                        class::FISHPATCH => {
                            assert_eq!(o.dword(56), 1000);
                            assert_eq!(o.data.len(), 60);
                            assert!(c >> 7 & 7 == 0 && c >> 4 & 7 == 0, "{label}: fish ashore");
                        }
                        26 | class::ITEM_PLUSPLUS | class::ITEM_MAGIC | class::ITEM_HEALTH => {
                            assert!(land && c & 0xfc00 == 0, "{label}: {} at sea", o.class);
                        }
                        class::GROUND_OBJECT => {
                            let [w, h] = cells(&lib, &o.custom_image);
                            assert!(!shapes.iter().any(|s| at[0] + w >= s.x
                                && at[0] < s.x + s.w
                                && at[1] + h >= s.y
                                && at[1] < s.y + s.h));
                            assert!((at[1]..at[1] + h).all(|y| {
                                (at[0]..at[0] + w).all(|x| g.cells[y * side + x] & 0xf800 == 0)
                            }));
                        }
                        _ => {}
                    }
                }
                let mut terrain = crate::terrain::Terrain::load(&assets, biome).unwrap();
                terrain.prepare(g).unwrap();
                // All frames, shadows and item pictures must resolve in real archives.
                if seed == 1 && side == 128 {
                    let mut scene = lib.build(&assets, m, side).unwrap();
                    crate::world::add_cliffs(&assets, g, biome, &mut scene).unwrap();
                    crate::world::add_trees(&assets, g, biome, &mut scene).unwrap();
                    assert!(scene.missing.is_empty(), "{:?}", scene.missing);
                }
            }
        }
    }
}
