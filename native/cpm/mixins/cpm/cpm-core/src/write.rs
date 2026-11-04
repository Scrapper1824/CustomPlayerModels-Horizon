//! Binary writer: a port of the write side of CPM's `IOHelper`, the inverse of [`crate::io::Reader`].

use crate::io::DIV;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }

    /// Big-endian signed 16-bit.
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }

    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }

    /// `writeVarInt`: 7 bits per byte, low first, high bit = continuation.
    pub fn varint(&mut self, mut v: u32) -> &mut Self {
        while v & !0x7F != 0 {
            self.buf.push((v & 0x7F) as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
        self
    }

    /// `writeSignedVarInt`: first byte holds 6 value bits and the sign bit (0x40).
    pub fn svarint(&mut self, v: i32) -> &mut Self {
        let sign = if v < 0 { 0x40 } else { 0 };
        let mut t = v.unsigned_abs();
        let mut b = (t & 0x3F) as u8 | sign;
        t >>= 6;
        while t != 0 {
            self.buf.push(b | 0x80);
            b = (t & 0x7F) as u8;
            t >>= 7;
        }
        self.buf.push(b);
        self
    }

    /// Varint length followed by UTF-8 bytes.
    pub fn utf(&mut self, s: &str) -> &mut Self {
        self.varint(s.len() as u32);
        self.bytes(s.as_bytes())
    }

    /// `writeVec3ub`: each component `clamp((int)(x*10), 0, 255)`.
    pub fn vec3ub(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.u8(((f * 10.0) as i32).clamp(0, 255) as u8);
        }
        self
    }

    /// `writeVec3b`: each component `clamp((int)(x*10), -128, 127)`.
    pub fn vec3b(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.i8(((f * 10.0) as i32).clamp(-128, 127) as i8);
        }
        self
    }

    /// `writeFloat2`: `clamp((int)(f*DIV), i16 range)` as a short.
    pub fn float2(&mut self, f: f32) -> &mut Self {
        self.i16(((f * DIV as f32) as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    }

    pub fn vec6b(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.float2(f);
        }
        self
    }

    /// `writeVarFloat`: `writeSignedVarInt((int)(f*DIV))`.
    pub fn var_float(&mut self, f: f32) -> &mut Self {
        self.svarint((f * DIV as f32) as i32)
    }

    pub fn var_vec3(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.var_float(f);
        }
        self
    }

    /// `writeAngle` for degrees: normalised into 0..360 first (as `saveDefinitionCube` and
    /// `ActionBuilder.limitVec` do), then `clamp((int)(x/360*65535), 0, 65535)` as a short.
    /// `Reader::angle` reads it back as a signed 16-bit, so angles past 180 come back as the
    /// equivalent negative angle.
    pub fn angle(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            let n = f.rem_euclid(360.0);
            self.i16(((n / 360.0 * 65535.0) as i32).clamp(0, 65535) as u16 as i16);
        }
        self
    }

    /// Varint length followed by the other writer's bytes.
    pub fn block(&mut self, inner: &Writer) -> &mut Self {
        self.varint(inner.buf.len() as u32);
        self.bytes(&inner.buf)
    }

    /// `writeNextObjectBlock`: type byte, then a length-prefixed block.
    pub fn object_block(&mut self, kind: u8, inner: &Writer) -> &mut Self {
        self.u8(kind);
        self.block(inner)
    }
}

/// `header` + `body` + big-endian wrapping `u16` sum of the body bytes.
pub fn checksummed(header: u8, body: &[u8]) -> Vec<u8> {
    let sum = body.iter().fold(0u16, |s, &b| s.wrapping_add(b as u16));
    let mut out = Vec::with_capacity(body.len() + 3);
    out.push(header);
    out.extend_from_slice(body);
    out.extend_from_slice(&sum.to_be_bytes());
    out
}
