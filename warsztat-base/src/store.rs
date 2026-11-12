//! The translation store: translations/pl/**/*.json, split by where the texts are in the game (the
//! game decides the file of each text), so pull requests stay small and rarely collide.
//! Each file: {id: {"pl": text, "status": todo|draft|review|done}}, keys sorted, one-space indent.
//! An old single translations/pl.json is read and split on first load.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub const STATUSES: &[&str] = &["todo", "draft", "review", "done"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Rec {
    pub pl: String,
    pub status: String,
    /// last change (unix time); kept in memory only, not written to the files (it would make every
    /// save a diff line)
    #[serde(default, skip_serializing)]
    pub t: i64,
}

/// Which file (relative to translations/pl, e.g. "pokoje/DET.json") a text id belongs in.
pub type Placer = Box<dyn Fn(&str) -> String + Send + Sync>;

pub struct Store {
    dir: PathBuf,
    pub map: BTreeMap<String, Rec>,
    file_of: HashMap<String, String>,
    placer: Placer,
    dirty: BTreeSet<String>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// JSON with Python's layout: one-space indent.
pub fn to_python_json<T: Serialize>(v: &T) -> Result<String> {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    v.serialize(&mut ser)?;
    Ok(String::from_utf8(buf)?)
}

fn json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            json_files(&p, out);
        } else if p.extension().map_or(false, |x| x == "json") {
            out.push(p);
        }
    }
}

/// All texts of a translations/pl folder (or one JSON file): id -> Polish text. For tools that only
/// read (patcher, installer build). Accepts {id: {"pl": …}} and {id: "text"}.
pub fn read_texts(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = Vec::new();
    if path.is_dir() {
        json_files(path, &mut files);
    } else {
        files.push(path.to_path_buf());
    }
    let mut out = BTreeMap::new();
    for f in files {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&f)?).map_err(|e| anyhow::anyhow!("{}: {e}", f.display()))?;
        for (k, e) in v.as_object().into_iter().flatten() {
            if let Some(pl) = e.get("pl").and_then(|x| x.as_str()).or_else(|| e.as_str()) {
                out.insert(k.clone(), pl.to_string());
            }
        }
    }
    Ok(out)
}

impl Store {
    /// `translations` is the translations folder; texts live in translations/pl/.
    pub fn load(translations: &Path, placer: Placer) -> Result<Store> {
        let dir = translations.join("pl");
        let mut st = Store { dir: dir.clone(), map: BTreeMap::new(), file_of: HashMap::new(), placer, dirty: BTreeSet::new() };
        let mut files = Vec::new();
        json_files(&dir, &mut files);
        files.sort();
        for f in &files {
            let rel = f.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
            let m: BTreeMap<String, Rec> = serde_json::from_slice(&std::fs::read(f)?).map_err(|e| anyhow::anyhow!("{}: {e}", f.display()))?;
            for (k, r) in m {
                st.file_of.insert(k.clone(), rel.clone());
                st.map.insert(k, r);
            }
        }
        let legacy = translations.join("pl.json");
        if legacy.exists() {
            // the old single file: take what the folder does not have yet, split it, remove it
            let m: BTreeMap<String, Rec> = serde_json::from_slice(&std::fs::read(&legacy)?)?;
            for (k, r) in m {
                if !st.map.contains_key(&k) {
                    let f = st.place(&k);
                    st.dirty.insert(f);
                    st.map.insert(k, r);
                }
            }
            st.save()?;
            std::fs::remove_file(&legacy)?;
        }
        Ok(st)
    }

    fn place(&mut self, id: &str) -> String {
        if let Some(f) = self.file_of.get(id) {
            return f.clone();
        }
        let f = (self.placer)(id);
        self.file_of.insert(id.to_string(), f.clone());
        f
    }

    /// Write the files that changed.
    pub fn save(&mut self) -> Result<()> {
        let dirty: Vec<String> = std::mem::take(&mut self.dirty).into_iter().collect();
        for rel in dirty {
            let part: BTreeMap<&String, &Rec> = self.map.iter().filter(|(k, _)| self.file_of.get(*k) == Some(&rel)).collect();
            let path = self.dir.join(&rel);
            if part.is_empty() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            std::fs::create_dir_all(path.parent().unwrap())?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, to_python_json(&part)? + "\n")?;
            std::fs::rename(&tmp, &path)?;
        }
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
        let f = self.place(id);
        self.dirty.insert(f);
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
