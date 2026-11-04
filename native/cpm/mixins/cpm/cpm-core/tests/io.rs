use cpm_core::io::{CpmError, Reader};

#[test]
fn varint_multi_byte() {
    assert_eq!(Reader::new(&[0xBD, 0x20]).varint().unwrap(), 4157);
}

#[test]
fn svarint_negative() {
    assert_eq!(Reader::new(&[0x41]).svarint().unwrap(), -1);
    assert_eq!(Reader::new(&[0x7F]).svarint().unwrap(), -63);
}

#[test]
fn svarint_multi_byte() {
    assert_eq!(Reader::new(&[0x80 | 0x05, 0x01]).svarint().unwrap(), 69);
}

#[test]
fn vec6b_scale() {
    let mut b = Vec::new();
    for _ in 0..3 {
        b.extend_from_slice(&682i16.to_be_bytes());
    }
    assert_eq!(Reader::new(&b).vec6b().unwrap(), [1.0, 1.0, 1.0]);
}

#[test]
fn angle_quarter() {
    let b = 16384i16.to_be_bytes();
    let b3 = [b[0], b[1], b[0], b[1], b[0], b[1]];
    let a = Reader::new(&b3).angle().unwrap();
    for v in a {
        assert!((v - 90.0055).abs() < 0.01, "{v}");
    }
}

#[test]
fn utf_roundtrip() {
    assert_eq!(Reader::new(&[0x03, b'a', b'b', b'c']).utf().unwrap(), "abc");
}

#[test]
fn block_too_big_is_error() {
    // varint for 16 MiB + 1 = 0x100_0001
    let n: u32 = 16 * 1024 * 1024 + 1;
    let mut b = Vec::new();
    let mut v = n;
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            b.push(byte);
            break;
        }
        b.push(byte | 0x80);
    }
    assert!(matches!(
        Reader::new(&b).block(),
        Err(CpmError::BlockTooBig(x)) if x == n
    ));
}

#[test]
fn eof_is_error() {
    assert!(matches!(Reader::new(&[]).u8(), Err(CpmError::Eof)));
    assert!(matches!(Reader::new(&[0x80]).varint(), Err(CpmError::Eof)));
    assert!(matches!(Reader::new(&[1]).i16(), Err(CpmError::Eof)));
}

#[test]
fn misc_reads() {
    let mut r = Reader::new(&[10, 0xF6, 1, 1, 5]);
    assert_eq!(r.vec3ub().unwrap(), [1.0, 24.6, 0.1]);
    assert_eq!(r.remaining(), 2);
    assert_eq!(r.pos(), 3);
    assert!(r.bool().unwrap());
}
