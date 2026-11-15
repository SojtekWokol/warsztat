//! Escape Academy (part 1) in the shared workshop: texts from extracted/ (made from the game's clean
//! files on first start), list tabs, page keys, the names tab, building = `patch::run`.
//!
//! Text ids: "g:<GameplayStrings key>", "y:<Yarn line id>" (line without its "Speaker:" prefix),
//! "v:<voice clip>" (subtitle without its "[NAME]" tag; not written into the game yet).
use crate::patch::split_speaker;
use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use warsztat::{Ctx, Entry, Game};

const STYLE: &str = "Tłumaczysz grę Escape Academy (szkoła dla mistrzów ucieczek, escape roomy, lekki humor i gry słów) \
z angielskiego na naturalny, żywy polski. Znaczniki <...> i kody {0} przepisuj dokładnie. ";

/// Areas that are not escape rooms (GameplayStrings "Area" column and our own codes).
const AREA_NAMES: &[(&str, &str)] = &[
    ("GUI", "Interfejs"), ("General", "Ogólne"), ("Metagame", "Kampus"), ("Credits", "Napisy końcowe"), ("CHAR", "Postacie"),
    ("MAP", "Mapa"), ("FB", "Flashback (DLC)"), ("PG", "Pokoje losowe (ProcGen)"), ("MSC", "Różne"), ("VO", "Kwestie mówione"),
    ("TUT", "Szopa Ucieczek (wstęp)"),
];
const UI_AREAS: &[&str] = &["Menu String", "Menu UI", "Settings", "Multi", "Tutorial", "Meta"];
const CAMPUS: &[&str] = &["General", "Metagame", "CHAR", "MAP", "Credits"];

fn rx(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

fn read_json(p: &Path) -> Result<Value> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| anyhow!("{}: {e}", p.display()))?).map_err(|e| anyhow!("{}: {e}", p.display()))
}

/// A Yarn program that is a campus talk rather than a room cutscene.
fn is_talk(program: &str) -> bool {
    program.starts_with("MG_") || program.contains("_MG_") || program.starts_with("Gen_") || program.contains("_GEN_")
}

/// Room code of a Yarn program or voice clip part ("SPY_GuyCasePt1" -> "SPY").
fn code_of(s: &str) -> String {
    s.split('_').next().unwrap_or("").to_uppercase()
}

pub struct Ea1 {
    pub game: PathBuf,
    /// program name -> its line ids in order (from yarn_programs.json)
    programs: OnceLock<Vec<(String, Vec<String>)>>,
}

impl Default for Ea1 {
    fn default() -> Self {
        Ea1 { game: PathBuf::from(crate::DEFAULT_GAME), programs: OnceLock::new() }
    }
}

impl Ea1 {
    fn programs(&self) -> &Vec<(String, Vec<String>)> {
        self.programs.get().unwrap()
    }
    fn backup_dir(&self, root: &Path) -> PathBuf {
        root.join("backup").join(kopie::folder_id(&self.game))
    }
    fn fonts_dir(root: &Path) -> PathBuf {
        // the repository's fonts/ (shared with EA2) one level up, else ea1/fonts
        let up = root.parent().map(|p| p.join("fonts"));
        match up {
            Some(p) if p.is_dir() => p,
            _ => root.join("fonts"),
        }
    }
}

fn speakers(es: &[&Entry]) -> Vec<String> {
    es.iter().filter_map(|e| e.speaker.clone().filter(|s| !s.is_empty())).collect::<BTreeSet<_>>().into_iter().collect()
}

impl Game for Ea1 {
    fn name(&self) -> &str {
        "Escape Academy"
    }

    fn load(&self, root: &Path) -> Result<(Vec<Entry>, BTreeMap<String, String>)> {
        let ex = root.join("extracted");
        if !ex.join("summary.json").exists() {
            // first start on this computer: texts from the game's clean files
            let game = crate::Game { dir: self.game.clone() };
            if !game.is_valid() {
                return Err(anyhow!("Nie znaleziono gry Escape Academy w {}", self.game.display()));
            }
            let backup = self.backup_dir(root);
            let mut bundles = Vec::new();
            for b in game.bundles() {
                bundles.push(kopie::Originals::clean_copy(&crate::known::CONFIG, &b, &backup)?);
            }
            crate::extract::extract_bundles(&bundles, &ex, &mut |_| {}).map_err(|e| anyhow!("Nie udało się wyciągnąć tekstów z gry: {e}"))?;
        }
        let mut entries: Vec<Entry> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // GameplayStrings (4 tables): "g:<key>"; a key in several tables is one text (the game merges them)
        let mut room_names: HashMap<String, String> = HashMap::new();
        let mut tables: Vec<PathBuf> = std::fs::read_dir(&ex)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with("GameplayStrings"))).collect();
        tables.sort();
        static ROOM: OnceLock<Regex> = OnceLock::new();
        static CODE: OnceLock<Regex> = OnceLock::new();
        for t in &tables {
            let tag = match t.file_stem().unwrap().to_string_lossy().as_ref() {
                "GameplayStrings_Flashback" => "FB",
                "GameplayStrings_ProcGen" => "PG",
                "GameplayStrings_Island" => "MSC",
                _ => "MSC",
            };
            let rows = crate::extract::parse_csv(&String::from_utf8_lossy(&std::fs::read(t)?));
            let h: Vec<String> = rows[0].iter().map(|x| x.trim().to_string()).collect();
            let col = |n: &str| h.iter().position(|x| x == n);
            let (c_area, c_ctx) = (col("Area"), col("Context").or_else(|| col("Loc Context")));
            let refs_cols: Vec<(&str, usize)> = [("fr", "French"), ("de", "German"), ("es", "Spanish")].iter().filter_map(|(l, n)| Some((*l, col(n)?))).collect();
            for r in &rows[1..] {
                let key = r.first().map(|k| k.trim().to_string()).unwrap_or_default();
                let en = r.get(1).cloned().unwrap_or_default();
                if key.is_empty() || en.is_empty() {
                    continue;
                }
                if let Some(c) = rx(&ROOM, r"^([A-Z0-9]+)_(LevelName|Name)$").captures(&key) {
                    room_names.entry(c[1].to_string()).or_insert_with(|| en.clone());
                }
                if !seen.insert(format!("g:{key}")) {
                    continue;
                }
                let raw = c_area.and_then(|c| r.get(c)).map(|a| a.trim().to_string()).unwrap_or_default();
                let area = if UI_AREAS.contains(&raw.as_str()) {
                    "GUI".to_string()
                } else if rx(&CODE, r"^[A-Z]{2,4}$").is_match(&raw) || CAMPUS.contains(&raw.as_str()) {
                    raw
                } else if tag == "FB" || tag == "PG" {
                    tag.to_string()
                } else {
                    let c = code_of(&key);
                    if c.len() == 3 { c } else { "MSC".into() }
                };
                let ctx = c_ctx.and_then(|c| r.get(c)).cloned().unwrap_or_default();
                let kind = ea2::workshop::kind_of(&key, &ctx, &en);
                entries.push(Entry {
                    id: format!("g:{key}"), src: "g".into(), key: key.clone(), area, kind: kind.into(), ctx: ctx.trim().into(), en,
                    refs: refs_cols.iter().map(|(l, c)| (l.to_string(), r.get(*c).cloned().unwrap_or_default())).collect(), ..Default::default()
                });
            }
        }

        // Yarn: "y:<line id>", in program order; a line shared by several programs is one text
        let progs = read_json(&ex.join("yarn_programs.json"))?;
        let yarn: HashMap<&str, Value> = ["en", "fr", "de", "es"].iter().map(|l| Ok((*l, read_json(&ex.join(format!("yarn_{l}.json")))?))).collect::<Result<_>>()?;
        let txt = |l: &str, id: &str| yarn[l].get(id).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut programs = Vec::new();
        let mut order = 0;
        for (name, p) in progs.as_object().unwrap() {
            let mut ids = Vec::new();
            for (_, list) in p["nodes"].as_object().into_iter().flatten() {
                for id in list.as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                    ids.push(id.to_string());
                    if !seen.insert(format!("y:{id}")) {
                        continue;
                    }
                    let (speaker, line) = split_speaker(&txt("en", id));
                    if line.trim().is_empty() {
                        continue;
                    }
                    order += 1;
                    entries.push(Entry {
                        id: format!("y:{id}"), src: "y".into(), key: id.to_string(), area: if is_talk(name) { "TALK".into() } else { code_of(name) },
                        kind: if is_talk(name) { "talk".into() } else { "dialog".into() }, node: Some(name.clone()), order: Some(order),
                        speaker: Some(speaker.trim().to_string()), en: line,
                        refs: ["fr", "de", "es"].iter().map(|l| (l.to_string(), split_speaker(&txt(l, id)).1)).collect(), ..Default::default()
                    });
                }
            }
            programs.push((name.clone(), ids));
        }
        let _ = self.programs.set(programs);

        // voice-over subtitles: "v:<clip>" ("[COACH YOACH] text" -> speaker + text)
        static TAG: OnceLock<Regex> = OnceLock::new();
        let subs: HashMap<&str, Value> = ["en", "fr", "de", "es"].iter().map(|l| Ok((*l, read_json(&ex.join(format!("subtitles_{l}.json")))?))).collect::<Result<_>>()?;
        let split_tag = |s: &str| -> (String, String) {
            match rx(&TAG, r"^\s*\[([^\]]+)\]\s*").captures(s) {
                Some(c) => (c[1].to_string(), s[c.get(0).unwrap().end()..].to_string()),
                None => (String::new(), s.to_string()),
            }
        };
        for (clip, text) in subs["en"].as_object().unwrap() {
            let (speaker, en) = split_tag(text.as_str().unwrap_or(""));
            if en.trim().is_empty() || !seen.insert(format!("v:{clip}")) {
                continue;
            }
            let part = clip.split('_').nth(1).unwrap_or("").to_uppercase();
            let area = if part.len() == 3 && part != "MSC" { part } else { "VO".into() };
            entries.push(Entry {
                id: format!("v:{clip}"), src: "v".into(), key: clip.clone(), area, kind: "vo".into(), speaker: Some(speaker),
                ctx: "napis do nagrania (znacznik [IMIĘ] dodaje się sam); każda linia to osobny fragment czasowy: przy tej samej liczbie linii podział zostaje, inaczej cały tekst wisi przez całe nagranie".into(), en,
                refs: ["fr", "de", "es"].iter().filter_map(|l| Some((l.to_string(), split_tag(subs[l].get(clip)?.as_str()?).1))).collect(),
                ..Default::default()
            });
        }

        let mut areas = BTreeMap::new();
        for e in &entries {
            if !areas.contains_key(&e.area) {
                let name = AREA_NAMES.iter().find(|a| a.0 == e.area).map(|a| a.1.to_string()).or_else(|| room_names.get(&e.area).cloned()).unwrap_or_else(|| e.area.clone());
                areas.insert(e.area.clone(), name);
            }
        }
        areas.insert("TALK".into(), "Rozmowy na kampusie".into());
        Ok((entries, areas))
    }

    fn pages(&self, cx: &Ctx, tab: &str) -> Vec<Value> {
        let mut out = Vec::new();
        match tab {
            "scenes" | "talks" => {
                for (name, _) in self.programs() {
                    if is_talk(name) != (tab == "talks") {
                        continue;
                    }
                    let key = format!("{}:{name}", if tab == "talks" { "talk" } else { "scene" });
                    let es = self.page_entries(cx, &key).unwrap_or_default();
                    if es.is_empty() {
                        continue;
                    }
                    let code = code_of(name);
                    out.push(json!({"key": key, "id": name, "place": cx.areas.get(&code).cloned().unwrap_or(code), "who": speakers(&es),
                        "done": cx.done_count(&es), "total": es.len(), "start": es[0].en}));
                }
            }
            "rooms" | "campus" | "other" => {
                let codes: BTreeSet<&str> = cx.entries.iter().filter(|e| e.src != "y").map(|e| e.area.as_str()).collect();
                for c in codes {
                    let is_room = c.len() == 3 && c.chars().all(|x| x.is_ascii_uppercase()) && c != "GUI" && c != "MSC";
                    let is_campus = CAMPUS.contains(&c);
                    let want = match tab {
                        "rooms" => is_room,
                        "campus" => is_campus,
                        _ => !is_room && !is_campus,
                    };
                    if !want {
                        continue;
                    }
                    let es = self.page_entries(cx, &format!("area:{c}")).unwrap_or_default();
                    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                    for e in &es {
                        *kinds.entry(e.kind.clone()).or_insert(0) += 1;
                    }
                    out.push(json!({"key": format!("area:{c}"), "id": c, "name": cx.areas.get(c).cloned().unwrap_or(c.to_string()), "done": cx.done_count(&es),
                        "total": es.len(), "vo": kinds.get("vo").copied().unwrap_or(0), "kinds": kinds, "start": es.first().map_or("", |e| e.en.as_str())}));
                }
            }
            _ => {}
        }
        out
    }

    fn page_entries<'a>(&self, cx: &Ctx<'a>, key: &str) -> Option<Vec<&'a Entry>> {
        if let Some(name) = key.strip_prefix("scene:").or_else(|| key.strip_prefix("talk:")) {
            let ids = self.programs().iter().find(|p| p.0 == name).map(|p| p.1.clone()).unwrap_or_default();
            let mut seen = HashSet::new();
            return Some(ids.iter().filter(|id| seen.insert(id.to_string())).filter_map(|id| cx.entry(&format!("y:{id}"))).collect());
        }
        if let Some(code) = key.strip_prefix("area:") {
            return Some(cx.entries.iter().filter(|e| e.area == code && e.src != "y").collect());
        }
        None
    }

    fn page_title(&self, cx: &Ctx, key: &str) -> Option<String> {
        if let Some(c) = key.strip_prefix("area:") {
            return Some(cx.areas.get(c).cloned().unwrap_or_else(|| c.to_string()));
        }
        key.strip_prefix("scene:").or_else(|| key.strip_prefix("talk:")).map(|s| s.to_string())
    }

    fn row_extra(&self, _cx: &Ctx, e: &Entry) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("who".into(), json!(e.speaker.clone().unwrap_or_default()));
        m.insert("stage".into(), Value::Null);
        m
    }

    fn names(&self, cx: &Ctx) -> Vec<Value> {
        static NR: OnceLock<Regex> = OnceLock::new();
        let r = rx(&NR, r"(LevelName|_Name|QuestName)$");
        let mut out: Vec<Value> = cx
            .entries
            .iter()
            .filter(|e| e.src == "g" && (e.kind == "name" || e.kind == "item" || r.is_match(&e.key)))
            .map(|e| json!({"id": e.id, "key": e.key, "group": e.kind, "area": cx.areas.get(&e.area).cloned().unwrap_or(e.area.clone()), "en": e.en, "pl": cx.store.pl(&e.id)}))
            .collect();
        // dialogue speakers: "s:<English name>" (shown in the workshop; the game keeps the English prefix for now)
        let mut cnt: Vec<(String, usize)> = Vec::new();
        for e in cx.entries.iter().filter(|e| e.src == "y") {
            let Some(sp) = e.speaker.as_deref().filter(|s| !s.is_empty()) else { continue };
            match cnt.iter_mut().find(|x| x.0 == sp) {
                Some(x) => x.1 += 1,
                None => cnt.push((sp.to_string(), 1)),
            }
        }
        cnt.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, n) in cnt {
            let id = format!("s:{name}");
            out.push(json!({"id": id, "key": format!("{n} kwestii"), "group": "speaker", "area": "dialogi", "en": name, "pl": cx.store.pl(&id)}));
        }
        out
    }

    fn game_terms(&self, cx: &Ctx, en: &str) -> Vec<Value> {
        static STRIP: OnceLock<Regex> = OnceLock::new();
        let low = en.to_lowercase();
        let mut out = Vec::new();
        for e in cx.entries {
            let n = e.en.chars().count();
            if e.src != "g" || !["name", "item", "quest"].contains(&e.kind.as_str()) || !(4..=40).contains(&n) {
                continue;
            }
            let name = rx(&STRIP, r"<[^>]+>").replace_all(&e.en, "").trim().to_string();
            if name.is_empty() || !low.contains(&name.to_lowercase()) {
                continue;
            }
            if Regex::new(&format!(r"\b{}\b", regex::escape(&name.to_lowercase()))).map_or(false, |r| r.is_match(&low)) {
                out.push(json!({"en": name, "pl": cx.store.pl(&e.id), "key": e.key, "src": "game"}));
            }
        }
        out
    }

    /// translations/pl layout: cutscenes per room, campus talks together, interface texts per room/area.
    fn store_file(&self, id: &str, e: Option<&Entry>) -> String {
        let Some(e) = e else {
            return if id.starts_with("s:") { "nazwy.json".into() } else { "inne.json".into() };
        };
        let safe = |c: &str| c.chars().all(|x| x.is_ascii_alphanumeric()) && !c.is_empty();
        match e.src.as_str() {
            "y" if e.kind == "talk" => "rozmowy.json".into(),
            "y" if safe(&e.area) => format!("scenki/{}.json", e.area),
            "y" => "scenki/inne.json".into(),
            "v" => "napisy.json".into(),
            _ if e.kind == "name" => "nazwy.json".into(),
            _ if e.area.len() == 3 && safe(&e.area) && e.area != "GUI" && e.area != "MSC" => format!("pokoje/{}.json", e.area),
            _ if CAMPUS.contains(&e.area.as_str()) => "kampus.json".into(),
            _ => "interfejs.json".into(),
        }
    }

    fn style(&self) -> String {
        STYLE.to_string()
    }

    fn keep_english(&self) -> Vec<String> {
        vec!["Escape Academy".into()]
    }

    fn build(&self, root: &Path, log: &mut dyn FnMut(String)) -> Result<()> {
        let game = crate::Game { dir: self.game.clone() };
        let store = warsztat::store::read_texts(&root.join("translations").join("pl"))?;
        log(format!("Tłumaczenia: {} tekstów", store.len()));
        let fonts = Self::fonts_dir(root);
        let backup = self.backup_dir(root);
        crate::patch::run(crate::patch::Options { game: &game, store: &store, fonts_dir: &fonts, backup_dir: &backup, out_dir: None, dry_run: false }, log)
    }

    fn launch(&self) -> Result<()> {
        let exe = self.game.join("Escape Academy.exe");
        std::process::Command::new(&exe).current_dir(&self.game).spawn().map_err(|e| anyhow!("Nie udało się uruchomić gry ({}): {e}", exe.display()))?;
        Ok(())
    }
}
