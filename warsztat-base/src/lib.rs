//! Shared core of the translation workshops. A game plugs in through [`Game`]; everything the UI
//! calls goes through [`Workshop::api`] with the same routes the UI has always used ("pages",
//! "page", "save", …), so the desktop shell needs a single command.
pub mod checks;
pub mod corrections;
pub mod lang;
pub mod ollama;
pub mod snapshots;
pub mod store;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use store::Store;

/// One translatable text of the game.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Entry {
    pub id: String,
    pub src: String,
    pub key: String,
    pub area: String,
    pub kind: String,
    #[serde(default)]
    pub ctx: String,
    pub en: String,
    #[serde(rename = "ref", default)]
    pub refs: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub speaker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub order: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub node: Option<String>,
}

/// Read-only view the game adapter gets: all entries, area names and the store.
pub struct Ctx<'a> {
    pub entries: &'a [Entry],
    pub by_id: &'a HashMap<String, usize>,
    pub areas: &'a BTreeMap<String, String>,
    pub store: &'a Store,
}

impl<'a> Ctx<'a> {
    pub fn entry(&self, id: &str) -> Option<&'a Entry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }
    pub fn done_count(&self, es: &[&Entry]) -> usize {
        es.iter().filter(|e| !self.store.pl(&e.id).trim().is_empty()).count()
    }
}

/// What a game adds to the shared workshop.
pub trait Game: Send + Sync {
    /// Game name, for corrections files and prompts.
    fn name(&self) -> &str;
    /// Every translatable text and the display names of the areas.
    fn load(&self, root: &Path) -> Result<(Vec<Entry>, BTreeMap<String, String>)>;
    /// The rows of a list tab.
    fn pages(&self, cx: &Ctx, tab: &str) -> Vec<Value>;
    /// Entries of a page key the game defines ("scene:…", "area:…"); None = not a game key.
    fn page_entries<'a>(&self, cx: &Ctx<'a>, key: &str) -> Option<Vec<&'a Entry>>;
    /// Title of a game page key.
    fn page_title(&self, cx: &Ctx, key: &str) -> Option<String>;
    /// Extra fields of an editor row (speaker shown, preview data).
    fn row_extra(&self, cx: &Ctx, e: &Entry) -> Map<String, Value>;
    /// The names tab.
    fn names(&self, cx: &Ctx) -> Vec<Value>;
    /// Names and titles from the game that appear in a line (glossary card).
    fn game_terms(&self, cx: &Ctx, en: &str) -> Vec<Value>;
    /// Base instruction for Bielik.
    fn style(&self) -> String;
    /// Names Bielik must keep in English when they appear.
    fn keep_english(&self) -> Vec<String> {
        vec![]
    }
    /// File (relative to translations/pl/) that a text belongs in; `e` is None for ids that are not
    /// game texts (e.g. speaker names "s:…"). Group by where texts are in the game.
    fn store_file(&self, _id: &str, _e: Option<&Entry>) -> String {
        "teksty.json".into()
    }
    /// Extra checks of a line.
    fn extra_checks(&self, _en: &str, _pl: &str) -> Vec<String> {
        vec![]
    }
    /// Write the translation into the game.
    fn build(&self, root: &Path, log: &mut dyn FnMut(String)) -> Result<()>;
    /// Start the game.
    fn launch(&self) -> Result<()>;
}

#[derive(Default, Serialize, Clone)]
pub struct BuildState {
    pub running: bool,
    pub ok: Option<bool>,
}

pub struct Workshop {
    pub root: PathBuf,
    pub game: Arc<dyn Game>,
    pub entries: Vec<Entry>,
    pub by_id: HashMap<String, usize>,
    pub areas: BTreeMap<String, String>,
    pub store: Mutex<Store>,
    pub log: Mutex<VecDeque<String>>,
    pub build: Mutex<BuildState>,
}

impl Workshop {
    pub fn open(root: &Path, game: Arc<dyn Game>) -> Result<Arc<Workshop>> {
        let (entries, areas) = game.load(root)?;
        let by_id: HashMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.id.clone(), i)).collect();
        let (g2, lookup) = (game.clone(), Arc::new(entries.iter().map(|e| (e.id.clone(), e.clone())).collect::<HashMap<_, _>>()));
        let placer: store::Placer = Box::new(move |id| g2.store_file(id, lookup.get(id)));
        let store = Store::load(&root.join("translations"), placer)?;
        let ws = Arc::new(Workshop {
            root: root.to_path_buf(),
            game,
            entries,
            by_id,
            areas,
            store: Mutex::new(store),
            log: Mutex::new(VecDeque::new()),
            build: Mutex::new(BuildState::default()),
        });
        snapshots::migrate(&ws);
        ws.say(format!("Warsztat gotowy: {} tekstów.", ws.entries.len()));
        Ok(ws)
    }

    pub fn say(&self, msg: impl Into<String>) {
        let mut l = self.log.lock().unwrap();
        l.push_back(format!("{} {}", chrono::Local::now().format("%H:%M"), msg.into()));
        while l.len() > 300 {
            l.pop_front();
        }
    }

    pub fn with_ctx<R>(&self, f: impl FnOnce(&Ctx) -> R) -> R {
        let store = self.store.lock().unwrap();
        let cx = Ctx { entries: &self.entries, by_id: &self.by_id, areas: &self.areas, store: &store };
        f(&cx)
    }

    pub fn check(&self, en: &str, pl: &str) -> Vec<String> {
        let mut p = checks::check(en, pl);
        if !pl.trim().is_empty() {
            p.extend(self.game.extra_checks(en, pl));
        }
        p
    }

    fn page_entries<'a>(&self, cx: &Ctx<'a>, key: &str) -> Vec<&'a Entry> {
        if let Some(v) = self.game.page_entries(cx, key) {
            return v;
        }
        // every text with a status: "!status:review" (to check), "!status:done", "!status:draft"
        if let Some(st) = key.strip_prefix("!status:") {
            return cx.entries.iter().filter(|e| cx.store.map.get(&e.id).map_or(false, |r| r.status == st)).collect();
        }
        if key == "!znaczniki" {
            return cx
                .entries
                .iter()
                .filter(|e| !checks::tag_problems(&e.en, cx.store.pl(&e.id)).is_empty() && !cx.store.tags_accepted(&e.id, cx.store.pl(&e.id)))
                .collect();
        }
        if let Some(e) = cx.entry(key) {
            return vec![e];
        }
        let q = key.trim().to_lowercase();
        if let Some(e) = cx.entries.iter().find(|e| e.key.to_lowercase() == q) {
            return vec![e];
        }
        // a text search: every word in the key, the English or the Polish text
        let words: Vec<&str> = q.split_whitespace().collect();
        if words.is_empty() {
            return vec![];
        }
        let mut found = Vec::new();
        for e in cx.entries {
            let hay = format!("{} {} {}", e.key, e.en, cx.store.pl(&e.id)).to_lowercase();
            if words.iter().all(|w| hay.contains(w)) {
                found.push(e);
                if found.len() >= 300 {
                    break;
                }
            }
        }
        found
    }

    fn row(&self, cx: &Ctx, e: &Entry) -> Value {
        let mut v = serde_json::to_value(e).unwrap();
        let m = v.as_object_mut().unwrap();
        let rec = cx.store.map.get(&e.id);
        m.insert("pl".into(), rec.map_or(Value::Null, |r| Value::String(r.pl.clone())));
        m.insert("status".into(), Value::String(rec.map_or("todo".into(), |r| r.status.clone())));
        for (k, x) in self.game.row_extra(cx, e) {
            m.insert(k, x);
        }
        v
    }

    fn page(&self, key: &str) -> Value {
        let key = key.trim();
        self.with_ctx(|cx| {
            let es = self.page_entries(cx, key);
            if es.is_empty() {
                return if key == "!znaczniki" {
                    json!({"error": "Znaczniki wszędzie zgadzają się z oryginałem."})
                } else {
                    json!({"error": format!("Nic nie pasuje do: {key}")})
                };
            }
            let rows: Vec<Value> = es.iter().map(|e| self.row(cx, e)).collect();
            let title = if let Some(st) = key.strip_prefix("!status:") {
                let name = match st { "review" => "Do sprawdzenia", "done" => "Zatwierdzone", "draft" => "Szkice", other => other };
                format!("{name}: {}", es.len())
            } else if key == "!znaczniki" {
                format!("Błędy znaczników: {}", es.len())
            } else if let Some(t) = self.game.page_title(cx, key) {
                t
            } else if !key.contains(':') && !(es.len() == 1 && es[0].key.to_lowercase() == key.to_lowercase()) {
                format!("Szukanie „{key}”: {}", es.len()) + if es.len() >= 300 { " (pokazano pierwsze 300)" } else { "" }
            } else {
                key.split_once(':').map_or(key, |x| x.1).to_string()
            };
            json!({"key": key, "title": title, "rows": rows})
        })
    }

    fn save(&self, d: &Value) -> Result<Value> {
        let id = s(d, "id");
        let e = self.by_id.get(id).map(|&i| &self.entries[i]).ok_or_else(|| anyhow!("Nieznany wpis."))?;
        let pl = s(d, "pl");
        let status = if s(d, "status").is_empty() { "draft" } else { s(d, "status") };
        let targets: Vec<&Entry> = if d.get("everywhere").and_then(|x| x.as_bool()).unwrap_or(false) {
            self.entries.iter().filter(|x| x.en == e.en).collect()
        } else {
            vec![e]
        };
        {
            let mut st = self.store.lock().unwrap();
            for t in &targets {
                st.set(&t.id, pl, if pl.trim().is_empty() { "todo" } else { status })?;
            }
            st.save()?;
        }
        let more = if targets.len() > 1 { format!(" i {} takich samych", targets.len() - 1) } else { String::new() };
        self.say(format!("Zapisano {}{more}.", e.key));
        Ok(json!({"ok": true, "count": targets.len()}))
    }

    /// All UI calls: `route` like "page", `d` the query or body fields.
    pub fn api(self: &Arc<Self>, route: &str, d: &Value) -> Value {
        match self.api_inner(route, d) {
            Ok(v) => v,
            Err(e) => json!({"error": e.to_string()}),
        }
    }

    fn api_inner(self: &Arc<Self>, route: &str, d: &Value) -> Result<Value> {
        Ok(match route {
            "pages" => Value::Array(self.with_ctx(|cx| self.game.pages(cx, if s(d, "tab").is_empty() { "scenes" } else { s(d, "tab") }))),
            "page" => self.page(s(d, "key")),
            "state" => {
                let (edits, done) = self.with_ctx(|cx| {
                    (snapshots::edits_since_build(self, cx.store), self.entries.iter().filter(|e| !cx.store.pl(&e.id).is_empty()).count())
                });
                let mut statuses: BTreeMap<String, usize> = BTreeMap::new();
                for r in self.store.lock().unwrap().map.values() {
                    *statuses.entry(r.status.clone()).or_insert(0) += 1;
                }
                json!({"edits": edits, "statuses": statuses, "log": self.log.lock().unwrap().iter().cloned().collect::<Vec<_>>(),
                       "model": ollama::model(), "build": self.build.lock().unwrap().clone(),
                       "total": self.entries.len(), "done": done})
            }
            "releases" => serde_json::to_value(snapshots::releases(&self.root))?,
            "changes" => self.with_ctx(|cx| snapshots::changes(self, cx)),
            "memory" => self.with_ctx(|cx| lang::memory(cx, s(d, "q"))),
            "word" => self.with_ctx(|cx| json!({"rest": lang::word(cx.store, s(d, "part"))})),
            "names" => Value::Array(self.with_ctx(|cx| self.game.names(cx))),
            "check" => {
                // with an id: tag differences accepted for this text (and still the same tags) are not problems
                let mut p = self.check(s(d, "en"), s(d, "pl"));
                let has_tag = p.iter().any(|x| x.starts_with("znaczniki:"));
                let accepted = has_tag && !s(d, "id").is_empty() && self.store.lock().unwrap().tags_accepted(s(d, "id"), s(d, "pl"));
                if accepted {
                    p.retain(|x| !x.starts_with("znaczniki:"));
                }
                json!({"problems": p, "tag_problems": has_tag, "tags_ok": accepted})
            }
            "approve" => {
                // approve drafts (texts read and corrected by a person); "to check" ones stay as they are
                let ids: Vec<String> = d.get("ids").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                let mut n = 0;
                {
                    let mut st = self.store.lock().unwrap();
                    for id in &ids {
                        if let Some(r) = st.map.get(id).cloned() {
                            if r.status == "draft" && !r.pl.is_empty() {
                                st.set(id, &r.pl, "done")?;
                                n += 1;
                            }
                        }
                    }
                    st.save()?;
                }
                self.say(format!("Zatwierdzono {n} kwestii."));
                json!({"ok": true, "count": n})
            }
            "tags_ok" => {
                let (id, ok) = (s(d, "id"), d.get("ok").and_then(|x| x.as_bool()).unwrap_or(true));
                let mut st = self.store.lock().unwrap();
                // the tags of the text being edited (d.pl) or else of the saved text
                let text = if s(d, "pl").is_empty() { st.pl(id).to_string() } else { s(d, "pl").to_string() };
                let sig = ok.then(|| checks::tag_signature(&text));
                st.set_tags_ok(id, sig)?;
                drop(st);
                let key = self.by_id.get(id).map_or(id.to_string(), |&i| self.entries[i].key.clone());
                self.say(if ok { format!("{key}: znaczniki oznaczone jako celowo inne.") } else { format!("{key}: znaczniki znowu sprawdzane.") });
                json!({"ok": true})
            }
            "glossary" => json!({"terms": self.with_ctx(|cx| lang::glossary(self, cx, s(d, "en")))}),
            "suggest" => ollama::suggest(self, d)?,
            "continue" => ollama::cont(self, d)?,
            "save" => self.save(d)?,
            "save_many" => {
                let mut n = 0;
                {
                    let mut st = self.store.lock().unwrap();
                    for it in d.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                        let id = s(&it, "id");
                        if self.by_id.contains_key(id) || id.starts_with("s:") {
                            let status = if s(&it, "status").is_empty() { "draft" } else { s(&it, "status") };
                            st.set(id, s(&it, "pl"), status)?;
                            n += 1;
                        }
                    }
                    st.save()?;
                }
                let label = if s(d, "label").is_empty() { "fala" } else { s(d, "label") };
                self.say(format!("Wczytano {n} tłumaczeń ({label})."));
                json!({"ok": true, "count": n})
            }
            "restore" => {
                let id = s(d, "id");
                self.store.lock().unwrap().update(id, "", "todo")?;
                let key = self.by_id.get(id).map_or(id.to_string(), |&i| self.entries[i].key.clone());
                self.say(format!("Usunięto tłumaczenie {key}."));
                json!({"ok": true})
            }
            "name_save" => {
                let pl = s(d, "pl");
                self.store.lock().unwrap().update(s(d, "id"), pl, if pl.trim().is_empty() { "todo" } else { "draft" })?;
                json!({"ok": true})
            }
            "release" => snapshots::make_release(self, s(d, "version"))?,
            "build" => {
                snapshots::start_build(self)?;
                json!({"ok": true})
            }
            "launch" => {
                self.game.launch()?;
                self.say("Uruchamiam grę.");
                json!({"ok": true})
            }
            "corr_export" => corrections::export(self, s(d, "author"))?,
            "corr_review" => self.with_ctx(|cx| corrections::review(self, cx, d)),
            "corr_accept" => corrections::accept(self, d)?,
            _ => json!({"error": format!("nieznana trasa {route}")}),
        })
    }
}

/// String field of a JSON object ("" when missing).
pub fn s<'a>(d: &'a Value, k: &str) -> &'a str {
    d.get(k).and_then(|x| x.as_str()).unwrap_or("")
}
