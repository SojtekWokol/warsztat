//! Clean copies of the game files we change, kept in a backup folder, safe across game updates.
//!
//! state.json remembers, per file, the hash of the original we backed up and of the file we wrote.
//! When the game file is still the one we wrote, the backup is its original. When it is anything
//! else (first run, the game was updated or repaired), the game file itself is the new original.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct FileState {
    pub original: String,
    pub original_size: u64,
    #[serde(default)]
    pub written: Option<String>,
}

pub struct Originals {
    dir: PathBuf,
    state: BTreeMap<String, FileState>,
}

pub fn hash_file(p: &Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).with_context(|| p.display().to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The marker our patcher leaves in the game folder (ProjectFLEA_Data/ea2_pl.json): per file, the hash
/// of what it wrote and where the original is. Any copy of the patcher (workshop, installer) reads it,
/// so a file patched by one of them is never taken for an original by another.
fn marker_path(game_file: &Path) -> Option<PathBuf> {
    game_file.ancestors().find(|d| d.file_name().map_or(false, |n| n == "ProjectFLEA_Data")).map(|d| d.join("ea2_pl.json"))
}

fn marker_read(game_file: &Path) -> serde_json::Value {
    marker_path(game_file)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({"files": {}}))
}

/// (written hash, backup path) the marker records for this file.
fn marker_entry(game_file: &Path) -> Option<(String, PathBuf)> {
    let name = game_file.file_name()?.to_string_lossy().to_string();
    let m = marker_read(game_file);
    let e = m.get("files")?.get(&name)?;
    Some((e.get("written")?.as_str()?.to_string(), PathBuf::from(e.get("backup")?.as_str()?)))
}

fn marker_set(game_file: &Path, entry: Option<(&str, &Path)>) -> Result<()> {
    let Some(p) = marker_path(game_file) else { return Ok(()) };
    let name = game_file.file_name().unwrap().to_string_lossy().to_string();
    let mut m = marker_read(game_file);
    let files = m["files"].as_object_mut().unwrap();
    match entry {
        Some((h, b)) => {
            files.insert(name, serde_json::json!({"written": h, "backup": b}));
        }
        None => {
            files.remove(&name);
        }
    }
    std::fs::write(&p, serde_json::to_vec_pretty(&m)?).with_context(|| p.display().to_string())?;
    Ok(())
}

impl Originals {
    pub fn open(dir: &Path) -> Result<Originals> {
        std::fs::create_dir_all(dir)?;
        let sp = dir.join("state.json");
        let state = if sp.exists() { serde_json::from_slice(&std::fs::read(&sp)?)? } else { BTreeMap::new() };
        Ok(Originals { dir: dir.to_path_buf(), state })
    }

    fn save(&self) -> Result<()> {
        std::fs::write(self.dir.join("state.json"), serde_json::to_vec_pretty(&self.state)?)?;
        Ok(())
    }

    /// Path of the clean original of `game_file`, backing it up first when needed.
    pub fn original(&mut self, game_file: &Path) -> Result<PathBuf> {
        let name = game_file.file_name().unwrap().to_string_lossy().to_string();
        let backup = self.dir.join(&name);
        let current = hash_file(game_file)?;
        let st = self.state.get(&name).cloned();
        let backup_ok = |st: &FileState| backup.exists() && std::fs::metadata(&backup).map(|m| m.len()).unwrap_or(0) == st.original_size;
        // An existing backup is only ever replaced when we know the game file is not ours: we wrote
        // a different file before (so the game replaced it: update or repair). In every unclear case
        // stop instead of guessing - a wrong guess overwrites the only clean copy.
        let unclear = || {
            anyhow::anyhow!(
                "{name}: plik w grze różni się od kopii zapasowej i nie wiadomo, czy jest oryginałem. \
                 Nic nie zmieniono. Przywróć oryginalne pliki gry (aplikacja Xbox: Zarządzaj > Pliki > Sprawdź i napraw) i spróbuj ponownie."
            )
        };
        if st.as_ref().map_or(false, |s| crate::known::is_patched(&name, &s.original)) {
            return Err(unclear()); // the "original" on record is a patched file
        }
        match st {
            Some(st) if st.written.as_deref() == Some(current.as_str()) && backup_ok(&st) => return Ok(backup),
            Some(st) if st.original == current && backup_ok(&st) => return Ok(backup),
            Some(st) if st.written.is_some() => {} // the game replaced our file: it is the new original
            Some(_) => return Err(unclear()),
            None if backup.exists() => {
                // backup from before state.json existed (Python patcher): trust it only if it is a
                // known clean original, or the game file is something else than the backup
                let bh = hash_file(&backup)?;
                if crate::known::is_patched(&name, &bh) || (crate::known::knows(&name) && !crate::known::is_original(&name, &bh)) {
                    return Err(unclear());
                }
                let st = FileState { original: bh, original_size: std::fs::metadata(&backup)?.len(), written: None };
                self.state.insert(name, st);
                self.save()?;
                return Ok(backup);
            }
            None => {}
        }
        if crate::known::is_patched(&name, &current) {
            return Err(unclear());
        }
        if let Some((written, other)) = marker_entry(game_file) {
            if written == current {
                // patched by another copy of the patcher (workshop / installer): take its original
                if other == backup || !other.exists() {
                    return Err(unclear());
                }
                std::fs::copy(&other, &backup).with_context(|| format!("copy of the original of {name}"))?;
                let st = FileState { original: hash_file(&backup)?, original_size: std::fs::metadata(&backup)?.len(), written: Some(current) };
                self.state.insert(name, st);
                self.save()?;
                return Ok(backup);
            }
        }
        std::fs::copy(game_file, &backup).with_context(|| format!("backup of {name}"))?;
        self.state.insert(name, FileState { original: current, original_size: std::fs::metadata(&backup)?.len(), written: None });
        self.save()?;
        Ok(backup)
    }

    /// The clean version of a game file without making backups: the backup when the game file is the
    /// one our patch wrote, else the game file itself (refused if it is a known patched file).
    pub fn clean_copy(game_file: &Path, dir: &Path) -> Result<PathBuf> {
        let name = game_file.file_name().unwrap().to_string_lossy().to_string();
        let current = hash_file(game_file)?;
        let sp = dir.join("state.json");
        let state: BTreeMap<String, FileState> = if sp.exists() { serde_json::from_slice(&std::fs::read(&sp)?)? } else { BTreeMap::new() };
        if let Some(st) = state.get(&name) {
            let backup = dir.join(&name);
            if st.written.as_deref() == Some(current.as_str()) && backup.exists() {
                return Ok(backup);
            }
        }
        if let Some((written, other)) = marker_entry(game_file) {
            if written == current && other.exists() {
                return Ok(other);
            }
        }
        if crate::known::is_patched(&name, &current) {
            anyhow::bail!("{name}: w grze jest spolszczona wersja, a brak jej oryginału. Przywróć oryginalne pliki gry (aplikacja Xbox: Zarządzaj > Pliki > Sprawdź i napraw).");
        }
        Ok(game_file.to_path_buf())
    }

    pub fn mark_written(&mut self, game_file: &Path) -> Result<()> {
        let name = game_file.file_name().unwrap().to_string_lossy().to_string();
        let h = hash_file(game_file)?;
        if let Some(st) = self.state.get_mut(&name) {
            st.written = Some(h.clone());
        }
        marker_set(game_file, Some((&h, &self.dir.join(&name))))?;
        self.save()
    }

    /// Put back the originals of files that are still ones our patcher wrote - recorded in the game's
    /// marker (whichever copy of the patcher wrote them) or in our own state. Returns (restored, skipped).
    pub fn restore(&mut self, game_dir_for: impl Fn(&str) -> Option<PathBuf>) -> Result<(usize, usize)> {
        let (mut done, mut skipped) = (0, 0);
        let mut names: Vec<String> = self.state.keys().cloned().collect();
        if let Some(any) = names.iter().find_map(|n| game_dir_for(n)).or_else(|| game_dir_for("catalog.bin")) {
            if let Some(files) = marker_read(&any).get("files").and_then(|f| f.as_object()) {
                for k in files.keys() {
                    if !names.contains(k) {
                        names.push(k.clone());
                    }
                }
            }
        }
        for name in names {
            let Some(target) = game_dir_for(&name) else { skipped += 1; continue };
            if !target.exists() {
                skipped += 1;
                continue;
            }
            let current = hash_file(&target)?;
            let from_marker = marker_entry(&target).filter(|(w, b)| *w == current && b.exists()).map(|(_, b)| b);
            let from_state = self.state.get(&name).filter(|st| st.written.as_deref() == Some(current.as_str())).map(|_| self.dir.join(&name)).filter(|b| b.exists());
            match from_marker.or(from_state) {
                Some(src) => {
                    std::fs::copy(&src, &target).with_context(|| format!("restore {name}"))?;
                    if let Some(st) = self.state.get_mut(&name) {
                        st.written = None;
                    }
                    marker_set(&target, None)?;
                    done += 1;
                }
                None => skipped += 1,
            }
        }
        self.save()?;
        Ok((done, skipped))
    }

    /// Is this game file one our patcher wrote (any copy of it)?
    pub fn is_ours(game_file: &Path) -> bool {
        marker_entry(game_file).map_or(false, |(w, _)| hash_file(game_file).map_or(false, |h| h == w))
    }

    pub fn names(&self) -> Vec<String> {
        self.state.keys().cloned().collect()
    }
}
