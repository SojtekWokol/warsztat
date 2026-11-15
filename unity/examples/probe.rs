//! Probe a UnityFS bundle: list nodes, serialized-file format and whether type trees are present.
use unity::Bundle;

fn be32(b: &[u8], o: usize) -> u32 { u32::from_be_bytes(b[o..o + 4].try_into().unwrap()) }

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("bundle path");
    let file = std::fs::read(&path)?;
    let b = Bundle::read(&file)?;
    drop(file);
    println!("bundle format {} engine {} nodes {}", b.format, b.version_engine, b.nodes.len());
    for n in &b.nodes {
        let d = &n.data;
        let mut info = String::new();
        if d.len() > 48 {
            let ver = be32(d, 8);
            if (9..=30).contains(&ver) {
                // v<22 header: meta u32, size u32, ver u32, dataoff u32, endian u8, 3 reserved, then metadata
                let meta = if ver >= 22 { 48 } else { 20 };
                let end = d[meta..].iter().position(|&c| c == 0).unwrap_or(0);
                let uv = String::from_utf8_lossy(&d[meta..meta + end]);
                let tt = d[meta + end + 1 + 4];
                info = format!("serialized v{ver} unity {uv} typetree={tt}");
            }
        }
        println!("{:>12}  {}  {}", n.data.len(), n.path, info);
    }
    Ok(())
}
