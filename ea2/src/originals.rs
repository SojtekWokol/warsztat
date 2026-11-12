//! Backups of the game files the EA2 patch changes (shared logic in the `kopie` crate).
pub use kopie::{hash_file, Config, FileState};
use std::path::{Path, PathBuf};

/// EA2: the marker sits in ProjectFLEA_Data; known hashes in crate::known.
pub const CONFIG: Config = Config {
    marker_anchor: "ProjectFLEA_Data",
    marker_file: "ea2_pl.json",
    known_patched: crate::known::is_patched,
    knows: crate::known::knows,
    known_original: crate::known::is_original,
};

pub type Originals = kopie::Originals;

pub fn open(dir: &Path) -> anyhow::Result<Originals> {
    kopie::Originals::open(dir, CONFIG)
}
pub fn clean_copy(game_file: &Path, dir: &Path) -> anyhow::Result<PathBuf> {
    kopie::Originals::clean_copy(&CONFIG, game_file, dir)
}
pub fn is_ours(game_file: &Path) -> bool {
    kopie::Originals::is_ours(&CONFIG, game_file)
}
