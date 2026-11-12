//! Minimal Unity asset access for Unity 6 games: UnityFS bundles, serialized files (format 22)
//! and type-tree based object reading/writing.
pub mod bundle;
mod common_strings;
pub mod io;
pub mod serialized;
pub mod typetree;

pub use bundle::Bundle;
pub use serialized::SerializedFile;
pub use typetree::{TypeNode, Value};

/// Bundle node flag: the node is a serialized file (not a .resS resource stream).
pub const NODE_SERIALIZED: u32 = 4;

/// The fixed start of every MonoBehaviour: GameObject, enabled, script and name.
pub struct MonoHeader {
    pub script_file: i32,
    pub script_path: i64,
    pub name: String,
}

pub fn mono_header(data: &[u8]) -> Option<MonoHeader> {
    let mut r = io::Reader::new(data, false);
    r.take(12).ok()?;
    r.u8().ok()?;
    r.align(4);
    let script_file = r.i32().ok()?;
    let script_path = r.i64().ok()?;
    let len = r.i32().ok()? as usize;
    if len > 1024 {
        return None;
    }
    let name = String::from_utf8_lossy(r.take(len).ok()?).into_owned();
    Some(MonoHeader { script_file, script_path, name })
}

/// Type trees by class (and script, for MonoBehaviours), gathered from bundles that embed them.
/// Files without type trees (the player's data.unity3d) use these.
#[derive(Default)]
pub struct TypeRegistry {
    pub trees: std::collections::HashMap<(i32, Option<[u8; 16]>), TypeNode>,
}

impl TypeRegistry {
    pub fn add_file(&mut self, sf: &SerializedFile) {
        for t in &sf.types {
            if let Some(tree) = &t.tree {
                let key = (t.class_id, if t.class_id == 114 { t.script_id } else { None });
                self.trees.entry(key).or_insert_with(|| tree.clone());
            }
        }
    }
    pub fn add_bundle(&mut self, b: &Bundle) -> anyhow::Result<()> {
        for n in &b.nodes {
            if n.flags & NODE_SERIALIZED != 0 {
                self.add_file(&SerializedFile::read(&n.data)?);
            }
        }
        Ok(())
    }
    /// Built-in trees for simple native classes a game may not ship in any bundle with type trees.
    pub fn add_builtin(&mut self) {
        let s = |name: &str| TypeNode { ty: "string".into(), name: name.into(), level: 1, byte_size: -1, meta: 0x4000, children: vec![] };
        self.trees.entry((49, None)).or_insert_with(|| TypeNode {
            ty: "TextAsset".into(),
            name: "Base".into(),
            level: 0,
            byte_size: -1,
            meta: 0,
            children: vec![s("m_Name"), s("m_Script")],
        });
    }

    /// The tree for an object of `sf`: its own if the file has trees, else from the registry.
    pub fn tree_for<'a>(&'a self, sf: &'a SerializedFile, obj: &serialized::Object) -> Option<&'a TypeNode> {
        let t = &sf.types[obj.type_index];
        t.tree.as_ref().or_else(|| self.trees.get(&(t.class_id, if t.class_id == 114 { t.script_id } else { None })))
    }
}
