//! UnityFS asset bundles: a header, a (compressed) table of blocks and files, and the blocks.
//! `Bundle::read` decompresses everything into one buffer; `write` packs it again with LZ4.
use crate::io::{Reader, Writer};
use anyhow::{bail, Context, Result};

pub struct Node {
    pub path: String,
    pub flags: u32,
    pub data: Vec<u8>,
}

pub struct Bundle {
    pub format: u32,
    pub version_player: String,
    pub version_engine: String,
    pub flags: u32,
    pub nodes: Vec<Node>,
}

const COMPRESSION_MASK: u32 = 0x3f;
const BLOCKS_AND_DIR_COMBINED: u32 = 0x40;
const BLOCKS_INFO_AT_END: u32 = 0x80;
const PADDING_AT_START: u32 = 0x200;
const BLOCK_SIZE: usize = 0x20000;

fn decompress(kind: u32, src: &[u8], out_len: usize) -> Result<Vec<u8>> {
    Ok(match kind {
        0 => src.to_vec(),
        1 => {
            // Unity LZMA: 5 byte properties, no size field
            let mut hdr = src[..5].to_vec();
            hdr.extend_from_slice(&(out_len as u64).to_le_bytes());
            hdr.extend_from_slice(&src[5..]);
            let mut out = Vec::with_capacity(out_len);
            lzma_rs::lzma_decompress(&mut &hdr[..], &mut out).context("lzma")?;
            out
        }
        2 | 3 => lz4_flex::block::decompress(src, out_len).context("lz4")?,
        k => bail!("unknown compression {k}"),
    })
}

impl Bundle {
    pub fn read(file: &[u8]) -> Result<Bundle> {
        let mut r = Reader::new(file, true);
        let sig = r.cstr()?;
        if sig != "UnityFS" {
            bail!("not a UnityFS bundle ({sig})");
        }
        let format = r.u32()?;
        let version_player = r.cstr()?;
        let version_engine = r.cstr()?;
        let _size = r.i64()?;
        let csize = r.u32()? as usize;
        let usize_ = r.u32()? as usize;
        let flags = r.u32()?;
        if format >= 7 {
            r.align(16);
        }
        let info_c = if flags & BLOCKS_INFO_AT_END != 0 {
            &file[file.len() - csize..]
        } else {
            r.take(csize)?
        };
        let info = decompress(flags & COMPRESSION_MASK, info_c, usize_)?;
        if flags & PADDING_AT_START != 0 {
            r.align(16);
        }
        let mut ir = Reader::new(&info, true);
        ir.take(16)?; // uncompressed data hash
        let nblocks = ir.i32()? as usize;
        let mut blocks = Vec::with_capacity(nblocks);
        let mut total = 0usize;
        for _ in 0..nblocks {
            let u = ir.u32()? as usize;
            let c = ir.u32()? as usize;
            let f = ir.u16()? as u32;
            total += u;
            blocks.push((u, c, f));
        }
        let mut data = Vec::with_capacity(total);
        for (u, c, f) in blocks {
            let src = r.take(c)?;
            data.extend_from_slice(&decompress(f & COMPRESSION_MASK, src, u)?);
        }
        let nnodes = ir.i32()? as usize;
        let mut nodes = Vec::with_capacity(nnodes);
        for _ in 0..nnodes {
            let off = ir.i64()? as usize;
            let size = ir.i64()? as usize;
            let nflags = ir.u32()?;
            let path = ir.cstr()?;
            nodes.push(Node { path, flags: nflags, data: data[off..off + size].to_vec() });
        }
        Ok(Bundle { format, version_player, version_engine, flags, nodes })
    }

    pub fn node(&self, path: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.path == path)
    }
    pub fn node_mut(&mut self, path: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.path == path)
    }

    /// Pack again: data in 128 KB LZ4 blocks, block table LZ4-compressed right after the header.
    pub fn write(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let mut dir = Vec::new();
        for n in &self.nodes {
            dir.push((data.len(), n.data.len(), n.flags, n.path.clone()));
            data.extend_from_slice(&n.data);
        }
        let mut blocks: Vec<Vec<u8>> = Vec::new();
        let mut table = Vec::new();
        for chunk in data.chunks(BLOCK_SIZE) {
            let c = lz4_flex::block::compress(chunk);
            if c.len() < chunk.len() {
                table.push((chunk.len() as u32, c.len() as u32, 2u16));
                blocks.push(c);
            } else {
                table.push((chunk.len() as u32, chunk.len() as u32, 0u16));
                blocks.push(chunk.to_vec());
            }
        }
        let mut iw = Writer::new(true);
        iw.bytes(&[0u8; 16]);
        iw.i32(table.len() as i32);
        for (u, c, f) in &table {
            iw.u32(*u);
            iw.u32(*c);
            iw.u16(*f);
        }
        iw.i32(dir.len() as i32);
        for (off, size, f, path) in &dir {
            iw.i64(*off as i64);
            iw.i64(*size as i64);
            iw.u32(*f);
            iw.cstr(path);
        }
        let info = iw.buf;
        let info_c = lz4_flex::block::compress(&info);
        let flags = (self.flags & !(COMPRESSION_MASK | BLOCKS_INFO_AT_END)) | BLOCKS_AND_DIR_COMBINED | 2;

        let mut w = Writer::new(true);
        w.cstr("UnityFS");
        w.u32(self.format);
        w.cstr(&self.version_player);
        w.cstr(&self.version_engine);
        let size_pos = w.buf.len();
        w.i64(0);
        w.u32(info_c.len() as u32);
        w.u32(info.len() as u32);
        w.u32(flags);
        if self.format >= 7 {
            w.align(16);
        }
        w.bytes(&info_c);
        if flags & PADDING_AT_START != 0 {
            w.align(16);
        }
        for b in &blocks {
            w.bytes(b);
        }
        let total = w.buf.len() as i64;
        w.buf[size_pos..size_pos + 8].copy_from_slice(&total.to_be_bytes());
        w.buf
    }
}
