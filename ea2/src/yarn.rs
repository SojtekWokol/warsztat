//! Yarn Spinner dialogue: each YarnProject lists one Localization per language. We add a "pl"
//! Localization (a copy of "it" with Polish "Speaker:text" lines) and list it in the project.
use crate::pack::{At, Pack};
use crate::store::Store;
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;
use unity::{TypeRegistry, Value};

pub struct YarnResult {
    pub project: String,
    pub translated: usize,
    pub total: usize,
}

fn has_field(pack: &Pack, reg: &TypeRegistry, at: At, f: &str) -> bool {
    pack.tree(reg, at).map_or(false, |t| t.child(f).is_some())
}

/// Patch every YarnProject in the pack. "VisNov*" projects are the cutscenes (workshop ids "y:"),
/// the others the campus conversations ("n:").
pub fn patch_pack(pack: &mut Pack, reg: &TypeRegistry, store: &Store, speakers: &HashMap<String, String>) -> Result<Vec<YarnResult>> {
    let projects: Vec<At> = pack.all().filter(|&at| pack.class(at) == 114 && has_field(pack, reg, at, "localizations")).collect();
    let mut out = Vec::new();
    for p in projects {
        let mut proj = pack.read_obj(reg, p)?;
        let name = proj.get("m_Name").and_then(|x| x.str()).unwrap_or("").to_string();
        let prefix = if name.starts_with("VisNov") { "y:" } else { "n:" };
        let ptrs = proj.get("localizations").and_then(|a| a.array()).cloned().unwrap_or_default();
        let mut locs: HashMap<String, (At, Value)> = HashMap::new();
        for ptr in &ptrs {
            let at = pack.resolve(p.file, ptr).ok_or_else(|| anyhow!("{name}: localization not found"))?;
            let v = pack.read_obj(reg, at)?;
            let code = v.get("_LocaleCode").and_then(|x| x.str()).unwrap_or("").to_string();
            locs.insert(code, (at, v));
        }
        if locs.len() != 7 {
            continue; // not a full project copy
        }
        if locs.contains_key("pl") {
            bail!("{name}: Yarn already has a Polish table - is the input the original?");
        }
        let table = |v: &Value| -> (Vec<Value>, Vec<Value>) {
            let t = v.get("_stringTable");
            let k = t.and_then(|t| t.get("keys")).and_then(|x| x.array()).cloned().unwrap_or_default();
            let vals = t.and_then(|t| t.get("values")).and_then(|x| x.array()).cloned().unwrap_or_default();
            (k, vals)
        };
        let (en_k, en_v) = table(&locs["en"].1);
        let en: HashMap<String, String> = en_k.iter().zip(&en_v).filter_map(|(k, v)| Some((k.str()?.to_string(), v.str()?.to_string()))).collect();
        let (it_at, it_v) = locs["it"].clone();
        let (keys, _) = table(&it_v);
        let mut values = Vec::with_capacity(keys.len());
        let mut n = 0;
        for k in &keys {
            let k = k.str().unwrap_or("");
            let src = en.get(k).cloned().unwrap_or_default();
            if let Some(pl) = store.get(&format!("{prefix}{k}")) {
                let who = if src.chars().take(40).collect::<String>().contains(':') { src.split(':').next().unwrap_or("").to_string() } else { String::new() };
                let who_pl = speakers.get(&who).cloned().unwrap_or(who);
                values.push(Value::Str(format!("{who_pl}:{pl}")));
                n += 1;
            } else {
                values.push(Value::Str(src));
            }
        }
        let mut pl = it_v.clone();
        *pl.get_mut("m_Name").unwrap() = Value::Str("pl".into());
        *pl.get_mut("_LocaleCode").unwrap() = Value::Str("pl".into());
        {
            let t = pl.get_mut("_stringTable").ok_or_else(|| anyhow!("no _stringTable"))?;
            *t.get_mut("keys").unwrap() = Value::Array(keys.clone());
            *t.get_mut("values").unwrap() = Value::Array(values);
        }
        // the clone: same type as "it", new path id in the same file
        let new_id = pack.sf(it_at.file).max_path_id() + 1;
        let type_index = pack.sf(it_at.file).objects[it_at.obj].type_index;
        let tree = pack.tree(reg, it_at).ok_or_else(|| anyhow!("no Localization tree"))?.clone();
        let big = pack.sf(it_at.file).big_endian;
        let data = unity::typetree::write(&tree, &pl, big)?;
        pack.files[it_at.file].1.add_object(new_id, type_index, data);
        pack.touch(it_at.file);
        if it_at.file != p.file {
            bail!("{name}: localization in another file than the project");
        }
        let mut new_ptr = ptrs[0].clone();
        *new_ptr.get_mut("m_FileID").unwrap() = Value::Int(0);
        *new_ptr.get_mut("m_PathID").unwrap() = Value::Int(new_id);
        proj.get_mut("localizations").and_then(|a| a.array_mut()).unwrap().push(new_ptr);
        pack.write_obj(reg, p, &proj)?;
        out.push(YarnResult { project: name, translated: n, total: keys.len() });
    }
    Ok(out)
}
