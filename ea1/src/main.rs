use anyhow::{bail, Result};
use ea1::{Files, Game, DEFAULT_GAME};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The repository root: walk up from the current directory to the one with translations/.
fn project_root() -> PathBuf {
    let mut d = std::env::current_dir().unwrap();
    loop {
        if d.join("translations").is_dir() {
            return d;
        }
        if !d.pop() {
            return std::env::current_dir().unwrap();
        }
    }
}

/// Backups of one game installation: ea1/backup/<folder id> (or --backup DIR).
fn backup_dir(root: &std::path::Path, game: &Game, args: &[String]) -> PathBuf {
    arg(args, "--backup").map(PathBuf::from).unwrap_or_else(|| root.join("backup").join(kopie::folder_id(&game.dir)))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let game = Game { dir: PathBuf::from(arg(&args, "--game").unwrap_or(DEFAULT_GAME.into())) };
    if args.get(1).map(|s| s.as_str()) != Some("crc") && !game.is_valid() {
        bail!("To nie jest folder gry Escape Academy: {}", game.dir.display());
    }
    match args.get(1).map(|s| s.as_str()) {
        Some("scan") => {
            // which MonoBehaviour scripts carry readable strings (no type trees: a heuristic)
            let files = Files::load(&game)?;
            let mut by: BTreeMap<String, (usize, usize, usize, Vec<String>)> = BTreeMap::new();
            for (fi, (_, sf)) in files.files.iter().enumerate() {
                for o in sf.objects.iter().filter(|o| o.class_id == 114) {
                    let script = files.script_of(fi, &o.data).unwrap_or_else(|| "?".into());
                    let Ok((r, _, _)) = ea1::mono_start(&o.data) else { continue };
                    let strs = ea1::strings_in(&o.data, r.pos);
                    let e = by.entry(script).or_default();
                    e.0 += 1;
                    e.1 += !strs.is_empty() as usize;
                    e.2 += strs.len();
                    for s in strs {
                        if e.3.len() < 4 && !e.3.contains(&s) {
                            e.3.push(s);
                        }
                    }
                }
            }
            let mut v: Vec<_> = by.into_iter().filter(|e| e.1 .2 > 0).collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.1 .2));
            for (s, (n, with, cnt, samples)) in v {
                let sm: Vec<String> = samples.iter().map(|x| x.chars().take(60).collect::<String>().replace('\n', "\n")).collect();
                println!("{cnt:>6} strings in {with:>5}/{n:<5} {s}  | {}", sm.join(" | "));
            }
            Ok(())
        }
        Some("tmpscan") => {
            // TextMeshPro components: does the layout parse, which scripts sit next to them
            let files = Files::load(&game)?;
            let mut keys = std::collections::HashSet::new();
            for (_, sf) in &files.files {
                for o in sf.objects.iter().filter(|o| o.class_id == 49) {
                    let (name, body) = ea1::text_asset(&o.data)?;
                    if name.starts_with("GameplayStrings") {
                        for line in String::from_utf8_lossy(&body).lines() {
                            keys.insert(line.split(',').next().unwrap_or("").to_string());
                        }
                    }
                }
            }
            let (mut ok, mut bad, mut nonempty, mut is_key, mut font_ok) = (0, 0, 0, 0, 0);
            let mut neighbours: BTreeMap<String, usize> = BTreeMap::new();
            for (fi, (_, sf)) in files.files.iter().enumerate() {
                for o in sf.objects.iter().filter(|o| o.class_id == 114) {
                    let Some(script) = files.script_of(fi, &o.data) else { continue };
                    if script != "TMPro.TextMeshPro" && script != "TMPro.TextMeshProUGUI" {
                        continue;
                    }
                    let t = match ea1::tmp_text(&o.data) {
                        Ok(t) => t,
                        Err(_) => {
                            bad += 1;
                            continue;
                        }
                    };
                    ok += 1;
                    if files.deref(fi, t.font).map_or(false, |(f, fo)| files.script_of(f, &fo.data).as_deref() == Some("TMPro.TMP_FontAsset")) {
                        font_ok += 1;
                    }
                    if t.text.trim().is_empty() {
                        continue;
                    }
                    nonempty += 1;
                    is_key += keys.contains(t.text.trim()) as usize;
                    if let Some((_, go)) = files.deref(fi, t.game_object) {
                        if let Ok((comps, _)) = ea1::game_object(&go.data) {
                            for c in comps {
                                if let Some((cf, co)) = files.deref(fi, c) {
                                    if co.class_id == 114 {
                                        *neighbours.entry(files.script_of(cf, &co.data).unwrap_or("?".into())).or_default() += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            println!("TMP parsed {ok}, failed {bad}, font PPtr verified {font_ok}, non-empty {nonempty}, text is a GameplayStrings key {is_key}");
            let mut v: Vec<_> = neighbours.into_iter().collect();
            v.sort_by_key(|e| std::cmp::Reverse(e.1));
            for (s, n) in v.iter().take(40) {
                println!("{n:>6} {s}");
            }
            Ok(())
        }
        Some("fonts") => {
            // TMP font assets and source fonts: are the Polish letters there?
            let files = Files::load(&game)?;
            let pl: Vec<char> = ea1::POLISH_LETTERS.chars().collect();
            let missing = |has: &dyn Fn(char) -> bool| pl.iter().filter(|c| !has(**c)).collect::<String>();
            let mut seen = std::collections::HashSet::new();
            let mut used: BTreeMap<String, usize> = BTreeMap::new();
            for (fi, (_, sf)) in files.files.iter().enumerate() {
                for o in &sf.objects {
                    let script = if o.class_id == 114 { files.script_of(fi, &o.data) } else { None };
                    if matches!(script.as_deref(), Some("TMPro.TextMeshPro" | "TMPro.TextMeshProUGUI")) {
                        if let Ok(t) = ea1::tmp_text(&o.data) {
                            let name = files.deref(fi, t.font).and_then(|(_, fo)| ea1::mono_start(&fo.data).ok()).map(|x| x.2).unwrap_or("?".into());
                            *used.entry(name).or_default() += 1;
                        }
                    }
                    if script.as_deref() == Some("TMPro.TMP_FontAsset") {
                        let f = ea1::tmp_font(&o.data)?;
                        if !seen.insert(format!("tmp {}", f.name)) {
                            continue;
                        }
                        let src = files.deref(fi, f.source_font).and_then(|(_, so)| ea1::rd_str(&mut unity::io::Reader::new(&so.data, false)).ok());
                        let m = missing(&|c| f.chars.contains(&(c as u32)));
                        println!("TMP  {:<45} v{} mode={} chars={:<5} source={:<30} missing PL: {}", f.name, f.version, f.population_mode, f.chars.len(), src.unwrap_or("-".into()), if m.is_empty() { "none".into() } else { m });
                    } else if o.class_id == 128 {
                        let name = ea1::rd_str(&mut unity::io::Reader::new(&o.data, false)).unwrap_or_default();
                        if !seen.insert(format!("font {name}")) {
                            continue;
                        }
                        match ea1::find_ttf(&o.data).and_then(|d| ttf_parser::Face::parse(d, 0).ok()) {
                            Some(face) => {
                                let m = missing(&|c| face.glyph_index(c).is_some());
                                println!("FONT {:<45} glyphs={:<5} missing PL: {}", name, face.number_of_glyphs(), if m.is_empty() { "none".into() } else { m });
                            }
                            None => println!("FONT {name:<45} (no embedded font data)"),
                        }
                    }
                }
            }
            println!("TextMeshPro components per font asset:");
            for (n, c) in &used {
                println!("{c:>6} {n}");
            }
            Ok(())
        }
        Some("dump") => {
            // raw data of every MonoBehaviour whose script name contains the filter (hex + strings)
            let files = Files::load(&game)?;
            let filter = args.get(2).cloned().unwrap_or_default();
            for (fi, (fname, sf)) in files.files.iter().enumerate() {
                for o in sf.objects.iter().filter(|o| o.class_id == 114) {
                    let script = files.script_of(fi, &o.data).unwrap_or_else(|| "?".into());
                    if !script.contains(&filter) {
                        continue;
                    }
                    let (_, _, name) = ea1::mono_start(&o.data)?;
                    println!("== {script} '{name}' in {fname} #{} ({} bytes)", o.path_id, o.data.len());
                    for (i, chunk) in o.data.chunks(16).enumerate().take(64) {
                        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                        let txt: String = chunk.iter().map(|&b| if (32..127).contains(&b) { b as char } else { '.' }).collect();
                        println!("{:06x}  {:<48} {txt}", i * 16, hex.join(" "));
                    }
                }
            }
            Ok(())
        }
        Some("find") => {
            // objects whose raw data contains a UTF-8 string
            let files = Files::load(&game)?;
            let needle = args.get(2).cloned().unwrap_or_default().into_bytes();
            for (fi, (fname, sf)) in files.files.iter().enumerate() {
                for o in &sf.objects {
                    if let Some(at) = o.data.windows(needle.len()).position(|w| w == needle.as_slice()) {
                        let script = if o.class_id == 114 { files.script_of(fi, &o.data).unwrap_or("?".into()) } else { String::new() };
                        println!("{fname} #{} class {} {script} at {at} ({} bytes)", o.path_id, o.class_id, o.data.len());
                    }
                }
            }
            Ok(())
        }
        Some("scripts") => {
            // all MonoScripts (full class names), optionally filtered by a substring
            let files = Files::load(&game)?;
            let filter = args.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
            for s in files.script_names().into_iter().filter(|s| s.to_lowercase().contains(&filter)) {
                println!("{s}");
            }
            Ok(())
        }
        Some("roundtrip") => {
            // can every serialized file be written back byte for byte (the base of patching)?
            let files = Files::load(&game)?;
            let bundles: Vec<unity::Bundle> = files.bundles.iter().map(|p| unity::Bundle::read(&std::fs::read(p)?)).collect::<Result<_>>()?;
            let (mut same, mut diff) = (0, 0);
            for (i, (name, sf)) in files.files.iter().enumerate() {
                let orig = &bundles[files.bundle_of[i]].nodes.iter().find(|n| &n.path == name).unwrap().data;
                if sf.write() == *orig {
                    same += 1;
                } else {
                    diff += 1;
                    let w = sf.write();
                    let at = w.iter().zip(orig.iter()).position(|(a, b)| a != b).unwrap_or(w.len().min(orig.len()));
                    if diff <= 5 {
                        println!("differs: {name} (sizes {} -> {}, first difference at {at})", orig.len(), w.len());
                    }
                }
            }
            println!("{same} files identical after write, {diff} differ");
            Ok(())
        }
        Some("crc") => {
            // CRC32 of a bundle's uncompressed data (what the Addressables catalog stores)
            let Some(path) = args.get(2) else { bail!("bundle path required") };
            let b = unity::Bundle::read(&std::fs::read(path)?)?;
            let mut h = crc32fast::Hasher::new();
            for n in &b.nodes {
                h.update(&n.data);
            }
            println!("{}", h.finalize());
            Ok(())
        }
        Some("ilcheck") => {
            // is Assembly-CSharp.dll the original, our patched one, or unknown?
            let dll = std::fs::read(game.managed().join("Assembly-CSharp.dll"))?;
            println!("{:?}", ea1::il::state(&dll)?);
            Ok(())
        }
        Some("testlang") => {
            // test build of the fifth language ([PL]-marked English) into --out (never into the game)
            let Some(out) = arg(&args, "--out") else { bail!("--out FOLDER is required") };
            // --pl FILE: finished Polish Yarn lines {line id: text} instead of the marked English
            let polish: std::collections::HashMap<String, String> = match arg(&args, "--pl") {
                Some(p) => serde_json::from_str(&std::fs::read_to_string(&p)?)?,
                None => Default::default(),
            };
            let fonts = arg(&args, "--fonts").map(PathBuf::from).unwrap_or_else(|| project_root().join("fonts"));
            ea1::testlang::build_test(&game, std::path::Path::new(&out), &polish, &fonts, &mut |m| println!("{m}"))
        }
        Some("patch") => {
            // Polish from ea1/translations into the game (originals backed up in ea1/backup/<game id>)
            let root = ea1::project_root().ok_or_else(|| anyhow::anyhow!("no ea1/translations folder found"))?;
            let store = warsztat::store::read_texts(&root.join("translations").join("pl"))?;
            let fonts = arg(&args, "--fonts").map(PathBuf::from).unwrap_or_else(|| project_root().join("fonts"));
            let backup = backup_dir(&root, &game, &args);
            let out = arg(&args, "--out").map(PathBuf::from);
            println!("translations: {} texts", store.len());
            ea1::patch::run(
                ea1::patch::Options { game: &game, store: &store, fonts_dir: &fonts, backup_dir: &backup, out_dir: out.as_deref(), dry_run: args.iter().any(|a| a == "--dry-run") },
                &mut |m| println!("{m}"),
            )
        }
        Some("restore") => {
            let root = ea1::project_root().ok_or_else(|| anyhow::anyhow!("no ea1/translations folder found"))?;
            let (done, skipped) = ea1::patch::restore(&game, &backup_dir(&root, &game, &args))?;
            println!("restored {done} files, skipped {skipped}");
            Ok(())
        }
        Some("extract") => {
            let out = PathBuf::from(arg(&args, "--out").unwrap_or_else(|| project_root().join("extracted_ea1").to_string_lossy().into()));
            ea1::extract::extract(&game, &out, &mut |m| println!("{m}"))
        }
        _ => {
            println!("ea1 extract [--game DIR] [--out DIR]   texts of Escape Academy into extracted_ea1/");
            println!("ea1 scan [--game DIR]                  MonoBehaviour scripts with readable strings");
            println!("ea1 patch [--game DIR] [--dry-run] [--out DIR]   Polish from ea1/translations into the game");
            println!("ea1 restore [--game DIR]               put the original game files back");
            println!("ea1 testlang --out DIR [--pl FILE]     test build of Polish as a 5th language ([PL]-marked English)");
            println!("ea1 ilcheck [--game DIR]               state of the IL patch in Assembly-CSharp.dll");
            Ok(())
        }
    }
}
