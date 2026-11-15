//! Two in-place IL patches to Assembly-CSharp.dll that make a fifth language (Polish) possible.
//!
//! The game's enum `Escape.Localization.LanguageType` knows only English, Spanish, German and French.
//! Polish is added to `LanguageManager.languages` as an entry with the *English* enum value (so
//! graphics, materials, puzzle answers and other per-language assets behave as in English), its own
//! code "pl" and its own GameplayStrings column. Two places must then tell Polish from English:
//!
//! 1. `VisualNovelManager.InitializeVisualNovel` picked the Yarn language from a hard-coded
//!    enum -> code dictionary (en, es, fr, de); now it uses `currentLanguage.languageCode`
//!    (the same values for the four original languages, "pl" for Polish).
//! 2. `LanguageManager.get_IsCurrentLanguageDefault` compared the enum with the default one;
//!    now it checks `currentLanguage.csvIndex == 1` (English is the only language in column 1), so
//!    Polish subtitles (`<clip>_pl`, `VoSubtitleData`) are looked up instead of the English ones.
//!
//! Both bodies keep their size (the rest is filled with `nop`), so no metadata, RVA or section
//! changes. The original bytes are matched exactly: another game version gives an error.
use anyhow::{bail, Result};

struct Patch {
    what: &'static str,
    original: &'static [u8],
    patched: &'static [u8],
}

// Field tokens (Escape.Localization.Language): languageName 0x040014d9, languageCode 0x040014db,
// csvIndex 0x040014dc; LanguageManager::Instance 0x040014df, currentLanguage 0x040014e0,
// defaultLanguage 0x040014e2; get_CurrentLanguage 0x06001821.
const PATCHES: [Patch; 2] = [
    Patch {
        what: "VisualNovelManager.InitializeVisualNovel: Yarn language from languageCode",
        // ldstr ""; stloc.0; ldarg.0; ldfld escapeLanguageToYarn; ldsfld Instance; ldfld currentLanguage;
        // ldfld languageName; ldloca.s 0; callvirt TryGetValue; pop; ldarg.0; ldfld yarnRunner; ldloc.0
        original: &[
            0x72, 0x6b, 0x0c, 0x00, 0x70, 0x0a, 0x02, 0x7b, 0x97, 0x04, 0x00, 0x04, 0x7e, 0xdf, 0x14, 0x00, 0x04, 0x7b, 0xe0, 0x14, 0x00, 0x04, 0x7b, 0xd9, 0x14, 0x00,
            0x04, 0x12, 0x00, 0x6f, 0xfe, 0x04, 0x00, 0x0a, 0x26, 0x02, 0x7b, 0x9c, 0x04, 0x00, 0x04, 0x06,
        ],
        // ldsfld Instance; ldfld currentLanguage; ldfld languageCode; stloc.0; nop x20; ldarg.0; ldfld yarnRunner; ldloc.0
        patched: &[
            0x7e, 0xdf, 0x14, 0x00, 0x04, 0x7b, 0xe0, 0x14, 0x00, 0x04, 0x7b, 0xdb, 0x14, 0x00, 0x04, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x7b, 0x9c, 0x04, 0x00, 0x04, 0x06,
        ],
    },
    Patch {
        what: "LanguageManager.get_IsCurrentLanguageDefault: csvIndex == 1",
        // tiny header (23 bytes); call get_CurrentLanguage; ldfld languageName; ldsfld Instance;
        // ldfld defaultLanguage; ceq; ret
        original: &[0x5e, 0x28, 0x21, 0x18, 0x00, 0x06, 0x7b, 0xd9, 0x14, 0x00, 0x04, 0x7e, 0xdf, 0x14, 0x00, 0x04, 0x7b, 0xe2, 0x14, 0x00, 0x04, 0xfe, 0x01, 0x2a],
        // call get_CurrentLanguage; ldfld csvIndex; ldc.i4.1; nop x9; ceq; ret
        patched: &[0x5e, 0x28, 0x21, 0x18, 0x00, 0x06, 0x7b, 0xdc, 0x14, 0x00, 0x04, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x01, 0x2a],
    },
];

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    hay.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| i).collect()
}

#[derive(Debug, PartialEq)]
pub enum State {
    Original,
    Patched,
}

/// Is the assembly the original, already patched, or unknown (error)?
pub fn state(dll: &[u8]) -> Result<State> {
    let mut orig = 0;
    let mut done = 0;
    for p in &PATCHES {
        let (o, d) = (find_all(dll, p.original).len(), find_all(dll, p.patched).len());
        match (o, d) {
            (1, 0) => orig += 1,
            (0, 1) => done += 1,
            _ => bail!("Assembly-CSharp.dll: unexpected code for \"{}\" ({o} original, {d} patched matches) - another game version?", p.what),
        }
    }
    match (orig, done) {
        (n, 0) if n == PATCHES.len() => Ok(State::Original),
        (0, n) if n == PATCHES.len() => Ok(State::Patched),
        _ => bail!("Assembly-CSharp.dll is only partly patched"),
    }
}

/// The patched assembly (unchanged if already patched).
pub fn patch(dll: &[u8]) -> Result<Vec<u8>> {
    if state(dll)? == State::Patched {
        return Ok(dll.to_vec());
    }
    let mut out = dll.to_vec();
    for p in &PATCHES {
        assert_eq!(p.original.len(), p.patched.len());
        let at = find_all(&out, p.original)[0];
        out[at..at + p.patched.len()].copy_from_slice(p.patched);
    }
    Ok(out)
}
