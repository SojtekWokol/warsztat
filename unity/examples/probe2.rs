//! List TextAssets (name, size, start) and MonoScripts mentioning text in a UnityFS bundle without type trees.
use unity::{Bundle, SerializedFile};

fn lstr(d: &[u8], o: &mut usize) -> String {
    let n = i32::from_le_bytes(d[*o..*o + 4].try_into().unwrap()) as usize;
    *o += 4;
    let s = String::from_utf8_lossy(&d[*o..(*o + n).min(d.len())]).to_string();
    *o += n;
    *o = (*o + 3) & !3;
    s
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("bundle path");
    let file = std::fs::read(&path)?;
    let b = Bundle::read(&file)?;
    drop(file);
    for n in b.nodes.iter().filter(|n| n.flags & unity::NODE_SERIALIZED != 0) {
        let sf = match SerializedFile::read(&n.data) { Ok(s) => s, Err(e) => { if !n.path.ends_with(".resS") && !n.path.ends_with(".resource") { println!("ERR	{}	{}", n.path, e); } continue } };
        let mut cnt = std::collections::BTreeMap::new(); for o in &sf.objects { *cnt.entry(o.class_id).or_insert(0) += 1; }
        println!("OK	{}	{} objs	TA={} MB={} MS={}", n.path, sf.objects.len(), cnt.get(&49).unwrap_or(&0), cnt.get(&114).unwrap_or(&0), cnt.get(&115).unwrap_or(&0));
        for o in &sf.objects {
            let d = &o.data;
            if o.class_id == 49 {
                let mut p = 0;
                let name = lstr(d, &mut p);
                let len = i32::from_le_bytes(d[p..p + 4].try_into().unwrap());
                let head: String = String::from_utf8_lossy(&d[p + 4..(p + 4 + 70).min(d.len())]).replace('\n', "\n");
                println!("TA\t{}\t{}\t{}\t{}", n.path, name, len, head);
            } else if o.class_id == 115 {
                let mut p = 0;
                let name = lstr(d, &mut p);
                let l = name.to_lowercase();
                if ["local", "yarn", "dialog", "string", "text", "subtit", "language"].iter().any(|k| l.contains(k)) {
                    println!("MS\t{}\t{}", n.path, name);
                }
            }
        }
    }
    Ok(())
}
