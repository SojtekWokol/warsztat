//! What is in the game and what changed: a snapshot of the texts per successful build
//! (translations/builds/<unix>.json, last 10) and released versions (translations/releases/<v>.json).
use crate::store::{to_python_json, Store};
use crate::{Ctx, Workshop};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

type Texts = BTreeMap<String, String>;

pub fn builds(root: &Path) -> Vec<(i64, Texts)> {
    let dir = root.join("translations").join("builds");
    let mut names: Vec<String> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json")).collect()).unwrap_or_default();
    names.sort();
    names.reverse();
    names
        .into_iter()
        .filter_map(|n| Some((n.trim_end_matches(".json").parse().ok()?, serde_json::from_slice(&std::fs::read(dir.join(&n)).ok()?).ok()?)))
        .collect()
}

fn latest_build(root: &Path) -> Option<(i64, Texts)> {
    builds(root).into_iter().next()
}

pub fn save_build_snapshot(root: &Path, snap: &Texts) -> Result<()> {
    let dir = root.join("translations").join("builds");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{}.json", crate::store::now())), serde_json::to_string(snap)?)?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".json")).collect();
    names.sort();
    if names.len() > 10 {
        for n in &names[..names.len() - 10] {
            let _ = std::fs::remove_file(dir.join(n));
        }
    }
    Ok(())
}

fn diff(old: &Texts, new: &Texts) -> BTreeSet<String> {
    old.keys().chain(new.keys()).filter(|k| old.get(*k).map_or("", |s| s) != new.get(*k).map_or("", |s| s)).cloned().collect()
}

#[derive(Serialize)]
pub struct Release {
    pub version: String,
    pub time: i64,
    pub count: usize,
}

pub fn releases(root: &Path) -> Vec<Release> {
    let dir = root.join("translations").join("releases");
    let mut out: Vec<Release> = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let v: Value = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
                    Some(Release { version: v.get("version")?.as_str()?.to_string(), time: v.get("time")?.as_i64()?, count: v.get("texts")?.as_object()?.len() })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|r| -r.time);
    out
}

fn release_texts(root: &Path, version: &str) -> Option<Texts> {
    let v: Value = serde_json::from_slice(&std::fs::read(root.join("translations").join("releases").join(format!("{version}.json"))).ok()?).ok()?;
    serde_json::from_value(v.get("texts")?.clone()).ok()
}

pub fn make_release(ws: &Workshop, version: &str) -> Result<Value> {
    let version = version.trim();
    let ok = version.split('.').count() == 3 && version.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if !ok {
        bail!("Numer wersji w postaci 1.0.0");
    }
    if releases(&ws.root).iter().any(|r| r.version == version) {
        bail!("Wersja {version} już istnieje");
    }
    // a release is what is in the game: the last build
    let texts = latest_build(&ws.root).map(|b| b.1).unwrap_or_else(|| ws.store.lock().unwrap().current_texts());
    let dir = ws.root.join("translations").join("releases");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{version}.json")), serde_json::to_string(&json!({"version": version, "time": crate::store::now(), "texts": texts}))?)?;
    ws.say(format!("Wydano wersję {version} ({} tłumaczeń).", texts.len()));
    Ok(json!({"version": version, "count": texts.len()}))
}

/// Differences from the latest release, each marked as in the game or not yet.
pub fn changes(ws: &Workshop, cx: &Ctx) -> Value {
    let rel = releases(&ws.root);
    let base = rel.first().and_then(|r| release_texts(&ws.root, &r.version));
    let lb = latest_build(&ws.root);
    let built = lb.as_ref().map(|b| b.1.clone()).unwrap_or_default();
    let cur = cx.store.current_texts();
    let (keys, base_) = match &base {
        None => (diff(&built, &cur), built.clone()),
        Some(b) => (diff(b, &cur).union(&diff(b, &built)).cloned().collect(), b.clone()),
    };
    let items: Vec<Value> = keys
        .iter()
        .map(|k| {
            let e = cx.entry(k);
            let short = k.get(2..).unwrap_or(k).to_string();
            json!({"id": k, "key": e.map_or(short.clone(), |e| e.key.clone()), "en": e.map_or(short, |e| e.en.clone()),
                   "old": base_.get(k).cloned().unwrap_or_default(), "new": cur.get(k).cloned().unwrap_or_default(),
                   "in_game": built.get(k).map_or("", |s| s) == cur.get(k).map_or("", |s| s)})
        })
        .collect();
    json!({"release": rel.first(), "releases": rel, "last_build": lb.map(|b| b.0), "items": items})
}

pub fn edits_since_build(ws: &Workshop, store: &Store) -> usize {
    diff(&latest_build(&ws.root).map(|b| b.1).unwrap_or_default(), &store.current_texts()).len()
}

/// Run the game's build in the background; on success remember what was written.
pub fn start_build(ws: &Arc<Workshop>) -> Result<()> {
    {
        let mut b = ws.build.lock().unwrap();
        if b.running {
            bail!("Wgrywanie już trwa.");
        }
        b.running = true;
        b.ok = None;
    }
    let snap = ws.store.lock().unwrap().current_texts();
    let ws = ws.clone();
    std::thread::spawn(move || {
        ws.say("Wgrywanie do gry...");
        let w2 = ws.clone();
        let res = ws.game.build(&ws.root, &mut |m: String| w2.say(format!("  {m}")));
        let ok = match res {
            Ok(()) => {
                let _ = save_build_snapshot(&ws.root, &snap);
                let meta_p = ws.root.join("translations").join("meta.json");
                let mut meta: Value = std::fs::read(&meta_p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
                meta["built_at"] = json!(crate::store::now());
                let _ = std::fs::write(&meta_p, to_python_json(&meta).unwrap_or_default());
                ws.say("Wgrano do gry. Uruchom grę i wybierz język POLSKI.");
                true
            }
            Err(e) => {
                ws.say(format!("Błąd: {e}"));
                ws.say("Wgrywanie nie powiodło się, szczegóły wyżej.");
                false
            }
        };
        let mut b = ws.build.lock().unwrap();
        b.running = false;
        b.ok = Some(ok);
    });
    Ok(())
}

/// First run with snapshots: rebuild the last build from what was saved before it.
pub fn migrate(ws: &Workshop) {
    if !builds(&ws.root).is_empty() {
        return;
    }
    let meta: Value = std::fs::read(ws.root.join("translations").join("meta.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(json!({}));
    if let Some(t) = meta.get("built_at").and_then(|x| x.as_i64()) {
        let st = ws.store.lock().unwrap();
        let snap: Texts = st.map.iter().filter(|(_, r)| !r.pl.is_empty() && r.t <= t).map(|(k, r)| (k.clone(), r.pl.clone())).collect();
        let _ = save_build_snapshot(&ws.root, &snap);
    }
}
