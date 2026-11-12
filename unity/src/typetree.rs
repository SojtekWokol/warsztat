//! Type trees: the field layout of a class, and reading/writing object data with it.
use crate::common_strings;
use crate::io::{Reader, Writer};
use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub struct TypeNode {
    pub ty: String,
    pub name: String,
    pub level: u8,
    pub byte_size: i32,
    pub meta: i32,
    pub children: Vec<TypeNode>,
}

const ALIGN: i32 = 0x4000;

impl TypeNode {
    /// The binary type tree blob of a serialized type (format 12+).
    pub fn read_blob(r: &mut Reader, version: u32) -> Result<TypeNode> {
        let n = r.i32()? as usize;
        let sbuf_len = r.i32()? as usize;
        let mut raw = Vec::with_capacity(n);
        for _ in 0..n {
            let _ver = r.u16()?;
            let level = r.u8()?;
            let _flags = r.u8()?;
            let t_off = r.u32()?;
            let n_off = r.u32()?;
            let byte_size = r.i32()?;
            let _index = r.i32()?;
            let meta = r.i32()?;
            if version >= 19 {
                r.u64()?;
            }
            raw.push((level, t_off, n_off, byte_size, meta));
        }
        let sbuf = r.take(sbuf_len)?;
        let s = |off: u32| -> String {
            if off & 0x8000_0000 != 0 {
                let o = off & 0x7fff_ffff;
                common_strings::get(o).map(|x| x.to_string()).unwrap_or_else(|| o.to_string())
            } else {
                let rest = &sbuf[off as usize..];
                let e = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                String::from_utf8_lossy(&rest[..e]).into_owned()
            }
        };
        let flat: Vec<TypeNode> = raw
            .into_iter()
            .map(|(level, t, nm, bs, meta)| TypeNode { ty: s(t), name: s(nm), level, byte_size: bs, meta, children: vec![] })
            .collect();
        Ok(build(flat))
    }

    pub fn child(&self, name: &str) -> Option<&TypeNode> {
        self.children.iter().find(|c| c.name == name)
    }
}

fn build(flat: Vec<TypeNode>) -> TypeNode {
    // nodes come in depth-first order with levels
    let mut stack: Vec<TypeNode> = Vec::new();
    for n in flat {
        while let Some(top) = stack.last() {
            if top.level >= n.level {
                let done = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(done);
            } else {
                break;
            }
        }
        stack.push(n);
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(done);
    }
    stack.pop().unwrap()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Struct(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Struct(f) => f.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Struct(f) => f.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// Path like "m_FaceInfo.m_PointSize".
    pub fn at(&self, path: &str) -> Option<&Value> {
        path.split('.').try_fold(self, |v, k| v.get(k))
    }
    pub fn at_mut(&mut self, path: &str) -> Option<&mut Value> {
        path.split('.').try_fold(self, |v, k| v.get_mut(k))
    }
    pub fn i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::UInt(u) => Some(*u as i64),
            Value::Bool(b) => Some(*b as i64),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            Value::UInt(u) => Some(*u as f64),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    /// Set a number keeping its variant (so it is written with the node's type anyway).
    pub fn set_num(&mut self, x: f64) {
        *self = match self {
            Value::Int(_) => Value::Int(x as i64),
            Value::UInt(_) => Value::UInt(x as u64),
            Value::Bool(_) => Value::Bool(x != 0.0),
            _ => Value::Float(x),
        }
    }
}

fn is_array(n: &TypeNode) -> Option<&TypeNode> {
    if n.ty == "Array" {
        return Some(n);
    }
    n.children.first().filter(|c| c.ty == "Array")
}

fn byte_elem(n: &TypeNode) -> bool {
    matches!(n.ty.as_str(), "UInt8" | "SInt8" | "char")
}

pub fn read(node: &TypeNode, data: &[u8], big: bool) -> Result<Value> {
    let mut r = Reader::new(data, big);
    read_node(node, &mut r)
}

fn read_node(n: &TypeNode, r: &mut Reader) -> Result<Value> {
    let mut align = n.meta & ALIGN != 0;
    let v = match n.ty.as_str() {
        "SInt8" => Value::Int(r.u8()? as i8 as i64),
        "UInt8" | "char" => Value::UInt(r.u8()? as u64),
        "bool" => Value::Bool(r.u8()? != 0),
        "SInt16" | "short" => Value::Int(r.i16()? as i64),
        "UInt16" | "unsigned short" => Value::UInt(r.u16()? as u64),
        "SInt32" | "int" => Value::Int(r.i32()? as i64),
        "UInt32" | "unsigned int" | "Type*" => Value::UInt(r.u32()? as u64),
        "SInt64" | "long long" => Value::Int(r.i64()?),
        "UInt64" | "unsigned long long" | "FileSize" => Value::UInt(r.u64()?),
        "float" => Value::Float(r.f32()? as f64),
        "double" => Value::Float(r.f64()?),
        "string" => {
            let len = r.i32()? as usize;
            let raw = r.take(len)?;
            if n.children.first().map_or(false, |c| c.meta & ALIGN != 0) {
                align = true;
            }
            // binary "strings" (e.g. TextAsset bytes) stay bytes, so they are written back unchanged
            match std::str::from_utf8(raw) {
                Ok(s) => Value::Str(s.to_string()),
                Err(_) => Value::Bytes(raw.to_vec()),
            }
        }
        "TypelessData" => {
            let len = r.i32()? as usize;
            Value::Bytes(r.take(len)?.to_vec())
        }
        _ => {
            if let Some(arr) = is_array(n) {
                if arr.meta & ALIGN != 0 {
                    align = true;
                }
                let count = r.i32()? as usize;
                let elem = &arr.children[1];
                if byte_elem(elem) {
                    Value::Bytes(r.take(count)?.to_vec())
                } else {
                    let mut items = Vec::with_capacity(count.min(1 << 20));
                    for _ in 0..count {
                        items.push(read_node(elem, r)?);
                    }
                    Value::Array(items)
                }
            } else if n.children.is_empty() {
                bail!("unknown primitive type {} ({})", n.ty, n.name)
            } else {
                let mut fields = Vec::with_capacity(n.children.len());
                for c in &n.children {
                    fields.push((c.name.clone(), read_node(c, r)?));
                }
                Value::Struct(fields)
            }
        }
    };
    if align {
        r.align(4);
    }
    Ok(v)
}

pub fn write(node: &TypeNode, v: &Value, big: bool) -> Result<Vec<u8>> {
    let mut w = Writer::new(big);
    write_node(node, v, &mut w)?;
    Ok(w.buf)
}

fn num(v: &Value, n: &TypeNode) -> Result<f64> {
    v.f64().or_else(|| v.i64().map(|i| i as f64)).ok_or_else(|| anyhow::anyhow!("{} ({}): expected a number", n.name, n.ty))
}
fn int(v: &Value, n: &TypeNode) -> Result<i64> {
    match v {
        Value::UInt(u) => Ok(*u as i64),
        _ => v.i64().ok_or_else(|| anyhow::anyhow!("{} ({}): expected an integer", n.name, n.ty)),
    }
}

fn write_node(n: &TypeNode, v: &Value, w: &mut Writer) -> Result<()> {
    let mut align = n.meta & ALIGN != 0;
    match n.ty.as_str() {
        "SInt8" | "UInt8" | "char" => w.u8(int(v, n)? as u8),
        "bool" => w.u8(int(v, n)? as u8),
        "SInt16" | "short" => w.i16(int(v, n)? as i16),
        "UInt16" | "unsigned short" => w.u16(int(v, n)? as u16),
        "SInt32" | "int" => w.i32(int(v, n)? as i32),
        "UInt32" | "unsigned int" | "Type*" => w.u32(int(v, n)? as u32),
        "SInt64" | "long long" => w.i64(int(v, n)?),
        "UInt64" | "unsigned long long" | "FileSize" => w.u64(match v {
            Value::UInt(u) => *u,
            _ => int(v, n)? as u64,
        }),
        "float" => w.f32(num(v, n)? as f32),
        "double" => w.f64(num(v, n)?),
        "string" => {
            let b: &[u8] = match v {
                Value::Str(s) => s.as_bytes(),
                Value::Bytes(b) => b,
                _ => bail!("{}: expected a string", n.name),
            };
            w.i32(b.len() as i32);
            w.bytes(b);
            if n.children.first().map_or(false, |c| c.meta & ALIGN != 0) {
                align = true;
            }
        }
        "TypelessData" => {
            let Value::Bytes(b) = v else { bail!("{}: expected bytes", n.name) };
            w.i32(b.len() as i32);
            w.bytes(b);
        }
        _ => {
            if let Some(arr) = is_array(n) {
                if arr.meta & ALIGN != 0 {
                    align = true;
                }
                let elem = &arr.children[1];
                match v {
                    Value::Bytes(b) => {
                        w.i32(b.len() as i32);
                        w.bytes(b);
                    }
                    Value::Array(items) => {
                        w.i32(items.len() as i32);
                        for it in items {
                            write_node(elem, it, w)?;
                        }
                    }
                    _ => bail!("{}: expected an array", n.name),
                }
            } else {
                let Value::Struct(fields) = v else { bail!("{} ({}): expected a struct", n.name, n.ty) };
                for c in &n.children {
                    let fv = fields
                        .iter()
                        .find(|(k, _)| *k == c.name)
                        .map(|(_, v)| v)
                        .ok_or_else(|| anyhow::anyhow!("{}: missing field {}", n.name, c.name))?;
                    write_node(c, fv, w)?;
                }
            }
        }
    }
    if align {
        w.align(4);
    }
    Ok(())
}
