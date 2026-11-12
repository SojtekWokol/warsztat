// EA2 Spolszczenie: the players' installer for Escape Academy 2 - the shared installer (instalator-base)
// with the EA2 patch, the translation and fonts built in, and the EA2 look.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use ea2::patch::{self, Game};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TEXTS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pl.json"));
const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/version.txt"));

fn fonts() -> HashMap<String, Vec<u8>> {
    macro_rules! f {
        ($($n:literal),*) => { HashMap::from([$(($n.to_string(), include_bytes!(concat!(env!("OUT_DIR"), "/fonts/", $n)).to_vec())),*]) };
    }
    f!("Urbanist-ExtraBold.ttf", "Urbanist-SemiBold.ttf", "ChunkFive-Regular.ttf", "Caveat-Bold.ttf", "Kalam-Bold.ttf", "Knewave-PL.ttf", "Jersey10-Regular.ttf")
}

struct Ea2;

impl instalator::Installer for Ea2 {
    fn game(&self) -> &str {
        "Escape Academy 2"
    }
    fn version(&self) -> &str {
        VERSION
    }
    fn texts(&self) -> &'static [u8] {
        TEXTS
    }
    fn update_repo(&self) -> Option<&str> {
        Some("SojtekWokol/ea2-tlumaczenie")
    }
    fn theme_css(&self) -> &str {
        include_str!("theme.css")
    }
    fn logo_html(&self) -> &str {
        r#"<span class="logo">EA2</span><span class="pl">PL</span>"#
    }
    fn data_dir_name(&self) -> &str {
        "EA2 Spolszczenie"
    }
    fn xbox_folder(&self) -> Option<&str> {
        Some(r"Escape Academy 2- Back 2 School\Content")
    }
    fn steam_prefix(&self) -> Option<&str> {
        Some("Escape Academy 2")
    }
    /// Content, its parent, or ProjectFLEA_Data -> the folder holding ProjectFLEA_Data.
    fn normalize(&self, p: &Path) -> Option<PathBuf> {
        let cands = [p.to_path_buf(), p.join("Content"), p.parent().map(Path::to_path_buf).unwrap_or_default()];
        cands.into_iter().find(|c| c.join("ProjectFLEA_Data").join("data.unity3d").is_file())
    }
    fn write_probe_dir(&self, game: &Path) -> PathBuf {
        game.join("ProjectFLEA_Data")
    }
    fn running(&self) -> bool {
        patch::game_running()
    }
    fn installed(&self, game: &Path) -> bool {
        ea2::originals::is_ours(&Game { content: game.to_path_buf() }.catalog())
    }
    fn install(&self, game: &Path, backups: &Path, texts: &[u8], log: &mut dyn FnMut(String)) -> Result<()> {
        let g = Game { content: game.to_path_buf() };
        let mut originals = ea2::originals::open(backups)?;
        let store = ea2::store::Store::from_slice(texts)?;
        let fonts = fonts();
        patch::run(patch::Options { game: &g, store: &store, fonts_dir: Path::new(""), fonts: Some(&fonts), originals: &mut originals, out_dir: None }, log)
    }
    fn uninstall(&self, game: &Path, backups: &Path, log: &mut dyn FnMut(String)) -> Result<()> {
        let g = Game { content: game.to_path_buf() };
        let mut originals = ea2::originals::open(backups)?;
        log("[1/1] Przywracam oryginalne pliki gry...".into());
        let (done, skipped) = originals.restore(|n| g.place_of(n))?;
        log(format!("Przywrócono {done} plików (pominięto {skipped})."));
        Ok(())
    }
    fn launch(&self) -> Result<()> {
        instalator::launch_xbox("Escape Academy 2")
    }
    fn finish_html(&self) -> String {
        "Uruchom grę i wybierz w ustawieniach język <b>POLSKI</b> (jeśli nie jest jeszcze wybrany).".into()
    }
    fn note_html(&self) -> String {
        "Gra musi być zamknięta. Po instalacji wybierz w ustawieniach gry język <b>POLSKI</b>.".into()
    }
}

fn main() {
    instalator::builder(Ea2).run(tauri::generate_context!()).expect("tauri app");
}
