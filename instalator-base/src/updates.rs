//! Keeping the translation up to date: the newest texts (built in, or downloaded from the translation's
//! GitHub releases), which version is in which game folder, settings, the desktop shortcut.
//!
//! A release of the translation repository carries `tlumaczenie.json` ({"version", "texts": {id: text},
//! optional "patch_format"}) and `tlumaczenie.json.sha256`. Only texts are downloaded, never programs.
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CACHE: &str = "tlumaczenie.json";
const INSTALLED: &str = "zainstalowane.json";
const SETTINGS: &str = "ustawienia.json";

/// Translation texts with their version; `texts` is the JSON object {id: text}.
pub struct Translation {
    pub version: String,
    pub texts: Vec<u8>,
    pub count: usize,
}

/// "1.2.10" > "1.2.9"; anything not numeric (e.g. "wersja robocza") is the oldest.
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Option<Vec<u64>> { v.trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect() };
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn read_json(p: &Path) -> Option<Value> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_json(p: &Path, v: &Value) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v)?)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

fn from_release(v: &Value) -> Option<Translation> {
    let texts = v.get("texts")?.as_object()?;
    Some(Translation { version: v.get("version")?.as_str()?.to_string(), texts: serde_json::to_vec(texts).ok()?, count: texts.len() })
}

/// The newest translation we have: built into the installer, or downloaded earlier.
pub fn best(dir: &Path, embedded_version: &str, embedded_texts: &[u8]) -> Translation {
    let count = serde_json::from_slice::<Value>(embedded_texts).ok().and_then(|v| v.as_object().map(|o| o.len())).unwrap_or(0);
    let built_in = Translation { version: embedded_version.to_string(), texts: embedded_texts.to_vec(), count };
    match read_json(&dir.join(CACHE)).as_ref().and_then(from_release) {
        Some(c) if cmp_versions(&c.version, &built_in.version) == Ordering::Greater => c,
        _ => built_in,
    }
}

fn get(url: &str) -> Result<ureq::Response> {
    // a generic client name; nothing about the player is sent
    Ok(ureq::get(url).set("User-Agent", "translation-installer").set("Accept", "application/vnd.github+json").timeout(Duration::from_secs(8)).call()?)
}

/// What the translation's newest GitHub release offers, and what we did about it.
pub struct Remote {
    pub version: String,
    pub page: String,
    /// the texts were downloaded and are ready to install
    pub downloaded: bool,
    /// the release needs a newer installer (the way of patching changed)
    pub needs_installer: bool,
}

/// Ask GitHub for the newest release; if it is newer than what we have, download its texts.
pub fn check(dir: &Path, repo: &str, have: &str, patch_format: u32) -> Result<Remote> {
    let rel: Value = get(&format!("https://api.github.com/repos/{repo}/releases/latest"))?.into_json()?;
    let version = rel.get("tag_name").and_then(|x| x.as_str()).unwrap_or("").trim_start_matches('v').to_string();
    let page = rel.get("html_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut out = Remote { version: version.clone(), page, downloaded: false, needs_installer: false };
    if cmp_versions(&version, have) != Ordering::Greater {
        return Ok(out);
    }
    let asset = |name: &str| -> Option<String> {
        rel.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))?.get("browser_download_url")?.as_str().map(String::from)
    };
    let (Some(json_url), Some(sha_url)) = (asset(CACHE), asset("tlumaczenie.json.sha256")) else {
        out.needs_installer = true; // an old-style release: only the installer has the texts
        return Ok(out);
    };
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut get(&json_url)?.into_reader().take(64 << 20), &mut body)?;
    let want = get(&sha_url)?.into_string()?.trim().to_lowercase();
    let got: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
    if want != got {
        bail!("Pobrany plik tłumaczenia jest uszkodzony (suma kontrolna się nie zgadza). Spróbuj ponownie później.");
    }
    let v: Value = serde_json::from_slice(&body).context("plik tłumaczenia")?;
    if v.get("patch_format").and_then(|x| x.as_u64()).unwrap_or(1) > patch_format as u64 {
        out.needs_installer = true;
        return Ok(out);
    }
    if from_release(&v).map(|t| t.version) != Some(version) {
        bail!("Plik tłumaczenia nie pasuje do wydania.");
    }
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("tlumaczenie.json.tmp");
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, dir.join(CACHE))?;
    out.downloaded = true;
    Ok(out)
}

fn key(game: &Path) -> String {
    game.to_string_lossy().to_lowercase()
}

/// The translation version installed into a game folder (as we last wrote it).
pub fn installed_version(dir: &Path, game: &Path) -> Option<String> {
    read_json(&dir.join(INSTALLED))?.get(key(game))?.as_str().map(String::from)
}

pub fn set_installed_version(dir: &Path, game: &Path, version: Option<&str>) -> Result<()> {
    let p = dir.join(INSTALLED);
    let mut v = read_json(&p).unwrap_or_else(|| json!({}));
    match version {
        Some(ver) => v[key(game)] = json!(ver),
        None => {
            v.as_object_mut().map(|o| o.remove(&key(game)));
        }
    }
    write_json(&p, &v)
}

pub fn settings(dir: &Path) -> Value {
    let mut v = read_json(&dir.join(SETTINGS)).unwrap_or_else(|| json!({}));
    if v.get("auto_check").is_none() {
        v["auto_check"] = json!(true);
    }
    v
}

pub fn set_setting(dir: &Path, name: &str, value: Value) -> Result<()> {
    let mut v = settings(dir);
    v[name] = value;
    write_json(&dir.join(SETTINGS), &v)
}

/// A desktop shortcut that starts the game through the installer (`--graj <game folder>`): before each
/// start it checks that the translation is in the game and up to date. The installer copies itself into
/// its data folder first, so the shortcut keeps working when the downloaded file is deleted.
pub fn create_shortcut(dir: &Path, name: &str, exe_name: &str, game: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let me = std::env::current_exe()?;
    let target = dir.join(exe_name);
    if me != target {
        std::fs::copy(&me, &target).context("kopiowanie programu")?;
    }
    let q = |s: &str| s.replace('\'', "''");
    let ps = format!(
        "$d = [Environment]::GetFolderPath('Desktop'); $p = Join-Path $d '{name}.lnk'; \
         $s = (New-Object -ComObject WScript.Shell).CreateShortcut($p); $s.TargetPath = '{t}'; \
         $s.Arguments = '--graj \"{g}\"'; $s.WorkingDirectory = '{w}'; $s.IconLocation = '{t},0'; \
         $s.Description = 'Uruchamia grę i pilnuje, żeby spolszczenie było aktualne'; $s.Save(); $p",
        name = q(name),
        t = q(&target.to_string_lossy()),
        g = q(&game.to_string_lossy()),
        w = q(&dir.to_string_lossy()),
    );
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(CREATE_NO_WINDOW).output()?;
    if !out.status.success() {
        return Err(anyhow!("Nie udało się utworzyć skrótu: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// `--graj <folder>` on the command line: the shortcut's launcher mode.
pub fn launcher_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--graj").and_then(|i| args.get(i + 1)).cloned()
}

use std::io::Read as _;
