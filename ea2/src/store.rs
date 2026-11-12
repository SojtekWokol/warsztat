//! The Polish translations: translations/pl.json from the workshop, {id: {"pl": text, ...}}.
//! Only entries with visible text count (like the Python patcher: text.strip() not empty).
use anyhow::Result;
use std::collections::HashMap;

pub struct Store {
    map: HashMap<String, String>,
}

impl Store {
    pub fn load(path: &std::path::Path) -> Result<Store> {
        Store::from_slice(&std::fs::read(path)?)
    }

    /// {id: {"pl": text}} (workshop store) or {id: text} (a release / the installer).
    pub fn from_slice(bytes: &[u8]) -> Result<Store> {
        let v: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut map = HashMap::new();
        if let Some(o) = v.as_object() {
            for (k, e) in o {
                let pl = e.get("pl").and_then(|p| p.as_str()).or_else(|| e.as_str());
                if let Some(pl) = pl {
                    if !pl.trim().is_empty() {
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
