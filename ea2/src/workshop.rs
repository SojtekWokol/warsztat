//! Escape Academy 2 in the shared workshop: where the texts come from (extracted/), the list tabs,
//! page keys, the scene preview data, the names tab, and building = the Rust patcher.
use crate::csv;
use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use warsztat::{Ctx, Entry, Game};

pub const DEFAULT_CONTENT: &str = r"C:\XboxGames\Escape Academy 2- Back 2 School\Content";
const LANGS: &[(&str, &str)] = &[("en", "English"), ("es", "Spanish"), ("fr", "French"), ("it", "Italian"), ("de", "German"), ("zh", "Chinese"), ("ja", "Japanese")];

const AREA_NAMES: &[(&str, &str)] = &[
    ("GUI", "Interfejs"), ("GUIMap", "Mapa"), ("PRO", "Akademik i kampus"), ("HUM", "Humanistyka"), ("SCI", "Nauki ścisłe"),
    ("PER", "Sztuka"), ("MUS", "Muzyka"), ("VON", "Wyprawa Vona"), ("QAD", "Dziedziniec"), ("MSC", "Różne"), ("Color", "Kolory"),
    ("Keypad", "Klawiatura"), ("achievement", "Osiągnięcia"), ("acheivement", "Osiągnięcia"), ("Action", "Akcje"), ("name", "Postacie"),
    ("Name", "Postacie"), ("Hint", "Podpowiedzi"), ("VN", "Scenki fabularne"), ("WLD", "Rozmowy na kampusie"), ("VO", "Inne kwestie mówione"),
    ("SEW", "Kanały"), ("ART", "Sztuka (pokój)"), ("ADM", "Administracja"), ("TALK", "Rozmowy"), ("SUB", "Okrzyki Gillian (napisy)"),
    ("CRD", "Napisy końcowe"),
];

/// Credit headings that are company or product names: not translatable.
const CREDIT_KEEP: &[&str] = &[
    "Escape Academy 2: Back 2 School", "Fortyseven Communications", "iam8bit XP", "Snoop Game QA", "HUWIZ QA/UX", "iXie Gaming",
    "Plastic Fern Studios", "Local Heroes", "Loc-3 Ltd", "All The Way", "4-Real Intermedia GmbH", "Sound Map", "Yeehe",
    "Active Gaming Media Inc.", "Transperfect", "Game Tester - Lionbridge Studios",
];

const ROOM_CODES: &[&str] = &["TIC", "TOW", "HEY", "HMI", "DET", "TRK", "HST", "GUN", "LAR", "RAT", "PAL", "SEW", "AHY", "CY1", "CY2", "CY3"];
const CAMPUS_CODES: &[&str] = &["PRO", "HUM", "SCI", "PER", "MUS", "VON", "QAD", "ART", "ADM", "WLD", "VO"];

const STYLE: &str = "Tłumaczysz grę Escape Academy 2 (akademia dla mistrzów ucieczek, escape roomy, lekki humor i gry słów) \
z angielskiego na naturalny, żywy polski. Znaczniki <...> i kody {0} przepisuj dokładnie. ";

fn rx(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

pub fn area_of(key: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let code = rx(&R, r"^([A-Za-z]+[0-9]?)(?:_|$)").captures(key).map(|c| c[1].to_string()).unwrap_or_else(|| "MSC".into());
    let l = code.to_lowercase();
    if l.starts_with("achie") || l.starts_with("acheiv") { "achievement".into() } else { code }
}

pub fn kind_of(key: &str, ctx: &str, en: &str) -> &'static str {
    static P: OnceLock<Regex> = OnceLock::new();
    static L: OnceLock<Regex> = OnceLock::new();
    let (k, c) = (key.to_lowercase(), ctx.to_lowercase());
    if ["GUI", "Load", "GameMode", "Warning", "DEMO", "ReplayComp"].iter().any(|p| key.starts_with(p)) || c.contains("button") {
        return "ui";
    }
    if k.starts_with("name_") || key.starts_with("QuantyNametag") {
        return "name";
    }
    if k.contains("quest") || k.contains("_step") {
        return "quest";
    }
    if k.contains("_item_") || k.ends_with("_item") {
        return "item";
    }
    if k.contains("achie") || k.contains("acheiv") {
        return "ui";
    }
    if rx(&P, r"correct answer|distractor|letter of|same as the above|valid characters|do not translate").is_match(&c)
        || rx(&L, r"^[A-Z0-9](,[A-Z0-9])+$").is_match(en.trim())
    {
        return "puzzle";
    }
    if k.contains("hint") {
        return "hint";
    }
    "text"
}

fn read_json(p: &Path) -> Result<Value> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| anyhow!("{}: {e}", p.display()))?).map_err(|e| anyhow!("{}: {e}", p.display()))
}

/// "Speaker:line" when a colon appears in the first 40 characters.
fn split_speaker(text: &str) -> (String, String) {
    let head: String = text.chars().take(40).collect();
    if head.contains(':') {
        let (a, b) = text.split_once(':').unwrap();
        (a.to_string(), b.to_string())
    } else {
        (String::new(), text.to_string())
    }
}

pub struct Ea2 {
    /// filled in load(): scenes (yarn_scenes.json), line id -> stage data, name_* key -> English
    scenes: OnceLock<Vec<Value>>,
    line_stage: OnceLock<HashMap<String, Value>>,
    names_en: OnceLock<HashMap<String, String>>,
    /// campus talks per first part of the Yarn node name ("Gil", "HUM", …): the store file grouping
    talk_prefixes: OnceLock<HashMap<String, usize>>,
    pub content: PathBuf,
}

impl Default for Ea2 {
    fn default() -> Self {
        Ea2 { scenes: OnceLock::new(), line_stage: OnceLock::new(), names_en: OnceLock::new(), talk_prefixes: OnceLock::new(), content: PathBuf::from(DEFAULT_CONTENT) }
    }
}

fn talk_group(node: &str) -> String {
    if node.ends_with('_') {
        return node.to_string(); // Python: re.sub(r"_[^_]+$", "", node) needs one character after "_"
    }
    match node.rfind('_') {
        Some(i) => node[..i].to_string(),
        None => node.to_string(),
    }
}

impl Ea2 {
    fn names_en(&self) -> &HashMap<String, String> {
        self.names_en.get().unwrap()
    }
    fn scenes(&self) -> &Vec<Value> {
        self.scenes.get().unwrap()
    }
    fn who_label(&self, who: &str) -> String {
        if who.is_empty() { String::new() } else { self.names_en().get(who).cloned().unwrap_or_else(|| who.replace("name_", "")) }
    }
    fn scene_place(&self, cx: &Ctx, node: &str) -> String {
        let code = node.replace("DEMO_", "");
        let code = code.split('_').next().unwrap_or("");
        cx.areas.get(code).cloned().unwrap_or_else(|| code.to_string())
    }
}

fn speakers(es: &[&Entry]) -> Vec<String> {
    es.iter().filter_map(|e| e.speaker.clone().filter(|s| !s.is_empty())).collect::<BTreeSet<_>>().into_iter().collect()
}

impl Game for Ea2 {
    fn name(&self) -> &str {
        "Escape Academy 2"
    }

    fn load(&self, root: &Path) -> Result<(Vec<Entry>, BTreeMap<String, String>)> {
        let ex = root.join("extracted");
        // first run on this computer (texts of the game are not in the repository): take them from the game
        let needed = ["GameplayStrings.txt", "VOStrings.txt", "yarn_en.json", "yarn_scenes.json", "yarn2_en.json", "subtitles.json"];
        if needed.iter().any(|f| !ex.join(f).exists()) || !root.join("cache").join("vn").join("bg").exists() {
            let game = crate::patch::Game { content: self.content.clone() };
            crate::extract::extract(&game, &root.join("backup"), root, true, &mut |_| {})
                .map_err(|e| anyhow!("Nie udało się wyciągnąć tekstów z gry ({}): {e}", self.content.display()))?;
        }
        let mut entries: Vec<Entry> = Vec::new();

        // GameplayStrings: UI, quests, items, notes, puzzles ("g:<KEY>")
        let g = csv::parse(&std::fs::read_to_string(ex.join("GameplayStrings.txt"))?);
        let gh: Vec<String> = g[0].iter().map(|h| h.trim().to_string()).collect();
        let col: HashMap<&str, usize> = LANGS.iter().filter_map(|(l, n)| Some((*l, gh.iter().position(|h| *h == format!("{n} Value ({l})"))?))).collect();
        let cen = col["en"];
        static RN: OnceLock<Regex> = OnceLock::new();
        let mut room_names = HashMap::new();
        for row in &g[1..] {
            if row.first().map_or(false, |k| rx(&RN, r"^[A-Z0-9]+_RoomName$").is_match(k)) {
                room_names.insert(row[0].split('_').next().unwrap().to_string(), row.get(cen).cloned().unwrap_or_default());
            }
        }
        for row in &g[1..] {
            if row.is_empty() || row[0].trim().is_empty() {
                continue;
            }
            let (key, ctx) = (&row[0], row.get(1).cloned().unwrap_or_default());
            let en = row.get(cen).cloned().unwrap_or_default();
            if en.trim().is_empty() {
                continue;
            }
            let refs = ["de", "fr", "es"].iter().filter_map(|l| Some((l.to_string(), row.get(col[l])?.clone()))).collect();
            entries.push(Entry {
                id: format!("g:{key}"), src: "g".into(), key: key.clone(), area: area_of(key), kind: kind_of(key, &ctx, &en).into(),
                ctx: ctx.trim().into(), en, refs, ..Default::default()
            });
        }

        // VOStrings: spoken lines ("v:<File Name>[#n]")
        let v = csv::parse(&std::fs::read_to_string(ex.join("VOStrings.txt"))?);
        let vh: Vec<String> = v[0].iter().map(|h| h.trim().to_string()).collect();
        let vi = |n: &str| vh.iter().position(|h| h == n).ok_or_else(|| anyhow!("VOStrings: no column {n}"));
        let (c_rq, c_char, c_fn, c_ctx, c_trig) = (vi("Room/Quest")?, vi("CharacterID")?, vi("File Name")?, vi("Loc Context")?, vi("Trigger CRIT PATH CHAPTER 1 TIC - ROOM")?);
        let ven = vi("English Value (en)")?;
        let vref: Vec<(String, usize)> = [("de", "German"), ("fr", "French"), ("es", "Spanish")].iter().filter_map(|(l, n)| Some((l.to_string(), vi(&format!("{n} Value ({l})")).ok()?))).collect();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let cell = |r: &Vec<String>, i: usize| r.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        for row in &v[1..] {
            if row.len() <= ven || row[ven].trim().is_empty() {
                continue;
            }
            let mut fname = cell(row, c_fn);
            if fname.is_empty() {
                fname = format!("row{}", seen.len());
            }
            let n = *seen.get(&fname).unwrap_or(&0);
            seen.insert(fname.clone(), n + 1);
            let rq = cell(row, c_rq);
            let room = if rq.ends_with("_WLD") {
                "WLD".to_string()
            } else {
                let r = rq.split('_').next().unwrap_or("").trim_end_matches('#').to_string();
                if r.is_empty() { "VO".into() } else { r }
            };
            let ctx = [cell(row, c_trig), cell(row, c_ctx)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" / ");
            entries.push(Entry {
                id: format!("v:{fname}") + &if n > 0 { format!("#{n}") } else { String::new() },
                src: "v".into(), key: fname, area: room, kind: "vo".into(), speaker: Some(cell(row, c_char)), ctx, en: row[ven].clone(),
                refs: vref.iter().map(|(l, i)| (l.clone(), row.get(*i).cloned().unwrap_or_default())).collect(), ..Default::default()
            });
        }

        // cutscenes (Yarn VisNov project): "y:<line id>", in table order
        let yarn: HashMap<&str, Value> = ["en", "de", "fr", "es"].iter().map(|l| Ok((*l, read_json(&ex.join(format!("yarn_{l}.json")))?))).collect::<Result<_>>()?;
        let txt = |l: &str, id: &str| yarn[l].get(id).and_then(|x| x.as_str()).unwrap_or("").to_string();
        for (i, (lid, text)) in yarn["en"].as_object().unwrap().iter().enumerate() {
            let (speaker, line) = split_speaker(text.as_str().unwrap_or(""));
            entries.push(Entry {
                id: format!("y:{lid}"), src: "y".into(), key: lid.clone(), area: "VN".into(), kind: "dialog".into(), order: Some(i as i64),
                speaker: Some(speaker.trim().to_string()), en: line,
                refs: ["de", "fr", "es"].iter().map(|l| (l.to_string(), split_speaker(&txt(l, lid)).1)).collect(), ..Default::default()
            });
        }

        // campus conversations (Dialogue project): "n:<line id>", in program order, by Yarn node
        if ex.join("yarn2_en.json").exists() {
            let t2: HashMap<&str, Value> = ["en", "de", "fr", "es"].iter().map(|l| Ok((*l, read_json(&ex.join(format!("yarn2_{l}.json")))?))).collect::<Result<_>>()?;
            let scenes2 = read_json(&ex.join("yarn2_scenes.json"))?;
            for sc in scenes2.as_array().unwrap() {
                let node = sc["node"].as_str().unwrap_or("");
                if node.starts_with("TEST_") {
                    continue;
                }
                for l in sc["lines"].as_array().unwrap() {
                    let lid = l["id"].as_str().unwrap_or("");
                    let (speaker, line) = split_speaker(t2["en"].get(lid).and_then(|x| x.as_str()).unwrap_or(""));
                    let t = line.trim();
                    if t.is_empty() || ["Start", "Response", "Exit", "Both Nearby", "One Nearby"].contains(&t) {
                        continue;
                    }
                    entries.push(Entry {
                        id: format!("n:{lid}"), src: "n".into(), key: lid.into(), area: "TALK".into(), kind: "talk".into(), node: Some(node.into()),
                        speaker: Some(speaker.trim().to_string()), en: line,
                        refs: ["de", "fr", "es"].iter().map(|lg| (lg.to_string(), split_speaker(t2[lg].get(lid).and_then(|x| x.as_str()).unwrap_or("")).1)).collect(),
                        ..Default::default()
                    });
                }
            }
        }

        // Gillian's bark subtitles (.srt in the subtitles bundle): "b:<file name>"
        if ex.join("subtitles.json").exists() {
            static TAGX: OnceLock<Regex> = OnceLock::new();
            let text = |srt: &str| -> String {
                let lines: Vec<&str> = srt.split('\n').map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty() && !l.contains("-->") && !l.trim().chars().all(|c| c.is_ascii_digit())).collect();
                rx(&TAGX, r"^\[[^\]]+\]\s*").replace(&lines.join(" "), "").to_string()
            };
            for (name, langs) in read_json(&ex.join("subtitles.json"))?.as_object().unwrap() {
                let get = |l: &str| langs.get(l).and_then(|x| x.as_str());
                entries.push(Entry {
                    id: format!("b:{name}"), src: "b".into(), key: name.clone(), area: "SUB".into(), kind: "vo".into(), speaker: Some("name_Gillian".into()),
                    ctx: "napis do okrzyku Gillian (znacznik [GILLIAN] dodaje się sam)".into(), en: text(get("en").unwrap_or("")),
                    refs: ["de", "fr", "es"].iter().filter_map(|l| Some((l.to_string(), text(get(l)?)))).collect(), ..Default::default()
                });
            }
        }

        // end credits: headings and job titles, "c:<English>"
        if let Ok(cr) = std::fs::read_to_string(ex.join("GameCredits.txt")) {
            let mut seen_c = BTreeSet::new();
            for line in cr.lines() {
                let (text, kind) = if line.starts_with('#') {
                    (line.trim_start_matches('#').trim().to_string(), "nagłówek")
                } else if let Some((_, r)) = line.split_once("...") {
                    (r.trim().to_string(), "funkcja")
                } else {
                    continue;
                };
                if text.is_empty() || CREDIT_KEEP.contains(&text.as_str()) || !seen_c.insert(text.clone()) {
                    continue;
                }
                entries.push(Entry {
                    id: format!("c:{text}"), src: "c".into(), key: text.chars().take(40).collect(), area: "CRD".into(), kind: "text".into(),
                    ctx: format!("napisy końcowe, {kind}"), en: text, ..Default::default()
                });
            }
        }

        let mut areas = BTreeMap::new();
        for e in &entries {
            if !areas.contains_key(&e.area) {
                let name = AREA_NAMES.iter().find(|a| a.0 == e.area).map(|a| a.1.to_string()).or_else(|| room_names.get(&e.area).cloned()).unwrap_or_else(|| e.area.clone());
                areas.insert(e.area.clone(), name);
            }
        }

        // preview and names data
        let scenes = read_json(&ex.join("yarn_scenes.json"))?.as_array().cloned().unwrap_or_default();
        let stage: HashMap<String, Value> = scenes.iter().flat_map(|s| s["lines"].as_array().cloned().unwrap_or_default()).filter_map(|l| Some((l["id"].as_str()?.to_string(), l))).collect();
        let names: HashMap<String, String> = entries.iter().filter(|e| e.src == "g" && e.key.to_lowercase().starts_with("name_")).map(|e| (e.key.clone(), e.en.clone())).collect();
        let _ = self.scenes.set(scenes);
        let _ = self.line_stage.set(stage);
        let _ = self.names_en.set(names);
        let mut tp: HashMap<String, usize> = HashMap::new();
        for e in entries.iter().filter(|e| e.src == "n") {
            // lower case: "VON_…" and "Von_…" must not become two files that Windows sees as one
            *tp.entry(e.node.as_deref().unwrap_or("").split('_').next().unwrap_or("").to_lowercase()).or_insert(0) += 1;
        }
        let _ = self.talk_prefixes.set(tp);
        Ok((entries, areas))
    }

    fn pages(&self, cx: &Ctx, tab: &str) -> Vec<Value> {
        let mut out = Vec::new();
        match tab {
            "scenes" => {
                for s in self.scenes() {
                    let node = s["node"].as_str().unwrap_or("");
                    let es = self.page_entries(cx, &format!("scene:{node}")).unwrap_or_default();
                    out.push(json!({"key": format!("scene:{node}"), "id": node, "place": self.scene_place(cx, node), "who": speakers(&es),
                        "done": cx.done_count(&es), "total": es.len(), "start": es.first().map_or("", |e| e.en.as_str())}));
                }
            }
            "talks" => {
                let mut groups: Vec<(String, Vec<&Entry>)> = Vec::new();
                for e in cx.entries.iter().filter(|e| e.src == "n") {
                    let g = talk_group(e.node.as_deref().unwrap_or(""));
                    match groups.iter_mut().find(|x| x.0 == g) {
                        Some(x) => x.1.push(e),
                        None => groups.push((g, vec![e])),
                    }
                }
                for (g, es) in groups {
                    let code = g.split('_').next().unwrap_or("");
                    out.push(json!({"key": format!("talk:{g}"), "id": g, "place": cx.areas.get(&code.to_uppercase()).cloned().unwrap_or_else(|| code.to_string()),
                        "who": speakers(&es), "done": cx.done_count(&es), "total": es.len(), "start": es[0].en}));
                }
            }
            "rooms" | "campus" | "other" => {
                let codes: BTreeSet<&str> = cx.entries.iter().map(|e| e.area.as_str()).collect();
                let pick: Vec<String> = match tab {
                    "rooms" => ROOM_CODES.iter().filter(|c| codes.contains(*c)).map(|c| c.to_string()).collect(),
                    "campus" => CAMPUS_CODES.iter().filter(|c| codes.contains(*c) && **c != "WLD").map(|c| c.to_string()).collect(),
                    _ => codes.iter().filter(|c| !ROOM_CODES.contains(c) && !CAMPUS_CODES.contains(c) && **c != "VN" && **c != "TALK").map(|c| c.to_string()).collect(),
                };
                for c in pick {
                    let es = self.page_entries(cx, &format!("area:{c}")).unwrap_or_default();
                    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
                    for e in &es {
                        *kinds.entry(e.kind.clone()).or_insert(0) += 1;
                    }
                    out.push(json!({"key": format!("area:{c}"), "id": c, "name": cx.areas.get(&c).cloned().unwrap_or(c.clone()), "done": cx.done_count(&es),
                        "total": es.len(), "vo": kinds.get("vo").copied().unwrap_or(0), "kinds": kinds, "start": es.first().map_or("", |e| e.en.as_str())}));
                }
                if tab == "campus" {
                    let whos: BTreeSet<String> = cx.entries.iter().filter(|e| e.area == "WLD").map(|e| e.speaker.clone().unwrap_or_default()).collect();
                    for w in whos {
                        let es = self.page_entries(cx, &format!("who:{w}")).unwrap_or_default();
                        out.push(json!({"key": format!("who:{w}"), "id": "WLD", "name": format!("Rozmowy: {}", self.names_en().get(&w).cloned().unwrap_or_else(|| w.replace("name_", ""))),
                            "done": cx.done_count(&es), "total": es.len(), "vo": es.len(), "kinds": {"vo": es.len()}, "start": es.first().map_or("", |e| e.en.as_str())}));
                    }
                }
            }
            _ => {}
        }
        out
    }

    fn page_entries<'a>(&self, cx: &Ctx<'a>, key: &str) -> Option<Vec<&'a Entry>> {
        if let Some(node) = key.strip_prefix("scene:") {
            let sc = self.scenes().iter().find(|s| s["node"].as_str() == Some(node));
            return Some(sc.map_or(vec![], |s| s["lines"].as_array().unwrap().iter().filter_map(|l| cx.entry(&format!("y:{}", l["id"].as_str()?))).collect()));
        }
        if let Some(code) = key.strip_prefix("area:") {
            return Some(cx.entries.iter().filter(|e| e.area == code && e.src != "y").collect());
        }
        if let Some(who) = key.strip_prefix("who:") {
            return Some(cx.entries.iter().filter(|e| e.area == "WLD" && e.speaker.as_deref().unwrap_or("") == who).collect());
        }
        if let Some(g) = key.strip_prefix("talk:") {
            return Some(cx.entries.iter().filter(|e| e.src == "n" && talk_group(e.node.as_deref().unwrap_or("")) == g).collect());
        }
        None
    }

    fn page_title(&self, cx: &Ctx, key: &str) -> Option<String> {
        if let Some(c) = key.strip_prefix("area:") {
            return Some(cx.areas.get(c).cloned().unwrap_or_else(|| c.to_string()));
        }
        if let Some(w) = key.strip_prefix("who:") {
            return Some(format!("Rozmowy: {}", self.names_en().get(w).cloned().unwrap_or_else(|| w.to_string())));
        }
        if let Some(g) = key.strip_prefix("talk:") {
            return Some(format!("Rozmowa: {g}"));
        }
        key.strip_prefix("scene:").map(|s| s.to_string())
    }

    fn row_extra(&self, _cx: &Ctx, e: &Entry) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("who".into(), json!(self.who_label(e.speaker.as_deref().unwrap_or(""))));
        let st = if e.src == "y" { self.line_stage.get().unwrap().get(&e.key).cloned() } else { None };
        m.insert("stage".into(), st.unwrap_or(Value::Null));
        m
    }

    fn names(&self, cx: &Ctx) -> Vec<Value> {
        static NR: OnceLock<Regex> = OnceLock::new();
        let r = rx(&NR, r"(QuestName|_Quest_Name|RoomName|_Name)$");
        let mut out: Vec<Value> = cx
            .entries
            .iter()
            .filter(|e| e.src == "g" && (e.kind == "name" || e.kind == "item" || r.is_match(&e.key)))
            .map(|e| json!({"id": e.id, "key": e.key, "group": e.kind, "area": cx.areas.get(&e.area).cloned().unwrap_or(e.area.clone()), "en": e.en, "pl": cx.store.pl(&e.id)}))
            .collect();
        // dialogue speakers without a name_* entry: "s:<English name>"
        let known: BTreeSet<&String> = self.names_en().values().collect();
        let mut cnt: Vec<(String, usize)> = Vec::new();
        for e in cx.entries.iter().filter(|e| e.src == "y" || e.src == "n") {
            let Some(sp) = e.speaker.as_deref().filter(|s| !s.is_empty()) else { continue };
            match cnt.iter_mut().find(|x| x.0 == sp) {
                Some(x) => x.1 += 1,
                None => cnt.push((sp.to_string(), 1)),
            }
        }
        cnt.sort_by(|a, b| b.1.cmp(&a.1)); // stable: most common first, ties in first-seen order
        for (name, n) in cnt {
            if !known.contains(&name) && name != "???" {
                let id = format!("s:{name}");
                out.push(json!({"id": id, "key": format!("{n} kwestii"), "group": "speaker", "area": "dialogi", "en": name, "pl": cx.store.pl(&id)}));
            }
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
                continue; // cheap pre-check before the word-boundary regex
            }
            if Regex::new(&format!(r"\b{}\b", regex::escape(&name.to_lowercase()))).map_or(false, |r| r.is_match(&low)) {
                out.push(json!({"en": name, "pl": cx.store.pl(&e.id), "key": e.key, "src": "game"}));
            }
        }
        out
    }

    /// translations/pl layout, like the workshop tabs: one file per escape room, campus talks per place,
    /// the rest by kind.
    fn store_file(&self, id: &str, e: Option<&Entry>) -> String {
        let Some(e) = e else {
            return if id.starts_with("s:") { "nazwy.json".into() } else { "inne.json".into() };
        };
        match e.src.as_str() {
            "y" => "scenki.json".into(),
            "b" => "okrzyki.json".into(),
            "c" => "napisy-koncowe.json".into(),
            "n" => {
                // talks are named after a place ("HUM_…") or a character ("Gil_…"): one file per name with
                // enough lines, the small ones together
                let prefix = e.node.as_deref().unwrap_or("").split('_').next().unwrap_or("").to_lowercase();
                let n = self.talk_prefixes.get().and_then(|m| m.get(&prefix)).copied().unwrap_or(0);
                if n >= 40 && prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
                    format!("rozmowy/{prefix}.json")
                } else {
                    "rozmowy/inne.json".into()
                }
            }
            _ if ROOM_CODES.contains(&e.area.as_str()) => format!("pokoje/{}.json", e.area),
            _ if e.kind == "name" => "nazwy.json".into(),
            _ if CAMPUS_CODES.contains(&e.area.as_str()) => format!("kampus/{}.json", e.area),
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
        let game = crate::patch::Game { content: self.content.clone() };
        let store = crate::store::Store::load(&root.join("translations"))?;
        let mut originals = crate::originals::open(&root.join("backup"))?;
        crate::patch::run(
            crate::patch::Options { game: &game, store: &store, fonts_dir: &root.join("fonts"), fonts: None, originals: &mut originals, out_dir: None },
            log,
        )
    }

    fn launch(&self) -> Result<()> {
        let ps = "$p = Get-AppxPackage | Where-Object { $_.InstallLocation -like '*Escape Academy 2*' } | Select-Object -First 1; \
                  if (-not $p) { exit 2 }; $id = ($p | Get-AppxPackageManifest).Package.Applications.Application.Id | Select-Object -First 1; \
                  Start-Process ('shell:AppsFolder\\' + $p.PackageFamilyName + '!' + $id)";
        use std::os::windows::process::CommandExt;
        let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", ps]).creation_flags(0x0800_0000).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return Err(anyhow!("Nie udało się uruchomić gry przez aplikację Xbox. Uruchom ją ręcznie."));
        }
        Ok(())
    }
}
