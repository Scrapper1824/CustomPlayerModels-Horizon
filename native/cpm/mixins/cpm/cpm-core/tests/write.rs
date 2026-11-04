//! The binary writer round-trips through the reader and matches the literal bytes of IOHelper.

use cpm_core::format::read_definition;
use cpm_core::io::{Reader, DIV};
use cpm_core::write::{checksummed, Writer};

fn q() -> f32 {
    1.0 / DIV as f32
}

#[test]
fn roundtrip_primitives() {
    let mut w = Writer::new();
    w.u8(0).u8(255).i8(-128).i8(127).i16(-32768).i16(32767).i16(0).bool(true).bool(false);
    w.bytes(&[1, 2, 3]);
    for v in [0u32, 1, 127, 128, 4157, u32::MAX] {
        w.varint(v);
    }
    for v in [0i32, 1, -1, 63, -63, 64, 69, -69, 100_000, -100_000, i32::MAX, -i32::MAX] {
        w.svarint(v);
    }
    w.utf("").utf("h\u{e9}llo");
    w.vec3ub([0.0, 12.3, 25.5]).vec3ub([-5.0, 99.0, 25.6]);
    w.vec3b([-12.8, 0.0, 12.7]).vec3b([-99.0, 0.5, 99.0]);
    w.float2(0.0).float2(-1.5).float2(48.0).float2(-48.0).float2(1000.0).float2(-1000.0);
    w.vec6b([1.0, -2.0, 3.5]);
    w.var_float(0.0).var_float(-7.25).var_float(300.0);
    w.var_vec3([1.0, -2.0, 3.0]);
    w.angle([0.0, 90.0, 179.0]).angle([270.0, -90.0, 360.0 + 45.0]);
    let bytes = w.into_bytes();

    let mut r = Reader::new(&bytes);
    assert_eq!(r.u8().unwrap(), 0);
    assert_eq!(r.u8().unwrap(), 255);
    assert_eq!(r.i8().unwrap(), -128);
    assert_eq!(r.i8().unwrap(), 127);
    assert_eq!(r.i16().unwrap(), -32768);
    assert_eq!(r.i16().unwrap(), 32767);
    assert_eq!(r.i16().unwrap(), 0);
    assert!(r.bool().unwrap());
    assert!(!r.bool().unwrap());
    assert_eq!(r.bytes(3).unwrap(), &[1, 2, 3]);
    for v in [0u32, 1, 127, 128, 4157, u32::MAX] {
        assert_eq!(r.varint().unwrap(), v);
    }
    for v in [0i32, 1, -1, 63, -63, 64, 69, -69, 100_000, -100_000, i32::MAX, -i32::MAX] {
        assert_eq!(r.svarint().unwrap(), v);
    }
    assert_eq!(r.utf().unwrap(), "");
    assert_eq!(r.utf().unwrap(), "h\u{e9}llo");
    let close = |a: f32, b: f32, tol: f32| assert!((a - b).abs() <= tol, "{a} vs {b}");
    let close3 = |a: [f32; 3], b: [f32; 3], tol: f32| {
        for (x, y) in a.into_iter().zip(b) {
            close(x, y, tol);
        }
    };
    close3(r.vec3ub().unwrap(), [0.0, 12.3, 25.5], 0.1);
    close3(r.vec3ub().unwrap(), [0.0, 25.5, 25.5], 0.0);
    close3(r.vec3b().unwrap(), [-12.8, 0.0, 12.7], 0.1);
    close3(r.vec3b().unwrap(), [-12.8, 0.5, 12.7], 0.1);
    close(r.float2().unwrap(), 0.0, q());
    close(r.float2().unwrap(), -1.5, q());
    close(r.float2().unwrap(), 48.0, q());
    close(r.float2().unwrap(), -48.0, q());
    // clamped to the i16 range
    close(r.float2().unwrap(), 32767.0 / DIV as f32, 0.0);
    close(r.float2().unwrap(), -32768.0 / DIV as f32, 0.0);
    close3(r.vec6b().unwrap(), [1.0, -2.0, 3.5], q());
    for want in [0.0, -7.25, 300.0] {
        close(r.var_float().unwrap(), want, q());
    }
    close3(r.var_vec3().unwrap(), [1.0, -2.0, 3.0], q());
    let step = 360.0 / 65535.0;
    close3(r.angle().unwrap(), [0.0, 90.0, 179.0], step);
    // the reader returns signed angles: 270 -> -90, -90 -> 270 -> -90, 405 -> 45
    close3(r.angle().unwrap(), [-90.0, -90.0, 45.0], step * 2.0);
    assert_eq!(r.remaining(), 0);
}

#[test]
fn literal_varint_bytes() {
    let mut w = Writer::new();
    w.varint(4157).svarint(-1).svarint(69);
    assert_eq!(w.len(), 5);
    assert_eq!(w.into_bytes(), [0xBD, 0x20, 0x41, 0x85, 0x01]);
}

#[test]
fn blocks() {
    let mut inner = Writer::new();
    inner.u8(7).u8(8);
    let mut w = Writer::new();
    w.block(&inner).object_block(11, &inner);
    assert_eq!(w.into_bytes(), [2, 7, 8, 11, 2, 7, 8]);
}

#[test]
fn checksummed_matches_reader() {
    let mut body = Writer::new();
    let mut skin = Writer::new();
    skin.u8(1);
    body.object_block(11, &skin).object_block(0, &Writer::new());
    let data = checksummed(0x53, &body.into_bytes());
    assert!(read_definition(&data).is_ok());
    let mut bad = data.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(read_definition(&bad).is_err());
}
