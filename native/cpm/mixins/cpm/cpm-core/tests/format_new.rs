//! New-format (CUBES / TEXTURE / ROOT_INFO / SCALE / ANIMATION_NEW) part decoding.
//! The `W` writer mirrors `IOHelper.write*` and `Cube.saveDefinitionCubeV2`.

use cpm_core::format::read_definition;
use cpm_core::io::DIV;

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, b: u8) -> &mut Self {
        self.0.push(b);
        self
    }
    fn varint(&mut self, mut v: u32) -> &mut Self {
        while v & !0x7F != 0 {
            self.0.push((v & 0x7F) as u8 | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
        self
    }
    fn svarint(&mut self, v: i32) -> &mut Self {
        let sign = if v < 0 { 0x40 } else { 0 };
        let mut t = v.unsigned_abs();
        let mut b = (t & 0x3F) as u8 | sign;
        t >>= 6;
        while t != 0 {
            self.0.push(b | 0x80);
            b = (t & 0x7F) as u8;
            t >>= 7;
        }
        self.0.push(b);
        self
    }
    fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn float2(&mut self, f: f32) -> &mut Self {
        self.i16((f * DIV as f32) as i32 as i16)
    }
    fn var_vec3(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.svarint((f * DIV as f32) as i32);
        }
        self
    }
    fn angle(&mut self, v: [f32; 3]) -> &mut Self {
        for f in v {
            self.i16(((f / 360.0 * 65535.0) as i32).clamp(0, 65535) as u16 as i16);
        }
        self
    }
    fn block(&mut self, inner: &[u8]) -> &mut Self {
        self.varint(inner.len() as u32);
        self.0.extend_from_slice(inner);
        self
    }
}

/// Fields of `Cube` that `saveDefinitionCubeV2` writes.
struct C {
    flags: u8,
    parent: u32,
    pos: [f32; 3],
    rot: [f32; 3],
    size: [f32; 3],
    offset: [f32; 3],
    tex_size: u8,
    u: u32,
    v: u32,
    rgb: u32,
    mc_scale: f32,
}

const HAS_MESH: u8 = 1;
const HAS_TEXTURE: u8 = 2;
const HIDDEN: u8 = 4;
const UV_SCALED: u8 = 16;
const MC_SCALED: u8 = 32;

fn save_cube(w: &mut W, c: &C) {
    w.u8(c.flags).varint(c.parent).var_vec3(c.pos).angle(c.rot);
    if c.flags & HAS_MESH != 0 {
        w.var_vec3(c.size).var_vec3(c.offset);
        if c.flags & HAS_TEXTURE != 0 {
            if c.flags & UV_SCALED != 0 {
                w.u8(c.tex_size);
            }
            w.varint(c.u).varint(c.v);
        } else {
            w.u8((c.rgb >> 16) as u8).u8((c.rgb >> 8) as u8).u8(c.rgb as u8);
        }
        if c.flags & MC_SCALED != 0 {
            w.float2(c.mc_scale);
        }
    }
}

fn base() -> C {
    C {
        flags: HAS_MESH,
        parent: 0,
        pos: [0.0; 3],
        rot: [0.0; 3],
        size: [4.0, 4.0, 4.0],
        offset: [0.0; 3],
        tex_size: 1,
        u: 0,
        v: 0,
        rgb: 0,
        mc_scale: 0.0,
    }
}

/// Wraps parts as `[type, block]...` into a complete definition with END and checksum.
fn definition(parts: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut w = W::default();
    for (ty, body) in parts {
        w.u8(*ty).block(body);
    }
    w.u8(0).block(&[]);
    cpm_core::write::checksummed(0x53, &w.0)
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 0.05)
}

const CUBES: u8 = 21;
const TEXTURE: u8 = 17;
const ROOT_INFO: u8 = 22;
const SCALE: u8 = 16;
const ANIMATION_NEW: u8 = 23;

fn cubes_part(cubes: &[C]) -> (u8, Vec<u8>) {
    let mut w = W::default();
    w.varint(cubes.len() as u32);
    for c in cubes {
        save_cube(&mut w, c);
    }
    (CUBES, w.0)
}

#[test]
fn cubes_v2_roundtrip() {
    let a = C {
        flags: HAS_MESH | HAS_TEXTURE | UV_SCALED,
        parent: 0,
        pos: [1.0, 2.0, -3.0],
        rot: [90.0, 45.0, 10.0],
        size: [8.0, 4.0, 2.0],
        offset: [-4.0, -8.0, -1.0],
        tex_size: 2,
        u: 20,
        v: 300,
        ..base()
    };
    let b = C { flags: HAS_MESH | MC_SCALED, parent: 10, rgb: 0x112233, mc_scale: 0.5, ..base() };
    let m = read_definition(&definition(&[cubes_part(&[a, b])])).unwrap();
    assert_eq!(m.cubes.len(), 2);
    let (a, b) = (&m.cubes[0], &m.cubes[1]);
    assert_eq!((a.id, b.id), (10, 11));
    assert_eq!(a.parent, 0);
    assert!(close(a.pos, [1.0, 2.0, -3.0]));
    assert!(close(a.rotation, [90.0, 45.0, 10.0]));
    assert!(close(a.size, [8.0, 4.0, 2.0]));
    assert!(close(a.offset, [-4.0, -8.0, -1.0]));
    assert_eq!((a.tex_size, a.u, a.v), (2, 20, 300));
    assert_eq!(a.mc_scale, 0.0);
    assert_eq!(a.mesh_scale, [1.0; 3]);
    assert_eq!(a.scale, [1.0; 3]);
    assert!(!a.hidden);
    assert_eq!(b.parent, 10);
    assert_eq!(b.tex_size, 0);
    assert_eq!(b.rgb, 0xFF112233);
    assert!((b.mc_scale - 0.5).abs() < 0.05);
}

#[test]
fn cubes_v2_hidden_flag() {
    let h = C { flags: HAS_MESH | HAS_TEXTURE | HIDDEN, ..base() };
    let m = read_definition(&definition(&[cubes_part(&[h])])).unwrap();
    assert!(m.cubes[0].hidden);
    assert_eq!(m.cubes[0].tex_size, 1);
}

#[test]
fn cubes_v2_no_mesh_mesh_scale_and_scale() {
    // Group cube: no mesh, MESH_SCALED (8) and SCALED (64) trailing vectors.
    let mut w = W::default();
    w.varint(1);
    w.u8(8 | 64).varint(3).var_vec3([0.0; 3]).angle([0.0; 3]);
    w.var_vec3([2.0, 2.0, 2.0]).var_vec3([0.5, 1.0, 1.5]);
    let m = read_definition(&definition(&[(CUBES, w.0)])).unwrap();
    let c = &m.cubes[0];
    assert_eq!(c.size, [0.0; 3]);
    assert!(close(c.mesh_scale, [2.0, 2.0, 2.0]));
    assert!(close(c.scale, [0.5, 1.0, 1.5]));
    assert_eq!(c.parent, 3);
}

fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, 2, 2);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&[255u8; 16]).unwrap();
    }
    out
}

fn texture_part(kind: u8, size: [i16; 2], png: &[u8]) -> (u8, Vec<u8>) {
    let mut w = W::default();
    w.u8(kind).i16(size[0]).i16(size[1]).block(png);
    (TEXTURE, w.0)
}

#[test]
fn texture_part_skin() {
    let png = tiny_png();
    let m = read_definition(&definition(&[texture_part(0, [64, 64], &png)])).unwrap();
    let t = m.texture.unwrap();
    assert_eq!(t.declared, [64, 64]);
    assert_eq!(t.png, png);
}

#[test]
fn texture_part_other_sheets_ignored() {
    let png = tiny_png();
    // LIST_ICON = 1, CAPE = 2 must not replace the SKIN texture.
    let parts = [
        texture_part(0, [64, 64], &png),
        texture_part(1, [8, 8], b"junk"),
        texture_part(2, [64, 32], b"junk"),
    ];
    let m = read_definition(&definition(&parts)).unwrap();
    let t = m.texture.unwrap();
    assert_eq!(t.declared, [64, 64]);
    assert_eq!(t.png, png);
}

#[test]
fn root_info_partpos() {
    // ROOT_TRANSFORM (8); part RIGHT_ARM = 3
    let mut w = W::default();
    w.u8(8).u8(3).var_vec3([1.0, -2.0, 0.5]).angle([0.0, 90.0, 0.0]);
    // HEAD, no transform
    let mut w2 = W::default();
    w2.u8(0).u8(0);
    // ROOT_MODEL (4) root: not a vanilla part, ignored
    let mut w3 = W::default();
    w3.u8(4 | 8).u8(0).var_vec3([5.0, 5.0, 5.0]).angle([0.0; 3]);
    let m = read_definition(&definition(&[(ROOT_INFO, w.0), (ROOT_INFO, w2.0), (ROOT_INFO, w3.0)])).unwrap();
    assert_eq!(m.part_pos.len(), 2);
    assert_eq!(m.part_pos[0].part, 3);
    assert!(close(m.part_pos[0].pos, [1.0, -2.0, 0.5]));
    assert!(close(m.part_pos[0].rot, [0.0, 90.0, 0.0]));
    assert_eq!(m.part_pos[1].part, 0);
    assert_eq!(m.part_pos[1].pos, [0.0; 3]);
}

#[test]
fn scale_and_new_anim_raw() {
    let m = read_definition(&definition(&[(SCALE, vec![(-5i8) as u8]), (ANIMATION_NEW, vec![1, 2, 3, 4])])).unwrap();
    assert_eq!(m.scale, Some(-0.5));
    assert_eq!(m.new_anim, Some(vec![1, 2, 3, 4]));
}

#[test]
fn rejects_unknown_part_gracefully() {
    let h = C { flags: HAS_MESH | HAS_TEXTURE, ..base() };
    let m = read_definition(&definition(&[(200, vec![9, 9, 9]), cubes_part(&[h])])).unwrap();
    assert_eq!(m.cubes.len(), 1);
}

#[test]
fn no_player_part_keeps_all_vanilla() {
    let m = read_definition(&definition(&[])).unwrap();
    assert_eq!(m.keep_vanilla, [true; 6]);
    assert_eq!(m.vanilla_anim_disabled, [false; 6]);
}

#[test]
fn root_info_hidden_clears_keep_vanilla() {
    // ROOT_HIDDEN (1) on LEFT_ARM (2)
    let m = read_definition(&definition(&[(ROOT_INFO, vec![1, 2])])).unwrap();
    assert_eq!(m.keep_vanilla, [true, true, false, true, true, true]);
    assert_eq!(m.vanilla_anim_disabled, [false; 6]);
}

#[test]
fn root_info_disable_vanilla_sets_anim_disabled() {
    // ROOT_DISABLE_VANILLA (16) on BODY (1)
    let m = read_definition(&definition(&[(ROOT_INFO, vec![16, 1])])).unwrap();
    assert_eq!(m.vanilla_anim_disabled, [false, true, false, false, false, false]);
    assert_eq!(m.keep_vanilla, [true; 6]);
}

#[test]
fn cube_v2_literal_bytes() {
    // flags HAS_MESH|HAS_TEXTURE|UV_SCALED = 0x13, parent 5, pos (0,0,0), rot (0,0,0),
    // size (1,0,0): svarint(1*DIV=682) = 0xAA 0x0A, then 0 0; offset 0 0 0; texSize 2, u 3, v 4.
    // 682 = 0b10_1010_1010: first byte 6 bits 0x2A|0x80 = 0xAA, then 682>>6 = 10 = 0x0A.
    let bytes: Vec<u8> = vec![
        1, // count
        0x13, 5, // flags, parent
        0, 0, 0, // pos
        0, 0, 0, 0, 0, 0, // rot
        0xAA, 0x0A, 0, 0, // size
        0, 0, 0, // offset
        2, 3, 4, // texSize, u, v
    ];
    let m = read_definition(&definition(&[(CUBES, bytes)])).unwrap();
    let c = &m.cubes[0];
    assert_eq!((c.id, c.parent, c.tex_size, c.u, c.v), (10, 5, 2, 3, 4));
    assert!(close(c.size, [1.0, 0.0, 0.0]));
    assert_eq!(c.offset, [0.0; 3]);
    assert!(!c.hidden);
}
