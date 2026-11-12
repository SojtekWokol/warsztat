//! TextMeshPro-style SDF glyphs (render mode SDF8: coverage at 8x, exact distance transform,
//! averaged back to 1x). Metrics follow FreeType's unhinted metrics in TMP's GlyphMetrics units.
use ab_glyph_rasterizer::{point, Rasterizer};
use anyhow::{anyhow, Result};
use ttf_parser::{Face, OutlineBuilder};

const SS: usize = 8;

#[derive(Clone, Debug)]
pub struct Metrics {
    pub width: f64,
    pub height: f64,
    pub bearing_x: f64,
    pub bearing_y: f64,
    pub advance: f64,
}

pub struct Glyph {
    /// top-down rows, (height + 2*pad) x (width + 2*pad)
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u8>,
    pub met: Metrics,
}

/// FreeType keeps scaled metrics in 26.6 fixed point.
fn q64(x: f64) -> f64 {
    (x * 64.0).round() / 64.0
}

struct Outline {
    segs: Vec<Seg>,
    last: (f32, f32),
    start: (f32, f32),
}
enum Seg {
    Line((f32, f32), (f32, f32)),
    Quad((f32, f32), (f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32), (f32, f32)),
}
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = (x, y);
        self.start = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.segs.push(Seg::Line(self.last, (x, y)));
        self.last = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.segs.push(Seg::Quad(self.last, (x1, y1), (x, y)));
        self.last = (x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.segs.push(Seg::Cubic(self.last, (x1, y1), (x2, y2), (x, y)));
        self.last = (x, y);
    }
    fn close(&mut self) {
        if self.last != self.start {
            self.segs.push(Seg::Line(self.last, self.start));
        }
        self.last = self.start;
    }
}

pub fn render(font: &[u8], ch: char, point_size: f64, pad: usize) -> Result<Glyph> {
    let face = Face::parse(font, 0).map_err(|e| anyhow!("font: {e:?}"))?;
    let gid = face.glyph_index(ch).ok_or_else(|| anyhow!("font has no {ch}"))?;
    let scale = point_size / face.units_per_em() as f64;
    let mut ol = Outline { segs: vec![], last: (0.0, 0.0), start: (0.0, 0.0) };
    let bb = face.outline_glyph(gid, &mut ol).ok_or_else(|| anyhow!("no outline for {ch}"))?;
    // FreeType places the outline so its left edge is at the hmtx side bearing (a composite with
    // USE_MY_METRICS takes advance and bearing from that component)
    let mgid = my_metrics_glyph(&face, gid).unwrap_or(gid);
    let adv = face.glyph_hor_advance(mgid).unwrap_or(0) as f64;
    let lsb = face.glyph_hor_side_bearing(mgid).unwrap_or(bb.x_min) as f64;
    let shift = lsb - bb.x_min as f64;
    let x_min = q64((bb.x_min as f64 + shift) * scale);
    let x_max = q64((bb.x_max as f64 + shift) * scale);
    let y_min = q64(bb.y_min as f64 * scale);
    let y_max = q64(bb.y_max as f64 * scale);
    let met = Metrics { width: x_max - x_min, height: y_max - y_min, bearing_x: x_min, bearing_y: y_max, advance: q64(adv * scale) };
    let shift = shift as f32;

    let w = met.width.round() as usize;
    let h = met.height.round() as usize;
    let (out_w, out_h) = (w + 2 * pad, h + 2 * pad);
    let (cw, chh) = (out_w * SS, out_h * SS);
    // canvas origin in glyph space (SS units): x0 = (bearingX - pad)*SS, top = (bearingY + pad)*SS
    let ox = ((met.bearing_x - pad as f64) * SS as f64).round() as f32;
    let oy = ((met.bearing_y + pad as f64) * SS as f64).round() as f32;
    let s = (scale * SS as f64) as f32;
    let tr = |p: (f32, f32)| point((p.0 + shift) * s - ox, oy - p.1 * s);
    let mut ras = Rasterizer::new(cw, chh);
    for seg in &ol.segs {
        match *seg {
            Seg::Line(a, b) => ras.draw_line(tr(a), tr(b)),
            Seg::Quad(a, c, b) => ras.draw_quad(tr(a), tr(c), tr(b)),
            Seg::Cubic(a, c1, c2, b) => ras.draw_cubic(tr(a), tr(c1), tr(c2), tr(b)),
        }
    }
    let mut inside = vec![false; cw * chh];
    ras.for_each_pixel_2d(|x, y, v| {
        // FreeType 8-bit coverage, thresholded at 128 like the Python generator
        let c = (v.clamp(0.0, 1.0) * 255.0) as u32;
        inside[y as usize * cw + x as usize] = c >= 128;
    });
    let d_out = edt(cw, chh, |i| !inside[i]);
    let d_in = edt(cw, chh, |i| inside[i]);
    let mut pixels = vec![0u8; out_w * out_h];
    let ssf = SS as f64;
    for oy_ in 0..out_h {
        for ox_ in 0..out_w {
            let mut sum = 0.0;
            for dy in 0..SS {
                let row = (oy_ * SS + dy) * cw;
                for dx in 0..SS {
                    let i = row + ox_ * SS + dx;
                    sum += (d_in[i] - d_out[i]) / ssf;
                }
            }
            let dist = sum / (SS * SS) as f64;
            let a = (0.5 + dist / (2.0 * pad as f64)).clamp(0.0, 1.0);
            pixels[oy_ * out_w + ox_] = (a * 255.0 + 0.5) as u8;
        }
    }
    Ok(Glyph { w: out_w, h: out_h, pixels, met })
}

/// Exact Euclidean distance of every pixel to the nearest pixel where `feature` is false
/// (scipy's distance_transform_edt semantics: 0 where false). Felzenszwalb & Huttenlocher.
fn edt(w: usize, h: usize, feature: impl Fn(usize) -> bool) -> Vec<f64> {
    const INF: f64 = 1e20;
    let mut f: Vec<f64> = (0..w * h).map(|i| if feature(i) { INF } else { 0.0 }).collect();
    let n = w.max(h);
    let mut buf = vec![0.0; n];
    let mut out = vec![0.0; n];
    let mut v = vec![0usize; n];
    let mut z = vec![0.0; n + 1];
    // columns
    for x in 0..w {
        for y in 0..h {
            buf[y] = f[y * w + x];
        }
        dt1(&buf[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            f[y * w + x] = out[y];
        }
    }
    // rows
    for y in 0..h {
        buf[..w].copy_from_slice(&f[y * w..(y + 1) * w]);
        dt1(&buf[..w], &mut out[..w], &mut v, &mut z);
        f[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    f.iter().map(|x| x.sqrt()).collect()
}

fn dt1(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: replace
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f64::INFINITY;
            break;
        }
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        d[q] = dq * dq + f[p];
    }
}

/// The component of a composite glyph flagged USE_MY_METRICS, if any (TrueType glyf).
fn my_metrics_glyph(face: &Face, gid: ttf_parser::GlyphId) -> Option<ttf_parser::GlyphId> {
    let raw = face.raw_face();
    let head = raw.table(ttf_parser::Tag::from_bytes(b"head"))?;
    let loca = raw.table(ttf_parser::Tag::from_bytes(b"loca"))?;
    let glyf = raw.table(ttf_parser::Tag::from_bytes(b"glyf"))?;
    let long = i16::from_be_bytes([head[50], head[51]]) == 1;
    let i = gid.0 as usize;
    let (a, b) = if long {
        let r = |k: usize| -> Option<usize> { Some(u32::from_be_bytes(loca.get(k * 4..k * 4 + 4)?.try_into().ok()?) as usize) };
        (r(i)?, r(i + 1)?)
    } else {
        let r = |k: usize| Some(u16::from_be_bytes(loca.get(k * 2..k * 2 + 2)?.try_into().ok()?) as usize * 2);
        (r(i)?, r(i + 1)?)
    };
    let g = glyf.get(a..b)?;
    if g.len() < 10 || i16::from_be_bytes([g[0], g[1]]) >= 0 {
        return None; // simple glyph
    }
    let mut p = 10;
    loop {
        let flags = u16::from_be_bytes(g.get(p..p + 2)?.try_into().ok()?);
        let comp = u16::from_be_bytes(g.get(p + 2..p + 4)?.try_into().ok()?);
        if flags & 0x0200 != 0 {
            return Some(ttf_parser::GlyphId(comp));
        }
        p += 4 + if flags & 1 != 0 { 4 } else { 2 };
        p += if flags & 0x08 != 0 { 2 } else if flags & 0x40 != 0 { 4 } else if flags & 0x80 != 0 { 8 } else { 0 };
        if flags & 0x20 == 0 {
            return None;
        }
    }
}
