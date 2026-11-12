// EA2 PL Warsztat: the desktop workshop. Everything runs in this process: the shared workshop core
// (warsztat-base) with the Escape Academy 2 adapter (ea2). The UI (warsztat/ui) calls one command,
// `api(route, d)`, with the same routes the old local server had; pictures and fonts come through
// the "zasob" protocol (http://zasob.localhost/cache/..., /fonts/...).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::http::{Response, StatusCode};
use warsztat::Workshop;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The project folder: searched upwards from the exe (and the working folder) for translations/pl (or an old translations/pl.json).
fn project_root() -> Option<PathBuf> {
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|p| p.ancestors().find(|d| d.join("translations").join("pl").is_dir() || d.join("translations").join("pl.json").is_file()).map(Path::to_path_buf))
}

fn start_ollama() {
    let running = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ollama.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("ollama.exe"))
        .unwrap_or(true);
    if !running {
        let _ = std::process::Command::new("ollama").arg("serve").creation_flags(CREATE_NO_WINDOW).spawn();
    }
}

struct State(Result<Arc<Workshop>, String>);

#[tauri::command]
async fn api(state: tauri::State<'_, State>, route: String, d: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = state.0.clone()?;
    tauri::async_runtime::spawn_blocking(move || ws.api(&route, &d)).await.map_err(|e| e.to_string())
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn main() {
    let root = project_root();
    let state = match &root {
        Some(r) => Workshop::open(r, Arc::new(ea2::workshop::Ea2::default())).map_err(|e| format!("Nie udało się wczytać danych: {e}")),
        None => Err("Nie znaleziono folderu projektu (translations/pl). Program musi leżeć w folderze tłumaczenia.".into()),
    };
    let asset_root = root.clone();
    tauri::Builder::default()
        .manage(State(state))
        .register_uri_scheme_protocol("zasob", move |_ctx, req| {
            let path = urlencoding_decode(req.uri().path());
            let file = asset_root.as_ref().and_then(|r| {
                let rel = path.trim_start_matches('/');
                let (base, rest) = rel.split_once('/')?;
                if !matches!(base, "cache" | "fonts") || rest.contains("..") {
                    return None;
                }
                Some(r.join(base).join(rest))
            });
            match file.as_ref().and_then(|f| std::fs::read(f).ok().map(|b| (f, b))) {
                Some((f, b)) => Response::builder().header("Content-Type", mime(f)).header("Access-Control-Allow-Origin", "*").header("Cache-Control", "max-age=3600").body(b).unwrap(),
                None => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap(),
            }
        })
        .setup(|_| {
            start_ollama();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("tauri app");
}

fn urlencoding_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
