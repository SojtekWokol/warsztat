// What goes into the installer, so it can be built anywhere (also on GitHub Actions, without the game):
//   EA2_TRANSLATIONS  folder with releases/ and builds/   (default ../../translations)
//   EA2_FONTS         folder with the .ttf files           (default ../../fonts)
//   EA2_RELEASE       version to build, e.g. 1.0.0 or v1.0.0 (default: the newest release; before the
//                     first release, the last build written into the game, as "wersja robocza")
use std::path::{Path, PathBuf};

const FONTS: &[&str] = &["Urbanist-ExtraBold.ttf", "Urbanist-SemiBold.ttf", "ChunkFive-Regular.ttf", "Caveat-Bold.ttf", "Kalam-Bold.ttf", "Knewave-PL.ttf", "Jersey10-Regular.ttf"];

fn newest(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "json")).collect();
    v.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    v.pop()
}

fn main() {
    tauri_build::build();
    for v in ["EA2_TRANSLATIONS", "EA2_FONTS", "EA2_RELEASE"] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let tr = PathBuf::from(std::env::var("EA2_TRANSLATIONS").unwrap_or_else(|_| "../../translations".into()));
    let fonts = PathBuf::from(std::env::var("EA2_FONTS").unwrap_or_else(|_| "../../fonts".into()));
    println!("cargo:rerun-if-changed={}", tr.join("releases").display());
    println!("cargo:rerun-if-changed={}", tr.join("builds").display());
    let wanted = std::env::var("EA2_RELEASE").ok().map(|v| v.trim_start_matches('v').to_string()).filter(|v| !v.is_empty());
    let release_file = match &wanted {
        Some(v) => {
            let p = tr.join("releases").join(format!("{v}.json"));
            assert!(p.exists(), "release {v} not found: {}", p.display());
            Some(p)
        }
        None => newest(&tr.join("releases")),
    };
    let (texts, version) = if let Some(p) = release_file {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        (v["texts"].clone(), v["version"].as_str().unwrap_or("?").to_string())
    } else if let Some(p) = newest(&tr.join("builds")) {
        (serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap(), "wersja robocza".to_string())
    } else {
        panic!("no release and no build snapshot in {}: nothing to put into the installer", tr.display());
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("pl.json"), serde_json::to_vec(&texts).unwrap()).unwrap();
    std::fs::write(out.join("version.txt"), version).unwrap();
    std::fs::create_dir_all(out.join("fonts")).unwrap();
    for f in FONTS {
        let src = fonts.join(f);
        println!("cargo:rerun-if-changed={}", src.display());
        std::fs::copy(&src, out.join("fonts").join(f)).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
    }
}
