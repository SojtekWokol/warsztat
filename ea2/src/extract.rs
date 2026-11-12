//! Texts and preview pictures from the player's own copy of the game, into extracted/ and cache/vn/
//! (what the workshop reads). Always from clean files: if the game holds our patch, the backups.
//!
//! extracted/GameplayStrings.txt, VOStrings.txt, GameCredits.txt   text tables (as in the game)
//! extracted/yarn_<lang>.json, yarn_program.bin, yarn_scenes.json   cutscenes (VisNov project, yarnprojects bundle)
//! extracted/yarn2_<lang>.json, yarn2_program.bin, yarn2_scenes.json campus talks (Dialogue project, data.unity3d)
//! extracted/subtitles.json                                         Gillian's bark subtitles
//! cache/vn/bg/<name>.jpg, cache/vn/chars/<name>.png                backgrounds and character sprites
use crate::originals::Originals;
use crate::pack::{text_of, At, Pack};
use crate::patch::Game;
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unity::{TypeRegistry, Value as UV};

/// The clean version of a game file: the backup when the game file is the one our patch wrote.
fn clean(game_file: &Path, backup_dir: &Path) -> Result<PathBuf> {
    Originals::clean_copy(game_file, backup_dir)
}

fn write_json(p: &Path, v: &Value) -> Result<()> {
    std::fs::write(p, warsztat::store::to_python_json(v)?).with_context(|| p.display().to_string())
}

/// Yarn projects of a pack: (project name, compiled program, {lang: [(key, value)]}).
fn yarn_projects(pack: &Pack, reg: &TypeRegistry) -> Result<Vec<(String, Vec<u8>, Vec<(String, Vec<(String, String)>)>)>> {
    let mut out = Vec::new();
    let projects: Vec<At> = pack.all().filter(|&at| pack.class(at) == 114 && pack.tree(reg, at).map_or(false, |t| t.child("localizations").is_some())).collect();
    for p in projects {
        let v = pack.read_obj(reg, p)?;
        let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
        let program = match v.get("compiledYarnProgram") {
            Some(UV::Bytes(b)) => b.clone(),
            Some(UV::Array(a)) => a.iter().map(|x| x.i64().unwrap_or(0) as u8).collect(),
            _ => vec![],
        };
        let mut tables = Vec::new();
        for ptr in v.get("localizations").and_then(|a| a.array()).cloned().unwrap_or_default() {
            let Some(at) = pack.resolve(p.file, &ptr) else { continue };
            let lv = pack.read_obj(reg, at)?;
            let code = lv.get("_LocaleCode").and_then(|x| x.str()).unwrap_or("").to_string();
            let t = lv.get("_stringTable");
            let keys = t.and_then(|t| t.get("keys")).and_then(|x| x.array()).cloned().unwrap_or_default();
            let vals = t.and_then(|t| t.get("values")).and_then(|x| x.array()).cloned().unwrap_or_default();
            tables.push((code, keys.iter().zip(&vals).map(|(k, v)| (text_of(k), text_of(v))).collect()));
        }
        out.push((name, program, tables));
    }
    Ok(out)
}

/// Scenes of a compiled Yarn program (protobuf): its readable strings in order are enough - a node
/// starts where its name appears twice in a row, lines are "line:xxxx", stage commands are strings
/// like `setBackground "TOW_BG_1"` or `placeCharacter "Name" "sprite" "Left"`.
pub fn yarn_scenes(prog: &[u8]) -> Value {
    let mut strs: Vec<String> = Vec::new();
    let mut run = Vec::new();
    for &b in prog.iter().chain(std::iter::once(&0u8)) {
        if (0x20..=0x7e).contains(&b) {
            run.push(b);
        } else {
            if run.len() >= 4 {
                strs.push(String::from_utf8_lossy(&run).into_owned());
            }
            run.clear();
        }
    }
    let name = Regex::new(r"^[A-Za-z][A-Za-z0-9_]+$").unwrap();
    let label = Regex::new(r"^L[0-9]+[a-z_]").unwrap();
    let bg_re = Regex::new(r#"(?:setBackground|crossfadeBackground) "([^"]+)""#).unwrap();
    let place = Regex::new(r#"placeCharacter "([^"]+)" "([^"]+)" "([^"]+)""#).unwrap();
    let remove = Regex::new(r#"(?:removeCharacter|hideCharacter) "([^"]+)""#).unwrap();
    let mv = Regex::new(r#"moveCharacter "([^"]+)" "([^"]+)""#).unwrap();
    let mut scenes: Vec<(String, Vec<Value>)> = Vec::new();
    let mut bg = String::new();
    let mut chars: Vec<(String, String, String)> = Vec::new(); // name, sprite, pos (insertion order)
    let mut i = 0;
    while i < strs.len() {
        let s = &strs[i];
        if name.is_match(s) && !label.is_match(s) && i + 1 < strs.len() && strs[i + 1] == *s && s != "title" {
            if scenes.last().map_or(true, |c| c.0 != *s) {
                scenes.push((s.clone(), vec![]));
                bg.clear();
                chars.clear();
            }
            i += 2;
            continue;
        }
        if let Some(c) = bg_re.captures(s) {
            bg = c[1].to_string();
        }
        if let Some(c) = place.captures(s) {
            match chars.iter_mut().find(|x| x.0 == c[1]) {
                Some(x) => {
                    x.1 = c[2].to_string();
                    x.2 = c[3].to_string();
                }
                None => chars.push((c[1].to_string(), c[2].to_string(), c[3].to_string())),
            }
        }
        if let Some(c) = remove.captures(s) {
            chars.retain(|x| x.0 != c[1]);
        }
        if let Some(c) = mv.captures(s) {
            if let Some(x) = chars.iter_mut().find(|x| x.0 == c[1]) {
                x.2 = c[2].to_string();
            }
        }
        if s.starts_with("line:") {
            if let Some(cur) = scenes.last_mut() {
                let lid = s.trim().to_string();
                if !cur.1.iter().any(|l| l["id"] == lid.as_str()) {
                    let cs: Vec<Value> = chars.iter().map(|(n, sp, p)| json!({"name": n, "sprite": sp, "pos": p})).collect();
                    cur.1.push(json!({"id": lid, "bg": bg, "chars": cs}));
                }
            }
        }
        i += 1;
    }
    Value::Array(scenes.into_iter().filter(|s| !s.1.is_empty()).map(|(n, l)| json!({"node": n, "lines": l})).collect())
}

fn table_json(t: &[(String, String)]) -> Value {
    let mut m = Map::new();
    for (k, v) in t {
        m.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(m)
}

pub fn extract(game: &Game, backup_dir: &Path, root: &Path, images: bool, log: &mut dyn FnMut(String)) -> Result<()> {
    if !game.is_valid() {
        bail!("To nie jest folder gry Escape Academy 2: {}", game.content.display());
    }
    let ex = root.join("extracted");
    std::fs::create_dir_all(&ex)?;
    let mut reg = TypeRegistry::default();
    let yarn_b = game.bundle("yarnprojects_").into_iter().next().ok_or_else(|| anyhow!("no yarnprojects bundle"))?;
    let subs_b = game.bundle("subtitles_").into_iter().next().ok_or_else(|| anyhow!("no subtitles bundle"))?;
    let yarn_pack = Pack::read(&std::fs::read(clean(&yarn_b, backup_dir)?)?)?;
    let subs_pack = Pack::read(&std::fs::read(clean(&subs_b, backup_dir)?)?)?;
    for (_, sf) in yarn_pack.files.iter().chain(&subs_pack.files) {
        reg.add_file(sf);
    }
    for b in game.bundle("duplicateassetisolation_").into_iter().chain(game.bundle("6d5cd4d4522ca99553199616f94088e3_")) {
        reg.add_bundle(&unity::Bundle::read(&std::fs::read(clean(&b, backup_dir)?)?)?)?;
    }

    log("[1/4] Teksty z głównego pliku gry...".into());
    let main = Pack::read(&std::fs::read(clean(&game.main_file(), backup_dir)?)?).context("data.unity3d")?;
    let mut n_ta = 0;
    for at in main.all().filter(|&at| main.class(at) == 49) {
        let v = main.read_obj(&reg, at)?;
        let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("");
        if ["GameplayStrings", "VOStrings", "GameCredits"].contains(&name) {
            let bytes = match v.get("m_Script") {
                Some(UV::Str(s)) => s.as_bytes().to_vec(),
                Some(UV::Bytes(b)) => b.clone(),
                _ => vec![],
            };
            if name == "GameplayStrings" && String::from_utf8_lossy(&bytes).contains("Polish Value (pl)") {
                bail!("GameplayStrings w grze ma już polską kolumnę, a kopii zapasowej nie ma - przywróć oryginalne pliki gry.");
            }
            std::fs::write(ex.join(format!("{name}.txt")), &bytes)?;
            n_ta += 1;
        }
    }
    if n_ta < 3 {
        bail!("Nie znaleziono tabel tekstów w grze ({n_ta} z 3)");
    }

    log("[2/4] Dialogi (Yarn)...".into());
    let mut done = 0;
    for (pack, want_visnov, prefix) in [(&yarn_pack, true, "yarn"), (&main, false, "yarn2")] {
        for (name, program, tables) in yarn_projects(pack, &reg)? {
            if name.starts_with("VisNov") != want_visnov || tables.len() < 7 {
                continue;
            }
            if tables.iter().any(|t| t.0 == "pl") {
                bail!("Dialogi w grze mają już polską tabelę - przywróć oryginalne pliki gry.");
            }
            for (lang, t) in &tables {
                write_json(&ex.join(format!("{prefix}_{lang}.json")), &table_json(t))?;
            }
            std::fs::write(ex.join(format!("{prefix}_program.bin")), &program)?;
            let scenes = yarn_scenes(&program);
            let ns = scenes.as_array().map_or(0, |a| a.len());
            write_json(&ex.join(format!("{prefix}_scenes.json")), &scenes)?;
            log(format!("  {name}: {} kwestii, {ns} scen", tables.iter().find(|t| t.0 == "en").map_or(0, |t| t.1.len())));
            done += 1;
            break;
        }
    }
    if done < 2 {
        bail!("Nie znaleziono obu projektów dialogów ({done} z 2)");
    }

    log("[3/4] Napisy okrzyków...".into());
    let suffix = Regex::new(r"^(.+?)_(es|fr|it|de|zh|ja)$").unwrap();
    let mut subs: Map<String, Value> = Map::new();
    for at in subs_pack.all().filter(|&at| subs_pack.class(at) == 49) {
        let v = subs_pack.read_obj(&reg, at)?;
        let n = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
        let (name, lang) = match suffix.captures(&n) {
            Some(c) => (c[1].to_string(), c[2].to_string()),
            None => (n.clone(), "en".to_string()),
        };
        let e = subs.entry(name).or_insert_with(|| json!({}));
        e[lang] = Value::String(text_of(v.get("m_Script").unwrap_or(&UV::Str(String::new()))));
    }
    write_json(&ex.join("subtitles.json"), &Value::Object(subs))?;

    if images {
        log("[4/4] Obrazki do podglądu scenek...".into());
        let n = vn_images(game, &reg, &root.join("cache").join("vn"))?;
        log(format!("  zapisano {n} obrazków"));
    }
    log("Gotowe: teksty wyciągnięte z gry.".into());
    Ok(())
}

/// Decode a Texture2D to top-down RGBA.
fn texture_rgba(pack: &Pack, tex: &UV) -> Result<image::RgbaImage> {
    let w = tex.get("m_Width").and_then(|x| x.i64()).unwrap_or(0) as usize;
    let h = tex.get("m_Height").and_then(|x| x.i64()).unwrap_or(0) as usize;
    let fmt = tex.get("m_TextureFormat").and_then(|x| x.i64()).unwrap_or(0);
    let sd = tex.get("m_StreamData");
    let size = sd.and_then(|s| s.get("size")).and_then(|x| x.i64()).unwrap_or(0) as usize;
    let data = if size > 0 {
        let off = sd.and_then(|s| s.get("offset")).and_then(|x| x.i64()).unwrap_or(0) as u64;
        let path = sd.and_then(|s| s.get("path")).and_then(|x| x.str()).unwrap_or("");
        pack.stream(path, off, size).ok_or_else(|| anyhow!("texture stream {path} not found"))?
    } else {
        match tex.get("image data") {
            Some(UV::Bytes(b)) => b.clone(),
            _ => bail!("texture without data"),
        }
    };
    let mut rgba = vec![0u8; w * h * 4];
    match fmt {
        1 => {
            for i in 0..w * h {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, data[i]]);
            }
        }
        3 => {
            for i in 0..w * h {
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[data[i * 3], data[i * 3 + 1], data[i * 3 + 2], 255]);
            }
        }
        4 => rgba.copy_from_slice(&data[..w * h * 4]),
        10 | 12 => {
            let mut px = vec![0u32; w * h];
            if fmt == 10 {
                texture2ddecoder::decode_bc1(&data, w, h, &mut px).map_err(|e| anyhow!("DXT1: {e}"))?;
            } else {
                texture2ddecoder::decode_bc3(&data, w, h, &mut px).map_err(|e| anyhow!("DXT5: {e}"))?;
            }
            for (i, p) in px.iter().enumerate() {
                // BGRA in a u32 (little endian)
                let [b, g, r, a] = p.to_le_bytes();
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, a]);
            }
        }
        f => bail!("texture format {f} not supported"),
    }
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| anyhow!("bad texture size"))?;
    Ok(image::imageops::flip_vertical(&img)) // Unity stores rows bottom-up
}

/// Backgrounds (sprites of the vn_* bundles) as JPG, characters (vn_characters*) as PNG.
fn vn_images(game: &Game, reg: &TypeRegistry, out: &Path) -> Result<usize> {
    let (out_bg, out_ch) = (out.join("bg"), out.join("chars"));
    std::fs::create_dir_all(&out_bg)?;
    std::fs::create_dir_all(&out_ch)?;
    let mut n = 0;
    for b in game.bundle("vn_") {
        let is_chars = b.file_name().unwrap().to_string_lossy().starts_with("vn_characters");
        let pack = Pack::read(&std::fs::read(&b)?)?;
        let mut cache: HashMap<(usize, usize), image::RgbaImage> = HashMap::new();
        for at in pack.all() {
            let class = pack.class(at);
            if !(class == 213 || (class == 28 && is_chars)) {
                continue; // backgrounds also come as sprites: only sprites there
            }
            let Ok(v) = pack.read_obj(reg, at) else { continue };
            let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
            let img = if class == 28 {
                match texture_rgba(&pack, &v) {
                    Ok(i) => i,
                    Err(_) => continue,
                }
            } else {
                // sprite: its texture, cut to the texture rect (bottom-up coordinates)
                let Some(tex_ptr) = v.at("m_RD.texture") else { continue };
                let Some(tat) = pack.resolve(at.file, tex_ptr) else { continue };
                if !cache.contains_key(&(tat.file, tat.obj)) {
                    let Ok(tv) = pack.read_obj(reg, tat) else { continue };
                    let Ok(i) = texture_rgba(&pack, &tv) else { continue };
                    cache.insert((tat.file, tat.obj), i);
                }
                let full = &cache[&(tat.file, tat.obj)];
                let r = |k: &str| v.at(&format!("m_RD.textureRect.{k}")).and_then(|x| x.f64()).unwrap_or(0.0);
                let (x, y, w, h) = (r("x").round() as u32, r("y").round() as u32, r("width").round() as u32, r("height").round() as u32);
                if w == 0 || h == 0 || x + w > full.width() || y + h > full.height() {
                    continue;
                }
                image::imageops::crop_imm(full, x, full.height() - y - h, w, h).to_image()
            };
            if is_chars {
                let p = out_ch.join(format!("{name}.png"));
                if !p.exists() {
                    fit(&img, 900, 900).unwrap_or(img).save(&p)?;
                    n += 1;
                }
            } else {
                let t = fit(&img, 1280, 720).unwrap_or(img);
                let rgb = image::DynamicImage::ImageRgba8(t).to_rgb8();
                let mut f = std::io::BufWriter::new(std::fs::File::create(out_bg.join(format!("{name}.jpg")))?);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 85).encode_image(&rgb)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Shrink to fit in max_w x max_h keeping the aspect (never enlarge), like PIL's thumbnail.
fn fit(img: &image::RgbaImage, max_w: u32, max_h: u32) -> Option<image::RgbaImage> {
    let (w, h) = (img.width(), img.height());
    if w <= max_w && h <= max_h {
        return Some(img.clone());
    }
    let s = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
    let (nw, nh) = (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1));
    Some(image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3))
}
