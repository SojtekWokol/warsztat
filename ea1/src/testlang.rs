//! A test build of the fifth language: "Polski" in the menu, with the English texts marked "[PL] ",
//! written to a separate folder (never into the game). Proves the whole chain before translating:
//! IL patch, LanguageManager entry, GameplayStrings column 7, Yarn "pl" tables, bundle repack and
//! the Addressables catalog CRC.
//!
//! Output (same layout as the game folder):
//!   Escape Academy_Data/data.unity3d
//!   Escape Academy_Data/Managed/Assembly-CSharp.dll
//!   Escape Academy_Data/StreamingAssets/aa/StandaloneWindows64/<bundle>
//!   Escape Academy_Data/StreamingAssets/aa/catalog.json
use crate::extract::parse_csv;
use crate::{base_name, il, language_manager_add, text_asset, text_asset_data, yarn_program, yarn_program_add, Files, Game, Language, PPtr};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const POLISH: &str = "pl";

/// EA1 font assets with a source font in the repository's fonts/ folder (weights checked against the
/// game's glyph advances: Urbanist = ExtraBold, Urbanist_body = SemiBold).
pub const FONT_SOURCES: &[(&str, &str)] = &[
    ("Urbanist", "Urbanist-ExtraBold.ttf"),
    ("Urbanist_body", "Urbanist-SemiBold.ttf"),
    ("ChunkFive", "ChunkFive-Regular.ttf"),
    ("Kalam-Bold SDF", "Kalam-Bold.ttf"),
    ("knewave SDF", "Knewave-PL.ttf"),
];
pub const POLISH_CSV_INDEX: i32 = 7;

/// Marks sentences only (numbers, keys and single words stay as they are: puzzles compare them).
fn mark(s: &str) -> String {
    if !s.contains(' ') {
        return s.to_string();
    }
    // keep a Yarn speaker prefix "Name:" first
    match s.split_once(':') {
        Some((who, rest)) if !who.is_empty() && who.len() < 40 && who.split(' ').count() <= 3 && !who.contains('<') && !rest.is_empty() => format!("{who}:[PL] {rest}"),
        _ => format!("[PL] {s}"),
    }
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

/// GameplayStrings with a Polish column at index 7 (the English text marked).
fn gameplay_with_polish(body: &[u8]) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(body);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut rows = parse_csv(&text);
    let width = rows.first().map_or(0, |h| h.len());
    if width != POLISH_CSV_INDEX as usize {
        bail!("GameplayStrings has {width} columns, expected {POLISH_CSV_INDEX}");
    }
    for (i, r) in rows.iter_mut().enumerate() {
        if r.is_empty() {
            continue;
        }
        r.resize(width, String::new());
        let v = if i == 0 { "Polish".to_string() } else { mark(&r[1]) };
        r.push(v);
    }
    Ok(write_csv(&rows, eol).into_bytes())
}

/// Yarn string table (id,text,file,node,lineNumber) with marked texts, or the given Polish line.
fn yarn_table_marked(body: &[u8], polish: &HashMap<String, String>) -> Vec<u8> {
    let text = String::from_utf8_lossy(body);
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut rows = parse_csv(&text);
    for r in rows.iter_mut().skip(1) {
        if r.len() >= 2 {
            r[1] = polish.get(&r[0]).cloned().unwrap_or_else(|| mark(&r[1]));
        }
    }
    write_csv(&rows, eol).into_bytes()
}

/// Replace `"key":<number>` in the UTF-16 JSON of the catalog's extra data, padding with spaces so
/// that the length (and the length prefix in front of the JSON) stays the same.
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

/// catalog.json with a new CRC and size for the bundle whose name ends in `_<hash>.bundle`.
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

/// `polish`: finished Polish Yarn lines {line id: text} used instead of the marked English.
pub fn build(game: &Game, out: &Path, polish: &HashMap<String, String>, fonts_dir: &Path, log: &mut dyn FnMut(String)) -> Result<()> {
    let mut files = Files::load(game)?;
    log(format!("{} files from {} bundles", files.files.len(), files.bundles.len()));
    let mut changed: HashSet<usize> = HashSet::new();

    // 1. Yarn: a "pl" table next to every program (a marked copy of the base table)
    let mut adds: Vec<(usize, i64, String, Vec<u8>)> = Vec::new(); // file, program path id, table name, table body
    for (fi, (_, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 114) {
            if files.script_of(fi, &o.data).as_deref() != Some("YarnProgram") {
                continue;
            }
            let p = yarn_program(&o.data)?;
            let (_, base) = files.deref(fi, p.base_table).ok_or_else(|| anyhow!("{}: base table not found", p.name))?;
            let (bname, body) = text_asset(&base.data)?;
            let name = format!("{} ({POLISH})", bname.rsplit_once(" (").map_or(bname.as_str(), |x| x.0));
            adds.push((fi, o.path_id, name, yarn_table_marked(&body, polish)));
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
    log(format!("Yarn: {} programs got a \"{POLISH}\" table", adds.len()));

    // 2. GameplayStrings column 7, 3. LanguageManager entry
    let mut gp = 0;
    let mut lm = 0;
    for fi in 0..files.files.len() {
        let mut edits: Vec<(i64, Vec<u8>)> = Vec::new();
        {
            let sf = &files.files[fi].1;
            for o in &sf.objects {
                if o.class_id == 49 {
                    let (name, body) = text_asset(&o.data)?;
                    if name.starts_with("GameplayStrings") {
                        edits.push((o.path_id, text_asset_data(&name, &gameplay_with_polish(&body).with_context(|| name.clone())?)));
                        gp += 1;
                    }
                } else if o.class_id == 114 && files.script_of(fi, &o.data).as_deref() == Some("Escape.Localization.LanguageManager") {
                    // the English enum value: graphics, materials and puzzle answers as in English
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
        bail!("expected 4 GameplayStrings tables and 1 LanguageManager, found {gp} and {lm}");
    }
    log(format!("GameplayStrings: {gp} tables got a Polish column; LanguageManager: + Polski (pl, csv {POLISH_CSV_INDEX})"));

    // 4. repack the changed bundles
    let data_dir = out.join("Escape Academy_Data");
    let mut written: Vec<(usize, std::path::PathBuf)> = Vec::new();
    for (bi, path) in files.bundles.iter().enumerate() {
        let touched: Vec<usize> = (0..files.files.len()).filter(|&fi| files.bundle_of[fi] == bi && changed.contains(&fi)).collect();
        if touched.is_empty() {
            continue;
        }
        let mut b = unity::Bundle::read(&std::fs::read(path)?)?;
        for fi in &touched {
            let (name, sf) = &files.files[*fi];
            b.node_mut(name).unwrap().data = sf.write();
        }
        let rel = path.strip_prefix(&game.dir.join("Escape Academy_Data"))?;
        let dest = data_dir.join(rel);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        std::fs::write(&dest, b.write())?;
        written.push((bi, dest));
    }
    let bundles = files.bundles.clone();
    drop(files);

    // 5. Polish letters in the SDF fonts (ea2's patcher; type trees from the Addressables bundle,
    // which has them, serve data.unity3d too: same scripts and classes)
    let mut reg = unity::TypeRegistry::default();
    for p in bundles.iter().skip(1) {
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(p)?)?)?;
    }
    reg.add_builtin();
    let mut fp = ea2::fonts::FontPatcher::with_sources(fonts_dir, FONT_SOURCES);
    let mut catalog = None;
    for (bi, dest) in &written {
        let mut pack = ea2::pack::Pack::read(&std::fs::read(dest)?)?;
        let n = fp.patch_pack(&mut pack, &reg, log)?;
        pack.commit();
        let bytes = pack.bundle.write();
        std::fs::write(dest, &bytes)?;
        log(format!("written {} ({} MB, {n} fonts patched)", dest.display(), bytes.len() >> 20));
        if *bi > 0 {
            // Addressables bundle: new CRC (of the uncompressed data) and size in the catalog
            let crc = pack.crc();
            let hash = base_name(&bundles[*bi].to_string_lossy()).rsplit_once('_').map(|x| x.1.trim_end_matches(".bundle").to_string()).unwrap_or_default();
            let src = catalog.take().map_or_else(|| std::fs::read_to_string(game.dir.join("Escape Academy_Data/StreamingAssets/aa/catalog.json")), Ok)?;
            catalog = Some(catalog_update(&src, &hash, crc, bytes.len() as u64)?);
        }
    }
    if let Some(c) = catalog {
        let dest = data_dir.join("StreamingAssets/aa/catalog.json");
        std::fs::write(&dest, c)?;
        log(format!("written {}", dest.display()));
    }

    // 6. the IL patch
    let dll = std::fs::read(game.managed().join("Assembly-CSharp.dll"))?;
    let patched = il::patch(&dll)?;
    std::fs::create_dir_all(data_dir.join("Managed"))?;
    std::fs::write(data_dir.join("Managed").join("Assembly-CSharp.dll"), patched)?;
    log("written Managed/Assembly-CSharp.dll (IL patch)".into());
    Ok(())
}
