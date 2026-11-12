//! Corrections files: export our translations for someone, review a file someone sent, accept chosen lines.
use crate::{s, Ctx, Workshop};
use anyhow::Result;
use serde_json::{json, Value};

pub fn export(ws: &Workshop, author: &str) -> Result<Value> {
    let dir = ws.root.join("poprawki");
    std::fs::create_dir_all(&dir)?;
    let nick: String = if author.is_empty() { "anonim".to_string() } else { author.to_string() };
    let nick: String = nick.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let nick: String = collapse_underscores(&nick).chars().take(30).collect();
    let now = chrono::Local::now();
    let path = dir.join(format!("poprawki_{nick}_{}.json", now.format("%Y-%m-%d")));
    // only what differs from the latest release (all texts when there is no release yet): small files,
    // easy to attach to a GitHub issue and to review
    let base = crate::snapshots::releases(&ws.root).first().and_then(|r| crate::snapshots::release_texts(&ws.root, &r.version).map(|t| (r.version.clone(), t)));
    let items: Vec<Value> = {
        let st = ws.store.lock().unwrap();
        st.map
            .iter()
            .filter(|(id, r)| base.as_ref().map_or(true, |(_, t)| t.get(*id).map_or("", |s| s.as_str()) != r.pl))
            .filter_map(|(id, r)| ws.by_id.get(id).map(|&i| json!({"id": id, "en": ws.entries[i].en, "pl": r.pl, "status": r.status})))
            .collect()
    };
    if items.is_empty() {
        anyhow::bail!("Nie masz zmian względem wydania {} – nie ma czego eksportować.", base.map(|b| b.0).unwrap_or_default());
    }
    let doc = json!({"author": author, "created": now.format("%Y-%m-%d %H:%M").to_string(), "game": ws.game.name(),
                     "base": base.as_ref().map(|b| b.0.clone()), "items": items});
    std::fs::write(&path, crate::store::to_python_json(&doc)?)?;
    let n = doc["items"].as_array().map_or(0, |a| a.len());
    ws.say(format!("Zapisano {n} tłumaczeń w {}. Wyślij ten plik.", path.display()));
    Ok(json!({"path": path.to_string_lossy(), "lines": n}))
}

fn collapse_underscores(s: &str) -> String {
    // Python: re.sub(r"[^\w-]+", "_", …) turns a run of other characters into one "_"
    let mut out = String::new();
    let mut prev_sub = false;
    for c in s.chars() {
        if c == '_' && prev_sub {
            continue;
        }
        prev_sub = c == '_';
        out.push(c);
    }
    out
}

pub fn review(ws: &Workshop, cx: &Ctx, payload: &Value) -> Value {
    let items: Vec<Value> = payload
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|x| {
            let id = s(x, "id");
            let e = cx.entry(id);
            let cur = cx.store.map.get(id).map(|r| r.pl.clone());
            let pl = s(x, "pl");
            let status = match e {
                None => "zła",
                Some(_) if pl.trim().is_empty() => "zła",
                Some(e) if e.en != s(x, "en") => "zła",
                _ if cur.as_deref() == Some(pl) => "jest",
                _ if cur.as_deref().map_or(false, |c| !c.is_empty()) => "konflikt",
                _ => "nowa",
            };
            let problems = if e.is_some() { ws.check(s(x, "en"), pl) } else { vec!["nieznany wpis".to_string()] };
            json!({"id": id, "en": s(x, "en"), "pl": pl, "current": cur, "status": status, "problems": problems})
        })
        .collect();
    json!({"author": payload.get("author"), "created": payload.get("created"), "items": items})
}

pub fn accept(ws: &Workshop, d: &Value) -> Result<Value> {
    let ids: Vec<&str> = d.get("ids").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let mut taken = 0;
    {
        let mut st = ws.store.lock().unwrap();
        for x in d.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
            let id = s(&x, "id");
            if ids.contains(&id) && ws.by_id.contains_key(id) {
                st.set(id, s(&x, "pl"), "review")?;
                taken += 1;
            }
        }
        st.save()?;
    }
    ws.say(format!("Przyjęto {taken} poprawek (status: do sprawdzenia)."));
    Ok(json!({"lines": taken}))
}
