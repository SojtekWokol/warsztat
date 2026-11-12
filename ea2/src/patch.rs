//! The whole Polish patch, from clean originals: the dialogue/font/subtitle bundles + catalog, then
//! the main data.unity3d (text tables, languages, fonts, Yarn projects).
use crate::fonts::FontPatcher;
use crate::originals::Originals;
use crate::pack::{script_class, text_of, Pack};
use crate::store::Store;
use crate::{texts, yarn};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unity::{TypeRegistry, Value};

pub struct Game {
    pub content: PathBuf,
}

impl Game {
    pub fn data(&self) -> PathBuf {
        self.content.join("ProjectFLEA_Data")
    }
    pub fn main_file(&self) -> PathBuf {
        self.data().join("data.unity3d")
    }
    pub fn aa(&self) -> PathBuf {
        self.data().join("StreamingAssets").join("aa")
    }
    pub fn bundles(&self) -> PathBuf {
        self.aa().join("StandaloneWindows64")
    }
    pub fn catalog(&self) -> PathBuf {
        self.aa().join("catalog.bin")
    }
    /// Steam builds keep the game data as loose files (no data.unity3d).
    pub fn is_loose(&self) -> bool {
        !self.main_file().exists() && self.data().join("globalgamemanagers").exists()
    }
    pub fn is_valid(&self) -> bool {
        (self.main_file().exists() || self.is_loose()) && self.catalog().exists()
    }
    pub fn bundle(&self, prefix: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.bundles())
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with(prefix))).collect())
            .unwrap_or_default();
        v.sort();
        v
    }
    /// Where a backed-up file belongs in the game.
    pub fn place_of(&self, name: &str) -> Option<PathBuf> {
        match name {
            "data.unity3d" => Some(self.main_file()),
            "catalog.bin" => Some(self.catalog()),
            n if n.ends_with(".bundle") => Some(self.bundles().join(n)),
            n if self.data().join(n).exists() => Some(self.data().join(n)),
            _ => None,
        }
    }
}

pub fn game_running() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ProjectFLEA.exe"])
        .creation_flags(0x0800_0000) // no console window flashing up
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ProjectFLEA.exe"))
        .unwrap_or(false)
}

/// Distinctive parts of our fonts' asset names, to find bundles with font copies.
const FONT_TOKENS: &[&str] = &["ChunkFive", "Urbanist", "knewave", "Jersey10", "Kalam-Bold", "Caveat-Var"];

const FONT_BUNDLES: &[&str] = &[
    "duplicateassetisolation_",
    "inventoryitemruntimedata_assets_mus_fertilizer_run",
    "inventoryitemruntimedata_assets_mus_guitarstrumcha",
];

pub struct Options<'a> {
    pub game: &'a Game,
    pub store: &'a Store,
    pub fonts_dir: &'a Path,
    /// font files in memory instead of fonts_dir (the installer)
    pub fonts: Option<&'a HashMap<String, Vec<u8>>>,
    pub originals: &'a mut Originals,
    /// write results here instead of into the game (for comparing)
    pub out_dir: Option<&'a Path>,
    /// compatibility check: do everything in memory, write nothing (no backups either), report
    pub dry_run: bool,
}

fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = target.with_extension("pl_tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, target).with_context(|| format!("cannot replace {}", target.display()))?;
    Ok(())
}

/// The 8 bytes that follow the size in every bundle record of this catalog (two ids that differ between
/// game builds, e.g. 0x8711 0x8752 on Xbox, 0x733d 0x737e in the Steam demo): the pair seen most often
/// after the sizes of the game's bundles.
fn catalog_signature(cat: &[u8], bundles_dir: &Path) -> Result<[u8; 8]> {
    let mut counts: HashMap<[u8; 8], usize> = HashMap::new();
    for e in std::fs::read_dir(bundles_dir)?.filter_map(|e| e.ok()) {
        let size = (e.metadata()?.len() as u32).to_le_bytes();
        for i in cat.windows(4).enumerate().filter(|(_, w)| *w == size).map(|(i, _)| i) {
            if let Some(tail) = cat.get(i + 4..i + 12) {
                *counts.entry(tail.try_into().unwrap()).or_insert(0) += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| *n >= 3).map(|(k, _)| k).ok_or_else(|| anyhow!("unknown catalog format"))
}

/// In catalog bytes, set the CRC and size of one bundle. Record: crc u32, size u32, then the catalog's
/// signature; found by the bundle's original size (several same-size bundles: nearest after its name).
fn update_catalog(cat: &mut [u8], sig: &[u8; 8], old_size: u64, name: &str, crc: u32, new_size: u64) -> Result<()> {
    let mut pat = (old_size as u32).to_le_bytes().to_vec();
    pat.extend_from_slice(sig);
    let find = |hay: &[u8], needle: &[u8]| -> Vec<usize> { hay.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| i).collect() };
    let mut hits = find(cat, &pat);
    if hits.len() > 1 {
        let names = find(cat, name.as_bytes());
        hits.retain(|&h| names.iter().any(|&n| h > n && h - n < 2000));
    }
    if hits.len() != 1 {
        bail!("catalog record for {name} not found exactly once ({} hits)", hits.len());
    }
    let h = hits[0];
    cat[h - 4..h].copy_from_slice(&crc.to_le_bytes());
    cat[h..h + 4].copy_from_slice(&(new_size as u32).to_le_bytes());
    Ok(())
}

pub fn run(o: Options, log: &mut dyn FnMut(String)) -> Result<()> {
    let g = o.game;
    if !g.is_valid() {
        bail!("To nie jest folder gry Escape Academy 2: {}", g.content.display());
    }
    if o.out_dir.is_none() && !o.dry_run && game_running() {
        bail!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie.");
    }
    let target = |p: &Path| -> PathBuf {
        match o.out_dir {
            Some(d) => d.join(p.file_name().unwrap()),
            None => p.to_path_buf(),
        }
    };

    // the clean file to start from: makes backups, except in a dry run (read-only lookup)
    let dry = o.dry_run;
    let backup_dir = o.originals.dir().to_path_buf();
    let orig = |o: &mut Originals, p: &Path| -> Result<PathBuf> { if dry { crate::originals::clean_copy(p, &backup_dir) } else { o.original(p) } };

    // type trees from the game's own bundles (the main file has none)
    log("[1/4] Dialogi w paczce gry...".into());
    let mut reg = TypeRegistry::default();
    let yarn_b = g.bundle("yarnprojects_").into_iter().next().ok_or_else(|| anyhow!("no yarnprojects bundle"))?;
    let subs_b = g.bundle("subtitles_");
    let mut font_b: Vec<PathBuf> = FONT_BUNDLES.iter().flat_map(|p| g.bundle(p)).collect();
    // Steam builds have no shared font bundle: many bundles (notes, cutscenes) carry their own copy of
    // the fonts with its atlas. Find every bundle that mentions one of our fonts (LZ4 keeps names as
    // literals), then check it really holds an SDF font asset when patching.
    for b in g.bundle("") {
        if font_b.contains(&b) || b.extension().map_or(true, |e| e != "bundle") {
            continue;
        }
        let Ok(src) = crate::originals::clean_copy(&b, &backup_dir) else { continue };
        let Ok(bytes) = std::fs::read(&src) else { continue };
        let has = |data: &[u8]| FONT_TOKENS.iter().any(|t| data.windows(t.len()).any(|w| w == t.as_bytes()));
        // quick look at the packed bytes first; LZ4 can hide even a short name inside a back-reference,
        // so a bundle without a hit is unpacked and searched inside
        if has(&bytes) || unity::Bundle::read(&bytes).map_or(false, |u| u.nodes.iter().any(|n| has(&n.data))) {
            font_b.push(b);
        }
    }
    font_b.sort();
    font_b.dedup();
    let mut originals_of: HashMap<PathBuf, PathBuf> = HashMap::new();
    for b in std::iter::once(&yarn_b).chain(&font_b).chain(&subs_b) {
        let src = orig(o.originals, b)?;
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(&src)?)?)?;
        originals_of.insert(b.clone(), src);
    }
    for b in g.bundle("6d5cd4d4522ca99553199616f94088e3_") {
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(&b)?)?)?; // native class trees (MonoScript, Material)
    }
    reg.add_builtin();
    let cat_orig = orig(o.originals, &g.catalog())?;
    let mut catalog = std::fs::read(&cat_orig)?;

    // speaker names need the English name_* texts: from the original GameplayStrings
    let loose = g.is_loose();
    let mut main = if loose {
        // read-only lookups here; backups of the files we change are made right before writing them
        Pack::read_dir(&g.data(), &|p: &Path| crate::originals::clean_copy(p, &backup_dir)).context("game data files")?
    } else {
        let main_orig = orig(o.originals, &g.main_file())?;
        Pack::read(&std::fs::read(&main_orig)?).context("data.unity3d")?
    };
    let text_assets: Vec<_> = main.all().filter(|&at| main.class(at) == 49).collect();
    let mut gameplay_at = None;
    for &at in &text_assets {
        let v = main.read_obj(&reg, at)?;
        if v.get("m_Name").and_then(|x| x.str()) == Some("GameplayStrings") {
            gameplay_at = Some((at, v));
            break;
        }
    }
    let (gp_at, gp_v) = gameplay_at.ok_or_else(|| anyhow!("GameplayStrings not found"))?;
    let gp_text = text_of(gp_v.get("m_Script").unwrap());
    let speakers = texts::speaker_map(o.store, &texts::english_names(&gp_text));

    let mut new_bundles: Vec<(PathBuf, u64, Vec<u8>, u32)> = Vec::new();
    {
        let orig = &originals_of[&yarn_b];
        let mut p = Pack::read(&std::fs::read(orig)?)?;
        let res = yarn::patch_pack(&mut p, &reg, o.store, &speakers)?;
        for r in &res {
            log(format!("Dialogue bundle {}: {} of {} lines in Polish", r.project, r.translated, r.total));
        }
        p.commit();
        new_bundles.push((yarn_b.clone(), std::fs::metadata(orig)?.len(), p.bundle.write(), p.crc()));
    }

    log("[2/4] Polskie litery w kopiach czcionek...".into());
    let mut n_font_bundles = 0;
    let mut fp = match o.fonts {
        Some(f) => FontPatcher::with_files(f.clone()),
        None => FontPatcher::new(o.fonts_dir),
    };
    for b in &font_b {
        let orig = &originals_of[b];
        let mut p = Pack::read(&std::fs::read(orig)?)?;
        let n = fp.patch_pack(&mut p, &reg, &mut |_| {})?;
        if n == 0 {
            continue; // no font asset of ours (or already complete): leave the bundle as it is
        }
        n_font_bundles += 1;
        p.commit();
        new_bundles.push((b.clone(), std::fs::metadata(orig)?.len(), p.bundle.write(), p.crc()));
    }
    log(format!("Fonts: Polish letters added in {n_font_bundles} bundle(s) with font copies"));
    for b in &subs_b {
        let orig = &originals_of[b];
        let mut p = Pack::read(&std::fs::read(orig)?)?;
        let mut n = 0;
        let ats: Vec<_> = p.all().filter(|&at| p.class(at) == 49).collect();
        for at in ats {
            let mut v = p.read_obj(&reg, at)?;
            let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
            if ["_es", "_fr", "_it", "_de", "_zh", "_ja"].iter().any(|s| name.ends_with(s)) {
                continue;
            }
            let Some(pl) = o.store.get(&format!("b:{name}")) else { continue };
            let src = text_of(v.get("m_Script").unwrap());
            *v.get_mut("m_Script").unwrap() = Value::Str(texts::srt(&src, pl));
            p.write_obj(&reg, at, &v)?;
            n += 1;
        }
        log(format!("Bark subtitles: {n} in Polish"));
        p.commit();
        new_bundles.push((b.clone(), std::fs::metadata(orig)?.len(), p.bundle.write(), p.crc()));
    }
    let sig = catalog_signature(&catalog, &g.bundles())?;
    for (b, old_size, bytes, crc) in &new_bundles {
        update_catalog(&mut catalog, &sig, *old_size, &b.file_name().unwrap().to_string_lossy(), *crc, bytes.len() as u64)?;
    }

    log("[3/4] Główny plik gry: teksty, napisy, dialogi, czcionki...".into());
    let (gp_new, n_gp) = texts::gameplay(&gp_text, o.store)?;
    let mut gp_v = gp_v;
    *gp_v.get_mut("m_Script").unwrap() = Value::Str(gp_new);
    main.write_obj(&reg, gp_at, &gp_v)?;
    let mut done = vec!["g"];
    for &at in &text_assets {
        let mut v = main.read_obj(&reg, at)?;
        let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
        let src = text_of(v.get("m_Script").unwrap());
        let new = match name.as_str() {
            "VOStrings" => {
                let (t, n) = texts::vo(&src, o.store)?;
                log(format!("VOStrings: {n} Polish subtitles"));
                done.push("v");
                t
            }
            "GameCredits" => {
                done.push("c");
                texts::credits(&src, o.store)
            }
            _ => continue,
        };
        *v.get_mut("m_Script").unwrap() = Value::Str(new);
        main.write_obj(&reg, at, &v)?;
    }
    let mut n_lm = 0;
    let monos: Vec<_> = main.all().filter(|&at| main.class(at) == 114).collect();
    let mut class_cache: HashMap<(usize, i32, i64), Option<String>> = HashMap::new();
    for &at in &monos {
        if main.tree(&reg, at).is_some() {
            continue; // classes with trees are fonts/Yarn, not the LocalizationManager
        }
        let Some(h) = unity::mono_header(&main.sf(at.file).objects[at.obj].data) else { continue };
        let class = class_cache.entry((at.file, h.script_file, h.script_path)).or_insert_with(|| script_class(&main, &reg, at)).clone();
        if class.as_deref() == Some("LocalizationManager") {
            let sf = &mut main.files[at.file].1;
            sf.objects[at.obj].data = texts::add_language(&sf.objects[at.obj].data, "pl")?;
            main.touch(at.file);
            n_lm += 1;
        }
    }
    let n_font = fp.patch_pack(&mut main, &reg, log)?;
    let yarn_res = yarn::patch_pack(&mut main, &reg, o.store, &speakers)?;
    log(format!("GameplayStrings: {n_gp} Polish texts; LocalizationManagers: {n_lm}; fonts: {n_font}; yarn projects: {}", yarn_res.len()));
    for r in &yarn_res {
        log(format!("Dialogue {}: {} of {} lines in Polish", r.project, r.translated, r.total));
    }
    let checks = [
        ("lista języków (LocalizationManager)", n_lm >= 2, format!("{n_lm} z 2")),
        ("polskie litery w fontach", n_font >= crate::fonts::SOURCES.len(), format!("{n_font} z {}", crate::fonts::SOURCES.len())),
        ("napisy (VOStrings)", done.contains(&"v"), String::new()),
        ("napisy końcowe (GameCredits)", done.contains(&"c"), String::new()),
        ("dialogi (projekty Yarn)", yarn_res.len() >= 2, format!("{} z 2", yarn_res.len())),
        ("katalog paczek (CRC i rozmiary)", true, format!("{} paczek", new_bundles.len())),
    ];
    let failed: Vec<&str> = checks.iter().filter(|c| !c.1).map(|c| c.0).collect();
    if dry {
        log("Wynik sprawdzenia zgodności:".into());
        for (name, ok, info) in &checks {
            log(format!("  {} {name}{}", if *ok { "[OK]  " } else { "[BRAK]" }, if info.is_empty() { String::new() } else { format!(" ({info})") }));
        }
        if !failed.is_empty() {
            bail!("Gra różni się od wersji, którą znamy: {}. Nic nie zostało zmienione.", failed.join(", "));
        }
        log("Zgodne: spolszczenie da się zainstalować w tej wersji gry. Nic nie zostało zmienione.".into());
        return Ok(());
    }
    if !failed.is_empty() {
        bail!("Something was not patched ({}) - not writing the game files.", failed.join(", "));
    }
    log("[4/4] Pakuję i zapisuję pliki gry...".into());
    // loose layout: only the changed files; bundle layout: the whole data.unity3d
    let main_out: Vec<(PathBuf, Vec<u8>)> = if loose {
        main.dirty.iter().map(|&f| (g.data().join(main.file_name(f)), main.files[f].1.write())).collect()
    } else {
        main.commit();
        vec![(g.main_file(), main.bundle.write())]
    };
    drop(main);
    if loose && o.out_dir.is_none() {
        for (p, _) in &main_out {
            o.originals.original(p)?; // back up the clean file before it is replaced
        }
    }

    // write: bundles, catalog, main file(s); then remember what we wrote
    for (b, _, bytes, _) in &new_bundles {
        write_atomic(&target(b), bytes)?;
    }
    write_atomic(&target(&g.catalog()), &catalog)?;
    for (p, bytes) in &main_out {
        write_atomic(&target(p), bytes)?;
    }
    if o.out_dir.is_none() {
        for (b, ..) in &new_bundles {
            o.originals.mark_written(b)?;
        }
        o.originals.mark_written(&g.catalog())?;
        for (p, _) in &main_out {
            o.originals.mark_written(p)?;
        }
    }
    log(format!("written: {} file(s) of game data", main_out.len()));
    Ok(())
}
