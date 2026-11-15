//! Escape Academy (part 1): reading the game's texts from `Escape Academy_Data/data.unity3d`.
//!
//! Unity 2020.3, Mono build, one UnityFS bundle with 88 serialized files, type trees stripped.
//! Almost all text is in TextAssets (a built-in class whose layout is fixed: m_Name, m_Script), so
//! the few MonoBehaviours we need (YarnProgram) use hand-written layouts - see `yarn_program`.
pub mod extract;
pub mod il;
pub mod testlang;

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use unity::io::Reader;
use unity::{Bundle, SerializedFile, NODE_SERIALIZED};

pub const DEFAULT_GAME: &str = r"C:\Users\grejs\Epic Games\EscapeAcademyOcy7i";

pub struct Game {
    pub dir: PathBuf,
}

impl Game {
    pub fn data_file(&self) -> PathBuf {
        self.dir.join("Escape Academy_Data").join("data.unity3d")
    }
    pub fn managed(&self) -> PathBuf {
        self.dir.join("Escape Academy_Data").join("Managed")
    }
    /// Addressables bundles (StreamingAssets/aa): the campus (metagame), its Yarn programs, the
    /// LanguageManager and more - these keep their type trees.
    pub fn aa_bundles(&self) -> Vec<PathBuf> {
        let dir = self.dir.join("Escape Academy_Data").join("StreamingAssets").join("aa").join("StandaloneWindows64");
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "bundle")).collect()).unwrap_or_default();
        v.sort();
        v
    }
    /// Every bundle with game data: data.unity3d first, then the Addressables bundles.
    pub fn bundles(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_file()];
        v.extend(self.aa_bundles());
        v
    }
    pub fn is_valid(&self) -> bool {
        self.data_file().is_file() && self.managed().join("Assembly-CSharp.dll").is_file()
    }
}

/// Every serialized file of the bundle, by node name, plus MonoScript names to tell MonoBehaviours apart.
pub struct Files {
    pub files: Vec<(String, SerializedFile)>,
    /// bundle file of each entry of `files` (index into `bundles`)
    pub bundle_of: Vec<usize>,
    pub bundles: Vec<PathBuf>,
    /// (file name, path id) of a MonoScript -> "Namespace.Class"
    scripts: HashMap<(String, i64), String>,
}

pub fn base_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// Length-prefixed, 4-aligned string (Unity's serialized string).
pub fn rd_str(r: &mut Reader) -> Result<String> {
    let n = r.i32()?;
    if n < 0 || n as usize > r.data.len() - r.pos {
        bail!("bad string length {n} at {}", r.pos - 4);
    }
    let s = String::from_utf8_lossy(r.take(n as usize)?).into_owned();
    r.align(4);
    Ok(s)
}

/// TextAsset (class 49): name and contents.
pub fn text_asset(data: &[u8]) -> Result<(String, Vec<u8>)> {
    let mut r = Reader::new(data, false);
    let name = rd_str(&mut r)?;
    let n = r.i32()? as usize;
    Ok((name, r.take(n)?.to_vec()))
}

/// A PPtr: file id (0 = same file, else 1 + index into externals) and path id.
#[derive(Clone, Copy, Debug)]
pub struct PPtr {
    pub file: i32,
    pub path: i64,
}

fn rd_pptr(r: &mut Reader) -> Result<PPtr> {
    Ok(PPtr { file: r.i32()?, path: r.i64()? })
}

/// MonoBehaviour start (GameObject, enabled, script, name); the reader is left at the script's own fields.
pub fn mono_start<'a>(data: &'a [u8]) -> Result<(Reader<'a>, PPtr, String)> {
    let mut r = Reader::new(data, false);
    rd_pptr(&mut r)?;
    r.u8()?;
    r.align(4);
    let script = rd_pptr(&mut r)?;
    let name = rd_str(&mut r)?;
    Ok((r, script, name))
}

/// YarnSpinner 1.x `YarnProgram` (a ScriptableObject):
/// compiledProgram byte[], baseLocalisationStringTable PPtr<TextAsset>, baseLocalizationId string,
/// localizations YarnTranslation[] { languageName string, text PPtr<TextAsset> }.
pub struct YarnProgram {
    pub name: String,
    pub program: Vec<u8>,
    pub base_table: PPtr,
    pub base_id: String,
    pub localizations: Vec<(String, PPtr)>,
}

pub fn yarn_program(data: &[u8]) -> Result<YarnProgram> {
    let (mut r, _, name) = mono_start(data)?;
    let n = r.i32()? as usize;
    let program = r.take(n)?.to_vec();
    r.align(4);
    let base_table = rd_pptr(&mut r)?;
    let base_id = rd_str(&mut r)?;
    let count = r.i32()?;
    if !(0..100).contains(&count) {
        bail!("bad YarnProgram layout ({count} localizations)");
    }
    let mut localizations = Vec::new();
    for _ in 0..count {
        let lang = rd_str(&mut r)?;
        localizations.push((lang, rd_pptr(&mut r)?));
    }
    if r.pos != data.len() {
        bail!("YarnProgram {name}: {} bytes left after the known fields", data.len() - r.pos);
    }
    Ok(YarnProgram { name, program, base_table, base_id, localizations })
}

impl Files {
    pub fn load(game: &Game) -> Result<Files> {
        Files::load_bundles(&game.bundles())
    }
    pub fn load_bundles(paths: &[PathBuf]) -> Result<Files> {
        let mut files = Vec::new();
        let mut bundle_of = Vec::new();
        for (bi, path) in paths.iter().enumerate() {
            let file = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            let b = Bundle::read(&file)?;
            drop(file);
            for n in b.nodes.into_iter().filter(|n| n.flags & NODE_SERIALIZED != 0) {
                let sf = SerializedFile::read(&n.data).map_err(|e| anyhow::anyhow!("{}: {e}", n.path))?;
                files.push((n.path, sf));
                bundle_of.push(bi);
            }
        }
        let mut scripts = HashMap::new();
        for (fname, sf) in &files {
            for o in sf.objects.iter().filter(|o| o.class_id == 115) {
                let mut r = Reader::new(&o.data, false);
                let Ok(name) = rd_str(&mut r) else { continue };
                r.take(4 + 16).ok();
                let _class = rd_str(&mut r).unwrap_or_default();
                let ns = rd_str(&mut r).unwrap_or_default();
                let full = if ns.is_empty() { name } else { format!("{ns}.{name}") };
                scripts.insert((base_name(fname).to_lowercase(), o.path_id), full);
            }
        }
        Ok(Files { files, bundle_of, bundles: paths.to_vec(), scripts })
    }

    /// Every MonoScript as "Namespace.Class".
    pub fn script_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.scripts.values().cloned().collect();
        v.sort();
        v.dedup();
        v
    }

    /// The file a PPtr in file `from` points into.
    pub fn target_file(&self, from: usize, p: PPtr) -> Option<usize> {
        if p.file == 0 {
            return Some(from);
        }
        let ext = self.files[from].1.externals.get(p.file as usize - 1)?;
        let want = base_name(&ext.path).to_lowercase();
        self.files.iter().position(|(n, _)| base_name(n).to_lowercase() == want)
    }

    pub fn deref(&self, from: usize, p: PPtr) -> Option<(usize, &unity::serialized::Object)> {
        let f = self.target_file(from, p)?;
        Some((f, self.files[f].1.object(p.path)?))
    }

    /// Full class name of a MonoBehaviour's script.
    pub fn script_of(&self, file: usize, data: &[u8]) -> Option<String> {
        let (_, script, _) = mono_start(data).ok()?;
        let f = self.target_file(file, script)?;
        self.scripts.get(&(base_name(&self.files[f].0).to_lowercase(), script.path)).cloned()
    }
}

/// Readable strings inside raw object data (aligned, length-prefixed, with a space and mostly letters):
/// a type-tree-free way to see which components carry text.
pub fn strings_in(data: &[u8], from: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut p = from;
    while p + 4 <= data.len() {
        let n = i32::from_le_bytes(data[p..p + 4].try_into().unwrap());
        if (3..20000).contains(&n) && p + 4 + n as usize <= data.len() {
            let b = &data[p + 4..p + 4 + n as usize];
            if let Ok(s) = std::str::from_utf8(b) {
                let letters = s.chars().filter(|c| c.is_alphabetic() || *c == ' ').count();
                if s.contains(' ') && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t') && letters * 10 >= s.chars().count() * 7 {
                    out.push(s.to_string());
                    p = (p + 4 + n as usize + 3) / 4 * 4;
                    continue;
                }
            }
        }
        p += 4;
    }
    out
}

fn skip_unity_event(r: &mut Reader) -> Result<()> {
    let n = r.i32()?;
    if !(0..1000).contains(&n) {
        bail!("bad UnityEvent call count {n}");
    }
    for _ in 0..n {
        rd_pptr(r)?; // m_Target
        rd_str(r)?; // m_TargetAssemblyTypeName
        rd_str(r)?; // m_MethodName
        r.i32()?; // m_Mode
        rd_pptr(r)?; // m_Arguments.m_ObjectArgument
        rd_str(r)?; // m_ObjectArgumentAssemblyTypeName
        r.i32()?; // m_IntArgument
        r.f32()?; // m_FloatArgument
        rd_str(r)?; // m_StringArgument
        r.u8()?; // m_BoolArgument
        r.align(4);
        r.i32()?; // m_CallState
    }
    Ok(())
}

/// A TextMeshPro / TextMeshProUGUI component (TMP 3.x on uGUI 1.0): its text and font asset.
pub struct TmpText {
    pub game_object: PPtr,
    pub text: String,
    pub font: PPtr,
    /// byte range of m_text (length prefix included) inside the object data, for writing back
    pub text_range: (usize, usize),
}

/// Graphic (m_Material, m_Color, m_RaycastTarget, m_RaycastPadding), MaskableGraphic (m_Maskable,
/// m_OnCullStateChanged), then TMP_Text.m_text, m_isRightToLeft, m_fontAsset. m_RaycastPadding is
/// only in some uGUI versions, so both variants are tried; the right one ends in a sane font PPtr.
pub fn tmp_text(data: &[u8]) -> Result<TmpText> {
    let mut r = Reader::new(data, false);
    let game_object = rd_pptr(&mut r)?;
    r.u8()?;
    r.align(4);
    rd_pptr(&mut r)?;
    rd_str(&mut r)?;
    let after_name = r.pos;
    for padding in [true, false] {
        let mut r = Reader::new(data, false);
        r.pos = after_name;
        let res = (|| -> Result<TmpText> {
            rd_pptr(&mut r)?; // m_Material
            r.take(16)?; // m_Color
            r.u8()?; // m_RaycastTarget
            r.align(4);
            if padding {
                r.take(16)?;
            }
            r.u8()?; // m_Maskable
            r.align(4);
            skip_unity_event(&mut r)?;
            let start = r.pos;
            let text = rd_str(&mut r)?;
            let end = r.pos;
            r.u8()?; // m_isRightToLeft
            r.align(4);
            let font = rd_pptr(&mut r)?;
            if !(0..64).contains(&font.file) || font.path == 0 && font.file != 0 {
                bail!("not a font PPtr");
            }
            Ok(TmpText { game_object, text, font, text_range: (start, end) })
        })();
        if res.is_ok() {
            return res;
        }
    }
    bail!("unknown TextMeshPro layout")
}

/// GameObject (class 1): components and name.
pub fn game_object(data: &[u8]) -> Result<(Vec<PPtr>, String)> {
    let mut r = Reader::new(data, false);
    let n = r.i32()?;
    if !(0..10000).contains(&n) {
        bail!("bad component count");
    }
    let mut comps = Vec::new();
    for _ in 0..n {
        comps.push(rd_pptr(&mut r)?);
    }
    r.i32()?; // m_Layer
    Ok((comps, rd_str(&mut r)?))
}

pub const POLISH_LETTERS: &str = "ĄĆĘŁŃÓŚŹŻąćęłńóśźż";

/// What a TMP_FontAsset can show: its character table, and whether it may add glyphs at run time.
pub struct TmpFont {
    pub name: String,
    pub version: String,
    pub source_font: PPtr,
    /// m_AtlasPopulationMode: 0 static, 1 dynamic (glyphs rendered from the source font when needed)
    pub population_mode: i32,
    pub chars: Vec<u32>,
}

/// TMP 3.x font asset: TMP_Asset fields, then m_Version, m_SourceFontFileGUID, m_SourceFontFile, m_AtlasPopulationMode
/// right after the name; the character table (records of element type 1, unicode, glyph index,
/// scale 1.0) is found by its shape instead of walking the face info and glyph table.
pub fn tmp_font(data: &[u8]) -> Result<TmpFont> {
    let (r0, _, name) = mono_start(data)?;
    let mut r = Reader::new(data, false);
    r.pos = r0.pos;
    let head = (|| -> Result<(String, PPtr, i32)> {
        r.take(4 + 12 + 4)?; // TMP_Asset: hashCode, material, materialHashCode
        let version = rd_str(&mut r)?;
        if !version.starts_with(|c: char| c.is_ascii_digit()) {
            bail!("no m_Version");
        }
        let _guid = rd_str(&mut r)?;
        Ok((version, rd_pptr(&mut r)?, r.i32()?))
    })();
    // older font assets (TMP 1.x layout) have no version: unknown mode, table search only
    let (version, source_font, population_mode) = head.unwrap_or_else(|_| {
        r.pos = r0.pos;
        ("?".into(), PPtr { file: 0, path: 0 }, -1)
    });
    let mut best: Vec<u32> = Vec::new();
    let mut p = r.pos;
    while p + 4 <= data.len() {
        let n = u32::from_le_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        let rec = |i: usize| -> Option<u32> {
            let o = p + 4 + i * 16;
            let b = data.get(o..o + 16)?;
            let w = |k: usize| u32::from_le_bytes(b[k * 4..k * 4 + 4].try_into().unwrap());
            (w(0) == 1 && w(3) == 1.0f32.to_bits()).then(|| w(1))
        };
        if n > best.len() && n < 100_000 && p + 4 + n * 16 <= data.len() && (0..n).all(|i| rec(i).is_some()) {
            best = (0..n).map(|i| rec(i).unwrap()).collect();
            p += 4 + n * 16;
            continue;
        }
        p += 4;
    }
    Ok(TmpFont { name, version, source_font, population_mode, chars: best })
}

/// An embedded TrueType/OpenType font inside a Font object (class 128): the first offset where a
/// font parses.
pub fn find_ttf(data: &[u8]) -> Option<&[u8]> {
    (0..data.len().saturating_sub(12)).step_by(4).find_map(|p| {
        let tag = &data[p..p + 4];
        if tag != [0, 1, 0, 0] && tag != b"OTTO" && tag != b"true" {
            return None;
        }
        ttf_parser::Face::parse(&data[p..], 0).ok().map(|_| &data[p..])
    })
}

/// One entry of LanguageManager.languages.
#[derive(Debug, Clone)]
pub struct Language {
    /// Escape.Localization.LanguageType: 0 English, 1 Spanish, 2 German, 3 French (no other values)
    pub kind: i32,
    pub display_name: String,
    pub code: String,
    /// column of GameplayStrings*.csv (StringReplacementManager reads the text from this column)
    pub csv_index: i32,
}

/// Escape.Localization.LanguageManager: currentLanguage, languages[], defaultLanguage,
/// overrideLanguageInEditor, setLanguageInEditor.
pub fn language_manager(data: &[u8]) -> Result<(Vec<Language>, i32)> {
    let (mut r, _, _) = mono_start(data)?;
    let lang = |r: &mut Reader| -> Result<Language> { Ok(Language { kind: r.i32()?, display_name: rd_str(r)?, code: rd_str(r)?, csv_index: r.i32()? }) };
    lang(&mut r)?;
    let n = r.i32()?;
    if !(0..64).contains(&n) {
        bail!("bad LanguageManager layout ({n} languages)");
    }
    let langs = (0..n).map(|_| lang(&mut r)).collect::<Result<Vec<_>>>()?;
    let default = r.i32()?;
    r.u8()?;
    r.align(4);
    r.i32()?;
    if r.pos != data.len() {
        bail!("LanguageManager: {} bytes left after the known fields", data.len() - r.pos);
    }
    Ok((langs, default))
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as i32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// TextAsset object data.
pub fn text_asset_data(name: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    put_str(&mut out, name);
    out.extend_from_slice(&(body.len() as i32).to_le_bytes());
    out.extend_from_slice(body);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// YarnProgram data with one more YarnTranslation (localizations is the last field).
pub fn yarn_program_add(data: &[u8], lang: &str, table: PPtr) -> Result<Vec<u8>> {
    let p = yarn_program(data)?;
    if p.localizations.iter().any(|l| l.0 == lang) {
        bail!("YarnProgram {} already has {lang}", p.name);
    }
    // the count sits right before the entries: walk back from the end over the entries
    let entries: usize = p.localizations.iter().map(|(l, _)| 4 + (l.len() + 3) / 4 * 4 + 12).sum();
    let count_at = data.len() - entries - 4;
    let mut out = data[..count_at].to_vec();
    out.extend_from_slice(&((p.localizations.len() + 1) as i32).to_le_bytes());
    out.extend_from_slice(&data[count_at + 4..]);
    put_str(&mut out, lang);
    out.extend_from_slice(&table.file.to_le_bytes());
    out.extend_from_slice(&table.path.to_le_bytes());
    yarn_program(&out)?;
    Ok(out)
}

/// LanguageManager data with one more language at the end of `languages`.
pub fn language_manager_add(data: &[u8], l: &Language) -> Result<Vec<u8>> {
    let (langs, _) = language_manager(data)?;
    if langs.iter().any(|x| x.code == l.code) {
        bail!("LanguageManager already has {}", l.code);
    }
    // header, currentLanguage, then count: the count is at the end of currentLanguage
    let (r, _, _) = mono_start(data)?;
    let mut rr = Reader::new(data, false);
    rr.pos = r.pos;
    rr.i32()?;
    rd_str(&mut rr)?;
    rd_str(&mut rr)?;
    rr.i32()?;
    let count_at = rr.pos;
    let entries_end = count_at + 4 + langs.iter().map(|x| 4 + 4 + (x.display_name.len() + 3) / 4 * 4 + 4 + (x.code.len() + 3) / 4 * 4 + 4).sum::<usize>();
    let mut out = data[..count_at].to_vec();
    out.extend_from_slice(&((langs.len() + 1) as i32).to_le_bytes());
    out.extend_from_slice(&data[count_at + 4..entries_end]);
    out.extend_from_slice(&l.kind.to_le_bytes());
    put_str(&mut out, &l.display_name);
    put_str(&mut out, &l.code);
    out.extend_from_slice(&l.csv_index.to_le_bytes());
    out.extend_from_slice(&data[entries_end..]);
    let (check, _) = language_manager(&out)?;
    if check.last().map(|x| &x.code) != Some(&l.code) {
        bail!("LanguageManager rewrite failed");
    }
    Ok(out)
}
