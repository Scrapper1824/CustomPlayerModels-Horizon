mod common;

use cpm_core::format::{read_cpmmodel, read_definition, SkinType};
use cpm_core::io::{CpmError, Reader};

fn reggie() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/ReggieTheRat.cpmmodel")).unwrap()
}
fn skulldog() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/SkullDog.cpmmodel")).unwrap()
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 0.002)
}

#[test]
fn reggie_header() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap();
    assert_eq!(m.name, "Reggie The Rat");
    assert_eq!(m.desc, "Made by Hachiro");
    assert!(!m.icon_png.is_empty());
}

#[test]
fn reggie_cubes() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap().data;
    assert_eq!(m.cubes.len(), 26);
    let c = m.cubes.iter().find(|c| c.id == 10).unwrap();
    assert_eq!(c.size, [8.0, 8.0, 8.0]);
    assert!(close(c.offset, [-4.0, -8.0, -4.0]));
    assert_eq!(c.parent, 0);
    assert_eq!((c.u, c.v), (0, 0));
    assert_eq!(c.tex_size, 1);
    assert_eq!(c.mesh_scale, [1.0; 3]);
    assert_eq!(c.scale, [1.0; 3]);
    assert!(!c.hidden);
    assert_eq!(c.mc_scale, 0.0);
    let c = m.cubes.iter().find(|c| c.id == 21).unwrap();
    assert_eq!(c.rgb, 0xFF000000);
    assert_eq!(c.tex_size, 0);
    assert_eq!(m.cubes.iter().find(|c| c.id == 23).unwrap().parent, 22);
}

#[test]
fn reggie_texture() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap().data;
    let t = m.texture.unwrap();
    assert_eq!(t.declared, [75, 64]);
    assert!(t.png.starts_with(b"\x89PNG"));
}

#[test]
fn reggie_part_pos() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap().data;
    assert_eq!(m.part_pos.len(), 3);
    assert_eq!(m.part_pos[0].part, 0);
    assert!(close(m.part_pos[0].pos, [0.0, -1.5, 0.0]));
    assert_eq!(m.part_pos[1].part, 2);
    assert!(close(m.part_pos[1].pos, [-0.5, 0.0, 0.0]));
}

#[test]
fn reggie_keep_mask() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap().data;
    assert_eq!(m.keep_vanilla, [false; 6]);
    // SkinType.java: SLIM = 0, DEFAULT = 1.
    assert_eq!(m.skin_type, SkinType::Default);
    assert_eq!(read_cpmmodel(&skulldog()).unwrap().data.skin_type, SkinType::Default);
}

#[test]
fn reggie_effects_and_anim() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&reggie()).unwrap().data;
    assert_eq!(m.effects.len(), 10);
    assert_eq!(m.effects[0].kind, 0x01);
    assert_eq!(m.legacy_anim.as_ref().unwrap().len(), 142);
    assert!(m.new_anim.is_none());
    assert!(m.scale.is_none());
}

#[test]
fn skulldog_counts() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let m = read_cpmmodel(&skulldog()).unwrap().data;
    assert_eq!(m.cubes.len(), 53);
    assert_eq!(m.texture.unwrap().declared, [256, 256]);
    assert_eq!(m.part_pos.len(), 6);
}

fn data_range(file: &[u8]) -> (usize, usize) {
    let mut r = Reader::new(&file[1..]);
    r.utf().unwrap();
    r.utf().unwrap();
    let len = r.varint().unwrap() as usize;
    (1 + r.pos(), len)
}

#[test]
fn bad_checksum_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let mut f = reggie();
    let (start, len) = data_range(&f);
    // Flip a byte in the middle of the data block (inside the embedded PNG).
    f[start + len / 2] ^= 0x01;
    let data = &f[start..start + len];
    assert!(matches!(read_definition(data), Err(CpmError::Checksum { .. })));
    assert!(matches!(read_cpmmodel(&f), Err(CpmError::Checksum { .. })));
    // Intact data still decodes.
    assert!(read_definition(&reggie()[start..start + len]).is_ok());
}

#[test]
fn truncated_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let f = reggie();
    assert!(read_cpmmodel(&f[..2000]).is_err());
}

#[test]
fn bad_header_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let mut f = reggie();
    f[0] = 0x54;
    assert_eq!(read_cpmmodel(&f).unwrap_err(), CpmError::BadHeader);
}

/// Legacy `MODEL_ROOT` (part 12, `ModelPartRoot`): the `CUSTOM_PART` child with that id
/// becomes a root of the given `RootModelType` (here the cape, ordinal 0).
#[test]
fn legacy_model_root_creates_a_root() {
    use cpm_core::write::{checksummed, Writer};
    let mut root = Writer::new();
    root.varint(10).u8(0);
    let mut def = Writer::new();
    def.varint(0).object_block(12, &root).object_block(0, &Writer::new());
    let mut body = Writer::new();
    body.object_block(3, &def).object_block(0, &Writer::new());
    let m = read_definition(&checksummed(0x53, &body.into_bytes())).unwrap();
    assert_eq!(m.extra_roots.len(), 1);
    assert_eq!(m.extra_roots[0].root, 0);
    assert_eq!(m.extra_roots[0].create_from, 10);
}

/// `ModelPartRoot.apply`: the root takes its fake cube's hidden state once the effects (the
/// legacy `HIDE`) are applied, so an empty cape or elytra root hides the vanilla piece.
#[test]
fn legacy_root_is_hidden_by_its_fake_cubes_hide_effect() {
    use cpm_core::format::{RawCube, RawEffect};
    use cpm_core::model::{Model, CUSTOM_PART};
    use cpm_core::write::{checksummed, Writer};
    let mut root = Writer::new();
    root.varint(10).u8(0); // cube 10 becomes the cape root
    let mut def = Writer::new();
    def.varint(0).object_block(12, &root).object_block(0, &Writer::new());
    let mut body = Writer::new();
    body.object_block(3, &def).object_block(0, &Writer::new());
    let mut raw = read_definition(&checksummed(0x53, &body.into_bytes())).unwrap();
    raw.cubes = vec![RawCube {
        id: 10, parent: CUSTOM_PART, size: [0.0; 3], pos: [0.0; 3], offset: [0.0; 3], rotation: [0.0; 3],
        tex_size: 1, u: 0, v: 0, rgb: 0, mesh_scale: [1.0; 3], scale: [1.0; 3], mc_scale: 0.0, hidden: false,
    }];
    let shown = Model::build(raw.clone()).unwrap();
    assert!(!shown.extra_roots[0].hidden);
    raw.effects = vec![RawEffect { kind: 2, payload: vec![10] }];
    let hidden = Model::build(raw).unwrap();
    assert!(hidden.extra_roots[0].hidden, "HIDE on the fake cube hides the root");
}

/// `EXTRUDE` (`BoxRender.createTexturedExtruded`): a front and a back face over the texture, then
/// a slice per texel column on each side and per texel row on each side, each one texel line.
#[test]
fn extruded_cube_is_sliced_per_texel() {
    use cpm_core::anim::engine::{Frame, PoseInput};
    use cpm_core::format::{RawCube, RawEffect};
    use cpm_core::mesh::{pose_model, BoxSource};
    use cpm_core::model::Model;
    use cpm_core::write::{checksummed, Writer};
    let mut def = Writer::new();
    def.varint(0).object_block(0, &Writer::new());
    let mut body = Writer::new();
    body.object_block(3, &def).object_block(0, &Writer::new());
    let mut raw = read_definition(&checksummed(0x53, &body.into_bytes())).unwrap();
    raw.cubes = vec![RawCube {
        id: 10, parent: 1, size: [3.0, 2.0, 1.0], pos: [0.0; 3], offset: [0.0; 3], rotation: [0.0; 3],
        tex_size: 1, u: 4, v: 8, rgb: 0, mesh_scale: [1.0; 3], scale: [1.0; 3], mc_scale: 0.0, hidden: false,
    }];
    let quads = |raw: cpm_core::format::RawModel| {
        let m = Model::build(raw).unwrap();
        let boxes = pose_model(&m, &Frame::default(), &PoseInput::default());
        boxes.into_iter().filter(|b| b.source == BoxSource::Cube(10)).flat_map(|b| b.faces.into_iter().flatten()).collect::<Vec<_>>()
    };
    assert_eq!(quads(raw.clone()).len(), 6, "a plain box");
    raw.effects = vec![RawEffect { kind: 10, payload: vec![10] }];
    let q = quads(raw);
    assert_eq!(q.len(), 2 + 2 * 3 + 2 * 2, "front, back, 3 columns and 2 rows each side");
    // The front face shows the whole 3x2 texture at (4, 8), across the box's front (z 1) face.
    let front = &q[0];
    let (mut us, mut vs) = ([front.uv[0], front.uv[2]], [front.uv[1], front.uv[3]]);
    us.sort_by(f32::total_cmp);
    vs.sort_by(f32::total_cmp);
    assert_eq!((us, vs), ([4.0, 7.0], [8.0, 10.0]));
    assert!(front.corners.iter().all(|c| (c[2] - 1.0).abs() < 1e-5), "{:?}", front.corners);
    // Every slice samples a single texel line: zero width in u or in v.
    assert!(q[2..].iter().all(|f| f.uv[0] == f.uv[2] || f.uv[1] == f.uv[3]));
}
