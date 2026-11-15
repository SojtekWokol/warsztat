//! Smoke test of the EA1 workshop adapter without the window: load, list tabs, open a scene page.
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let root = ea1::project_root().expect("ea1/translations");
    let ws = warsztat::Workshop::open(&root, Arc::new(ea1::workshop::Ea1::default()))?;
    println!("entries: {}", ws.entries.len());
    let mut by_src = std::collections::BTreeMap::new();
    for e in &ws.entries {
        *by_src.entry(e.src.clone()).or_insert(0) += 1;
    }
    println!("by source: {by_src:?}");
    for tab in ["scenes", "talks", "rooms", "campus", "other"] {
        let v = ws.api("pages", &serde_json::json!({"tab": tab}));
        let a = v.as_array().cloned().unwrap_or_default();
        println!("{tab}: {} rows, first: {}", a.len(), a.first().map(|x| format!("{} {} {}/{}", x["key"], x.get("place").or(x.get("name")).unwrap_or(&serde_json::Value::Null), x["done"], x["total"])).unwrap_or_default());
    }
    let p = ws.api("page", &serde_json::json!({"key": "scene:TUT_Intro"}));
    for r in p["rows"].as_array().into_iter().flatten().take(4) {
        println!("  {} | {} | {} | {}", r["id"], r["who"], r["en"], r["pl"]);
    }
    let n = ws.api("names", &serde_json::json!({}));
    println!("names: {}", n.as_array().map_or(0, |a| a.len()));
    Ok(())
}
