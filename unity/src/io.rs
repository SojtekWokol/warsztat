//! Byte reader/writer with switchable endianness and Unity's alignment rules.
use anyhow::{bail, Result};

pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub big: bool,
}

macro_rules! rd {
    ($name:ident, $t:ty, $n:expr) => {
        pub fn $name(&mut self) -> Result<$t> {
            let b = self.take($n)?;
            let a: [u8; $n] = b.try_into().unwrap();
            Ok(if self.big { <$t>::from_be_bytes(a) } else { <$t>::from_le_bytes(a) })
        }
    };
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], big: bool) -> Self {
        Reader { data, pos: 0, big }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            bail!("read past end ({} + {} > {})", self.pos, n, self.data.len());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    rd!(u16, u16, 2);
    rd!(i16, i16, 2);
    rd!(u32, u32, 4);
    rd!(i32, i32, 4);
    rd!(u64, u64, 8);
    rd!(i64, i64, 8);
    rd!(f32, f32, 4);
    rd!(f64, f64, 8);
    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let n = rest.iter().position(|&b| b == 0).ok_or_else(|| anyhow::anyhow!("unterminated string"))?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.pos += n + 1;
        Ok(s)
    }
    /// Align relative to the start of `data`.
    pub fn align(&mut self, n: usize) {
        self.pos = (self.pos + n - 1) / n * n;
    }
}

pub struct Writer {
    pub buf: Vec<u8>,
    pub big: bool,
}

macro_rules! wr {
    ($name:ident, $t:ty) => {
        pub fn $name(&mut self, v: $t) {
            if self.big {
                self.buf.extend_from_slice(&v.to_be_bytes())
            } else {
                self.buf.extend_from_slice(&v.to_le_bytes())
            }
        }
    };
}

impl Writer {
    pub fn new(big: bool) -> Self {
        Writer { buf: Vec::new(), big }
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v)
    }
    wr!(u16, u16);
    wr!(i16, i16);
    wr!(u32, u32);
    wr!(i32, i32);
    wr!(u64, u64);
    wr!(i64, i64);
    wr!(f32, f32);
    wr!(f64, f64);
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b)
    }
    pub fn cstr(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
    }
    pub fn align(&mut self, n: usize) {
        while self.buf.len() % n != 0 {
            self.buf.push(0)
        }
    }
}
