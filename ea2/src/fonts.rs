//! Polish letters in the game's static TextMeshPro SDF fonts: render the missing letters, place them
//! in free space of the atlas (doubling its height when full) and register glyph + character.
use crate::pack::{At, Pack};
use crate::sdf;
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use unity::{TypeRegistry, Value};

pub const POLISH: &str = "ąćęłńóśźżĄĆĘŁŃÓŚŹŻ";

/// Font asset name -> source font file (in the fonts folder).
pub const SOURCES: &[(&str, &str)] = &[
    ("Urbanist-Bold SDF", "Urbanist-ExtraBold.ttf"),
    ("Urbanist-SemiBold SDF", "Urbanist-SemiBold.ttf"),
    ("ChunkFive SDF", "ChunkFive-Regular.ttf"),
    ("Caveat-VariableFont_wght SDF", "Caveat-Bold.ttf"),
    ("Kalam-Bold SDF", "Kalam-Bold.ttf"),
    ("knewave SDF", "Knewave-PL.ttf"),
    ("Jersey10-Regular SDF", "Jersey10-Regular.ttf"),
];

pub struct FontPatcher {
    dir: PathBuf,
    files: HashMap<String, Vec<u8>>,
    cache: HashMap<(String, char, i64, usize), sdf::Glyph>,
    /// (font name, old height, new height) of atlases enlarged in the current pack
    pub grown: Vec<(String, i64, i64)>,
}

impl FontPatcher {
    pub fn new(dir: &Path) -> FontPatcher {
        FontPatcher { dir: dir.to_path_buf(), files: HashMap::new(), cache: HashMap::new(), grown: vec![] }
    }

    /// Font files given in memory (file name -> bytes), e.g. built into the installer.
    pub fn with_files(files: HashMap<String, Vec<u8>>) -> FontPatcher {
        FontPatcher { dir: PathBuf::new(), files, cache: HashMap::new(), grown: vec![] }
    }

    fn glyph(&mut self, file: &str, ch: char, pt: f64, pad: usize) -> Result<&sdf::Glyph> {
        let key = (file.to_string(), ch, (pt * 1000.0) as i64, pad);
        if !self.cache.contains_key(&key) {
            if !self.files.contains_key(file) {
                let p = self.dir.join(file);
                self.files.insert(file.to_string(), std::fs::read(&p).map_err(|e| anyhow!("{}: {e}", p.display()))?);
            }
            let g = sdf::render(&self.files[file], ch, pt, pad)?;
            self.cache.insert(key.clone(), g);
        }
        Ok(&self.cache[&key])
    }

    /// Patch every known font asset in the pack. Returns how many were changed.
    pub fn patch_pack(&mut self, pack: &mut Pack, reg: &TypeRegistry, log: &mut dyn FnMut(String)) -> Result<usize> {
        self.grown.clear();
        let mut n = 0;
        let fonts: Vec<At> = pack
            .all()
            .filter(|&at| pack.class(at) == 114)
            .filter(|&at| pack.sf(at.file).objects[at.obj].data.get(..120).map_or(false, |d| d.windows(4).any(|w| w == b" SDF")))
            .filter(|&at| pack.tree(reg, at).map_or(false, |t| t.child("m_GlyphTable").is_some()))
            .collect();
        for at in fonts {
            let mut v = pack.read_obj(reg, at)?;
            let name = v.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
            let Some(&(_, file)) = SOURCES.iter().find(|s| s.0 == name) else { continue };
            if self.inject(pack, reg, at, &mut v, &name, file, log)? {
                pack.write_obj(reg, at, &v)?;
                n += 1;
            }
        }
        self.fix_material_heights(pack, reg, log)?;
        Ok(n)
    }

    #[allow(clippy::too_many_arguments)]
    fn inject(&mut self, pack: &mut Pack, reg: &TypeRegistry, at: At, v: &mut Value, name: &str, file: &str, log: &mut dyn FnMut(String)) -> Result<bool> {
        let pad = v.get("m_AtlasPadding").and_then(|x| x.i64()).unwrap_or(9) as usize;
        let pt = v.at("m_FaceInfo.m_PointSize").and_then(|x| x.f64()).ok_or_else(|| anyhow!("{name}: no point size"))?;
        let have: Vec<i64> = v.get("m_CharacterTable").and_then(|t| t.array()).map(|a| a.iter().filter_map(|c| c.get("m_Unicode")?.i64()).collect()).unwrap_or_default();
        let todo: Vec<char> = POLISH.chars().filter(|c| !have.contains(&(*c as i64))).collect();
        if todo.is_empty() {
            return Ok(false);
        }
        let tex_ptr = v.get("m_AtlasTextures").and_then(|a| a.array()).and_then(|a| a.first()).cloned().ok_or_else(|| anyhow!("{name}: no atlas"))?;
        if tex_ptr.get("m_FileID").and_then(|x| x.i64()) != Some(0) {
            log(format!("  {name}: atlas in another file, skipped"));
            return Ok(false);
        }
        let tex_at = pack.resolve(at.file, &tex_ptr).ok_or_else(|| anyhow!("{name}: atlas not found"))?;
        let mut tex = pack.read_obj(reg, tex_at)?;
        let w = tex.get("m_Width").and_then(|x| x.i64()).unwrap_or(0) as usize;
        let h0 = tex.get("m_Height").and_then(|x| x.i64()).unwrap_or(0) as usize;
        let fmt = tex.get("m_TextureFormat").and_then(|x| x.i64()).unwrap_or(0);
        if fmt != 1 {
            bail!("{name}: atlas format {fmt}, expected Alpha8");
        }
        let raw = atlas_bytes(pack, &tex, w * h0)?;
        // work top-down (like an image), the texture stores rows bottom-up
        let mut alpha: Vec<u8> = (0..h0).rev().flat_map(|y| raw[y * w..(y + 1) * w].to_vec()).collect();
        let mut hgt = h0;
        let glyphs = v.get("m_GlyphTable").and_then(|t| t.array()).cloned().unwrap_or_default();
        let mut occ = occupancy(&alpha, w, hgt, &glyphs, pad);
        let mut next_index = glyphs.iter().filter_map(|g| g.get("m_Index")?.i64()).max().unwrap_or(0) + 1;
        let mut grown = false;
        for &ch in &todo {
            let g = self.glyph(file, ch, pt, pad)?;
            let (gw_, gh_) = (g.w, g.h);
            let pixels = g.pixels.clone();
            let met = g.met.clone();
            let spot = match find_empty_spot(&occ, w, hgt, gw_, gh_, 4) {
                Some(s) => s,
                None => {
                    if grown {
                        bail!("{name}: no free space left in atlas");
                    }
                    // double the height: new rows on top of the image = high y in TMP coordinates,
                    // so existing glyph rects stay valid
                    let mut a2 = vec![0u8; w * hgt];
                    a2.extend_from_slice(&alpha);
                    alpha = a2;
                    let mut o2 = vec![false; w * hgt];
                    o2.extend_from_slice(&occ);
                    occ = o2;
                    hgt *= 2;
                    grown = true;
                    if let Some(x) = v.get_mut("m_AtlasHeight") {
                        x.set_num(hgt as f64);
                    }
                    log(format!("  {name}: atlas full, height doubled to {hgt}"));
                    find_empty_spot(&occ, w, hgt, gw_, gh_, 4).ok_or_else(|| anyhow!("{name}: glyph does not fit"))?
                }
            };
            let (cy, cx) = spot;
            for y in 0..gh_ {
                for x in 0..gw_ {
                    alpha[(cy + y) * w + cx + x] = pixels[y * gw_ + x];
                    occ[(cy + y) * w + cx + x] = true;
                }
            }
            let (gw, gh) = (gw_ - 2 * pad, gh_ - 2 * pad);
            let rect_y = hgt - (cy + pad + gh);
            push_glyph(v, next_index, &met, cx + pad, rect_y, gw, gh)?;
            push_char(v, ch, next_index)?;
            next_index += 1;
        }
        // back to bottom-up rows, regenerate mips if the texture has them
        let raw_new: Vec<u8> = (0..hgt).rev().flat_map(|y| alpha[y * w..(y + 1) * w].to_vec()).collect();
        let mips = tex.get("m_MipCount").and_then(|x| x.i64()).unwrap_or(1).max(1) as usize;
        let data = with_mips(&raw_new, w, hgt, mips);
        set_texture(&mut tex, data, hgt)?;
        pack.write_obj(reg, tex_at, &tex)?;
        if grown {
            self.grown.push((name.to_string(), h0 as i64, hgt as i64));
        }
        log(format!("  {name}: added {}", todo.iter().collect::<String>()));
        Ok(true)
    }

    /// Every material of an enlarged atlas stores its height in _TextureHeight (used for outline and
    /// shadow offsets): match materials by font name prefix and the old height.
    fn fix_material_heights(&self, pack: &mut Pack, reg: &TypeRegistry, log: &mut dyn FnMut(String)) -> Result<()> {
        if self.grown.is_empty() {
            return Ok(());
        }
        let mats: Vec<At> = pack.all().filter(|&at| pack.class(at) == 21).collect();
        let mut n = 0;
        for at in mats {
            let Ok(mut m) = pack.read_obj(reg, at) else { continue };
            let mname = m.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_lowercase();
            let mut changed = false;
            for (font, old_h, new_h) in &self.grown {
                let prefix = font.to_lowercase();
                let prefix = prefix.split('-').next().unwrap().split(' ').next().unwrap().to_string();
                if !mname.starts_with(&prefix) {
                    continue;
                }
                if let Some(floats) = m.at_mut("m_SavedProperties.m_Floats").and_then(|f| f.array_mut()) {
                    for pair in floats.iter_mut() {
                        if pair.get("first").and_then(|x| x.str()) == Some("_TextureHeight") && pair.get("second").and_then(|x| x.f64()) == Some(*old_h as f64) {
                            pair.get_mut("second").unwrap().set_num(*new_h as f64);
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                pack.write_obj(reg, at, &m)?;
                n += 1;
            }
        }
        log(format!("  materials with atlas height fixed: {n}"));
        Ok(())
    }
}

fn atlas_bytes(pack: &Pack, tex: &Value, need: usize) -> Result<Vec<u8>> {
    let sd = tex.get("m_StreamData");
    let size = sd.and_then(|s| s.get("size")).and_then(|x| x.i64()).unwrap_or(0) as usize;
    let data = if size > 0 {
        let off = sd.and_then(|s| s.get("offset")).and_then(|x| x.i64()).unwrap_or(0) as u64;
        let path = sd.and_then(|s| s.get("path")).and_then(|x| x.str()).unwrap_or("");
        pack.stream(path, off, size).ok_or_else(|| anyhow!("atlas stream {path} not found"))?
    } else {
        match tex.get("image data") {
            Some(Value::Bytes(b)) => b.clone(),
            _ => bail!("atlas has no image data"),
        }
    };
    if data.len() < need {
        bail!("atlas data too short ({} < {need})", data.len());
    }
    Ok(data[..need].to_vec())
}

fn with_mips(base: &[u8], w: usize, h: usize, mips: usize) -> Vec<u8> {
    let mut out = base.to_vec();
    let (mut cw, mut ch, mut cur) = (w, h, base.to_vec());
    for _ in 1..mips {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let mut s = 0u32;
                for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let (sy, sx) = ((2 * y + dy).min(ch - 1), (2 * x + dx).min(cw - 1));
                    s += cur[sy * cw + sx] as u32;
                }
                next[y * nw + x] = ((s + 2) / 4) as u8;
            }
        }
        out.extend_from_slice(&next);
        cur = next;
        cw = nw;
        ch = nh;
    }
    out
}

/// Store the pixels inside the texture object (not in the shared .resS stream) with the new height.
fn set_texture(tex: &mut Value, data: Vec<u8>, height: usize) -> Result<()> {
    let len = data.len();
    tex.get_mut("m_Height").ok_or_else(|| anyhow!("no m_Height"))?.set_num(height as f64);
    if let Some(x) = tex.get_mut("m_CompleteImageSize") {
        x.set_num(len as f64);
    }
    *tex.get_mut("image data").ok_or_else(|| anyhow!("no image data field"))? = Value::Bytes(data);
    if let Some(sd) = tex.get_mut("m_StreamData") {
        if let Some(x) = sd.get_mut("offset") {
            x.set_num(0.0);
        }
        if let Some(x) = sd.get_mut("size") {
            x.set_num(0.0);
        }
        if let Some(x) = sd.get_mut("path") {
            *x = Value::Str(String::new());
        }
    }
    Ok(())
}

/// Pixels a new glyph must not use: anything drawn, plus every existing glyph rect grown by the
/// padding (TMP draws each quad over its padding, a neighbour there shows as thin lines).
fn occupancy(alpha: &[u8], w: usize, h: usize, glyphs: &[Value], pad: usize) -> Vec<bool> {
    let mut occ: Vec<bool> = alpha.iter().map(|&a| a > 0).collect();
    let pad = pad as i64;
    for g in glyphs {
        let Some(r) = g.get("m_GlyphRect") else { continue };
        let gi = |k: &str| r.get(k).and_then(|x| x.i64()).unwrap_or(0);
        let (rx, ry, rw, rh) = (gi("m_X"), gi("m_Y"), gi("m_Width"), gi("m_Height"));
        let top = h as i64 - (ry + rh) - pad;
        let (y0, y1) = (top.max(0), (top + rh + 2 * pad).max(0).min(h as i64));
        let (x0, x1) = ((rx - pad).max(0), (rx + rw + pad).max(0).min(w as i64));
        for y in y0..y1 {
            for x in x0..x1 {
                occ[y as usize * w + x as usize] = true;
            }
        }
    }
    occ
}

/// First top-left (row, col), scanning rows top-down, where a w x h box plus margin is free.
fn find_empty_spot(occ: &[bool], aw: usize, ah: usize, w: usize, h: usize, margin: usize) -> Option<(usize, usize)> {
    let (bw, bh) = (w + 2 * margin, h + 2 * margin);
    if bw > aw || bh > ah {
        return None;
    }
    // summed-area table with a zero row/column in front
    let sw = aw + 1;
    let mut sat = vec![0u32; sw * (ah + 1)];
    for y in 0..ah {
        let mut row = 0u32;
        for x in 0..aw {
            row += occ[y * aw + x] as u32;
            sat[(y + 1) * sw + x + 1] = sat[y * sw + x + 1] + row;
        }
    }
    for r in 0..=ah - bh {
        for c in 0..=aw - bw {
            let s = sat[(r + bh) * sw + c + bw] + sat[r * sw + c] - sat[r * sw + c + bw] - sat[(r + bh) * sw + c];
            if s == 0 {
                return Some((r + margin, c + margin));
            }
        }
    }
    None
}

fn set_field(v: &mut Value, path: &str, x: f64) {
    if let Some(f) = v.at_mut(path) {
        f.set_num(x);
    }
}

fn push_glyph(font: &mut Value, index: i64, met: &sdf::Metrics, x: usize, y: usize, w: usize, h: usize) -> Result<()> {
    let table = font.get_mut("m_GlyphTable").and_then(|t| t.array_mut()).ok_or_else(|| anyhow!("no glyph table"))?;
    let mut g = table.last().cloned().ok_or_else(|| anyhow!("empty glyph table"))?;
    set_field(&mut g, "m_Index", index as f64);
    set_field(&mut g, "m_Metrics.m_Width", met.width);
    set_field(&mut g, "m_Metrics.m_Height", met.height);
    set_field(&mut g, "m_Metrics.m_HorizontalBearingX", met.bearing_x);
    set_field(&mut g, "m_Metrics.m_HorizontalBearingY", met.bearing_y);
    set_field(&mut g, "m_Metrics.m_HorizontalAdvance", met.advance);
    set_field(&mut g, "m_GlyphRect.m_X", x as f64);
    set_field(&mut g, "m_GlyphRect.m_Y", y as f64);
    set_field(&mut g, "m_GlyphRect.m_Width", w as f64);
    set_field(&mut g, "m_GlyphRect.m_Height", h as f64);
    set_field(&mut g, "m_Scale", 1.0);
    set_field(&mut g, "m_AtlasIndex", 0.0);
    set_field(&mut g, "m_ClassDefinitionType", 0.0);
    table.push(g);
    Ok(())
}

fn push_char(font: &mut Value, ch: char, glyph: i64) -> Result<()> {
    let table = font.get_mut("m_CharacterTable").and_then(|t| t.array_mut()).ok_or_else(|| anyhow!("no character table"))?;
    let mut c = table.last().cloned().ok_or_else(|| anyhow!("empty character table"))?;
    set_field(&mut c, "m_ElementType", 1.0);
    set_field(&mut c, "m_Unicode", ch as u32 as f64);
    set_field(&mut c, "m_GlyphIndex", glyph as f64);
    set_field(&mut c, "m_Scale", 1.0);
    table.push(c);
    Ok(())
}
