
use anyhow::{anyhow, Result};
use ea2::{csv, originals, patch, sdf, store};
use std::path::PathBuf;

const DEFAULT_CONTENT: &str = r"C:\XboxGames\Escape Academy 2- Back 2 School\Content";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The workshop folder: walk up from the current directory to the one with translations/.
fn project_root() -> PathBuf {
    let mut d = std::env::current_dir().unwrap();
    loop {
        if d.join("translations").join("pl.json").exists() {
            return d;
        }
        if !d.pop() {
            return std::env::current_dir().unwrap();
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = project_root();
    let game = patch::Game { content: PathBuf::from(arg(&args, "--content").unwrap_or(DEFAULT_CONTENT.into())) };
    let backup = PathBuf::from(arg(&args, "--backup").unwrap_or_else(|| root.join("backup").to_string_lossy().into()));
    let mut log = |m: String| println!("{m}");
    match args.get(1).map(|s| s.as_str()) {
        Some("patch") => {
            let store_p = PathBuf::from(arg(&args, "--store").unwrap_or_else(|| root.join("translations/pl.json").to_string_lossy().into()));
            let fonts = PathBuf::from(arg(&args, "--fonts").unwrap_or_else(|| root.join("fonts").to_string_lossy().into()));
            let out = arg(&args, "--out").map(PathBuf::from);
            if let Some(o) = &out {
                std::fs::create_dir_all(o)?;
            }
            let store = store::Store::load(&store_p)?;
            log(format!("translations: {} texts", store.len()));
            let mut originals = originals::Originals::open(&backup)?;
            let t = std::time::Instant::now();
            patch::run(patch::Options { game: &game, store: &store, fonts_dir: &fonts, fonts: None, originals: &mut originals, out_dir: out.as_deref() }, &mut log)?;
            log(format!("done in {:.0} s", t.elapsed().as_secs_f64()));
            Ok(())
        }
        Some("restore") => {
            if patch::game_running() {
                return Err(anyhow!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie."));
            }
            let mut originals = originals::Originals::open(&backup)?;
            let (done, skipped) = originals.restore(|n| game.place_of(n))?;
            log(format!("restored {done} files, skipped {skipped}"));
            Ok(())
        }
        Some("extract") => {
            // texts and preview pictures from the game into extracted/ and cache/vn (--out DIR: another root)
            let out = arg(&args, "--out").map(PathBuf::from).unwrap_or(root.clone());
            let t = std::time::Instant::now();
            ea2::extract::extract(&game, &backup, &out, !args.iter().any(|a| a == "--no-images"), &mut log)?;
            log(format!("done in {:.0} s", t.elapsed().as_secs_f64()));
            Ok(())
        }
        Some("api") => {
            // the workshop API without the window, for testing: ea2 api <route> [json]
            let ws = warsztat::Workshop::open(&root, std::sync::Arc::new(ea2::workshop::Ea2::default()))?;
            let d: serde_json::Value = serde_json::from_str(args.get(3).map(|s| s.as_str()).unwrap_or("{}"))?;
            println!("{}", serde_json::to_string(&ws.api(&args[2], &d))?);
            Ok(())
        }
        Some("csvtest") => {
            let t = std::fs::read_to_string(&args[2])?;
            std::fs::write(&args[3], csv::write(&csv::parse(&t)))?;
            Ok(())
        }
        Some("glyph") => {
            let font = std::fs::read(&args[2])?;
            let ch = args[3].chars().next().unwrap();
            let g = sdf::render(&font, ch, args[4].parse()?, args[5].parse()?)?;
            std::fs::write(&args[6], &g.pixels)?;
            println!("{} {} {:?}", g.w, g.h, g.met);
            Ok(())
        }
        _ => {
            eprintln!("usage: ea2 patch [--content DIR] [--store pl.json] [--fonts DIR] [--backup DIR] [--out DIR]\n       ea2 restore [--content DIR] [--backup DIR]");
            Ok(())
        }
    }
}
