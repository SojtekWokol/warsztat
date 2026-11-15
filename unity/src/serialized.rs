//! Unity serialized files (format 22: Unity 2020.1+ incl. 2020.3 and Unity 6): the type table, the object table and object data.
//! Only the object table is rebuilt on write; the type table and everything after the objects
//! (scripts, externals, ref types) are kept as the original bytes.
use crate::io::{Reader, Writer};
use crate::typetree::TypeNode;
use anyhow::{bail, Result};

pub struct SType {
    pub class_id: i32,
    pub script_type_index: i16,
    pub script_id: Option<[u8; 16]>,
    pub old_hash: [u8; 16],
    pub tree: Option<TypeNode>,
}

pub struct Object {
    pub path_id: i64,
    pub type_index: usize,
    pub class_id: i32,
    pub data: Vec<u8>,
    /// Where the data was in the original file: on write, objects keep their original data order
    /// (Unity does not always store data in table order); new objects go at the end.
    pub order: u64,
}

pub struct External {
    pub path: String,
}

pub struct SerializedFile {
    pub version: u32,
    pub big_endian: bool,
    pub unity_version: String,
    pub type_tree: bool,
    pub types: Vec<SType>,
    pub objects: Vec<Object>,
    pub externals: Vec<External>,
    header_tail: [u8; 8],
    meta_prefix: Vec<u8>,
    meta_suffix: Vec<u8>,
    orig_data_offset: u64,
    /// Alignment of object data on write: 16 for Unity 6, else as in the original (8 in Unity 2020.3).
    obj_align: u64,
}

/// An element count; rejects values that cannot fit in the remaining bytes (garbage input).
fn count(r: &mut Reader) -> Result<usize> {
    let n = r.i32()?;
    if n < 0 || n as usize > r.data.len() - r.pos {
        bail!("bad element count {n} at {}", r.pos - 4);
    }
    Ok(n as usize)
}

fn read_type(r: &mut Reader, version: u32, tt: bool, is_ref: bool) -> Result<SType> {
    let class_id = r.i32()?;
    let _stripped = r.u8()?;
    let script_type_index = r.i16()?;
    let script_id = if (is_ref && script_type_index >= 0) || class_id == 114 {
        Some(r.take(16)?.try_into().unwrap())
    } else {
        None
    };
    let old_hash: [u8; 16] = r.take(16)?.try_into().unwrap();
    let mut tree = None;
    if tt {
        tree = Some(TypeNode::read_blob(r, version)?);
        if version >= 21 {
            if is_ref {
                r.cstr()?;
                r.cstr()?;
                r.cstr()?;
            } else {
                let n = r.i32()? as usize;
                r.take(n * 4)?;
            }
        }
    }
    Ok(SType { class_id, script_type_index, script_id, old_hash, tree })
}

impl SerializedFile {
    pub fn read(file: &[u8]) -> Result<SerializedFile> {
        if file.len() < 48 {
            bail!("too short for a serialized file");
        }
        let mut r = Reader::new(file, true);
        r.u32()?;
        r.u32()?;
        let version = r.u32()?;
        r.u32()?;
        if !(22..=64).contains(&version) {
            bail!("serialized file format {version} not supported (need 22+; not a serialized file?)");
        }
        let big_endian = r.u8()? != 0;
        r.take(3)?;
        let meta_size = r.u32()? as usize;
        let _file_size = r.i64()?;
        let data_offset = r.i64()? as u64;
        let header_tail: [u8; 8] = r.take(8)?.try_into().unwrap();
        let meta_start = r.pos;
        if meta_start + meta_size > file.len() || data_offset as usize > file.len() {
            bail!("metadata or data offset past the end of the file (not a serialized file?)");
        }
        r.big = big_endian;
        let unity_version = r.cstr()?;
        let _platform = r.i32()?;
        let type_tree = r.u8()? != 0;
        let ntypes = count(&mut r)?;
        let mut types = Vec::with_capacity(ntypes);
        for _ in 0..ntypes {
            types.push(read_type(&mut r, version, type_tree, false)?);
        }
        let prefix_end = r.pos;
        let nobj = count(&mut r)?;
        let mut table = Vec::with_capacity(nobj);
        for _ in 0..nobj {
            r.align(4);
            let path_id = r.i64()?;
            let start = r.i64()? as u64;
            let size = r.u32()? as usize;
            let type_index = r.i32()? as usize;
            table.push((path_id, start, size, type_index));
        }
        let suffix_start = r.pos;
        // scripts, externals (parsed for PPtr file ids), then the rest kept raw
        let nscripts = count(&mut r)?;
        for _ in 0..nscripts {
            r.i32()?;
            r.align(4);
            r.i64()?;
        }
        let next = count(&mut r)?;
        let mut externals = Vec::with_capacity(next);
        for _ in 0..next {
            r.cstr()?;
            r.take(16)?;
            r.i32()?;
            externals.push(External { path: r.cstr()? });
        }
        let meta_end = meta_start + meta_size;
        // Unity 6 files are written 16-aligned as before (the shipped EA2 patch is tested that way);
        // older files keep their own alignment (2020.3 uses 8), so an unchanged file is rewritten 1:1
        let unity6 = unity_version.split('.').next().and_then(|y| y.parse::<u32>().ok()).map_or(true, |y| y >= 6000);
        let obj_align = if unity6 || table.iter().all(|t| t.1 % 16 == 0) { 16 } else { 8 };
        let objects = table
            .into_iter()
            .map(|(path_id, start, size, ti)| {
                let off = (data_offset + start) as usize;
                let (Some(t), Some(data)) = (types.get(ti), file.get(off..off + size)) else {
                    bail!("object {path_id}: bad type index {ti} or data range {off}+{size}");
                };
                Ok(Object { path_id, type_index: ti, class_id: t.class_id, data: data.to_vec(), order: start })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SerializedFile {
            version,
            big_endian,
            unity_version,
            type_tree,
            types,
            objects,
            externals,
            header_tail,
            meta_prefix: file[meta_start..prefix_end].to_vec(),
            meta_suffix: file[suffix_start..meta_end].to_vec(),
            orig_data_offset: data_offset,
            obj_align,
        })
    }

    pub fn object(&self, path_id: i64) -> Option<&Object> {
        self.objects.iter().find(|o| o.path_id == path_id)
    }
    pub fn object_mut(&mut self, path_id: i64) -> Option<&mut Object> {
        self.objects.iter_mut().find(|o| o.path_id == path_id)
    }
    /// Add an object (e.g. a clone); it is stored after all existing data.
    pub fn add_object(&mut self, path_id: i64, type_index: usize, data: Vec<u8>) {
        let class_id = self.types[type_index].class_id;
        self.objects.push(Object { path_id, type_index, class_id, data, order: u64::MAX });
    }
    pub fn max_path_id(&self) -> i64 {
        self.objects.iter().map(|o| o.path_id).max().unwrap_or(0)
    }

    pub fn write(&self) -> Vec<u8> {
        // metadata: kept prefix + new object table + kept suffix; offsets are relative to data start
        let mut by_order: Vec<usize> = (0..self.objects.len()).collect();
        by_order.sort_by_key(|&i| self.objects[i].order);
        let mut placed = vec![0u64; self.objects.len()];
        let mut pos = 0u64;
        for &i in &by_order {
            pos = (pos + self.obj_align - 1) / self.obj_align * self.obj_align;
            placed[i] = pos;
            pos += self.objects[i].data.len() as u64;
        }
        let header_len = 48usize;
        let mut m = Writer::new(self.big_endian);
        // pad so that alignment inside the metadata matches its absolute position in the file
        m.buf.resize(header_len, 0);
        m.bytes(&self.meta_prefix);
        m.i32(self.objects.len() as i32);
        for (o, start) in self.objects.iter().zip(&placed) {
            m.align(4);
            m.i64(o.path_id);
            m.i64(*start as i64);
            m.u32(o.data.len() as u32);
            m.i32(o.type_index as i32);
        }
        m.bytes(&self.meta_suffix);
        let meta = &m.buf[header_len..];
        let meta_end = (header_len + meta.len()) as u64;
        let data_offset = if meta_end <= self.orig_data_offset { self.orig_data_offset } else { (meta_end + 15) / 16 * 16 };
        let file_size = data_offset + pos;

        let mut w = Writer::new(true);
        w.u32(0);
        w.u32(0);
        w.u32(self.version);
        w.u32(0);
        w.u8(self.big_endian as u8);
        w.bytes(&[0, 0, 0]);
        w.u32(meta.len() as u32);
        w.i64(file_size as i64);
        w.i64(data_offset as i64);
        w.bytes(&self.header_tail);
        w.bytes(meta);
        w.buf.resize(data_offset as usize, 0);
        for &i in &by_order {
            w.buf.resize((data_offset + placed[i]) as usize, 0);
            w.bytes(&self.objects[i].data);
        }
        w.buf
    }
}
