//! All player-visible texts of Escape Academy into one folder (default: extracted_ea1/):
//!
//! GameplayStrings*.txt         UI/gameplay string tables, CSV: Key, Value-En, ..., French, German, Spanish
//! yarn_<lang>.json             Yarn dialogue lines {line id: text} (en, fr, de, es), speaker as "Name: text"
//! yarn_programs.json           per Yarn program: string tables per language, nodes with their line ids
//! yarn_tables_extra.json       Yarn string tables no program points at (e.g. the NEW_<lang>_* revisions)
//! subtitles_<lang>.json        voice-over subtitles (.srt TextAssets) {clip name: text}
//! tmp_texts.json               texts typed into TextMeshPro components (scene/prefab UI and 3D signs):
//!                              "@key" refers to GameplayStrings (gameplay_key: true), the rest is literal
//! summary.json                 counts per source, the LanguageManager language list
//!
//! Sources: data.unity3d (no type trees) and the Addressables bundles in StreamingAssets/aa (the campus
//! with its Yarn programs, the LanguageManager, the credits; these keep type trees).
use crate::{base_name, text_asset, tmp_text, yarn_program, Files, Game};
use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub const LANGS: [&str; 4] = ["en", "fr", "de", "es"];

fn write_json(p: &Path, v: &Value) -> Result<()> {
    std::fs::write(p, serde_json::to_string_pretty(v)? + "\n").with_context(|| p.display().to_string())
}

/// Minimal CSV (quoted fields with "" escapes and newlines).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut field, mut in_q, mut fresh, mut any) = (Vec::new(), String::new(), false, true, false);
    let mut it = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = it.next() {
        if in_q {
            if c == '"' {
                if it.peek() == Some(&'"') {
                    it.next();
                    field.push('"');
                } else {
                    in_q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if fresh => {
                in_q = true;
                fresh = false;
                any = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                fresh = true;
                any = true;
            }
            '\r' | '\n' => {
                if c == '\r' && it.peek() == Some(&'\n') {
                    it.next();
                }
                if any || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                }
                rows.push(std::mem::take(&mut row));
                fresh = true;
                any = false;
            }
            _ => {
                field.push(c);
                fresh = false;
                any = true;
            }
        }
    }
    if any || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Language of a Yarn string table TextAsset: "Prog (de)" / "Prog (en-US)" / "NEW_de_Prog".
fn yarn_table_lang(name: &str) -> Option<(String, String)> {
    if let Some(open) = name.rfind(" (") {
        if name.ends_with(')') {
            let code = &name[open + 2..name.len() - 1];
            return Some((name[..open].to_string(), code.split('-').next().unwrap_or(code).to_string()));
        }
    }
    let rest = name.strip_prefix("NEW_")?;
    let (code, prog) = rest.split_once('_')?;
    LANGS.contains(&code).then(|| (format!("NEW_{prog}"), code.to_string()))
}

/// Subtitle text of an .srt: cue texts (index and time lines dropped), cues joined with "\n".
fn srt_text(src: &str) -> String {
    src.trim_start_matches('\u{feff}')
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty() && !l.contains("-->") && !l.trim().chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_srt(body: &[u8]) -> bool {
    let s = String::from_utf8_lossy(&body[..body.len().min(64)]);
    let s = s.trim_start_matches('\u{feff}').trim_start();
    s.starts_with(|c: char| c.is_ascii_digit()) && s.contains("-->")
}

/// "Coach_USB_BootThat_de" -> ("Coach_USB_BootThat", "de"); no suffix = English.
fn subtitle_lang(name: &str) -> (String, String) {
    for l in ["de", "fr", "es"] {
        if let Some(base) = name.strip_suffix(&format!("_{l}")) {
            return (base.to_string(), l.to_string());
        }
    }
    (name.to_string(), "en".to_string())
}

pub fn extract(game: &Game, out: &Path, log: &mut dyn FnMut(String)) -> Result<()> {
    if !game.is_valid() {
        bail!("To nie jest folder gry Escape Academy: {}", game.dir.display());
    }
    std::fs::create_dir_all(out)?;
    let t = std::time::Instant::now();
    let files = Files::load(game)?;
    log(format!("{} serialized files read in {:.0} s", files.files.len(), t.elapsed().as_secs_f64()));

    // every TextAsset once (the same asset can sit in several files)
    let mut assets: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut by_ptr: HashMap<(usize, i64), String> = HashMap::new();
    let mut dup_conflicts = Vec::new();
    for (fi, (_, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 49) {
            let (name, body) = text_asset(&o.data)?;
            by_ptr.insert((fi, o.path_id), name.clone());
            match assets.get(&name) {
                Some(b) if *b != body => dup_conflicts.push(name.clone()),
                Some(_) => {}
                None => {
                    assets.insert(name, body);
                }
            }
        }
    }
    let mut summary = Map::new();
    summary.insert("text_assets".into(), json!(assets.len()));
    summary.insert("text_assets_same_name_different_content".into(), json!(dup_conflicts));

    // 1. GameplayStrings tables
    let mut gp = Map::new();
    for (name, body) in assets.iter().filter(|(n, _)| n.starts_with("GameplayStrings")) {
        std::fs::write(out.join(format!("{name}.txt")), body)?;
        let rows = parse_csv(&String::from_utf8_lossy(body));
        let header = rows.first().cloned().unwrap_or_default();
        let col = |want: &str| header.iter().position(|h| h.trim() == want);
        let mut counts = Map::new();
        for (lang, h) in [("en", "Value-En"), ("fr", "French"), ("de", "German"), ("es", "Spanish")] {
            let n = col(h).map_or(0, |c| rows.iter().skip(1).filter(|r| r.get(c).map_or(false, |v| !v.trim().is_empty())).count());
            counts.insert(lang.into(), json!(n));
        }
        let keys = rows.iter().skip(1).filter(|r| r.first().map_or(false, |k| !k.trim().is_empty())).count();
        gp.insert(name.clone(), json!({"keys": keys, "non_empty": counts, "header": header}));
    }
    summary.insert("gameplay_strings".into(), Value::Object(gp));

    // 2. Yarn programs and their string tables
    let mut programs = Map::new();
    let mut used_tables = HashSet::new();
    let mut yarn: BTreeMap<String, Map<String, Value>> = LANGS.iter().map(|l| (l.to_string(), Map::new())).collect();
    let add_table = |table: &str, lang: &str, nodes: Option<&mut Map<String, Value>>, yarn: &mut BTreeMap<String, Map<String, Value>>| -> usize {
        let Some(body) = assets.get(table) else { return 0 };
        let rows = parse_csv(&String::from_utf8_lossy(body));
        let mut n = 0;
        let mut nodes = nodes;
        for r in rows.iter().skip(1).filter(|r| r.len() >= 2 && !r[0].is_empty()) {
            yarn.entry(lang.to_string()).or_default().insert(r[0].clone(), Value::String(r[1].clone()));
            n += 1;
            if let Some(nodes) = nodes.as_deref_mut() {
                let node = r.get(3).cloned().unwrap_or_default();
                let list = nodes.entry(node).or_insert_with(|| json!([]));
                list.as_array_mut().unwrap().push(Value::String(r[0].clone()));
            }
        }
        n
    };
    let mut yarn_errors = Vec::new();
    for (fi, (_, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 114) {
            if files.script_of(fi, &o.data).as_deref() != Some("YarnProgram") {
                continue;
            }
            let p = match yarn_program(&o.data) {
                Ok(p) => p,
                Err(e) => {
                    yarn_errors.push(e.to_string());
                    continue;
                }
            };
            if programs.contains_key(&p.name) {
                continue;
            }
            let name_of = |ptr| files.target_file(fi, ptr).and_then(|f| by_ptr.get(&(f, ptr.path)).cloned());
            let mut tables = Map::new();
            let mut nodes = Map::new();
            let mut lines = Map::new();
            if let Some(base) = name_of(p.base_table) {
                let lang = match p.base_id.split('-').next() { Some(l) if !l.is_empty() => l.to_string(), _ => "en".to_string() };
                lines.insert(lang.clone(), json!(add_table(&base, &lang, Some(&mut nodes), &mut yarn)));
                tables.insert(lang, Value::String(base.clone()));
                used_tables.insert(base);
            }
            for (lang, ptr) in &p.localizations {
                let lang = lang.split('-').next().unwrap_or(lang).to_string();
                if let Some(t) = name_of(*ptr) {
                    lines.insert(lang.clone(), json!(add_table(&t, &lang, None, &mut yarn)));
                    tables.insert(lang, Value::String(t.clone()));
                    used_tables.insert(t);
                }
            }
            programs.insert(p.name.clone(), json!({"base_language": p.base_id, "program_bytes": p.program.len(), "tables": tables, "lines": lines, "nodes": nodes}));
        }
    }
    // string tables no program points at
    let mut extra = Map::new();
    for name in assets.keys() {
        if used_tables.contains(name) || !assets[name].starts_with(b"id,text") {
            continue;
        }
        let Some((prog, lang)) = yarn_table_lang(name) else { continue };
        let rows = parse_csv(&String::from_utf8_lossy(&assets[name]));
        let mut m = Map::new();
        for r in rows.iter().skip(1).filter(|r| r.len() >= 2 && !r[0].is_empty()) {
            m.insert(r[0].clone(), Value::String(r[1].clone()));
        }
        extra.insert(name.clone(), json!({"program": prog, "language": lang, "lines": m}));
    }
    for (lang, m) in &yarn {
        write_json(&out.join(format!("yarn_{lang}.json")), &Value::Object(m.clone()))?;
    }
    let mut ysum = Map::new();
    ysum.insert("programs".into(), json!(programs.len()));
    ysum.insert("lines".into(), json!(yarn.iter().map(|(l, m)| (l.clone(), json!(m.len()))).collect::<Map<_, _>>()));
    ysum.insert("unreferenced_tables".into(), json!(extra.len()));
    ysum.insert("unreferenced_lines".into(), json!(extra.values().map(|v| v["lines"].as_object().unwrap().len()).sum::<usize>()));
    ysum.insert("parse_errors".into(), json!(yarn_errors));
    summary.insert("yarn".into(), Value::Object(ysum));
    write_json(&out.join("yarn_programs.json"), &Value::Object(programs))?;
    write_json(&out.join("yarn_tables_extra.json"), &Value::Object(extra))?;

    // 3. voice-over subtitles (.srt)
    let mut subs: BTreeMap<String, Map<String, Value>> = LANGS.iter().map(|l| (l.to_string(), Map::new())).collect();
    let mut multi_cue = 0;
    for (name, body) in assets.iter().filter(|(_, b)| is_srt(b)) {
        let (base, lang) = subtitle_lang(name);
        let text = srt_text(&String::from_utf8_lossy(body));
        multi_cue += (String::from_utf8_lossy(body).matches("-->").count() > 1) as usize;
        subs.entry(lang).or_default().insert(base, Value::String(text));
    }
    for (lang, m) in &subs {
        write_json(&out.join(format!("subtitles_{lang}.json")), &Value::Object(m.clone()))?;
    }
    summary.insert(
        "subtitles".into(),
        json!({"per_language": subs.iter().map(|(l, m)| (l.clone(), json!(m.len()))).collect::<Map<_, _>>(), "files_with_several_cues": multi_cue}),
    );

    // 4. TextMeshPro texts in scenes and prefabs
    let keys: HashSet<String> = assets
        .iter()
        .filter(|(n, _)| n.starts_with("GameplayStrings"))
        .flat_map(|(_, b)| parse_csv(&String::from_utf8_lossy(b)).into_iter().skip(1).filter_map(|r| r.into_iter().next()))
        .collect();
    let mut tmp: BTreeMap<String, (bool, Vec<String>)> = BTreeMap::new();
    let (mut comps, mut failed) = (0, 0);
    for (fi, (fname, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 114) {
            let Some(script) = files.script_of(fi, &o.data) else { continue };
            if script != "TMPro.TextMeshPro" && script != "TMPro.TextMeshProUGUI" {
                continue;
            }
            comps += 1;
            let Ok(t) = tmp_text(&o.data) else {
                failed += 1;
                continue;
            };
            let text = t.text.trim();
            if text.is_empty() || !text.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            let go = files.deref(fi, t.game_object).and_then(|(_, g)| crate::game_object(&g.data).ok()).map(|g| g.1).unwrap_or_default();
            let kind = if script.ends_with("UGUI") { "ui" } else { "3d" };
            // "@key" is looked up in GameplayStrings at run time; anything else is shown as typed
            let e = tmp.entry(t.text.clone()).or_insert_with(|| (text.strip_prefix('@').map_or(false, |k| keys.contains(k)), vec![]));
            let place = format!("{kind} {} #{} {go}", base_name(fname), o.path_id);
            if e.1.len() < 5 {
                e.1.push(place);
            }
        }
    }
    let as_keys = tmp.values().filter(|v| v.0).count();
    let missing_keys: Vec<&String> = tmp.iter().filter(|(t, v)| !v.0 && t.trim().starts_with('@')).map(|(t, _)| t).collect();
    let tmp_json: Vec<Value> = tmp.iter().map(|(text, (is_key, places))| json!({"text": text, "gameplay_key": is_key, "places": places})).collect();
    write_json(&out.join("tmp_texts.json"), &Value::Array(tmp_json))?;
    summary.insert("tmp".into(), json!({"components": comps, "layout_failed": failed, "distinct_texts_with_letters": tmp.len(), "distinct_gameplay_key_refs": as_keys, "key_refs_not_in_tables": missing_keys, "distinct_literal_texts": tmp.len() - as_keys - missing_keys.len()}));

    // 5. the language list the game offers
    for (fi, (_, sf)) in files.files.iter().enumerate() {
        for o in sf.objects.iter().filter(|o| o.class_id == 114) {
            if files.script_of(fi, &o.data).as_deref() == Some("Escape.Localization.LanguageManager") {
                let (langs, default) = crate::language_manager(&o.data)?;
                let v: Vec<Value> = langs.iter().map(|l| json!({"type": l.kind, "name": l.display_name, "code": l.code, "csv_index": l.csv_index})).collect();
                summary.insert("languages".into(), json!({"default_type": default, "list": v}));
            }
        }
    }

    write_json(&out.join("summary.json"), &Value::Object(summary.clone()))?;
    log(serde_json::to_string_pretty(&Value::Object(summary))?);
    Ok(())
}
