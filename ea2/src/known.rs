//! SHA-256 of game files we know: clean originals of the supported game version, and files patched
//! by earlier versions of the patch (which must never be taken for originals).
const ORIGINAL: &[(&str, &str)] = &[
    ("data.unity3d", "75287bc9b801300973c04762ea9d79e110ad2663ad2680ae0661f9b0baa94343"),
];

const PATCHED: &[(&str, &str)] = &[
    // Python patcher, 2026-11-12 build
    ("catalog.bin", "632559c96a106589e12347ce8694a12180e41dc75453f0020dca6c2034dca8bb"),
    ("data.unity3d", "a445727b1ab41e7efe41295f8cd0ee4431c583459c70100a3ade6704fec76ce4"),
    ("duplicateassetisolation_assets_all_bf2c5b991214e3f85fc406632992e22d.bundle", "7a0ce588cc3b000b5739f867a28f34dc2960b666229d41b85f94748120070dc5"),
    ("inventoryitemruntimedata_assets_mus_fertilizer_runtime_59119917cb899eddda6bbd086992854c.bundle", "b8df2b20d206d6ae190acd3c42ecfd005eb7d672ceef19734f0f70f8ae84c802"),
    ("inventoryitemruntimedata_assets_mus_guitarstrumchart_runtime_b8f267b1460216c440875cf8ec35844d.bundle", "15ee1417ca795b60ef26703edadadfe96225ae6db53e0a8f54da0803dcca03d7"),
    ("subtitles_assets_all_b4c8728c3e4961faa2f0ff32d3f928f4.bundle", "929fd71759e201e270edd90c5bd2b64c02002db5f0279d5019168ee670e6aa17"),
    ("yarnprojects_assets_all_f0a13e41fdc3a3fd4b2c0d9f96309fcd.bundle", "aa32b10cc45ca4c7709d5ab7e3a410066b0e9b352ba239bd03b70333374f786b"),
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
