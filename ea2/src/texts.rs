//! The game's text tables: GameplayStrings (UI etc.), VOStrings (subtitles), GameCredits,
//! the LocalizationManager language list and Gillian's .srt bark subtitles.
use crate::csv;
use crate::store::Store;
use anyhow::{bail, Result};
use std::collections::HashMap;

const PL_HEADER: &str = "Polish Value (pl)";
const EN_HEADER: &str = "English Value (en)";
pub const POLISH_LETTERS: &str = "ĄĆĘŁŃÓŚŹŻąćęłńóśźż";

/// GameplayStrings: append a Polish column (the game reads column 2 + language index; "pl" is the
/// 8th language). Untranslated keys get the English text.
pub fn gameplay(text: &str, store: &Store) -> Result<(String, usize)> {
    let mut rows = csv::parse(text);
    let header = &mut rows[0];
    if header.iter().any(|h| h == PL_HEADER) {
        bail!("GameplayStrings already has a Polish column - is the input really the original?");
    }
    let en = header.iter().position(|h| h.trim() == EN_HEADER).ok_or_else(|| anyhow::anyhow!("no English column"))?;
    header.push(PL_HEADER.into());
    let width = header.len();
    let mut n = 0;
    for row in rows.iter_mut().skip(1) {
        if row.is_empty() {
            continue;
        }
        let key = row[0].clone();
        let mut value = row.get(en).cloned().unwrap_or_default();
        if key == "DEMO_EFIGS_Characters" {
            value.push_str(POLISH_LETTERS);
        }
        while row.len() < width - 1 {
            row.push(String::new());
        }
        let pl = store.get(&format!("g:{key}"));
        n += pl.is_some() as usize;
        row.push(pl.map(|s| s.to_string()).unwrap_or(value));
    }
    Ok((csv::write(&rows), n))
}

/// English name of every name_* key (to translate the speaker prefix of Yarn lines).
pub fn english_names(gameplay_text: &str) -> HashMap<String, String> {
    let rows = csv::parse(gameplay_text);
    let Some(en) = rows.first().and_then(|h| h.iter().position(|h| h.trim() == EN_HEADER)) else { return HashMap::new() };
    rows.iter()
        .skip(1)
        .filter(|r| r.first().map_or(false, |k| k.to_lowercase().starts_with("name_")))
        .filter_map(|r| Some((r[0].clone(), r.get(en)?.clone())))
        .collect()
}

/// English speaker name -> Polish: translated name_* entries plus the workshop's "s:<name>" entries.
pub fn speaker_map(store: &Store, names_en: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (key, en) in names_en {
        if let Some(pl) = store.get(&format!("g:{key}")) {
            out.insert(en.clone(), pl.to_string());
        }
    }
    for (k, v) in store.iter() {
        if let Some(name) = k.strip_prefix("s:") {
            out.insert(name.to_string(), v.to_string());
        }
    }
    out
}

const VO_TEXT_EN: usize = 10;
const VO_ASSIST_EN: usize = 17;
const PL_INDEX: usize = 7;

/// VOStrings: the game reads language i's subtitle from column 10 + i and the assisted variant from
/// 17 + i. Polish (7) goes into column 17 (over the English assisted text), assisted Polish is a new
/// column 24. Ids as in the workshop: "v:<File Name>" (+ "#n" for repeats), "row<n>" without a name.
pub fn vo(text: &str, store: &Store) -> Result<(String, usize)> {
    let mut rows = csv::parse(text);
    if rows[0].len() > VO_ASSIST_EN + 7 {
        bail!("VOStrings already has extra columns - is the input the original?");
    }
    let fn_col = rows[0].iter().position(|h| h.trim() == "File Name").ok_or_else(|| anyhow::anyhow!("no File Name column"))?;
    rows[0][VO_TEXT_EN + PL_INDEX] = PL_HEADER.into();
    rows[0].push("Assisted Polish Value (pl)".into());
    let width = rows[0].len();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut n = 0;
    for row in rows.iter_mut().skip(1) {
        if row.is_empty() {
            continue;
        }
        while row.len() < width - 1 {
            row.push(String::new());
        }
        let en = row[VO_TEXT_EN].clone();
        let en_assist = row[VO_ASSIST_EN].clone();
        let pl = if !en.trim().is_empty() {
            let mut fname = row[fn_col].trim().to_string();
            if fname.is_empty() {
                fname = format!("row{}", seen.len());
            }
            let k = *seen.get(&fname).unwrap_or(&0);
            seen.insert(fname.clone(), k + 1);
            let id = if k > 0 { format!("v:{fname}#{k}") } else { format!("v:{fname}") };
            store.get(&id).map(|s| s.to_string())
        } else {
            None
        };
        n += pl.is_some() as usize;
        row[VO_TEXT_EN + PL_INDEX] = pl.clone().unwrap_or(en);
        row.push(pl.unwrap_or(en_assist));
    }
    Ok((csv::write(&rows), n))
}

/// End credits: headings ("## Art Team") and job titles ("Name... Art Director") from "c:<English>".
pub fn credits(text: &str, store: &Store) -> String {
    let pl = |t: &str| store.get(&format!("c:{t}")).map(|s| s.to_string()).unwrap_or_else(|| t.to_string());
    text.split('\n')
        .map(|line| {
            let (body, cr) = match line.strip_suffix('\r') {
                Some(b) => (b, "\r"),
                None => (line, ""),
            };
            let out = if body.starts_with('#') {
                let title = body.trim_start_matches('#').trim();
                let hashes = &body[..body.len() - body.trim_start_matches('#').len()];
                if title.is_empty() { body.to_string() } else { format!("{hashes} {}", pl(title)) }
            } else if let Some((name, role)) = body.split_once("...") {
                if role.trim().is_empty() { body.to_string() } else { format!("{name}... {}", pl(role.trim())) }
            } else {
                body.to_string()
            };
            out + cr
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// LocalizationManager raw data: MonoBehaviour header, then supportedLanguageCodes (count + aligned
/// strings). Adds `code` at the end.
pub fn add_language(raw: &[u8], code: &str) -> Result<Vec<u8>> {
    let rd_i32 = |p: usize| i32::from_le_bytes(raw[p..p + 4].try_into().unwrap());
    let rd_str = |p: usize| -> (String, usize) {
        let n = rd_i32(p) as usize;
        let s = String::from_utf8_lossy(&raw[p + 4..p + 4 + n]).into_owned();
        let end = p + 4 + n;
        (s, (end + 3) / 4 * 4)
    };
    let (_, mut pos) = rd_str(12 + 4 + 12);
    let count_at = pos;
    let count = rd_i32(pos) as usize;
    pos += 4;
    let mut codes = Vec::new();
    for _ in 0..count {
        let (s, p) = rd_str(pos);
        codes.push(s);
        pos = p;
    }
    if codes.len() < 7 || codes[..7] != ["en", "es", "fr", "it", "de", "zh", "ja"] {
        bail!("Unexpected LocalizationManager layout: {codes:?}");
    }
    if codes.iter().any(|c| c == code) {
        return Ok(raw.to_vec());
    }
    let mut out = raw[..count_at].to_vec();
    out.extend_from_slice(&((count + 1) as i32).to_le_bytes());
    out.extend_from_slice(&raw[count_at + 4..pos]);
    out.extend_from_slice(&(code.len() as i32).to_le_bytes());
    out.extend_from_slice(code.as_bytes());
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(&raw[pos..]);
    Ok(out)
}

/// One bark .srt: replace the first text line with Polish, keeping a leading "[TAG] ".
pub fn srt(src: &str, pl: &str) -> String {
    let mut done = false;
    src.split('\n')
        .map(|line| {
            let body = line.trim_end_matches('\r');
            if !done && !body.trim().is_empty() && !body.contains("-->") && !body.trim().chars().all(|c| c.is_ascii_digit()) {
                done = true;
                let tag = if body.starts_with('[') { body.find(']').map(|i| { let mut e = i + 1; while body[e..].starts_with(char::is_whitespace) { e += 1; } &body[..e] }).unwrap_or("") } else { "" };
                format!("{tag}{pl}{}", if line.ends_with('\r') { "\r" } else { "" })
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
