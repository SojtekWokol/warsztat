//! Writing Polish into Escape Academy: a fifth language "Polski" (see `il.rs` for why it is an
//! English-enum entry with code "pl" and GameplayStrings column 7).
//!
//! Changed files (all four are backed up through `kopie` before the first patch):
//!   Escape Academy_Data/data.unity3d                         GameplayStrings, room Yarn programs, fonts
//!   Escape Academy_Data/StreamingAssets/aa/StandaloneWindows64/areadioramas_assets_all_*.bundle
//!                                                            LanguageManager, campus Yarn, fonts
//!   Escape Academy_Data/StreamingAssets/aa/catalog.json      CRC and size of that bundle
//!   Escape Academy_Data/Managed/Assembly-CSharp.dll          the IL patch
//!
//! Texts come from the store: "g:<GameplayStrings key>" and "y:<Yarn line id>" (the line without its
//! "Speaker:" prefix, which stays English: the game picks the speaking character by it).
use crate::extract::parse_csv;
use crate::{base_name, il, language_manager_add, text_asset, text_asset_data, yarn_program, yarn_program_add, Files, Game, Language, PPtr};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const POLISH: &str = "pl";
pub const POLISH_CSV_INDEX: i32 = 7;

/// EA1 font assets with a source font in the repository's fonts/ folder (weights checked against the
/// game's glyph advances: Urbanist = ExtraBold, Urbanist_body = SemiBold).
pub const FONT_SOURCES: &[(&str, &str)] = &[
    ("Urbanist", "Urbanist-ExtraBold.ttf"),
    ("Urbanist_body", "Urbanist-SemiBold.ttf"),
    ("ChunkFive", "ChunkFive-Regular.ttf"),
    ("Kalam-Bold SDF", "Kalam-Bold.ttf"),
    ("knewave SDF", "Knewave-PL.ttf"),
];

/// The four files the patch changes (as paths of one game folder or of an output folder).
#[derive(Clone, Debug)]
pub struct Place {
    pub data: PathBuf,
    pub aa: PathBuf,
    pub catalog: PathBuf,
    pub dll: PathBuf,
}

/// Name of the Addressables bundle with the LanguageManager and the campus.
pub const AA_BUNDLE: &str = "areadioramas_assets_all_ecc21e2d9ade8095ee803e26e46fd1e6.bundle";

impl Place {
    /// Paths under a game folder (or an output folder with the same layout).
    pub fn under(dir: &Path) -> Place {
        let d = dir.join("Escape Academy_Data");
        Place {
            data: d.join("data.unity3d"),
            aa: d.join("StreamingAssets").join("aa").join("StandaloneWindows64").join(AA_BUNDLE),
            catalog: d.join("StreamingAssets").join("aa").join("catalog.json"),
            dll: d.join("Managed").join("Assembly-CSharp.dll"),
        }
    }
    pub fn all(&self) -> [&PathBuf; 4] {
        [&self.data, &self.aa, &self.catalog, &self.dll]
    }
    /// Game path of a backed-up file name.
    pub fn by_name(&self, name: &str) -> Option<PathBuf> {
        self.all().into_iter().find(|p| p.file_name().map_or(false, |n| n == name)).cloned()
    }
}

/// Where the Polish texts come from.
pub struct Texts<'a> {
    /// GameplayStrings: (key, English) -> the Polish column
    pub gameplay: &'a dyn Fn(&str, &str) -> String,
    /// Yarn: (line id, English line with its speaker prefix) -> the line of the "pl" table
    pub yarn: &'a dyn Fn(&str, &str) -> String,
}

/// "Speaker:line" when a colon appears in the first 40 characters (as in the EA2 workshop).
pub fn split_speaker(text: &str) -> (String, String) {
    let head: String = text.chars().take(40).collect();
    match text.split_once(':') {
        Some((a, b)) if head.contains(':') => (a.to_string(), b.to_string()),
        _ => (String::new(), text.to_string()),
    }
}

/// Texts from a translation store {id: Polish}; untranslated texts stay English.
pub fn store_texts(store: &BTreeMap<String, String>) -> (impl Fn(&str, &str) -> String + '_, impl Fn(&str, &str) -> String + '_) {
    let g = move |key: &str, en: &str| store.get(&format!("g:{key}")).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| en.to_string());
    let y = move |id: &str, en: &str| {
        let (speaker, line) = split_speaker(en);
        // keep the "Speaker:" prefix exactly, also an empty one (":DING!" = nobody speaks)
        let has_prefix = line.len() < en.len();
        match store.get(&format!("y:{id}")).filter(|s| !s.is_empty()) {
            Some(pl) if has_prefix => format!("{speaker}:{pl}"),
            Some(pl) => pl.clone(),
            None => en.to_string(),
        }
    };
    (g, y)
}

pub fn write_csv(rows: &[Vec<String>], eol: &str) -> String {
    let field = |f: &String| {
        if f.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", f.replace('"', "\"\""))
        } else {
            f.clone()
        }
    };
    rows.iter().map(|r| r.iter().map(field).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join(eol) + eol
}

fn gameplay_with_polish(body: &[u8], texts: &Texts) -> Result<(Vec<u8>, usize)> {
    let text = String::from_utf8_lossy(body);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut rows = parse_csv(&text);
    let width = rows.first().map_or(0, |h| h.len());
    if width != POLISH_CSV_INDEX as usize {
        bail!("GameplayStrings has {width} columns, expected {POLISH_CSV_INDEX} - is it the original?");
    }
    let mut changed = 0;
    for (i, r) in rows.iter_mut().enumerate() {
        if r.is_empty() {
            continue;
        }
        r.resize(width, String::new());
        let v = if i == 0 { "Polish".to_string() } else { (texts.gameplay)(&r[0], &r[1]) };
        changed += (i > 0 && v != r[1]) as usize;
        r.push(v);
    }
    Ok((write_csv(&rows, eol).into_bytes(), changed))
}

fn yarn_table_polish(body: &[u8], texts: &Texts, changed: &mut usize) -> Vec<u8> {
    let text = String::from_utf8_lossy(body);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut rows = parse_csv(&text);
    for r in rows.iter_mut().skip(1) {
        if r.len() >= 2 {
            let v = (texts.yarn)(&r[0], &r[1]);
            *changed += (v != r[1]) as usize;
            r[1] = v;
        }
    }
    write_csv(&rows, eol).into_bytes()
}

fn replace_number_utf16(blob: &mut [u8], from: usize, key: &str, value: u64) -> Result<()> {
    let pat: Vec<u8> = format!("\"{key}\":").encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let at = blob[from..].windows(pat.len()).position(|w| w == pat.as_slice()).ok_or_else(|| anyhow!("{key} not in catalog"))? + from + pat.len();
    let mut end = at;
    while end + 1 < blob.len() && blob[end + 1] == 0 && blob[end].is_ascii_digit() {
        end += 2;
    }
    let old_chars = (end - at) / 2;
    let new = value.to_string();
    if new.len() > old_chars {
        bail!("catalog: new {key} {new} is longer than the old value");
    }
    let padded = format!("{new:<old_chars$}");
    for (i, c) in padded.encode_utf16().enumerate() {
        blob[at + i * 2..at + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }
    Ok(())
}

/// catalog.json with a new CRC and size for the bundle `hash`: the numbers sit in UTF-16 JSON inside
/// m_ExtraDataString (base64) behind a length prefix, so they are padded with spaces to keep the length.
pub fn catalog_update(catalog: &str, hash: &str, crc: u32, size: u64) -> Result<String> {
    let mut v: serde_json::Value = serde_json::from_str(catalog)?;
    let b64 = v["m_ExtraDataString"].as_str().ok_or_else(|| anyhow!("no m_ExtraDataString"))?;
    let mut blob = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let hpat: Vec<u8> = format!("\"m_Hash\":\"{hash}\"").encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let at = blob.windows(hpat.len()).position(|w| w == hpat.as_slice()).ok_or_else(|| anyhow!("bundle {hash} not in catalog"))?;
    replace_number_utf16(&mut blob, at, "m_Crc", crc as u64)?;
    replace_number_utf16(&mut blob, at, "m_BundleSize", size)?;
    v["m_ExtraDataString"] = serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(&blob));
    Ok(serde_json::to_string(&v)?)
}

/// Write `bytes` to `dest` through a temporary file next to it (a half-written game file is worse
/// than none).
fn write_safely(dest: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let tmp = dest.with_extension("pl-tmp");
    std::fs::write(&tmp, bytes).with_context(|| tmp.display().to_string())?;
    std::fs::rename(&tmp, dest).with_context(|| dest.display().to_string())?;
    Ok(())
}

/// Build the four patched files from clean `src` into `dst`. With `write` false nothing is written
/// (a compatibility check: every step runs, the results are dropped).
pub fn build(src: &Place, dst: &Place, texts: &Texts, fonts_dir: &Path, write: bool, log: &mut dyn FnMut(String)) -> Result<()> {
    let bundles = vec![src.data.clone(), src.aa.clone()];
    let mut files = Files::load_bundles(&bundles)?;
    log(format!("[1/4] Wczytano {} plików z 2 paczek gry", files.files.len()));
    let mut changed: HashSet<usize> = HashSet::new();

    // Yarn: a "pl" table next to every program
    let mut adds: Vec<(usize, i64, String, Vec<u8>)> = Vec::new();
    let mut yarn_pl = 0;
    for (fi, (_, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 114) {
            if files.script_of(fi, &o.data).as_deref() != Some("YarnProgram") {
                continue;
            }
            let p = yarn_program(&o.data)?;
            let (_, base) = files.deref(fi, p.base_table).ok_or_else(|| anyhow!("{}: base table not found", p.name))?;
            let (bname, body) = text_asset(&base.data)?;
            let name = format!("{} ({POLISH})", bname.rsplit_once(" (").map_or(bname.as_str(), |x| x.0));
            adds.push((fi, o.path_id, name, yarn_table_polish(&body, texts, &mut yarn_pl)));
        }
    }
    for (fi, prog, name, body) in &adds {
        let (fname, sf) = &mut files.files[*fi];
        let ti = sf.types.iter().position(|t| t.class_id == 49).ok_or_else(|| anyhow!("{fname}: no TextAsset type"))?;
        let id = sf.max_path_id() + 1;
        sf.add_object(id, ti, text_asset_data(name, body));
        let o = sf.object_mut(*prog).unwrap();
        o.data = yarn_program_add(&o.data, POLISH, PPtr { file: 0, path: id })?;
        changed.insert(*fi);
    }
    log(format!("[2/4] Dialogi: {} programów Yarn, {yarn_pl} kwestii po polsku", adds.len()));

    // GameplayStrings column 7, LanguageManager entry
    let (mut gp, mut gp_pl, mut lm) = (0, 0, 0);
    for fi in 0..files.files.len() {
        let mut edits: Vec<(i64, Vec<u8>)> = Vec::new();
        {
            let sf = &files.files[fi].1;
            for o in &sf.objects {
                if o.class_id == 49 {
                    let (name, body) = text_asset(&o.data)?;
                    if name.starts_with("GameplayStrings") {
                        let (b, n) = gameplay_with_polish(&body, texts).with_context(|| name.clone())?;
                        edits.push((o.path_id, text_asset_data(&name, &b)));
                        gp += 1;
                        gp_pl += n;
                    }
                } else if o.class_id == 114 && files.script_of(fi, &o.data).as_deref() == Some("Escape.Localization.LanguageManager") {
                    let pl = Language { kind: 0, display_name: "Polski".into(), code: POLISH.into(), csv_index: POLISH_CSV_INDEX };
                    edits.push((o.path_id, language_manager_add(&o.data, &pl)?));
                    lm += 1;
                }
            }
        }
        for (id, data) in edits {
            files.files[fi].1.object_mut(id).unwrap().data = data;
            changed.insert(fi);
        }
    }
    if gp != 4 || lm != 1 {
        bail!("Nieznana wersja gry: {gp} tabel GameplayStrings (miało być 4), {lm} LanguageManager (miał być 1)");
    }
    log(format!("[2/4] Teksty interfejsu: {gp_pl} po polsku; język „Polski” w menu"));

    // repack, fonts, catalog
    let dsts = [&dst.data, &dst.aa];
    let mut repacked: Vec<(usize, unity::Bundle)> = Vec::new();
    for (bi, path) in bundles.iter().enumerate() {
        let mut b = unity::Bundle::read(&std::fs::read(path)?)?;
        for (fi, (name, sf)) in files.files.iter().enumerate() {
            if files.bundle_of[fi] == bi && changed.contains(&fi) {
                b.node_mut(name).unwrap().data = sf.write();
            }
        }
        repacked.push((bi, b));
    }
    drop(files);
    let mut reg = unity::TypeRegistry::default();
    reg.add_bundle(&repacked[1].1)?; // the Addressables bundle keeps its type trees
    reg.add_builtin();
    let mut fp = ea2::fonts::FontPatcher::with_sources(fonts_dir, FONT_SOURCES);
    let mut catalog = std::fs::read_to_string(&src.catalog)?;
    for (bi, b) in repacked {
        let mut pack = ea2::pack::Pack { bundle: b, files: Vec::new(), dirty: Default::default() };
        for (i, n) in pack.bundle.nodes.iter().enumerate() {
            if n.flags & unity::NODE_SERIALIZED != 0 {
                pack.files.push((i, unity::SerializedFile::read(&n.data)?));
            }
        }
        let mut quiet = |m: String| {
            if !m.contains("bez źródła") {
                log(m)
            }
        };
        let n = fp.patch_pack(&mut pack, &reg, &mut quiet)?;
        pack.commit();
        let bytes = pack.bundle.write();
        if bi == 1 {
            let hash = base_name(&src.aa.to_string_lossy()).rsplit_once('_').map(|x| x.1.trim_end_matches(".bundle").to_string()).unwrap_or_default();
            catalog = catalog_update(&catalog, &hash, pack.crc(), bytes.len() as u64)?;
        }
        log(format!("[3/4] {}: {n} fontów z polskimi literami, {} MB", base_name(&dsts[bi].to_string_lossy()), bytes.len() >> 20));
        if write {
            write_safely(dsts[bi], &bytes)?;
        }
    }
    let dll = il::patch(&std::fs::read(&src.dll)?)?;
    if write {
        write_safely(&dst.catalog, catalog.as_bytes())?;
        write_safely(&dst.dll, &dll)?;
    }
    log("[4/4] Katalog paczek (CRC) i kod gry (łatka IL) gotowe".into());
    Ok(())
}

pub fn game_running() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Escape Academy.exe"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Escape Academy.exe"))
        .unwrap_or(false)
}

pub struct Options<'a> {
    pub game: &'a Game,
    pub store: &'a BTreeMap<String, String>,
    pub fonts_dir: &'a Path,
    /// backups of the original files (kopie)
    pub backup_dir: &'a Path,
    /// write into this folder (same layout as the game) instead of the game
    pub out_dir: Option<&'a Path>,
    /// check only, write nothing
    pub dry_run: bool,
}

/// The patch: originals from the backups (made on the first run), Polish from the store.
pub fn run(o: Options, log: &mut dyn FnMut(String)) -> Result<()> {
    if !o.game.is_valid() {
        bail!("To nie jest folder gry Escape Academy: {}", o.game.dir.display());
    }
    let into_game = o.out_dir.is_none() && !o.dry_run;
    if into_game && game_running() {
        bail!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie.");
    }
    let game_place = Place::under(&o.game.dir);
    let mut originals = kopie::Originals::open(o.backup_dir, crate::known::CONFIG)?;
    let mut src_paths = Vec::new();
    for p in game_place.all() {
        src_paths.push(if into_game { originals.original(p)? } else { kopie::Originals::clean_copy(&crate::known::CONFIG, p, o.backup_dir)? });
    }
    let src = Place { data: src_paths[0].clone(), aa: src_paths[1].clone(), catalog: src_paths[2].clone(), dll: src_paths[3].clone() };
    if il::state(&std::fs::read(&src.dll)?)? != il::State::Original {
        bail!("Kopia Assembly-CSharp.dll nie jest oryginałem - przywróć pliki gry.");
    }
    let dst = match o.out_dir {
        Some(d) => Place::under(d),
        None => game_place.clone(),
    };
    let (g, y) = store_texts(o.store);
    let texts = Texts { gameplay: &g, yarn: &y };
    build(&src, &dst, &texts, o.fonts_dir, !o.dry_run, log)?;
    if into_game {
        for p in game_place.all() {
            originals.mark_written(p)?;
        }
        log("Spolszczenie wgrane. W grze wybierz w ustawieniach język „Polski”.".into());
    } else if o.dry_run {
        log("Zgodne: spolszczenie da się wgrać do tej wersji gry. Nic nie zostało zmienione.".into());
    }
    Ok(())
}

/// Put the original files back. Returns (restored, skipped).
pub fn restore(game: &Game, backup_dir: &Path) -> Result<(usize, usize)> {
    if game_running() {
        bail!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie.");
    }
    let place = Place::under(&game.dir);
    let mut originals = kopie::Originals::open(backup_dir, crate::known::CONFIG)?;
    originals.restore(|name| place.by_name(name))
}
