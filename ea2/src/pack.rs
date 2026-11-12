//! A bundle with its serialized files parsed, plus helpers to find, read and write objects.
use anyhow::{anyhow, Result};
use unity::{typetree, Bundle, SerializedFile, TypeNode, TypeRegistry, Value, NODE_SERIALIZED};

pub struct Pack {
    pub bundle: Bundle,
    /// (node index, parsed file)
    pub files: Vec<(usize, SerializedFile)>,
}

/// An object address inside a pack: file index (into `files`) and object index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct At {
    pub file: usize,
    pub obj: usize,
}

impl Pack {
    pub fn read(bytes: &[u8]) -> Result<Pack> {
        let bundle = Bundle::read(bytes)?;
        let mut files = Vec::new();
        for (i, n) in bundle.nodes.iter().enumerate() {
            if n.flags & NODE_SERIALIZED != 0 {
                files.push((i, SerializedFile::read(&n.data)?));
            }
        }
        Ok(Pack { bundle, files })
    }

    pub fn sf(&self, f: usize) -> &SerializedFile {
        &self.files[f].1
    }

    pub fn all(&self) -> impl Iterator<Item = At> + '_ {
        self.files.iter().enumerate().flat_map(|(fi, (_, sf))| (0..sf.objects.len()).map(move |oi| At { file: fi, obj: oi }))
    }

    pub fn class(&self, at: At) -> i32 {
        self.files[at.file].1.objects[at.obj].class_id
    }

    pub fn tree<'a>(&'a self, reg: &'a TypeRegistry, at: At) -> Option<&'a TypeNode> {
        let sf = &self.files[at.file].1;
        reg.tree_for(sf, &sf.objects[at.obj])
    }

    pub fn read_obj(&self, reg: &TypeRegistry, at: At) -> Result<Value> {
        let tree = self.tree(reg, at).ok_or_else(|| anyhow!("no type tree for object"))?;
        let sf = &self.files[at.file].1;
        typetree::read(tree, &sf.objects[at.obj].data, sf.big_endian)
    }

    pub fn write_obj(&mut self, reg: &TypeRegistry, at: At, v: &Value) -> Result<()> {
        let tree = self.tree(reg, at).ok_or_else(|| anyhow!("no type tree for object"))?.clone();
        let big = self.files[at.file].1.big_endian;
        let data = typetree::write(&tree, v, big)?;
        self.files[at.file].1.objects[at.obj].data = data;
        Ok(())
    }

    /// Resolve a PPtr {m_FileID, m_PathID} seen from file `from`.
    pub fn resolve(&self, from: usize, ptr: &Value) -> Option<At> {
        let fid = ptr.get("m_FileID")?.i64()?;
        let pid = ptr.get("m_PathID")?.i64()?;
        let file = if fid == 0 {
            from
        } else {
            let ext = &self.files[from].1.externals.get(fid as usize - 1)?.path;
            let name = ext.rsplit('/').next()?.to_lowercase();
            self.files.iter().position(|(ni, _)| self.bundle.nodes[*ni].path.to_lowercase() == name)?
        };
        let obj = self.files[file].1.objects.iter().position(|o| o.path_id == pid)?;
        Some(At { file, obj })
    }

    /// Bytes of a resource stream (".resS") by the path a texture stores.
    pub fn stream(&self, path: &str, offset: u64, size: usize) -> Option<Vec<u8>> {
        let name = path.rsplit('/').next()?;
        let n = self.bundle.nodes.iter().find(|n| n.path == name)?;
        n.data.get(offset as usize..offset as usize + size).map(|s| s.to_vec())
    }

    /// Put the (changed) serialized files back into the bundle nodes.
    pub fn commit(&mut self) {
        for (ni, sf) in &self.files {
            self.bundle.nodes[*ni].data = sf.write();
        }
    }

    /// CRC32 of the uncompressed data, as the Addressables catalog checks it.
    pub fn crc(&self) -> u32 {
        let mut h = crc32fast::Hasher::new();
        for n in &self.bundle.nodes {
            h.update(&n.data);
        }
        h.finalize()
    }
}

/// Name of a MonoBehaviour's script class (via its MonoScript), e.g. "LocalizationManager".
pub fn script_class(pack: &Pack, reg: &TypeRegistry, at: At) -> Option<String> {
    let sf = pack.sf(at.file);
    let h = unity::mono_header(&sf.objects[at.obj].data)?;
    let ptr = Value::Struct(vec![("m_FileID".into(), Value::Int(h.script_file as i64)), ("m_PathID".into(), Value::Int(h.script_path))]);
    let ms = pack.resolve(at.file, &ptr)?;
    pack.read_obj(reg, ms).ok()?.get("m_ClassName")?.str().map(|s| s.to_string())
}

pub fn text_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        _ => String::new(),
    }
}
