//! Translation help: memory (how we translated before), word completion, glossary card.
use crate::store::Store;
use crate::{Ctx, Workshop};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

pub fn memory(cx: &Ctx, q: &str) -> Value {
    let ql = q.trim().to_lowercase();
    if ql.chars().count() < 2 {
        return json!({"hits": [], "total": 0});
    }
    let mut hits: Vec<(String, String)> = cx
        .store
        .map
        .iter()
        .filter_map(|(id, r)| {
            let e = cx.entry(id)?;
            (!r.pl.is_empty() && (e.en.to_lowercase().contains(&ql) || r.pl.to_lowercase().contains(&ql))).then(|| (e.en.clone(), r.pl.clone()))
        })
        .collect();
    hits.sort_by_key(|h| h.0.chars().count());
    let total = hits.len();
    hits.truncate(60);
    json!({"hits": hits, "total": total})
}

static WORD: OnceLock<Regex> = OnceLock::new();

/// The most frequent word of our translations that continues `part`.
pub fn word(store: &Store, part: &str) -> String {
    if part.chars().count() < 2 {
        return String::new();
    }
    let re = WORD.get_or_init(|| Regex::new(r"[A-Za-zĄĆĘŁŃÓŚŹŻąćęłńóśźż]{3,}").unwrap());
    let mut c: HashMap<&str, usize> = HashMap::new();
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut n = 0;
    for r in store.map.values() {
        for m in re.find_iter(&r.pl) {
            *c.entry(m.as_str()).or_insert(0) += 1;
            first.entry(m.as_str()).or_insert_with(|| { n += 1; n });
        }
    }
    let pl = part.to_lowercase();
    let best = c
        .iter()
        .filter(|(w, _)| w.to_lowercase().starts_with(&pl) && w.chars().count() > part.chars().count())
        // most common first; ties: first seen (like Counter.most_common)
        .max_by(|a, b| a.1.cmp(b.1).then(first[b.0].cmp(&first[a.0])))
        .map(|(w, _)| *w);
    best.map(|w| w.chars().skip(part.chars().count()).collect()).unwrap_or_default()
}

/// translations/glossary.json, read on every call so edits apply at once.
pub fn glossary_file(ws: &Workshop) -> Value {
    std::fs::read(ws.root.join("translations").join("glossary.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({"rules": [], "terms": []}))
}

/// Glossary terms, then names and titles from the game, that appear in this line (longest first, 12).
pub fn glossary(ws: &Workshop, cx: &Ctx, en: &str) -> Vec<Value> {
    let low = en.to_lowercase();
    let mut terms: Vec<Value> = Vec::new();
    for t in glossary_file(ws).get("terms").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
        let ten = t.get("en").and_then(|x| x.as_str()).unwrap_or("");
        if ten.is_empty() {
            continue;
        }
        let Ok(r) = Regex::new(&format!(r"\b{}s?\b", regex::escape(&ten.to_lowercase()))) else { continue };
        if r.is_match(&low) {
            terms.push(json!({"en": ten, "pl": t.get("pl").cloned().unwrap_or(json!("")),
                "note": t.get("note").cloned().unwrap_or(json!("")),
                "draft": t.get("status").and_then(|x| x.as_str()) != Some("ok"), "src": "glossary"}));
        }
    }
    terms.extend(ws.game.game_terms(cx, en));
    // longest first (stable), unique by English
    let mut idx: Vec<usize> = (0..terms.len()).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse(terms[i].get("en").and_then(|x| x.as_str()).map_or(0, |s| s.chars().count())));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for i in idx {
        let k = terms[i].get("en").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if seen.insert(k) {
            out.push(terms[i].clone());
        }
    }
    out.truncate(12);
    out
}
