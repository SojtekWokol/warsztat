// EA2 Spolszczenie: the players' installer. One file with the translation and the fonts inside;
// finds the game (Xbox app or Steam), writes the Polish version into it with the same patcher as the
// workshop, and can put the original files back. Backups of the changed game files go to
// %LOCALAPPDATA%\EA2 Spolszczenie\kopie (about 1.1 GB).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ea2::originals::Originals;
use ea2::patch::{self, Game};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TEXTS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pl.json"));
const VERSION: &str = include_str!(concat!(env!("OUT_DIR"), "/version.txt"));
const GAME_DIR: &str = "Escape Academy 2- Back 2 School";

fn fonts() -> HashMap<String, Vec<u8>> {
    macro_rules! f {
        ($($n:literal),*) => { HashMap::from([$(($n.to_string(), include_bytes!(concat!(env!("OUT_DIR"), "/fonts/", $n)).to_vec())),*]) };
    }
    f!("Urbanist-ExtraBold.ttf", "Urbanist-SemiBold.ttf", "ChunkFive-Regular.ttf", "Caveat-Bold.ttf", "Kalam-Bold.ttf", "Knewave-PL.ttf", "Jersey10-Regular.ttf")
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("EA2 Spolszczenie")
}
fn backup_dir() -> PathBuf {
    data_dir().join("kopie")
}

/// Accept the folder the player picked: Content, its parent, or ProjectFLEA_Data.
fn normalize(p: &Path) -> Option<PathBuf> {
    let cands = [p.to_path_buf(), p.join("Content"), p.parent().map(Path::to_path_buf).unwrap_or_default()];
    cands.into_iter().find(|c| c.join("ProjectFLEA_Data").join("data.unity3d").is_file())
}

fn steam_libraries() -> Vec<PathBuf> {
    let out = std::process::Command::new("reg").args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]).creation_flags(CREATE_NO_WINDOW).output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(steam) = text.lines().find_map(|l| l.split("REG_SZ").nth(1)).map(|s| PathBuf::from(s.trim())) else { return vec![] };
    let mut libs = vec![steam.clone()];
    if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 && parts[1] == "path" {
                libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    libs
}

/// Installed copies of the game: the Xbox app folder on every drive, Steam libraries.
fn find_games() -> Vec<Value> {
    let mut out = Vec::new();
    for d in b'C'..=b'Z' {
        let p = PathBuf::from(format!("{}:\\XboxGames\\{GAME_DIR}\\Content", d as char));
        if normalize(&p).is_some() {
            out.push(json!({"path": p, "kind": "Xbox"}));
        }
    }
    for lib in steam_libraries() {
        let common = lib.join("steamapps").join("common");
        for e in std::fs::read_dir(&common).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("Escape Academy 2") {
                if let Some(p) = normalize(&e.path()) {
                    out.push(json!({"path": p, "kind": "Steam"}));
                }
            }
        }
    }
    out
}

fn can_write(content: &Path) -> bool {
    let probe = content.join("ProjectFLEA_Data").join(".ea2pl_test");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Is the Polish version in this game now: the catalog is one our patcher wrote (installer or workshop).
fn installed(content: &Path) -> bool {
    Originals::is_ours(&Game { content: content.to_path_buf() }.catalog())
}

#[derive(Default)]
struct Job {
    running: bool,
    ok: Option<bool>,
    stage: String,
    log: Vec<String>,
}

struct App(Arc<Mutex<Job>>);

#[tauri::command]
fn info() -> Value {
    let n = serde_json::from_slice::<Value>(TEXTS).ok().and_then(|v| v.as_object().map(|o| o.len())).unwrap_or(0);
    json!({"version": VERSION, "count": n, "games": find_games(), "backup": backup_dir()})
}

#[tauri::command]
fn status(path: String) -> Value {
    let Some(c) = normalize(Path::new(&path)) else { return json!({"valid": false}) };
    json!({"valid": true, "path": c, "installed": installed(&c), "writable": can_write(&c), "running": patch::game_running()})
}

fn start(app: &App, path: String, install: bool) -> Result<(), String> {
    let c = normalize(Path::new(&path)).ok_or("To nie jest folder gry Escape Academy 2.")?;
    {
        let mut j = app.0.lock().unwrap();
        if j.running {
            return Err("Już trwa.".into());
        }
        *j = Job { running: true, ..Default::default() };
    }
    let job = app.0.clone();
    std::thread::spawn(move || {
        let push = |m: String| {
            let mut j = job.lock().unwrap();
            if m.starts_with('[') {
                j.stage = m.clone();
            }
            j.log.push(m);
        };
        let res: anyhow::Result<()> = (|| {
            let game = Game { content: c.clone() };
            let mut originals = Originals::open(&backup_dir())?;
            if install {
                let store = ea2::store::Store::from_slice(TEXTS)?;
                let fonts = fonts();
                patch::run(patch::Options { game: &game, store: &store, fonts_dir: Path::new(""), fonts: Some(&fonts), originals: &mut originals, out_dir: None }, &mut |m| push(m))
            } else {
                if patch::game_running() {
                    anyhow::bail!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie.");
                }
                push("[1/1] Przywracam oryginalne pliki gry...".into());
                let (done, skipped) = originals.restore(|n| game.place_of(n))?;
                push(format!("Przywrócono {done} plików (pominięto {skipped})."));
                Ok(())
            }
        })();
        let mut j = job.lock().unwrap();
        if let Err(e) = &res {
            j.log.push(format!("Błąd: {e}"));
        }
        j.ok = Some(res.is_ok());
        j.running = false;
    });
    Ok(())
}

#[tauri::command]
fn install(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    start(&app, path, true)
}

#[tauri::command]
fn uninstall(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    start(&app, path, false)
}

#[tauri::command]
fn progress(app: tauri::State<'_, App>) -> Value {
    let j = app.0.lock().unwrap();
    json!({"running": j.running, "ok": j.ok, "stage": j.stage, "log": j.log})
}

/// The Xbox app keeps its game folders read-only: give the current user the right to change files
/// in this game's folder (Windows asks for administrator consent).
#[tauri::command]
fn grant(path: String) -> Result<(), String> {
    let c = normalize(Path::new(&path)).ok_or("To nie jest folder gry.")?;
    let user = std::env::var("USERNAME").unwrap_or_default();
    let target = c.to_string_lossy().replace('\'', "''");
    let args = format!("'\"{target}\"','/grant','\"{user}\":(OI)(CI)M','/T','/C','/Q'");
    let ps = format!("Start-Process icacls -ArgumentList {args} -Verb RunAs -Wait -WindowStyle Hidden");
    let st = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(CREATE_NO_WINDOW).status().map_err(|e| e.to_string())?;
    if !st.success() || !can_write(&c) {
        return Err("Nie udało się nadać uprawnień (zgoda administratora odrzucona albo folder zablokowany).".into());
    }
    Ok(())
}

#[tauri::command]
fn launch() -> Result<(), String> {
    use warsztat_launch::launch_xbox;
    launch_xbox().map_err(|e| e.to_string())
}

mod warsztat_launch {
    use super::*;
    pub fn launch_xbox() -> anyhow::Result<()> {
        let ps = "$p = Get-AppxPackage | Where-Object { $_.InstallLocation -like '*Escape Academy 2*' } | Select-Object -First 1; \
                  if (-not $p) { exit 2 }; $id = ($p | Get-AppxPackageManifest).Package.Applications.Application.Id | Select-Object -First 1; \
                  Start-Process ('shell:AppsFolder\\' + $p.PackageFamilyName + '!' + $id)";
        let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", ps]).creation_flags(CREATE_NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            anyhow::bail!("Uruchom grę ręcznie (przez aplikację Xbox lub Steam).");
        }
        Ok(())
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(App(Arc::new(Mutex::new(Job::default()))))
        .invoke_handler(tauri::generate_handler![info, status, install, uninstall, progress, grant, launch])
        .run(tauri::generate_context!())
        .expect("tauri app");
}
