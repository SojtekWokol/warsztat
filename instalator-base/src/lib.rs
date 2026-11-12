//! Shared installer for fan translations. A game implements [`Installer`] (how to recognise its folder,
//! install, restore, and how the window looks); this crate gives the window and everything around it:
//! finding the game in the Xbox app folders and Steam libraries, unlocking an Xbox app folder for
//! writing, running install/restore in the background with progress, starting the game.
//!
//! The binary of a game:
//! ```ignore
//! fn main() { instalator::builder(MyGame).run(tauri::generate_context!()).expect("tauri app"); }
//! ```
//! with `"frontendDist": "../instalator-base/ui"` in its tauri.conf.json.
use anyhow::Result;
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a game adds to the shared installer.
pub trait Installer: Send + Sync + 'static {
    /// Game title, e.g. "Escape Academy 2".
    fn game(&self) -> &str;
    /// Translation version inside the installer, e.g. "1.0.0".
    fn version(&self) -> &str;
    /// Number of translated texts (shown to the player).
    fn count(&self) -> usize;
    /// The look: CSS setting the shared variables (see ui/index.html) and any extra rules.
    fn theme_css(&self) -> &str;
    /// Header logo (HTML).
    fn logo_html(&self) -> &str;
    /// Folder name under %LOCALAPPDATA% for backups and settings, e.g. "EA2 Spolszczenie".
    fn data_dir_name(&self) -> &str;
    /// Game folder name inside X:\XboxGames, if the game is in the Xbox app / Game Pass.
    fn xbox_folder(&self) -> Option<&str> {
        None
    }
    /// Start of the game folder name in Steam's steamapps\common, if the game is on Steam.
    fn steam_prefix(&self) -> Option<&str> {
        None
    }
    /// The game folder for a folder the player picked (it may pick a parent or a subfolder), if valid.
    fn normalize(&self, picked: &Path) -> Option<PathBuf>;
    /// A folder inside the game where writing is tested (the one the patch writes to).
    fn write_probe_dir(&self, game: &Path) -> PathBuf {
        game.to_path_buf()
    }
    /// Is the game running now (installing is refused then).
    fn running(&self) -> bool;
    /// Is the translation installed in this game folder.
    fn installed(&self, game: &Path) -> bool;
    /// Install; log lines starting "[n/m] " are shown as stages.
    fn install(&self, game: &Path, backups: &Path, log: &mut dyn FnMut(String)) -> Result<()>;
    /// Put the original files back.
    fn uninstall(&self, game: &Path, backups: &Path, log: &mut dyn FnMut(String)) -> Result<()>;
    /// Start the game.
    fn launch(&self) -> Result<()>;
    /// Short note under the buttons (HTML), e.g. which language to pick in the game.
    fn note_html(&self) -> String {
        String::new()
    }
}

pub fn data_dir(name: &str) -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(name)
}

/// Steam library folders (from the registry and libraryfolders.vdf).
pub fn steam_libraries() -> Vec<PathBuf> {
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
    libs.dedup();
    libs
}

fn find_games(inst: &dyn Installer) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut push = |p: PathBuf, kind: &str| {
        if let Some(g) = inst.normalize(&p) {
            if !out.iter().any(|v| v["path"] == json!(g)) {
                out.push(json!({"path": g, "kind": kind}));
            }
        }
    };
    if let Some(x) = inst.xbox_folder() {
        for d in b'C'..=b'Z' {
            push(PathBuf::from(format!("{}:\\XboxGames\\{x}", d as char)), "Xbox");
        }
    }
    if let Some(prefix) = inst.steam_prefix() {
        for lib in steam_libraries() {
            for e in std::fs::read_dir(lib.join("steamapps").join("common")).into_iter().flatten().flatten() {
                if e.file_name().to_string_lossy().starts_with(prefix) {
                    push(e.path(), "Steam");
                }
            }
        }
    }
    out
}

fn can_write(dir: &Path) -> bool {
    let probe = dir.join(".translation_write_test");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[derive(Default)]
struct Job {
    running: bool,
    ok: Option<bool>,
    stage: String,
    log: Vec<String>,
}

struct App {
    inst: Arc<dyn Installer>,
    job: Arc<Mutex<Job>>,
}

impl App {
    fn backups(&self) -> PathBuf {
        data_dir(self.inst.data_dir_name()).join("kopie")
    }
}

#[tauri::command]
fn info(app: tauri::State<'_, App>) -> Value {
    let i = &app.inst;
    json!({"game": i.game(), "version": i.version(), "count": i.count(), "theme": i.theme_css(), "logo": i.logo_html(),
           "note": i.note_html(), "games": find_games(i.as_ref()), "backup": app.backups()})
}

#[tauri::command]
fn status(app: tauri::State<'_, App>, path: String) -> Value {
    let Some(g) = app.inst.normalize(Path::new(&path)) else { return json!({"valid": false}) };
    json!({"valid": true, "path": g, "installed": app.inst.installed(&g), "writable": can_write(&app.inst.write_probe_dir(&g)), "running": app.inst.running()})
}

fn start(app: &App, path: String, install: bool) -> Result<(), String> {
    let g = app.inst.normalize(Path::new(&path)).ok_or("To nie jest folder tej gry.")?;
    {
        let mut j = app.job.lock().unwrap();
        if j.running {
            return Err("Już trwa.".into());
        }
        *j = Job { running: true, ..Default::default() };
    }
    let (inst, job, backups) = (app.inst.clone(), app.job.clone(), app.backups());
    std::thread::spawn(move || {
        let mut push = |m: String| {
            let mut j = job.lock().unwrap();
            if m.starts_with('[') {
                j.stage = m.clone();
            }
            j.log.push(m);
        };
        let res = if inst.running() {
            Err(anyhow::anyhow!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie."))
        } else if install {
            inst.install(&g, &backups, &mut push)
        } else {
            inst.uninstall(&g, &backups, &mut push)
        };
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
    let j = app.job.lock().unwrap();
    json!({"running": j.running, "ok": j.ok, "stage": j.stage, "log": j.log})
}

/// The Xbox app keeps game folders read-only: give the current user the right to change files in this
/// game's folder (Windows asks for administrator consent).
#[tauri::command]
fn grant(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    let g = app.inst.normalize(Path::new(&path)).ok_or("To nie jest folder gry.")?;
    let user = std::env::var("USERNAME").unwrap_or_default();
    let target = g.to_string_lossy().replace('\'', "''");
    let ps = format!("Start-Process icacls -ArgumentList '\"{target}\"','/grant','\"{user}\":(OI)(CI)M','/T','/C','/Q' -Verb RunAs -Wait -WindowStyle Hidden");
    let st = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(CREATE_NO_WINDOW).status().map_err(|e| e.to_string())?;
    if !st.success() || !can_write(&app.inst.write_probe_dir(&g)) {
        return Err("Nie udało się nadać uprawnień (zgoda administratora odrzucona albo folder zablokowany).".into());
    }
    Ok(())
}

#[tauri::command]
fn launch(app: tauri::State<'_, App>) -> Result<(), String> {
    app.inst.launch().map_err(|e| e.to_string())
}

/// Start an Xbox app (Game Pass) game whose install folder contains `folder_part`.
pub fn launch_xbox(folder_part: &str) -> Result<()> {
    let ps = format!(
        "$p = Get-AppxPackage | Where-Object {{ $_.InstallLocation -like '*{folder_part}*' }} | Select-Object -First 1; \
         if (-not $p) {{ exit 2 }}; $id = ($p | Get-AppxPackageManifest).Package.Applications.Application.Id | Select-Object -First 1; \
         Start-Process ('shell:AppsFolder\\' + $p.PackageFamilyName + '!' + $id)"
    );
    let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(CREATE_NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        anyhow::bail!("Uruchom grę ręcznie (przez aplikację Xbox lub Steam).");
    }
    Ok(())
}

/// Is a process with this exe name running.
pub fn process_running(exe: &str) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {exe}")])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(exe))
        .unwrap_or(false)
}

/// The Tauri app with the shared installer window and commands.
pub fn builder<I: Installer>(inst: I) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(App { inst: Arc::new(inst), job: Arc::new(Mutex::new(Job::default())) })
        .invoke_handler(tauri::generate_handler![info, status, install, uninstall, progress, grant, launch])
}
