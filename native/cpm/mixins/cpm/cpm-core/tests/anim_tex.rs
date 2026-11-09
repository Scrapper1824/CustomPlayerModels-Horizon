//! Animated textures (`ANIMATED_TEX`, `AnimatedTexture.update` / `copyTexture` / `copyTextureInt`).

use cpm_core::anim::texture::{AnimTex, AnimatedTexture};
use cpm_core::format::{read_definition, ModelFile};
use cpm_core::write::{checksummed, Writer};
use cpm_core::LoadedModel;

const TEXTURE: u8 = 17;
const ANIMATED_TEX: u8 = 18;

fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(rgba).unwrap();
    }
    out
}

fn anim_part(t: &AnimTex) -> Writer {
    let mut w = Writer::new();
    w.u8(t.sheet);
    for v in [t.uv_start, t.uv_size, t.anim_start] {
        w.varint(v[0] as u32).varint(v[1] as u32);
    }
    w.varint(t.frame_count as u32).varint(t.frame_time as u32);
    w.u8(t.an_x as u8 | (t.interpolate as u8) << 1);
    w
}

fn definition(parts: &[(u8, Writer)]) -> Vec<u8> {
    let mut w = Writer::new();
    for (k, p) in parts {
        w.object_block(*k, p);
    }
    w.u8(0).varint(0);
    checksummed(0x53, &w.into_bytes())
}

fn tex(sheet: u8, an_x: bool, interpolate: bool) -> AnimTex {
    AnimTex {
        sheet,
        uv_start: [0, 0],
        uv_size: [2, 2],
        anim_start: [0, 2],
        frame_count: 3,
        frame_time: 100,
        an_x,
        interpolate,
    }
}

/// A `w`x`h` image whose pixel (x, y) is `[x, y, x * 10 + y, 255]`.
fn coords(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[x as u8, y as u8, (x * 10 + y) as u8, 255]);
        }
    }
    v
}

fn px(rgba: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn anim_tex_parsed() {
    let t = AnimTex {
        sheet: 3,
        uv_start: [4, 300],
        uv_size: [8, 16],
        anim_start: [200, 0],
        frame_count: 7,
        frame_time: 250,
        an_x: true,
        interpolate: true,
    };
    let other = AnimTex { sheet: 0, an_x: false, interpolate: false, ..t };
    let m = read_definition(&definition(&[(ANIMATED_TEX, anim_part(&t)), (ANIMATED_TEX, anim_part(&other))])).unwrap();
    assert_eq!(m.anim_textures, vec![t, other]);
    // The model carries them, and the loaded model runs one per part.
    let file = ModelFile { name: "t".into(), desc: String::new(), data: m, icon_png: Vec::new() };
    let lm = LoadedModel::from_file(file).unwrap();
    assert_eq!(lm.model.anim_textures, vec![t, other]);
    assert_eq!(lm.anim_textures.len(), 2);
}

#[test]
fn anim_tex_frame_by_time() {
    let a = AnimatedTexture::new(tex(0, false, false));
    // `time / frameTime % frameCount`.
    assert_eq!(a.frame(0), 0);
    assert_eq!(a.frame(99), 0);
    assert_eq!(a.frame(100), 1);
    assert_eq!(a.frame(250), 2);
    assert_eq!(a.frame(300), 0);
    assert_eq!(a.frame(1_000_000_150), (1_000_000_150u64 / 100 % 3) as i32);
}

#[test]
fn anim_tex_copies_frame_x_and_y() {
    // y layout: frame 1 of a 2x2 region starting at (0, 2) is at (0, 4).
    let (w, h) = (8, 8);
    let mut img = coords(w, h);
    let mut a = AnimatedTexture::new(tex(0, false, false));
    assert!(a.update(150, &mut img, [w, h]));
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(px(&img, w, x, y), px(&coords(w, h), w, x, y + 4), "y layout at {x},{y}");
        }
    }
    // Pixels outside the region are untouched.
    assert_eq!(px(&img, w, 2, 0), px(&coords(w, h), w, 2, 0));

    // x layout: frame 2 of a 2x2 region starting at (0, 2) is at (4, 2).
    let mut img = coords(w, h);
    let mut a = AnimatedTexture::new(tex(0, true, false));
    assert!(a.update(250, &mut img, [w, h]));
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(px(&img, w, x, y), px(&coords(w, h), w, x + 4, y + 2), "x layout at {x},{y}");
        }
    }

    // A frame partly outside the image: only the pixels inside are copied.
    let (w, h) = (8, 5);
    let mut img = coords(w, h);
    let mut a = AnimatedTexture::new(tex(0, false, false));
    assert!(a.update(150, &mut img, [w, h])); // frame rows 4..6, row 5 is outside
    assert_eq!(px(&img, w, 0, 0), px(&coords(w, h), w, 0, 4));
    assert_eq!(px(&img, w, 0, 1), px(&coords(w, h), w, 0, 1));
}

#[test]
fn anim_tex_interpolates() {
    // One 1x1 region at (0, 0), frames at (1, 0), (2, 0), (3, 0) along x.
    let (w, h) = (4, 1);
    let frames = [[0, 0, 0, 0], [200, 100, 10, 255], [0, 50, 250, 128], [100, 100, 100, 7]];
    let base: Vec<u8> = frames.concat();
    let t = AnimTex {
        sheet: 0,
        uv_start: [0, 0],
        uv_size: [1, 1],
        anim_start: [1, 0],
        frame_count: 3,
        frame_time: 100,
        an_x: true,
        interpolate: true,
    };
    let java = |time: u64, first: [u8; 4], second: [u8; 4]| -> [u8; 4] {
        // copyTextureInt: t = frm + (time % frameTime) / frameTime; subfrm = 1 - (t - (int) t);
        // mix = (int) (subfrm * first + (1 - subfrm) * second), alpha from the first frame.
        let frm = (time / 100 % 3) as i32;
        let tt = frm as f32 + (time % 100) as f32 / 100.0;
        let sub = 1.0 - (tt - tt as i32 as f32);
        let mix = |a: u8, b: u8| (sub * a as f32 + (1.0 - sub) * b as f32) as i32 as u8;
        [mix(first[0], second[0]), mix(first[1], second[1]), mix(first[2], second[2]), first[3]]
    };
    let mut a = AnimatedTexture::new(t);
    let mut img = base.clone();
    // Not yet 50 ms past the last update (0).
    assert!(!a.update(49, &mut img, [w, h]));
    assert_eq!(img, base);
    // 125 ms: frame 1 a quarter of the way to frame 2.
    assert!(a.update(125, &mut img, [w, h]));
    assert_eq!(px(&img, w, 0, 0), java(125, frames[2], frames[3]));
    // At most every 50 ms (and so every 50 ms at 60 fps, not every 4th 16.7 ms frame).
    let before = img.clone();
    assert!(!a.update(174, &mut img, [w, h]));
    assert_eq!(img, before);
    assert!(a.update(175, &mut img, [w, h]));
    assert_eq!(px(&img, w, 0, 0), java(175, frames[2], frames[3]));
    // 290 ms: frame 2 blending into frame 0 (wraps).
    assert!(a.update(290, &mut img, [w, h]));
    assert_eq!(px(&img, w, 0, 0), java(290, frames[3], frames[1]));
}

#[test]
fn anim_tex_changed_only_on_new_frame() {
    let (w, h) = (8, 8);
    let mut img = coords(w, h);
    let mut a = AnimatedTexture::new(tex(0, false, false));
    // Frame 0 at first (Java's `lastFrame` starts at 0): nothing copied.
    assert!(!a.update(0, &mut img, [w, h]));
    assert!(!a.update(99, &mut img, [w, h]));
    assert_eq!(img, coords(w, h));
    assert!(a.update(100, &mut img, [w, h]));
    assert!(!a.update(150, &mut img, [w, h]));
    assert!(!a.update(199, &mut img, [w, h]));
    assert!(a.update(200, &mut img, [w, h]));
    // No frames or no frame time: never.
    for t in [AnimTex { frame_count: 0, ..tex(0, false, false) }, AnimTex { frame_time: 0, ..tex(0, false, false) }] {
        let mut a = AnimatedTexture::new(t);
        assert!(!a.update(1234, &mut img, [w, h]));
    }

    // On the loaded model: the skin, cape and elytra sheets change; other sheets are ignored.
    let skin = png(w, h, &coords(w, h));
    let mut sheet = Writer::new();
    sheet.u8(0).i16(8).i16(8).varint(skin.len() as u32).bytes(&skin);
    let mut cape = Writer::new();
    cape.u8(2).i16(8).i16(8).varint(skin.len() as u32).bytes(&skin);
    let def = definition(&[
        (TEXTURE, sheet),
        (TEXTURE, cape),
        (ANIMATED_TEX, anim_part(&tex(2, false, false))),
        (ANIMATED_TEX, anim_part(&tex(4, false, false))), // ARMOR1: ignored
        (ANIMATED_TEX, anim_part(&tex(3, false, false))), // ELYTRA, but the model has none
    ]);
    let file = ModelFile { name: "t".into(), desc: String::new(), data: read_definition(&def).unwrap(), icon_png: Vec::new() };
    let mut lm = LoadedModel::from_file(file).unwrap();
    assert!(!lm.tick_textures(50));
    assert!(lm.tick_textures(120));
    assert!(!lm.tick_textures(130));
    assert_eq!(lm.texture_rgba, coords(w, h), "the skin has no animation");
    let c = &lm.cape.as_ref().unwrap().rgba;
    assert_eq!(px(c, w, 1, 1), px(&coords(w, h), w, 1, 5));
}
