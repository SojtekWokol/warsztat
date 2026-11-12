//! The translation store: translations/pl.json, {id: {"pl": text, "status": todo|draft|review|done, "t": unix}}.
//! Written like Python's json.dump(indent=1, sort_keys=True, ensure_ascii=False), so diffs stay small.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STATUSES: &[&str] = &["todo", "draft", "review", "done"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rec {
    pub pl: String,
    pub status: String,
    #[serde(default)]
    pub t: i64,
}

pub struct Store {
    path: PathBuf,
    pub map: BTreeMap<String, Rec>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// JSON with Python's layout: one-space indent, ", " never at line ends, ": " after keys.
pub fn to_python_json<T: Serialize>(v: &T) -> Result<String> {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    v.serialize(&mut ser)?;
    Ok(String::from_utf8(buf)?)
}

impl Store {
    pub fn load(path: &Path) -> Result<Store> {
        let map = if path.exists() { serde_json::from_slice(&std::fs::read(path)?)? } else { BTreeMap::new() };
        Ok(Store { path: path.to_path_buf(), map })
    }

    pub fn save(&self) -> Result<()> {
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, to_python_json(&self.map)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn pl(&self, id: &str) -> &str {
        self.map.get(id).map(|r| r.pl.as_str()).unwrap_or("")
    }

    /// Set without saving (for bulk changes; call save()).
    pub fn set(&mut self, id: &str, pl: &str, status: &str) -> Result<()> {
        if !STATUSES.contains(&status) {
            bail!("unknown status {status}");
        }
        if pl.trim().is_empty() && status == "todo" {
            self.map.remove(id);
        } else {
            self.map.insert(id.to_string(), Rec { pl: pl.to_string(), status: status.to_string(), t: now() });
        }
        Ok(())
    }

    pub fn update(&mut self, id: &str, pl: &str, status: &str) -> Result<()> {
        self.set(id, pl, status)?;
        self.save()
    }

    /// All non-empty translations: what a build writes into the game.
    pub fn current_texts(&self) -> BTreeMap<String, String> {
        self.map.iter().filter(|(_, r)| !r.pl.is_empty()).map(|(k, r)| (k.clone(), r.pl.clone())).collect()
    }
}
