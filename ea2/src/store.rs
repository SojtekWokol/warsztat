//! The Polish translations: translations/pl.json from the workshop, {id: {"pl": text, ...}}.
//! Every non-empty text counts, also a single space: lines the developers want blank outside English
//! ("INTENTIONALLY LEFT BLANK") are translated as " ", and that must reach the game.
use anyhow::Result;
use std::collections::HashMap;

pub struct Store {
    map: HashMap<String, String>,
}

impl Store {
    /// A translations/pl folder (all files), a translations folder (its pl/, or an old pl.json),
    /// or one JSON file.
    pub fn load(path: &std::path::Path) -> Result<Store> {
        let p = if path.join("pl").is_dir() {
            path.join("pl")
        } else if path.is_dir() && path.join("pl.json").exists() {
            path.join("pl.json")
        } else {
            path.to_path_buf()
        };
        let texts = warsztat::store::read_texts(&p)?;
        Ok(Store { map: texts.into_iter().filter(|(_, v)| !v.is_empty()).collect() })
    }

    /// {id: {"pl": text}} (workshop store) or {id: text} (a release / the installer).
    pub fn from_slice(bytes: &[u8]) -> Result<Store> {
        let v: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut map = HashMap::new();
        if let Some(o) = v.as_object() {
            for (k, e) in o {
                let pl = e.get("pl").and_then(|p| p.as_str()).or_else(|| e.as_str());
                if let Some(pl) = pl {
                    if !pl.is_empty() {
                        map.insert(k.clone(), pl.to_string());
                    }
                }
            }
        }
        Ok(Store { map })
    }
    pub fn get(&self, id: &str) -> Option<&str> {
        self.map.get(id).map(|s| s.as_str())
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.map.iter()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
