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
pub mod updates;

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
    /// Translation version built into the installer, e.g. "1.0.0".
    fn version(&self) -> &str;
    /// The translation built into the installer: JSON object {id: text}.
    fn texts(&self) -> &'static [u8];
    /// GitHub repository ("owner/name") whose releases carry newer translations (tlumaczenie.json).
    fn update_repo(&self) -> Option<&str> {
        None
    }
    /// The way this installer patches; a release with a higher "patch_format" needs a new installer.
    fn patch_format(&self) -> u32 {
        1
    }
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
    /// Install the given texts (JSON object {id: text}); log lines starting "[n/m] " are shown as stages.
    fn install(&self, game: &Path, backups: &Path, texts: &[u8], log: &mut dyn FnMut(String)) -> Result<()>;
    /// Compatibility check: go through the whole install in memory, write nothing, log a report.
    fn check(&self, _game: &Path, _backups: &Path, _texts: &[u8], log: &mut dyn FnMut(String)) -> Result<()> {
        log("Ta gra nie ma jeszcze sprawdzania zgodności.".into());
        Ok(())
    }
    /// Put the original files back.
    fn uninstall(&self, game: &Path, backups: &Path, log: &mut dyn FnMut(String)) -> Result<()>;
    /// Start the game.
    /// Start the game installed in `game` (a Steam copy starts through Steam, see [`launch_auto`]).
    fn launch(&self, game: &Path) -> Result<()>;
    /// Short note under the buttons (HTML), e.g. which language to pick in the game.
    fn note_html(&self) -> String {
        String::new()
    }
    /// Text on the last page after installing (HTML), e.g. where to switch the game to Polish.
    fn finish_html(&self) -> String {
        "Uruchom grę i wybierz w ustawieniach język polski.".into()
    }
    /// Name of the desktop shortcut, e.g. "Escape Academy 2 (PL)".
    fn shortcut_name(&self) -> String {
        format!("{} (PL)", self.game())
    }
}

/// Short stable folder name for a game installation (same scheme as the `kopie` crate).
fn kopie_id(game: &Path) -> String {
    let full = game.to_string_lossy().to_lowercase().replace('/', "\\");
    let mut h: u64 = 0xcbf29ce484222325;
    for b in full.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let tail: Vec<String> = game.components().rev().take(2).map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let tail: String = tail.into_iter().rev().collect::<Vec<_>>().join(" ").chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).take(40).collect();
    format!("{} {:08x}", tail.trim(), h as u32)
}

pub fn data_dir(name: &str) -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(name)
}

/// Steam library folders (from the registry and libraryfolders.vdf).
pub fn steam_libraries() -> Vec<PathBuf> {
    let out = std::process::Command::new("reg").args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]).creation_flags(CREATE_NO_WINDOW).output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(steam) = text.lines().find_map(|l| l.split("REG_SZ").nth(1)).map(|s| PathBuf::from(s.trim().replace('/', "\\"))) else { return vec![] };
    let mut libs = Vec::new();
    if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 && parts[1] == "path" {
                libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
            }
        }
    }
    libs.push(steam.clone());
    let mut seen = std::collections::HashSet::new();
    libs.retain(|p| seen.insert(p.to_string_lossy().to_lowercase().replace('/', "\\")));
    libs
}

fn find_games(inst: &dyn Installer) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut push = |p: PathBuf, kind: &str| {
        if let Some(g) = inst.normalize(&p) {
            let key = |s: &str| s.to_lowercase().replace('/', "\\");
            if !out.iter().any(|v| key(v["path"].as_str().unwrap_or("")) == key(&g.to_string_lossy())) {
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
            // by name, and by contents: a Steam install folder may carry an internal name (the EA2
            // demo lives in "Project FLEA")
            for e in std::fs::read_dir(lib.join("steamapps").join("common")).into_iter().flatten().flatten() {
                let _ = prefix;
                push(e.path(), "Steam");
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
    fn dir(&self) -> PathBuf {
        data_dir(self.inst.data_dir_name())
    }
    /// Backups of one game installation (each copy of the game its own folder).
    fn backups(&self, game: &Path) -> PathBuf {
        self.dir().join("kopie").join(kopie_id(game))
    }
    fn best(&self) -> updates::Translation {
        updates::best(&self.dir(), self.inst.version(), self.inst.texts())
    }
}

#[tauri::command]
fn info(app: tauri::State<'_, App>) -> Value {
    let i = &app.inst;
    let t = app.best();
    json!({"game": i.game(), "version": t.version, "count": t.count, "theme": i.theme_css(), "logo": i.logo_html(),
           "note": i.note_html(), "games": find_games(i.as_ref()), "backup": app.dir().join("kopie"),
           "can_update": i.update_repo().is_some(), "settings": updates::settings(&app.dir()),
           "launcher": updates::launcher_arg(), "shortcut": i.shortcut_name(), "finish": i.finish_html()})
}

/// Off the window's thread: it runs tasklist and hashes files, which would freeze the window.
#[tauri::command]
async fn status(app: tauri::State<'_, App>, path: String) -> Result<Value, String> {
    let (inst, dir) = (app.inst.clone(), app.dir());
    tauri::async_runtime::spawn_blocking(move || status_of(inst.as_ref(), &dir, &path)).await.map_err(|e| e.to_string())
}

fn status_of(inst: &dyn Installer, dir: &Path, path: &str) -> Value {
    let Some(g) = inst.normalize(Path::new(path)) else { return json!({"valid": false}) };
    let installed = inst.installed(&g);
    let iv = updates::installed_version(dir, &g);
    let best = updates::best(dir, inst.version(), inst.texts()).version;
    // the game replaced its files (update/repair): we installed here before, now the translation is gone
    let wiped = !installed && iv.is_some();
    let outdated = installed && iv.as_deref().map_or(false, |v| updates::cmp_versions(&best, v) == std::cmp::Ordering::Greater);
    json!({"valid": true, "path": g, "installed": installed, "writable": can_write(&inst.write_probe_dir(&g)), "running": inst.running(),
           "installed_version": iv, "available": best, "outdated": outdated, "wiped": wiped})
}

fn start(app: &App, path: String, what: &'static str) -> Result<(), String> {
    let g = app.inst.normalize(Path::new(&path)).ok_or("To nie jest folder tej gry.")?;
    {
        let mut j = app.job.lock().unwrap();
        if j.running {
            return Err("Już trwa.".into());
        }
        *j = Job { running: true, ..Default::default() };
    }
    let (inst, job, backups, dir) = (app.inst.clone(), app.job.clone(), app.backups(&g), app.dir());
    let t = app.best();
    std::thread::spawn(move || {
        let mut push = |m: String| {
            let mut j = job.lock().unwrap();
            if m.starts_with('[') {
                j.stage = m.clone();
            }
            j.log.push(m);
        };
        let res = if what == "check" {
            inst.check(&g, &backups, &t.texts, &mut push)
        } else if inst.running() {
            Err(anyhow::anyhow!("Gra jest uruchomiona - zamknij ją i spróbuj ponownie."))
        } else if what == "install" {
            inst.install(&g, &backups, &t.texts, &mut push).and_then(|_| updates::set_installed_version(&dir, &g, Some(&t.version)))
        } else {
            inst.uninstall(&g, &backups, &mut push).and_then(|_| updates::set_installed_version(&dir, &g, None))
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
    start(&app, path, "install")
}

#[tauri::command]
fn uninstall(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    start(&app, path, "uninstall")
}

#[tauri::command]
fn check(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    start(&app, path, "check")
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
fn launch(app: tauri::State<'_, App>, path: String) -> Result<(), String> {
    let g = app.inst.normalize(Path::new(&path)).ok_or("To nie jest folder gry.")?;
    app.inst.launch(&g).map_err(|e| e.to_string())
}

/// The Steam app id of a game installed in a Steam library folder (steamapps/common/<dir>), read from
/// the library's appmanifest files.
pub fn steam_app_id(game: &Path) -> Option<String> {
    let dir = game.ancestors().find(|a| a.parent().and_then(|p| p.file_name()).map_or(false, |n| n.eq_ignore_ascii_case("common")))?;
    let install = dir.file_name()?.to_string_lossy().to_lowercase();
    let steamapps = dir.parent()?.parent()?;
    for e in std::fs::read_dir(steamapps).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        let field = |k: &str| text.lines().find_map(|l| { let p: Vec<&str> = l.split('"').collect(); (p.len() >= 4 && p[1] == k).then(|| p[3].to_string()) });
        if field("installdir").map(|d| d.to_lowercase()) == Some(install.clone()) {
            return field("appid");
        }
    }
    None
}

/// Start a game: through Steam when it sits in a Steam library, else through the Xbox app.
pub fn launch_auto(game: &Path, xbox_folder_part: &str) -> Result<()> {
    if let Some(id) = steam_app_id(game) {
        std::process::Command::new("cmd").args(["/C", "start", "", &format!("steam://rungameid/{id}")]).creation_flags(CREATE_NO_WINDOW).spawn()?;
        return Ok(());
    }
    launch_xbox(xbox_folder_part)
}

/// Look for a newer translation on GitHub and download it (texts only).
#[tauri::command]
async fn check_update(app: tauri::State<'_, App>) -> Result<Value, String> {
    let Some(repo) = app.inst.update_repo().map(String::from) else { return Ok(json!({"latest": null})) };
    let (dir, have, pf) = (app.dir(), app.best().version, app.inst.patch_format());
    let r = tauri::async_runtime::spawn_blocking(move || updates::check(&dir, &repo, &have, pf)).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    Ok(json!({"latest": r.version, "downloaded": r.downloaded, "needs_installer": r.needs_installer, "page": r.page, "version": app.best().version}))
}

#[tauri::command]
fn set_setting(app: tauri::State<'_, App>, name: String, value: Value) -> Result<(), String> {
    updates::set_setting(&app.dir(), &name, value).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_shortcut(app: tauri::State<'_, App>, path: String) -> Result<String, String> {
    let g = app.inst.normalize(Path::new(&path)).ok_or("To nie jest folder gry.")?;
    let exe = format!("{}.exe", app.inst.data_dir_name());
    updates::create_shortcut(&app.dir(), &app.inst.shortcut_name(), &exe, &g).map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/") {
        return Err("Nieobsługiwany adres.".into());
    }
    std::process::Command::new("explorer").arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(window: tauri::Window) {
    let _ = window.close();
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
        .invoke_handler(tauri::generate_handler![info, status, install, uninstall, check, progress, grant, launch, check_update, set_setting, create_shortcut, open_url, quit])
}
