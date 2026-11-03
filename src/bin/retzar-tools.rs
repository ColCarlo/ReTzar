//! Development tools for the original formats. Read-only on the installation;
//! kept out of the game executable.
use retzar::{
    Result, archive::Archive, assets::GameAssets, bitmap::Bitmap, hpfs, map::MapGrid,
    paths::Locations, scenario::ScenarioCatalog, world::MissionWorld,
};
use std::path::Path;

const HELP: &str = "retzar-tools — strumenti di sviluppo per i formati originali

  --list                        scenari con identificativo e titolo
  --asset NOME                  una risorsa degli archivi
  --assets TESTO                nomi delle risorse che contengono TESTO
  --dump-text CARTELLA          tutti gli INI e TXT degli archivi in UTF-8
  --extract NOME --to FILE      copia una risorsa così com'è
  --sprite-bmp NOME --to FILE [--slot N]   foglio RLE decodificato in BMP
  --hd-export CARTELLA          i fogli degli oggetti in PNG, con le maschere dei colori dei regni,
                                da ridisegnare per un pacchetto HD (docs/hd-packs.md)
  --hpfs FILE [--nested N] [--entry E [--to FILE]]   voci di un contenitore
  --mission ID                  oggetti, proprietari e regni di uno scenario
  --records CLASSE [--scenario ID]   dati grezzi degli oggetti di una classe
  --visuals                     immagini e animazioni usate dalle missioni
  --inspect                     archivi, immagini e contenitori delle mappe (JSON)
  --check-audio                 WAV e MIDI originali, senza riprodurli (JSON)
  --audit-scripts               copioni di tutte le missioni: azioni, nomi, oggetti, file e mappe
  --script ID                   il copione di uno scenario, una riga per elemento
  --crea-pacchetto FILE [--americano CARTELLA] [--spagnolo CARTELLA]   il pacchetto
                                della copia portatile (ReTzar.dat) dall'installazione,
                                dal CD americano e dall'edizione spagnola di FX,
                                controllato file per file
  --verifica-pacchetto FILE [--americano CARTELLA] [--spagnolo CARTELLA]   lo stesso
                                controllo da solo

  --source CARTELLA  installazione originale (oppure TZAR_DATA_ROOT)";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return;
    }
    if let Err(error) = Locations::resolve(&args).and_then(|l| run(&l.source, &args)) {
        eprintln!("Errore: {error}");
        std::process::exit(1);
    }
}

fn value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2).find(|a| a[0] == key).map(|a| a[1].as_str())
}

fn run(source: &Path, args: &[String]) -> Result<()> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let american = || {
        value(args, "--americano")
            .map(std::path::PathBuf::from)
            .or_else(|| Some(source.join("USA/CD/SETUP/Tzar")).filter(|p| p.is_dir()))
    };
    let spanish = || value(args, "--spagnolo").map(Path::new);
    if let Some(out) = value(args, "--crea-pacchetto") {
        let started = std::time::Instant::now();
        let work = std::env::temp_dir().join(format!("retzar-filmati-{}", std::process::id()));
        let s = retzar::packing::build(
            source,
            american().as_deref(),
            spanish(),
            Path::new(out),
            &work,
            &mut |m| println!("{m}"),
        )?;
        let mb = |b: u64| b as f64 / 1e6;
        println!(
            "{out}: {} file, {} risorse, {:.1} MB in {:.1} MB ({:.0} s)\n\
             risparmiati: deflate {:.1} MB, PNG {:.1} MB, FLAC {:.1} MB",
            s.files,
            s.resources,
            mb(s.original),
            mb(s.packed),
            started.elapsed().as_secs_f64(),
            mb(s.saved[0]),
            mb(s.saved[1]),
            mb(s.saved[2])
        );
        return Ok(());
    }
    if let Some(file) = value(args, "--verifica-pacchetto") {
        retzar::packing::verify(Path::new(file), source, american().as_deref(), spanish())?;
        println!("{file}: ogni file uguale all'originale, filmati leggibili");
        return Ok(());
    }
    if has("--inspect") {
        return inspect(source);
    }
    if has("--check-audio") {
        println!(
            "{}",
            serde_json::to_string_pretty(&retzar::audio::audit(source)?)?
        );
        return Ok(());
    }
    if has("--list") {
        let catalog = ScenarioCatalog::load(source)?;
        for m in &catalog.missions {
            println!("{:<45} {}×{}  {}", m.id, m.side, m.side, m.title);
        }
        for c in &catalog.campaigns {
            println!("\n[{}]", c.title);
            for m in &c.missions {
                println!("{:<45} {}×{}  {}", m.id, m.side, m.side, m.title);
            }
        }
        return Ok(());
    }
    if let Some(pattern) = value(args, "--assets") {
        let pattern = pattern.to_ascii_uppercase();
        for name in GameAssets::open(source)?.names() {
            if name.to_ascii_uppercase().contains(&pattern) {
                println!("{name}");
            }
        }
        return Ok(());
    }
    if let Some(dir) = value(args, "--dump-text") {
        let assets = GameAssets::open(source)?;
        let mut count = 0;
        for name in assets.names() {
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".ini") || lower.ends_with(".txt")) {
                continue;
            }
            let target = Path::new(dir).join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(
                &target,
                retzar::assets::legacy_text(&assets.get(name)?).as_bytes(),
            )?;
            count += 1;
        }
        println!("{count} file di testo → {dir}");
        return Ok(());
    }
    if let Some(name) = value(args, "--asset") {
        let bytes = GameAssets::open(source)?.get(name)?;
        println!("{name} — {} bytes", bytes.len());
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".bmp") {
            let image = Bitmap::decode(&bytes, false)?;
            println!("BMP {} × {}", image.width, image.height);
        } else if lower.ends_with(".ini") || lower.ends_with(".txt") {
            println!("{}", retzar::assets::legacy_text(&bytes));
        } else {
            println!("{}", hex(&bytes[..256.min(bytes.len())]));
        }
        return Ok(());
    }
    if let Some(name) = value(args, "--sprite-bmp") {
        let target = value(args, "--to").ok_or("Usa --sprite-bmp NOME --to FILE.bmp")?;
        let data = GameAssets::open(source)?.get(name)?;
        let slot = value(args, "--slot").and_then(|s| s.parse().ok());
        let image = retzar::sprite::SpriteSheet::decode_for(&data, slot)?.image;
        std::fs::write(target, image.to_bmp([255, 0, 255]))?;
        println!("{name}: {} × {} → {target}", image.width, image.height);
        return Ok(());
    }
    if let Some(out) = value(args, "--hd-export") {
        return hd_export(source, Path::new(out));
    }
    if let Some(name) = value(args, "--extract") {
        let target = value(args, "--to").ok_or("Usa --extract NOME --to FILE")?;
        let bytes = GameAssets::open(source)?.get(name)?;
        std::fs::write(target, &bytes)?;
        println!("{name} → {target} ({} bytes)", bytes.len());
        return Ok(());
    }
    if let Some(file) = value(args, "--hpfs") {
        let mut c = hpfs::Container::open(Path::new(file))?;
        if let Some(n) = value(args, "--nested") {
            c = c.open_nested(n)?;
        }
        match value(args, "--entry") {
            Some(e) => {
                let bytes = c.get(e)?;
                if let Some(target) = value(args, "--to") {
                    std::fs::write(target, &bytes)?;
                    println!("{e} → {target} ({} bytes)", bytes.len());
                } else {
                    println!("{e} — {} bytes\n{}", bytes.len(), hex(&bytes));
                }
            }
            None => {
                for e in &c.entries {
                    println!("{:>10}  {}", e.size, e.name);
                }
            }
        }
        return Ok(());
    }
    if let Some(id) = value(args, "--script") {
        return script_dump(source, id);
    }
    if has("--audit-scripts") {
        return audit_scripts(source);
    }
    if let Some(id) = value(args, "--mission") {
        return mission_report(source, id);
    }
    if has("--visuals") {
        return visuals_survey(source);
    }
    if let Some(class) = value(args, "--records") {
        return records_dump(source, value(args, "--scenario"), class);
    }
    Err(format!("Opzione sconosciuta. Usa --help.\n{}", args.join(" ")).into())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, row)| {
            format!(
                "{:06x}  {:<48} {}",
                i * 16,
                row.iter().map(|b| format!("{b:02x} ")).collect::<String>(),
                row.iter()
                    .map(|&b| if (32..127).contains(&b) {
                        b as char
                    } else {
                        '.'
                    })
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn inspect(source: &Path) -> Result<()> {
    let mut packs = Vec::new();
    for p in std::fs::read_dir(source.join("Packs"))? {
        let p = p?.path();
        if !p.extension().is_some_and(|s| s.eq_ignore_ascii_case("wdt")) {
            continue;
        }
        let archive = Archive::open(&p)?;
        let mut bitmaps = 0;
        for entry in &archive.entries {
            if entry.name.to_ascii_lowercase().ends_with(".bmp") {
                Bitmap::decode(&archive.read(entry)?, false)
                    .map_err(|e| format!("{} / {}: {e}", p.display(), entry.name))?;
                bitmaps += 1;
            }
        }
        packs.push(serde_json::json!({"file":p.file_name().unwrap().to_string_lossy(),"entries":archive.entries,"decoded_bitmaps":bitmaps}));
    }
    let mut maps = Vec::new();
    for folder in ["Maps", "Campaign"] {
        for p in std::fs::read_dir(source.join(folder))? {
            let p = p?.path();
            if !p.is_file()
                || !p
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("wmp") || s.eq_ignore_ascii_case("wcm"))
            {
                continue;
            }
            let container = hpfs::Container::open(&p)?;
            for entry in &container.entries {
                container
                    .read(entry.inode)
                    .map_err(|e| format!("{} / {}: {e}", p.display(), entry.name))?;
            }
            let scenarios =
                retzar::map::scan(&container, &p.file_name().unwrap().to_string_lossy())?;
            maps.push(serde_json::json!({"file":p.file_name().unwrap().to_string_lossy(),"container":container.header,"entries":container.entries,"scenarios":scenarios}));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"packs":packs,"maps":maps}))?
    );
    Ok(())
}

/// Research aid: what a scenario contains, by class and owner, plus the raw
/// kingdom records whose remaining fields are not decoded yet.
fn mission_report(source: &Path, id: &str) -> Result<()> {
    let catalog = ScenarioCatalog::load(source)?;
    let (campaign, mission) = catalog
        .locate(&id.to_ascii_lowercase())
        .ok_or_else(|| format!("Scenario sconosciuto: {id}. Usa --list."))?;
    let m = match campaign {
        Some(c) => &catalog.campaigns[c].missions[mission],
        None => &catalog.missions[mission],
    };
    let c = m.container()?;
    let grid = MapGrid::read(&c, &m.header)?;
    let world = MissionWorld::read(&c, &grid)?;
    let mut classes = std::collections::BTreeMap::<(String, u8), usize>::new();
    for o in &world.objects {
        *classes
            .entry((o.class_name().to_owned(), o.owner))
            .or_default() += 1;
    }
    let kingdoms = c
        .get(&format!("{}.kingdominfo", grid.name))
        .unwrap_or_default();
    let report = serde_json::json!({
        "title": m.title,
        "side": grid.side,
        "records": world.records,
        "hidden": world.hidden,
        "script": {
            "events": world.script.events.len(),
            "actions": world.script.actions.len(),
            "rules": world.script.rules.len(),
        },
        "story": world.script.story().map(|s| s.chars().take(120).collect::<String>()),
        "player": world.player(),
        "home": world.home(),
        "objects_by_class_and_owner": classes
            .iter()
            .map(|((class, owner), n)| serde_json::json!({"class": class, "owner": owner, "count": n}))
            .collect::<Vec<_>>(),
        "kingdoms": world.kingdoms,
        "kingdom_records_hex": kingdoms.chunks(152).map(hex).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// Raw payloads of one class, in one scenario or in all of them.
fn records_dump(source: &Path, scenario: Option<&str>, class: &str) -> Result<()> {
    let catalog = ScenarioCatalog::load(source)?;
    let missions = catalog
        .missions
        .iter()
        .chain(catalog.campaigns.iter().flat_map(|c| &c.missions))
        .filter(|m| scenario.is_none_or(|id| m.id.eq_ignore_ascii_case(id)));
    for m in missions {
        let c = m.container()?;
        let grid = MapGrid::read(&c, &m.header)?;
        let world = MissionWorld::read(&c, &grid)?;
        for o in world
            .objects
            .iter()
            .filter(|o| o.class_name().eq_ignore_ascii_case(class))
        {
            println!(
                "{} #{} ({}, {}) owner {} variant {} hp {}/{} flags {:x} image '{}'\n{}\n",
                m.id,
                o.id,
                o.x,
                o.y,
                o.owner,
                o.variant,
                o.health,
                o.max_health,
                o.flags,
                o.custom_image,
                hex(&o.data)
            );
        }
    }
    Ok(())
}

/// Every picture drawn by the 26 missions, with the image keys that decide
/// its animation: the checklist for comparing the remake with the original.
fn visuals_survey(source: &Path) -> Result<()> {
    use retzar::world::{CLASSES, FLAG_INACTIVE, ObjectLibrary, class};
    use std::collections::{BTreeMap, BTreeSet};
    let assets = GameAssets::open(source)?;
    let catalog = ScenarioCatalog::load(source)?;
    let mut libraries = BTreeMap::new();
    // (class, section, overlay) → (objects, missions)
    let mut seen = BTreeMap::<(String, String, String), (usize, BTreeSet<String>)>::new();
    let missions = catalog
        .missions
        .iter()
        .chain(catalog.campaigns.iter().flat_map(|c| &c.missions));
    for m in missions {
        let c = m.container()?;
        let grid = MapGrid::read(&c, &m.header)?;
        let world = MissionWorld::read(&c, &grid)?;
        let biome = retzar::terrain::biome_of(&c.get(&m.header)?)?;
        if !libraries.contains_key(&biome) {
            libraries.insert(biome.clone(), ObjectLibrary::load(&assets, &biome)?);
        }
        let lib = &libraries[&biome];
        for o in &world.objects {
            if o.flags & FLAG_INACTIVE != 0 || class::is_marker(o.class) {
                continue;
            }
            let section = lib.image_for(o).unwrap_or_else(|| "?".into());
            let overlay = lib.property(o, "image_c").unwrap_or_default().to_owned();
            let entry = seen
                .entry((format!("{}:{biome}", CLASSES[o.class]), section, overlay))
                .or_default();
            entry.0 += 1;
            entry.1.insert(m.title.clone());
        }
    }
    let describe = |lib: &ObjectLibrary, section: &str| {
        if section.is_empty() {
            return String::new();
        }
        let key = |k: &str| lib.images.get(section, k).unwrap_or("-").to_owned();
        format!(
            "{} r{} c{} fps{} t{} d[{}] mv{} sr{}",
            section,
            key("rows"),
            key("cols"),
            key("fps"),
            key("anim_time"),
            key("delay"),
            key("move_len"),
            key("startrow"),
        )
    };
    for ((class, section, overlay), (count, titles)) in &seen {
        let biome = class.split(':').nth(1).unwrap_or("grass");
        let lib = &libraries[biome];
        println!(
            "{class:<26} {count:>5} in {:>2} | {} | {}",
            titles.len(),
            describe(lib, section),
            describe(lib, overlay)
        );
    }
    Ok(())
}

/// Every mission's script against what it names (`script::audit`).
fn audit_scripts(source: &Path) -> Result<()> {
    let mut total = 0;
    for r in retzar::script::audit::audit_catalog(source)? {
        let [rules, actions, events] = r.counts;
        println!(
            "{} ({}): {rules} regole, {actions} azioni, {events} eventi, {} problemi",
            r.title,
            r.id,
            r.found.len()
        );
        for f in &r.found {
            println!("  {f}");
        }
        total += r.found.len();
    }
    println!("problemi: {total}");
    Ok(())
}

/// A scenario's script as JSON: events, actions, rules, defeat texts.
fn script_dump(source: &Path, id: &str) -> Result<()> {
    let catalog = ScenarioCatalog::load(source)?;
    let (campaign, mission) = catalog
        .locate(&id.to_ascii_lowercase())
        .ok_or_else(|| format!("Scenario sconosciuto: {id}. Usa --list."))?;
    let m = match campaign {
        Some(c) => &catalog.campaigns[c].missions[mission],
        None => &catalog.missions[mission],
    };
    let c = m.container()?;
    let grid = MapGrid::read(&c, &m.header)?;
    let world = MissionWorld::read(&c, &grid)?;
    let s = &world.script;
    println!("EVENTI");
    for e in &s.events {
        println!("  {}: {:?}", e.name, e.value);
    }
    println!("AZIONI");
    for a in &s.actions {
        println!("  {}: {:?}", a.name, a.value);
    }
    println!("REGOLE");
    for r in &s.rules {
        println!(
            "  {} [{}] -> {}",
            r.name,
            r.condition.replace(['\r', '\n'], " "),
            r.actions.join(", ")
        );
    }
    println!("CONVERSAZIONI");
    for c in &world.conversations {
        println!("  {}", c.name);
        for p in &c.phrases {
            let text: String = p.text.chars().take(70).collect();
            println!(
                "    #{}{} ({}) se [{}] fa [{}] -> {:?}: {text}",
                p.index,
                if p.start { "*" } else { "" },
                p.speaker,
                p.condition,
                p.action,
                p.next
            );
        }
    }
    Ok(())
}

/// The object sheets as the sprite builder decodes them, to
/// `<out>/<ARCHIVE PATH>.png`: the originals of an HD pack
/// ([`retzar::world::hd_picture`]). A sheet with kingdom shades is written as
/// stored, its kingdom parts in light greys, with the mask of those parts in
/// `<out>/player/<ARCHIVE PATH>.png` ([`retzar::world::hd_kingdom_picture`]).
/// Shadows and the ground tiles are left out.
fn hd_export(source: &Path, out: &Path) -> Result<()> {
    use retzar::sprite::SpriteSheet;
    const SKIP: [&str; 4] = [
        "IMAGES/SCREENS/",
        "IMAGES/CURSORS/",
        "IMAGES/ICONS/",
        "IMAGES/ORDERS/",
    ];
    let assets = GameAssets::open(source)?;
    let (mut written, mut kingdom, mut pixels) = (0, 0, 0u64);
    for name in assets.names() {
        let upper = name.to_ascii_uppercase();
        let file = upper.rsplit('/').next().unwrap_or(&upper);
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        // The ground atlases (`NN.BMP`, but `N8.BMP` are the cliffs), opaque,
        // their transition markers made transparent so the upscaler never
        // smears them (the terrain takes the markers from the originals).
        let tile = upper.starts_with("IMAGES/TERRAIN/")
            && stem.len() == 2
            && stem.bytes().all(|b| b.is_ascii_digit())
            && !stem.ends_with('8');
        if !upper.starts_with("IMAGES/")
            || SKIP.iter().any(|p| upper.starts_with(p))
            || stem == "LIGHT"
            || stem.ends_with("_S")
            || stem.ends_with(" S")
            || stem.contains("SHADOW")
        {
            continue;
        }
        let data = assets.get(name)?;
        // As stored, and in two kingdoms' colours: the pixels that change
        // are the kingdom parts.
        let decode = |slot: Option<usize>| -> Option<Bitmap> {
            if tile {
                let mut b = Bitmap::decode_for(&data, false, None).ok()?;
                for p in b.rgba.chunks_exact_mut(4) {
                    if p[..3] == [255, 0, 255] || p[..3] == [0, 255, 0] {
                        p[3] = 0;
                    }
                }
                Some(b)
            } else if upper.ends_with(".RLE") {
                SpriteSheet::decode_for(&data, slot).ok().map(|s| s.image)
            } else if upper.ends_with(".BMP") && data.starts_with(b"BM") {
                Bitmap::decode_for(&data, true, slot).ok()
            } else {
                None
            }
        };
        let indexed = !tile
            && if upper.ends_with(".RLE") {
                SpriteSheet::is_indexed(&data)
            } else {
                Bitmap::is_indexed(&data)
            };
        let Some(image) = decode(None) else {
            continue;
        };
        // Run-only shadow sheets are the engine's 50% darkening, not art.
        if image
            .rgba
            .chunks_exact(4)
            .all(|p| p[3] == 0 || p == retzar::sprite::SHADOW)
        {
            continue;
        }
        let mut player = None;
        if indexed && let (Some(a), Some(b)) = (decode(Some(0)), decode(Some(3))) {
            let mut mask = Bitmap {
                width: image.width,
                height: image.height,
                rgba: vec![0; image.rgba.len()],
            };
            let mut any = false;
            for (i, m) in mask.rgba.chunks_exact_mut(4).enumerate() {
                let p = i * 4..i * 4 + 3;
                let on = image.rgba[i * 4 + 3] != 0
                    && (image.rgba[p.clone()] != a.rgba[p.clone()]
                        || image.rgba[p.clone()] != b.rgba[p]);
                any |= on;
                let v = if on { 255 } else { 0 };
                m.copy_from_slice(&[v, v, v, 255]);
            }
            if any {
                player = Some(mask);
            }
        }
        let write = |path: std::path::PathBuf, b: &Bitmap| -> Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, b.to_png()?)?;
            Ok(())
        };
        write(out.join(format!("{upper}.png")), &image)?;
        if let Some(mask) = &player {
            write(out.join("player").join(format!("{upper}.png")), mask)?;
            kingdom += 1;
        }
        written += 1;
        pixels += image.width as u64 * image.height as u64;
    }
    println!(
        "{written} fogli ({kingdom} con i colori dei regni), {pixels} pixel → {}",
        out.display()
    );
    Ok(())
}
