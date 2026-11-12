//! Bielik through a local Ollama: translation variants and ghost-text continuation.
use crate::{lang, s, Workshop};
use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::time::Duration;

const OLLAMA: &str = "http://127.0.0.1:11434";

pub fn model() -> Option<String> {
    let r: Value = ureq::get(&format!("{OLLAMA}/api/tags")).timeout(Duration::from_secs(2)).call().ok()?.into_json().ok()?;
    let models: Vec<String> = r.get("models")?.as_array()?.iter().filter_map(|m| m.get("name")?.as_str().map(String::from)).collect();
    models.iter().find(|m| m.to_lowercase().contains("bielik")).cloned().or_else(|| models.first().cloned())
}

pub fn generate(prompt: &str, temp: f64, n_predict: Option<u32>) -> Result<String> {
    let m = model().ok_or_else(|| anyhow!("Bielik nie działa: uruchom Ollamę (warsztat robi to sam, jeśli jest zainstalowana)."))?;
    let mut opts = json!({"temperature": temp});
    if let Some(n) = n_predict {
        opts["num_predict"] = json!(n);
    }
    let r: Value = ureq::post(&format!("{OLLAMA}/api/generate"))
        .timeout(Duration::from_secs(180))
        .send_json(json!({"model": m, "prompt": prompt, "stream": false, "options": opts}))?
        .into_json()?;
    Ok(r.get("response").and_then(|x| x.as_str()).unwrap_or("").trim().to_string())
}

/// The game's base instruction, the glossary rules and the glossary terms found in this line.
fn style_for(ws: &Workshop, en: &str) -> String {
    let g = lang::glossary_file(ws);
    let rules: Vec<String> = g.get("rules").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect()).unwrap_or_default();
    let terms: Vec<String> = ws
        .with_ctx(|cx| lang::glossary(ws, cx, en))
        .iter()
        .filter(|t| s(t, "src") == "glossary" || !s(t, "pl").is_empty())
        .map(|t| format!("{} = {}", s(t, "en"), s(t, "pl")))
        .collect();
    ws.game.style() + &rules.join(" ") + &if terms.is_empty() { " ".to_string() } else { format!(" Słowniczek: {}. ", terms.join("; ")) }
}

pub fn suggest(ws: &Workshop, d: &Value) -> Result<Value> {
    let e = ws.by_id.get(s(d, "id")).map(|&i| &ws.entries[i]).ok_or_else(|| anyhow!("Nieznany wpis."))?;
    let ctx: Vec<String> = d.get("context").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut keep: Vec<String> = ws
        .with_ctx(|cx| lang::glossary(ws, cx, &e.en))
        .iter()
        .filter(|t| s(t, "src") == "glossary" && s(t, "en") == s(t, "pl"))
        .map(|t| s(t, "en").to_string())
        .collect();
    for n in ws.game.keep_english() {
        if e.en.to_lowercase().contains(&n.to_lowercase()) && !keep.contains(&n) {
            keep.push(n);
        }
    }
    let mut p = String::new();
    if !keep.is_empty() {
        p += &format!("WAŻNE: nazwy {} zostaw po angielsku, bez tłumaczenia (tylko odmień). ", keep.join(", "));
    }
    p += &style_for(ws, &e.en);
    p += "Podaj dokładnie trzy różne warianty tłumaczenia, każdy w osobnym wierszu. Bez nagłówków typu „Wariant 1”, bez numeracji i komentarzy, tylko same tłumaczenia.\n";
    if !ctx.is_empty() {
        p += &format!("Wcześniejsze kwestie:\n{}\n", ctx.join("\n"));
    }
    if let Some(sp) = e.speaker.as_deref().filter(|x| !x.is_empty()) {
        p += &format!("Mówi: {sp}\n");
    }
    if !e.ctx.is_empty() {
        p += &format!("Notatka twórców: {}\n", e.ctx);
    }
    if !s(d, "pl").is_empty() {
        p += &format!("Obecne tłumaczenie: {}\n", s(d, "pl"));
    }
    if !s(d, "hint").is_empty() {
        p += &format!("Wskazówka: {}\n", s(d, "hint"));
    }
    p += &format!("Tekst: {}", e.en);
    let out = generate(&p, 0.8, None)?;
    let head = Regex::new(r"(?i)^\s*(\*\*)?\s*(wariant|opcja|propozycja|tłumaczenie)\s*\d*\s*[:.)-]?\s*(\*\*)?\s*").unwrap();
    let num = Regex::new(r"^\s*(\d+[.)]|[-*•])\s*").unwrap();
    let only = Regex::new(r"(?i)^(wariant|opcja)\s*\d*:?$").unwrap();
    let mut variants = Vec::new();
    for line in out.lines() {
        let v = head.replace(line, "");
        let v = num.replace(&v, "");
        let v = v.trim().trim_matches(|c| "\"„”*".contains(c)).trim().to_string();
        if !v.is_empty() && !only.is_match(&v) {
            variants.push(v);
        }
    }
    variants.truncate(3);
    Ok(json!({"variants": variants}))
}

pub fn cont(ws: &Workshop, d: &Value) -> Result<Value> {
    let en = ws.by_id.get(s(d, "id")).map(|&i| ws.entries[i].en.clone()).unwrap_or_default();
    let prefix = s(d, "prefix");
    let p = style_for(ws, &en) + &format!("Dokończ polskie tłumaczenie. Odpowiedz tylko brakującą końcówką, bez powtarzania początku.\nAngielski: {en}\nPolski (początek): {prefix}");
    let mut text = generate(&p, 0.3, Some(60))?.split('\n').next().unwrap_or("").to_string();
    if let Some(rest) = text.strip_prefix(prefix) {
        text = rest.to_string(); // the model sometimes repeats the start
    }
    Ok(json!({"text": text}))
}
