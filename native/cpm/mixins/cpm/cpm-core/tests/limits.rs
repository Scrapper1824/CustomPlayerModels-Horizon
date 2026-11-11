// Safety limits checked before a model's textures are decoded: CPM's MAX_TEX_SHEET_SIZE on the
// PNG header (`TextureProvider`) and MAX_CUBE_COUNT with extruded cubes counted as
// `BoxRender.getExtrudeSize` (`ModelDefinition.validate`), and the decoder's own backstop.
use cpm_core::mesh::{Limits, MAX_TEXTURE_PX};
use cpm_core::write::{Writer, checksummed};

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A PNG of only a signature and an IHDR claiming `w`x`h` RGBA (then an empty IDAT and IEND).
fn header_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend(data);
        out.extend(&c);
        out.extend(crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[]);
    chunk(b"IEND", &[]);
    out
}

/// A model file with a texture part (`declared` size, `png`) and `cubes` head cubes of `size`
/// (extruded when `extrude`, texture scale 1).
fn model(declared: [u16; 2], png: &[u8], cubes: u32, size: f32, extrude: bool) -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut tex = Writer::new();
    let mut block = Writer::new();
    block.bytes(png);
    tex.i16(declared[0] as i16).i16(declared[1] as i16).block(&block);
    let mut c = Writer::new();
    c.varint(cubes);
    for _ in 0..cubes {
        c.u8(0x03).varint(0);
        c.var_vec3([0.0; 3]).angle([0.0; 3]);
        c.var_vec3([size; 3]).var_vec3([-2.0; 3]).varint(0).varint(0);
    }
    let mut body = Writer::new();
    body.object_block(11, &skin_type).object_block(5, &tex).object_block(21, &c);
    if extrude {
        // RENDER_EFFECT: EXTRUDE on each cube (id 10 + index).
        for i in 0..cubes {
            let mut e = Writer::new();
            e.u8(EXTRUDE_EFFECT).varint(10 + i);
            body.object_block(RENDER_EFFECT, &e);
        }
    }
    body.object_block(0, &Writer::new());
    let data = checksummed(0x53, &body.into_bytes());
    cpm_core::skin::container(&data)
}

/// `ModelPartType.RENDER_EFFECT`, and `RenderEffects.EXTRUDE`'s id.
const RENDER_EFFECT: u8 = 8;
const EXTRUDE_EFFECT: u8 = 10;

fn small_png() -> Vec<u8> {
    let mut img = cpm_core::skin::Image::new(64, 64);
    img.argb.fill(0xFF00_FF00);
    img.encode_png()
}

#[test]
fn huge_header_refused_before_decoding() {
    // Claims 16000 x 2,000,000,000: refused from the header, nothing allocated (an attempt aborts).
    let file = cpm_core::format::read_cpmmodel(&model([64, 64], &header_png(16_000, 2_000_000_000), 1, 4.0, false)).unwrap();
    let err = cpm_core::LoadedModel::from_file(file).err().expect("refused").to_string();
    assert!(err.contains("16000x2000000000") && err.contains("too big"), "{err}");
    // The image decoder's own backstop, whatever loads it.
    assert!(cpm_core::skin::Image::decode(&header_png(MAX_TEXTURE_PX + 1, 1)).is_err());
    // Another player's limit: from the header too.
    let file = cpm_core::format::read_cpmmodel(&model([64, 64], &header_png(512, 512), 1, 4.0, false)).unwrap();
    let err = cpm_core::LoadedModel::from_file_limited(file, &Limits { max_texture_px: 256, max_cubes: 256 })
        .err()
        .expect("refused")
        .to_string();
    assert!(err.contains("texture 512x512 is too big (at most 256x256)"), "{err}");
    // A declared size past the limit with a small image: loads, as in CPM (`TextureProvider`
    // checks only the image block's own size).
    let file = cpm_core::format::read_cpmmodel(&model([512, 64], &small_png(), 1, 4.0, false)).unwrap();
    let loaded = cpm_core::LoadedModel::from_file_limited(file, &Limits { max_texture_px: 256, max_cubes: 256 }).expect("loads");
    assert_eq!((loaded.texture_px, loaded.png_px), ([512, 64], [64, 64]));
    // Within it: loads.
    let file = cpm_core::format::read_cpmmodel(&model([64, 64], &small_png(), 1, 4.0, false)).unwrap();
    assert!(cpm_core::LoadedModel::from_file_limited(file, &Limits { max_texture_px: 256, max_cubes: 256 }).is_ok());
}

#[test]
fn extruded_cubes_count_toward_the_cap() {
    // BoxRender.getExtrudeSize: ceil(w * ts) * ceil(h * ts) * 3 / 4 more per extruded cube.
    let file = cpm_core::format::read_cpmmodel(&model([64, 64], &small_png(), 2, 16.0, true)).unwrap();
    let built = cpm_core::model::Model::build(file.data.clone()).unwrap();
    assert!(built.cubes.iter().all(|c| c.extrude));
    assert_eq!(built.cubes[0].tex_scale, 1);
    assert_eq!(built.cube_count(), 2 + 2 * (16 * 16 * 3) / 4);
    let err = cpm_core::LoadedModel::from_file_limited(file, &Limits { max_texture_px: 256, max_cubes: 256 })
        .err()
        .expect("refused")
        .to_string();
    assert!(err.contains("cubes"), "{err}");
    // Not extruded: 2 cubes.
    let file = cpm_core::format::read_cpmmodel(&model([64, 64], &small_png(), 2, 16.0, false)).unwrap();
    assert!(cpm_core::LoadedModel::from_file_limited(file, &Limits { max_texture_px: 256, max_cubes: 256 }).is_ok());
}
