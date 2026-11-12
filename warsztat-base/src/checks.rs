//! Checks of one translated line against the original: tags and {codes}, length, spacing, gendered
//! forms addressed to the player, letter-list puzzle answers. Tags: TextMeshPro-style <…> (names and
//! values compared case-insensitively) and {…} placeholders (exact).
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

static TAG: OnceLock<Regex> = OnceLock::new();
static TAG_INNER: OnceLock<Regex> = OnceLock::new();
static OPEN_CLOSE: OnceLock<Regex> = OnceLock::new();
static GENDER: OnceLock<Regex> = OnceLock::new();
static LETTERS_EN: OnceLock<Regex> = OnceLock::new();
static LETTERS_PL: OnceLock<Regex> = OnceLock::new();

const PAIRED: &[&str] = &[
    "style", "i", "b", "u", "s", "size", "color", "link", "mspace", "sub", "sup", "mark", "align", "font", "cspace", "voffset", "indent",
    "lowercase", "uppercase", "smallcaps", "nobr", "noparse", "alpha", "pos", "width", "line-height", "margin", "rotate",
];

pub fn tag_regex() -> &'static Regex {
    re(&TAG, r"<[^<>]+>|\{[^{}]+\}")
}

pub fn norm_tag(t: &str) -> String {
    if t.starts_with('{') {
        return t.to_string();
    }
    let inner = t[1..t.len() - 1].trim();
    let r = re(&TAG_INNER, r"^(/?)\s*([A-Za-z-]+)\s*(?:=\s*(.*))?$");
    let Some(c) = r.captures(inner) else { return t.to_string() };
    let close = c.get(1).map_or("", |m| m.as_str());
    let name = c[2].to_lowercase();
    let val = c.get(3).map_or(String::new(), |m| m.as_str().trim().trim_matches(|ch| ch == '"' || ch == '\'').trim().to_lowercase());
    if val.is_empty() { format!("<{close}{name}>") } else { format!("<{close}{name}={val}>") }
}

pub fn tag_list(s: &str) -> Vec<String> {
    tag_regex().find_iter(s).map(|m| norm_tag(m.as_str())).collect()
}

/// The tags of a text as one string (normalized, in order): what an accepted deviation is tied to.
pub fn tag_signature(s: &str) -> String {
    tag_list(s).join(" ")
}

fn counts(v: &[String]) -> BTreeMap<String, i64> {
    let mut m = BTreeMap::new();
    for t in v {
        *m.entry(t.clone()).or_insert(0) += 1;
    }
    m
}

pub fn nesting_errors(tags: &[String]) -> Vec<String> {
    let r = re(&OPEN_CLOSE, r"^<(/?)([a-z-]+)");
    let mut stack: Vec<String> = Vec::new();
    let mut errs = Vec::new();
    for t in tags {
        let Some(c) = r.captures(t) else { continue };
        let name = c[2].to_string();
        if !PAIRED.contains(&name.as_str()) {
            continue;
        }
        if c[1].is_empty() {
            stack.push(name);
        } else if stack.last() == Some(&name) {
            stack.pop();
        } else if let Some(i) = stack.iter().position(|x| *x == name) {
            errs.push(format!("</{name}> zamyka przed </{}>", stack.last().unwrap()));
            stack.remove(i);
        } else {
            errs.push(format!("</{name}> bez otwarcia"));
        }
    }
    errs.extend(stack.iter().map(|n| format!("<{n}> bez zamknięcia")));
    errs
}

fn elements(m: &BTreeMap<String, i64>) -> String {
    m.iter().flat_map(|(k, &n)| std::iter::repeat(k.clone()).take(n.max(0) as usize)).collect::<Vec<_>>().join(" ")
}

pub fn tag_problems(en: &str, pl: &str) -> Vec<String> {
    if pl.trim().is_empty() {
        return vec![];
    }
    let (te, tp) = (tag_list(en), tag_list(pl));
    let (a, b) = (counts(&te), counts(&tp));
    let diff = |x: &BTreeMap<String, i64>, y: &BTreeMap<String, i64>| -> BTreeMap<String, i64> {
        x.iter().filter_map(|(k, &n)| { let d = n - y.get(k).copied().unwrap_or(0); (d > 0).then(|| (k.clone(), d)) }).collect()
    };
    let (missing, extra) = (diff(&a, &b), diff(&b, &a));
    let mut probs = Vec::new();
    if !missing.is_empty() {
        probs.push(format!("brakuje: {}", elements(&missing)));
    }
    if !extra.is_empty() {
        probs.push(format!("nadmiar: {}", elements(&extra)));
    }
    if missing.is_empty() && extra.is_empty() && nesting_errors(&te).is_empty() {
        probs.extend(nesting_errors(&tp));
    }
    probs
}

pub fn check(en: &str, pl: &str) -> Vec<String> {
    let mut probs = Vec::new();
    if pl.trim().is_empty() {
        return probs;
    }
    probs.extend(tag_problems(en, pl).into_iter().map(|p| format!("znaczniki: {p}")));
    let (le, lp) = (en.chars().count(), pl.chars().count());
    if le > 12 && lp as f64 > le as f64 * 1.45 {
        probs.push(format!("o {}% dłuższe", (100.0 * (lp as f64 / le as f64 - 1.0)).round() as i64));
    }
    if pl.contains("  ") {
        probs.push("podwójna spacja".into());
    }
    if re(&GENDER, r"(?i)\w(łeś|łaś|łabyś|łbyś)").is_match(pl) {
        probs.push("forma z rodzajem w 2. osobie – jeśli to do gracza, przerób na neutralną".into());
    }
    if re(&LETTERS_EN, r"^[A-Z0-9](,[A-Z0-9])+$").is_match(en.trim()) {
        if !re(&LETTERS_PL, r"^[A-ZĄĆĘŁŃÓŚŹŻ0-9](,[A-ZĄĆĘŁŃÓŚŹŻ0-9])+$").is_match(pl.trim()) {
            probs.push("hasło ma mieć postać L,I,T,E,R,Y".into());
        } else if pl.split(',').count() != en.split(',').count() {
            probs.push("inna liczba liter niż w oryginale".into());
        }
    }
    probs
}
