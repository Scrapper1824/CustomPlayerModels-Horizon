//! Binary reader: a port of the read side of CPM's `IOHelper`.

use std::fmt;

/// Fixed-point divisor used by CPM for positions (`Short.MAX_VALUE / Vec3f.MAX_POS`).
pub const DIV: i32 = 32767 / 48;
/// Largest block a [`Reader::block`] call accepts (16 MiB).
pub const MAX_BLOCK_SIZE: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpmError {
    Eof,
    BadHeader,
    Checksum { expected: u16, actual: u16 },
    BlockTooBig(u32),
    Invalid(String),
    Png(String),
    /// Past a safety limit (a texture's size, the cube count): the model is refused as it is.
    Limit(String),
    /// The model is a link model; the string is its link (`git:<user>/<id>`).
    NeedsLink(String),
}

impl fmt::Display for CpmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpmError::Eof => write!(f, "unexpected end of data"),
            CpmError::BadHeader => write!(f, "bad file header"),
            CpmError::Checksum { expected, actual } => {
                write!(f, "checksum mismatch: expected {expected:#06x}, got {actual:#06x}")
            }
            CpmError::BlockTooBig(n) => write!(f, "block too big: {n} bytes"),
            CpmError::Invalid(s) => write!(f, "invalid data: {s}"),
            CpmError::Png(s) => write!(f, "png error: {s}"),
            CpmError::Limit(s) => write!(f, "{s}"),
            CpmError::NeedsLink(l) => write!(f, "model is a link to {l}"),
        }
    }
}

impl std::error::Error for CpmError {}

pub type Result<T> = std::result::Result<T, CpmError>;

#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or(CpmError::Eof)?;
        self.pos += 1;
        Ok(b)
    }

    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    /// Big-endian signed 16-bit.
    pub fn i16(&mut self) -> Result<i16> {
        let b = self.bytes(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            return Err(CpmError::Eof);
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Unsigned LEB128-style varint, at most 5 bytes.
    pub fn varint(&mut self) -> Result<u32> {
        let mut v: u32 = 0;
        for j in 0..5 {
            let b = self.u8()?;
            v |= ((b & 0x7F) as u32).wrapping_shl(7 * j);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(CpmError::Invalid("VarInt too big".into()))
    }

    /// CPM signed varint: first byte holds 6 value bits and a sign bit (0x40).
    pub fn svarint(&mut self) -> Result<i32> {
        let b0 = self.u8()?;
        let sign: i32 = if b0 & 0x40 != 0 { -1 } else { 1 };
        let mut v: i32 = (b0 & 0x3F) as i32;
        let mut b = b0;
        let mut j: u32 = 6;
        while b & 0x80 != 0 {
            b = self.u8()?;
            v |= (((b & 0x7F) as i32).wrapping_shl(j)) as i32;
            j += 7;
            if j > 34 {
                return Err(CpmError::Invalid("SignedVarInt too big".into()));
            }
        }
        Ok(v.wrapping_mul(sign))
    }

    /// Varint length followed by UTF-8 bytes.
    pub fn utf(&mut self) -> Result<String> {
        let n = self.varint()? as usize;
        let b = self.bytes(n)?;
        String::from_utf8(b.to_vec()).map_err(|e| CpmError::Invalid(format!("utf8: {e}")))
    }

    /// Varint length followed by that many bytes, as a sub-reader.
    pub fn block(&mut self) -> Result<Reader<'a>> {
        let n = self.varint()?;
        if n > MAX_BLOCK_SIZE {
            return Err(CpmError::BlockTooBig(n));
        }
        Ok(Reader::new(self.bytes(n as usize)?))
    }

    pub fn vec3ub(&mut self) -> Result<[f32; 3]> {
        Ok([
            self.u8()? as f32 / 10.0,
            self.u8()? as f32 / 10.0,
            self.u8()? as f32 / 10.0,
        ])
    }

    pub fn vec3b(&mut self) -> Result<[f32; 3]> {
        Ok([
            self.i8()? as f32 / 10.0,
            self.i8()? as f32 / 10.0,
            self.i8()? as f32 / 10.0,
        ])
    }

    pub fn float2(&mut self) -> Result<f32> {
        Ok(self.i16()? as f32 / DIV as f32)
    }

    pub fn vec6b(&mut self) -> Result<[f32; 3]> {
        Ok([self.float2()?, self.float2()?, self.float2()?])
    }

    pub fn var_float(&mut self) -> Result<f32> {
        Ok(self.svarint()? as f32 / DIV as f32)
    }

    pub fn var_vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.var_float()?, self.var_float()?, self.var_float()?])
    }

    /// Three 16-bit angles, returned in degrees.
    pub fn angle(&mut self) -> Result<[f32; 3]> {
        let mut out = [0.0; 3];
        for o in &mut out {
            *o = self.i16()? as f32 / 65535.0 * 360.0;
        }
        Ok(out)
    }
}
