//! A test build of the fifth language: the English texts marked "[PL] " (or finished Polish Yarn
//! lines from a file), written to a separate folder with the game's layout. The game folder given
//! must hold the original files.
use crate::patch::{build, split_speaker, Place, Texts};
use crate::{il, Game};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;

/// Marks sentences only (numbers, keys and single words stay as they are: puzzles compare them).
pub fn mark(s: &str) -> String {
    if !s.contains(' ') {
        return s.to_string();
    }
    let (who, rest) = split_speaker(s);
    if !who.is_empty() && who.split(' ').count() <= 3 && !who.contains('<') && !rest.is_empty() {
        format!("{who}:[PL] {rest}")
    } else {
        format!("[PL] {s}")
    }
}

/// `polish`: finished Yarn lines {line id: full text incl. speaker} used instead of the marked English.
pub fn build_test(game: &Game, out: &Path, polish: &HashMap<String, String>, fonts_dir: &Path, log: &mut dyn FnMut(String)) -> Result<()> {
    let src = Place::under(&game.dir);
    if il::state(&std::fs::read(&src.dll)?)? != il::State::Original {
        bail!("{}: not the original files (restore them first)", game.dir.display());
    }
    let g = |_: &str, en: &str| mark(en);
    let y = |id: &str, en: &str| polish.get(id).cloned().unwrap_or_else(|| mark(en));
    let v = |_: &str, en: &str| Some(mark(en)).filter(|m| m != en);
    build(&src, &Place::under(out), &Texts { gameplay: &g, yarn: &y, subtitle: &v }, fonts_dir, true, log)
}
