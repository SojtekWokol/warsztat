//! SHA-256 of game files we know: clean originals of the supported version (Epic, 2026-11) and files
//! written by test builds of the patch (never to be taken for originals).
const ORIGINAL: &[(&str, &str)] = &[
    ("data.unity3d", "d99b865f6e08a615aa38334f2a2e876571e09f71603d8a71ff0cc83173637871"),
    ("Assembly-CSharp.dll", "6df862a5e1efd3e1828c212af3fa8286ef91215e65e5a5bfd1d341609e1f1dd1"),
    ("catalog.json", "ceb3a466c9954e4df4e8fe46eb724cdc43536aba16a8059077db88a53c31a0ad"),
    ("areadioramas_assets_all_ecc21e2d9ade8095ee803e26e46fd1e6.bundle", "ae351a430ad94b181776c13da399592b12d83a64f8c3721e702590671b655964"),
];

const PATCHED: &[(&str, &str)] = &[
    // the IL patch is deterministic: every patched Assembly-CSharp.dll is this file
    ("Assembly-CSharp.dll", "99eb07e34fcac9b7fc15c37ca7559ddbc7259658a97e2b759e62faa298bb45aa"),
    // testlang build installed by hand on 2026-11-15
    ("data.unity3d", "fcd49505083f5e032318019c61c54724ada6f985bd9e46ef438a6822aa3df0a9"),
    ("catalog.json", "23ab14b9a63c854331745cd997b4b763de2f937b4e38bc646efa62a1ac736cc5"),
    ("areadioramas_assets_all_ecc21e2d9ade8095ee803e26e46fd1e6.bundle", "36f58e6aa8677cb050795f07df6c88a73287f8a73b8118c49883c83ede7f6330"),
];

pub fn knows(name: &str) -> bool {
    ORIGINAL.iter().any(|(n, _)| *n == name)
}
pub fn is_original(name: &str, hash: &str) -> bool {
    ORIGINAL.iter().any(|(n, h)| *n == name && *h == hash)
}
pub fn is_patched(name: &str, hash: &str) -> bool {
    PATCHED.iter().any(|(n, h)| *n == name && *h == hash)
}

/// Backups of the files the EA1 patch changes; the marker sits in "Escape Academy_Data".
pub const CONFIG: kopie::Config = kopie::Config {
    marker_anchor: "Escape Academy_Data",
    marker_file: "ea1_pl.json",
    known_patched: is_patched,
    knows,
    known_original: is_original,
    repair_hint: "Epic Games Launcher: Biblioteka > Escape Academy > ... > Zarządzaj > Zweryfikuj",
};
