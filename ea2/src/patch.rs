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
    pub fn is_valid(&self) -> bool {
        self.main_file().exists() && self.catalog().exists()
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
            _ => None,
        }
    }
}

pub fn game_running() -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ProjectFLEA.exe"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ProjectFLEA.exe"))
        .unwrap_or(false)
}

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
}

fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = target.with_extension("pl_tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, target).with_context(|| format!("cannot replace {}", target.display()))?;
    Ok(())
}

/// In catalog bytes, set the CRC and size of one bundle. Record: crc u32, size u32, 0x8711, 0x8752,
/// found by the bundle's original size (several same-size bundles: nearest after the bundle's name).
fn update_catalog(cat: &mut [u8], old_size: u64, name: &str, crc: u32, new_size: u64) -> Result<()> {
    let mut pat = (old_size as u32).to_le_bytes().to_vec();
    pat.extend_from_slice(&[0x11, 0x87, 0, 0, 0x52, 0x87, 0, 0]);
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
    if o.out_dir.is_none() && game_running() {
        bail!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie.");
    }
    let target = |p: &Path| -> PathBuf {
        match o.out_dir {
            Some(d) => d.join(p.file_name().unwrap()),
            None => p.to_path_buf(),
        }
    };

    // type trees from the game's own bundles (the main file has none)
    log("[1/4] Dialogi w paczce gry...".into());
    let mut reg = TypeRegistry::default();
    let yarn_b = g.bundle("yarnprojects_").into_iter().next().ok_or_else(|| anyhow!("no yarnprojects bundle"))?;
    let subs_b = g.bundle("subtitles_");
    let mut font_b: Vec<PathBuf> = FONT_BUNDLES.iter().flat_map(|p| g.bundle(p)).collect();
    font_b.dedup();
    let mut originals_of: HashMap<PathBuf, PathBuf> = HashMap::new();
    for b in std::iter::once(&yarn_b).chain(&font_b).chain(&subs_b) {
        let orig = o.originals.original(b)?;
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(&orig)?)?)?;
        originals_of.insert(b.clone(), orig);
    }
    for b in g.bundle("6d5cd4d4522ca99553199616f94088e3_") {
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(&b)?)?)?; // native class trees (MonoScript, Material)
    }
    let cat_orig = o.originals.original(&g.catalog())?;
    let mut catalog = std::fs::read(&cat_orig)?;

    // speaker names need the English name_* texts: from the original GameplayStrings
    let main_orig = o.originals.original(&g.main_file())?;
    let mut main = Pack::read(&std::fs::read(&main_orig)?).context("data.unity3d")?;
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
    let mut fp = match o.fonts {
        Some(f) => FontPatcher::with_files(f.clone()),
        None => FontPatcher::new(o.fonts_dir),
    };
    for b in &font_b {
        let orig = &originals_of[b];
        let mut p = Pack::read(&std::fs::read(orig)?)?;
        let n = fp.patch_pack(&mut p, &reg, &mut |_| {})?;
        log(format!("Fonts in {}: {n}", &b.file_name().unwrap().to_string_lossy()[..45.min(b.file_name().unwrap().len())]));
        p.commit();
        new_bundles.push((b.clone(), std::fs::metadata(orig)?.len(), p.bundle.write(), p.crc()));
    }
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
    for (b, old_size, bytes, crc) in &new_bundles {
        update_catalog(&mut catalog, *old_size, &b.file_name().unwrap().to_string_lossy(), *crc, bytes.len() as u64)?;
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
            n_lm += 1;
        }
    }
    let n_font = fp.patch_pack(&mut main, &reg, log)?;
    let yarn_res = yarn::patch_pack(&mut main, &reg, o.store, &speakers)?;
    log(format!("GameplayStrings: {n_gp} Polish texts; LocalizationManagers: {n_lm}; fonts: {n_font}; yarn projects: {}", yarn_res.len()));
    for r in &yarn_res {
        log(format!("Dialogue {}: {} of {} lines in Polish", r.project, r.translated, r.total));
    }
    if n_lm < 2 || n_font < crate::fonts::SOURCES.len() || !done.contains(&"v") || yarn_res.len() < 2 {
        bail!("Something was not patched - not writing the game files.");
    }
    log("[4/4] Pakuję i zapisuję pliki gry...".into());
    main.commit();
    let main_bytes = main.bundle.write();
    drop(main);

    // write: bundles, catalog, main file; then remember what we wrote
    for (b, _, bytes, _) in &new_bundles {
        write_atomic(&target(b), bytes)?;
    }
    write_atomic(&target(&g.catalog()), &catalog)?;
    write_atomic(&target(&g.main_file()), &main_bytes)?;
    if o.out_dir.is_none() {
        for (b, ..) in &new_bundles {
            o.originals.mark_written(b)?;
        }
        o.originals.mark_written(&g.catalog())?;
        o.originals.mark_written(&g.main_file())?;
    }
    log(format!("written: {}", target(&g.main_file()).display()));
    Ok(())
}
