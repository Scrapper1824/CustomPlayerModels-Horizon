mod common;

use cpm_core::anim::engine::{Frame, PartDelta, PoseInput};
use cpm_core::effects::FaceUv;
use cpm_core::format::{read_cpmmodel, ModelFile, RawCube, RawEffect, RawModel, SkinType};
use cpm_core::mesh::{
    load, pose_arm, pose_model, BoxSource, FaceQuad, LoadedModel, PosedBox, FACE_DOWN, FACE_EAST, FACE_NORTH, FACE_SOUTH,
    FACE_UP, FACE_WEST,
};
use cpm_core::model::Model;
use glam::{Mat4, Vec3};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}.cpmmodel", env!("CARGO_MANIFEST_DIR"))).unwrap()
}
fn reggie_file() -> ModelFile {
    read_cpmmodel(&sample("ReggieTheRat")).unwrap()
}
fn standing() -> PoseInput {
    PoseInput { on_ground: true, ..Default::default() }
}
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}
fn by_cube(boxes: &[PosedBox], id: u32) -> Option<&PosedBox> {
    boxes.iter().find(|b| b.source == BoxSource::Cube(id))
}
fn world(b: &PosedBox, p: [f32; 3]) -> Vec3 {
    b.transform.transform_point3(Vec3::from_array(p))
}
/// Every face corner of `b`, in the draw frame (blocks).
fn world_corners(b: &PosedBox) -> Vec<Vec3> {
    b.faces.iter().flatten().flat_map(|f| f.corners.map(|c| world(b, c))).collect()
}
fn bounds(pts: &[Vec3]) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for p in pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo, hi)
}
fn raw_cube(id: u32, parent: u32) -> RawCube {
    RawCube {
        id,
        parent,
        size: [2.0, 3.0, 4.0],
        pos: [0.0; 3],
        offset: [0.0; 3],
        rotation: [0.0; 3],
        tex_size: 1,
        u: 0,
        v: 0,
        rgb: 0,
        mesh_scale: [1.0; 3],
        scale: [1.0; 3],
        mc_scale: 0.0,
        hidden: false,
    }
}
/// Reggie's header (skin type, texture, part positions) with the given cubes and nothing else.
fn synthetic(cubes: Vec<RawCube>, effects: Vec<RawEffect>) -> Model {
    let mut raw: RawModel = reggie_file().data;
    raw.cubes = cubes;
    raw.effects = effects;
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [false; 6];
    Model::build(raw).unwrap()
}
fn outward(q: &FaceQuad, centre: Vec3) -> f32 {
    let [tl, bl, br, _] = q.corners.map(Vec3::from_array);
    let n = (bl - tl).cross(br - tl);
    let mid = q.corners.iter().map(|c| Vec3::from_array(*c)).sum::<Vec3>() / 4.0;
    n.dot(mid - centre)
}
fn local_centre(b: &PosedBox) -> Vec3 {
    let pts: Vec<Vec3> = b.faces.iter().flatten().flat_map(|f| f.corners.map(Vec3::from_array)).collect();
    let (lo, hi) = bounds(&pts);
    (lo + hi) / 2.0
}

#[test]
fn reggie_standing_box_count() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let boxes = m.pose(&standing());
    // 26 cubes, minus the zero-size 14, 21, 22; Reggie keeps no vanilla part.
    assert_eq!(boxes.len(), 23);
    for id in [14, 21, 22] {
        assert!(by_cube(&boxes, id).is_none(), "zero-size cube {id} emitted");
    }
    assert!(boxes.iter().all(|b| matches!(b.source, BoxSource::Cube(_))));
}

#[test]
fn head_box_uv() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let boxes = m.pose(&standing());
    let head = by_cube(&boxes, 10).unwrap();
    // Box layout from (0,0), 8x8x8: the north (front) face is u 8..16, v 8..16.
    assert_eq!(head.faces[FACE_NORTH].as_ref().unwrap().uv, [8.0, 8.0, 16.0, 16.0]);
    // Top strip: up 8..16 x 0..8, down 16..24 with v flipped (8 -> 0).
    assert_eq!(head.faces[FACE_UP].as_ref().unwrap().uv, [8.0, 0.0, 16.0, 8.0]);
    assert_eq!(head.faces[FACE_DOWN].as_ref().unwrap().uv, [16.0, 8.0, 24.0, 0.0]);
    assert_eq!(head.faces[FACE_WEST].as_ref().unwrap().uv, [0.0, 8.0, 8.0, 16.0]);
    assert_eq!(head.faces[FACE_EAST].as_ref().unwrap().uv, [16.0, 8.0, 24.0, 16.0]);
    assert_eq!(head.faces[FACE_SOUTH].as_ref().unwrap().uv, [24.0, 8.0, 32.0, 16.0]);
    assert_eq!(head.tint, 0xFFFFFF);
    assert!(head.textured && !head.glow);
}

#[test]
fn head_box_position() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let boxes = m.pose(&standing());
    let head = by_cube(&boxes, 10).unwrap();
    let (lo, hi) = bounds(&world_corners(head));
    // Hand check (model px, Y down; standing, head yaw/pitch 0, no clip drives cube 10):
    //   head pivot = vanilla (0,0,0) + PLAYER_PARTPOS (0,-1.5,0) = (0,-1.5,0); rotation 0
    //   cube 10: pos 0, rot 0, offset (-4,-8,-4), size 8 -> local box x -4..4, y -8..0, z -4..4
    //   in model space: x -4..4, y -9.5..-1.5, z -4..4
    //   draw frame (feet origin, Y up, +Z forward), scaled by vanilla's 0.9375 about the feet:
    //   0.9375 * (x/16, 1.501 - y/16, -z/16)
    //     x: 0.9375 * +-4/16 = +-0.234375
    //     y: 0.9375 * (1.501 + 1.5/16) = 1.495078 .. 0.9375 * (1.501 + 9.5/16) = 1.963828
    //     z: +-0.234375
    assert!(near(lo.x, -0.234_375) && near(hi.x, 0.234_375), "{lo} {hi}");
    assert!(near(lo.y, HEAD_LO) && near(hi.y, HEAD_HI), "{lo} {hi}");
    assert!(near(lo.z, -0.234_375) && near(hi.z, 0.234_375), "{lo} {hi}");
    // The face (north, model -Z) looks forward (+Z): its corners all sit at the front,
    // and its top-left corner (seen from the front) is the viewer's left (-x) and up.
    let north = head.faces[FACE_NORTH].as_ref().unwrap();
    for c in north.corners {
        assert!(near(world(head, c).z, 0.234_375), "{:?}", world(head, c));
    }
    let tl = world(head, north.corners[0]);
    assert!(near(tl.x, -0.234_375) && near(tl.y, HEAD_HI), "{tl}");
    let br = world(head, north.corners[2]);
    assert!(near(br.x, 0.234_375) && near(br.y, HEAD_LO), "{br}");
}

/// Reggie's head cube (model y -9.5..-1.5 px) in the draw frame, at vanilla's 0.9375 scale.
const HEAD_LO: f32 = 0.9375 * (1.501 + 1.5 / 16.0);
const HEAD_HI: f32 = 0.9375 * (1.501 + 9.5 / 16.0);

#[test]
fn player_scale_shrinks_about_the_feet() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let boxes = m.pose(&standing());
    let head = by_cube(&boxes, 10).unwrap();
    // AvatarRenderer.scale (Horizon's species_scale): 0.9375 on every axis, times 1/16 px.
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(near(head.transform.transform_vector3(axis).length(), 0.9375 / 16.0));
    }
    // The model's feet (1.501 blocks below its origin; the head bone sits 1.5 px above the
    // origin) stay on the ground: the scale is about the feet, not the model origin.
    let feet = head.transform.transform_point3(Vec3::new(0.0, 1.501 * 16.0 + 1.5, 0.0));
    assert!(near(feet.y, 0.0), "{feet}");
}

#[test]
fn left_arm_is_on_plus_x() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let boxes = m.pose(&standing());
    // Cube 26 hangs off the left arm (bone 2); +x is the player's left in the draw frame.
    let (lo, _) = bounds(&world_corners(by_cube(&boxes, 26).unwrap()));
    assert!(lo.x > 0.0, "{lo}");
    let (_, hi) = bounds(&world_corners(by_cube(&boxes, 29).unwrap()));
    assert!(hi.x < 0.0, "{hi}");
}

#[test]
fn uv_uses_declared_texture_size() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let m = load(&sample("ReggieTheRat")).unwrap();
    assert_eq!(m.texture_px, [75, 64]);
    assert_eq!(m.texture_rgba.len(), (m.png_px[0] * m.png_px[1] * 4) as usize);

    // Same model with a PNG twice the declared size: UVs do not move.
    let mut file = reggie_file();
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, 150, 128);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&vec![0x7f; 150 * 128 * 4]).unwrap();
    }
    file.data.texture.as_mut().unwrap().png = png;
    let mut big = LoadedModel::from_file(file).unwrap();
    assert_eq!(big.texture_px, [75, 64]);
    assert_eq!(big.png_px, [150, 128]);
    assert_eq!(big.texture_rgba.len(), 150 * 128 * 4);
    let boxes = big.pose(&standing());
    let head = by_cube(&boxes, 10).unwrap();
    assert_eq!(head.faces[FACE_NORTH].as_ref().unwrap().uv, [8.0, 8.0, 16.0, 16.0]);
    for b in &boxes {
        for f in b.faces.iter().flatten() {
            for (i, t) in f.uv.iter().enumerate() {
                let max = if i % 2 == 0 { 75.0 } else { 64.0 };
                assert!((0.0..=max).contains(t), "cube {:?} uv {:?}", b.source, f.uv);
            }
        }
    }
}

#[test]
fn hidden_skips_children() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.legacy_anim = None;
    raw.cubes.iter_mut().find(|c| c.id == 21).unwrap().hidden = true;
    let model = Model::build(raw).unwrap();
    let boxes = pose_model(&model, &Frame::default(), &standing());
    // 21 -> 22 -> {23, 24, 25}
    for id in [23, 24, 25] {
        assert!(by_cube(&boxes, id).is_none(), "{id}");
    }
    assert_eq!(boxes.len(), 20);
}

#[test]
fn animated_hide_skips_children_and_show_reveals() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.legacy_anim = None;
    let model = Model::build(raw.clone()).unwrap();
    let mut frame = Frame::default();
    frame.parts.insert(22, PartDelta { visible: Some(false), ..Default::default() });
    let boxes = pose_model(&model, &frame, &standing());
    assert!(by_cube(&boxes, 23).is_none() && by_cube(&boxes, 25).is_none());
    assert_eq!(boxes.len(), 20);

    raw.cubes.iter_mut().find(|c| c.id == 10).unwrap().hidden = true;
    let model = Model::build(raw).unwrap();
    let mut frame = Frame::default();
    frame.parts.insert(10, PartDelta { visible: Some(true), ..Default::default() });
    assert!(by_cube(&pose_model(&model, &frame, &standing()), 10).is_some());
    assert!(by_cube(&pose_model(&model, &Frame::default(), &standing()), 10).is_none());
}

#[test]
fn walk_swings_legs() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let a = m.pose(&PoseInput { walk_position: 0.0, walk_speed: 1.0, ..standing() });
    let b = m.pose(&PoseInput { walk_position: 2.36, walk_speed: 1.0, ..standing() });
    for id in [31, 34] {
        let ta = by_cube(&a, id).unwrap().transform;
        let tb = by_cube(&b, id).unwrap().transform;
        assert!(!ta.abs_diff_eq(tb, 1e-3), "leg cube {id} did not move");
    }
    // Legs swing opposite ways: at position 0 the right leg (bone 5) is rotated by
    // cos(0) * 1.4 = 1.4 rad, the left one by -1.4.
    let model = &m.model;
    let r = pose_model(model, &Frame::default(), &PoseInput { walk_speed: 1.0, ..standing() });
    let foot = |id: u32| {
        let b = by_cube(&r, id).unwrap();
        bounds(&world_corners(b))
    };
    // Positive xRot turns a part hanging down (+Y model) towards +Z model, which is
    // backwards (-Z) in the draw frame: the right foot goes back, the left one forward.
    let (rlo, rhi) = foot(34);
    let (llo, lhi) = foot(31);
    assert!(rlo.z < -0.5 && lhi.z > 0.5, "right {rlo}..{rhi} left {llo}..{lhi}");
}

#[test]
fn sneaking_lowers_head() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = load(&sample("ReggieTheRat")).unwrap().model;
    let stand = pose_model(&model, &Frame::default(), &standing());
    let sneak = pose_model(&model, &Frame::default(), &PoseInput { sneaking: true, ..standing() });
    let (a, _) = bounds(&world_corners(by_cube(&stand, 10).unwrap()));
    let (b, _) = bounds(&world_corners(by_cube(&sneak, 10).unwrap()));
    // head.y += 4.2 px (model Y down) -> 4.2/16 blocks lower, at the 0.9375 player scale.
    assert!(near(a.y - b.y, 0.9375 * 4.2 / 16.0), "{a} {b}");
}

#[test]
fn disabled_vanilla_anim_ignores_head_yaw() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.legacy_anim = None;
    let turned = PoseInput { head_yaw: 60.0, pitch: 20.0, ..standing() };
    let model = Model::build(raw.clone()).unwrap();
    let a = by_cube(&pose_model(&model, &Frame::default(), &standing()), 10).unwrap().transform;
    let b = by_cube(&pose_model(&model, &Frame::default(), &turned), 10).unwrap().transform;
    assert!(!a.abs_diff_eq(b, 1e-3));
    raw.vanilla_anim_disabled[0] = true;
    let model = Model::build(raw).unwrap();
    let c = by_cube(&pose_model(&model, &Frame::default(), &turned), 10).unwrap().transform;
    assert!(a.abs_diff_eq(c, 1e-5));
    // Animation deltas still apply to a bone with vanilla animation disabled.
    let mut frame = Frame::default();
    frame.parts.insert(0, PartDelta { rot: Some([0.0, 45.0, 0.0]), rot_additive: true, additive: true, ..Default::default() });
    let d = by_cube(&pose_model(&model, &frame, &turned), 10).unwrap().transform;
    assert!(!a.abs_diff_eq(d, 1e-3));
}

#[test]
fn bone_delta_absolute_replaces_additive_adds() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.legacy_anim = None;
    let model = Model::build(raw).unwrap();
    let base = by_cube(&pose_model(&model, &Frame::default(), &standing()), 10).unwrap().transform;
    // Absolute pos equal to the bone's own pivot: unchanged.
    let mut f = Frame::default();
    f.parts.insert(0, PartDelta { pos: Some([0.0, -1.5, 0.0]), ..Default::default() });
    let same = by_cube(&pose_model(&model, &f, &standing()), 10).unwrap().transform;
    assert!(base.abs_diff_eq(same, 1e-5));
    // Additive +16 px on y: one block lower, at the 0.9375 player scale.
    let mut f = Frame::default();
    f.parts.insert(0, PartDelta { pos: Some([0.0, 16.0, 0.0]), pos_additive: true, additive: true, ..Default::default() });
    let moved = by_cube(&pose_model(&model, &f, &standing()), 10).unwrap().transform;
    let d = moved.transform_point3(Vec3::ZERO) - base.transform_point3(Vec3::ZERO);
    assert!(near(d.y, -0.9375) && near(d.x, 0.0) && near(d.z, 0.0), "{d}");
}

#[test]
fn faces_wind_outward() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut mirrored = raw_cube(10, 1);
    mirrored.tex_size = (-1i8) as u8;
    for (cubes, effects) in [
        (vec![raw_cube(10, 1)], vec![]),
        (vec![mirrored], vec![]),
        (vec![raw_cube(10, 1)], vec![RawEffect { kind: 4, payload: vec![10] }]),
    ] {
        let model = synthetic(cubes, effects);
        let boxes = pose_model(&model, &Frame::default(), &standing());
        assert_eq!(boxes.len(), 1);
        let b = &boxes[0];
        let c = local_centre(b);
        for (i, f) in b.faces.iter().enumerate() {
            let f = f.as_ref().unwrap_or_else(|| panic!("face {i} missing"));
            assert!(outward(f, c) > 0.0, "face {i} winds inward: {f:?}");
        }
        // Face planes: down at max y, up at min y, north min z, south max z, west min x, east max x.
        let plane = |i: usize, axis: usize| b.faces[i].as_ref().unwrap().corners.map(|p| p[axis]);
        let mirror = matches!(b.faces[FACE_NORTH].as_ref().unwrap().uv, [a, _, c, _] if a > c);
        assert!(plane(FACE_DOWN, 1).iter().all(|y| *y == 3.0));
        assert!(plane(FACE_UP, 1).iter().all(|y| *y == 0.0));
        assert!(plane(FACE_NORTH, 2).iter().all(|z| *z == 0.0));
        assert!(plane(FACE_SOUTH, 2).iter().all(|z| *z == 4.0));
        // A mirrored box trades its x sides (ModelPart.Cube mirror).
        let (w, e) = if mirror { (2.0, 0.0) } else { (0.0, 2.0) };
        assert!(plane(FACE_WEST, 0).iter().all(|x| *x == w), "{mirror}");
        assert!(plane(FACE_EAST, 0).iter().all(|x| *x == e), "{mirror}");
    }
}

#[test]
fn mirrored_box_flips_u() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut c = raw_cube(10, 1);
    c.tex_size = (-1i8) as u8;
    c.size = [8.0, 8.0, 8.0];
    let model = synthetic(vec![c], vec![]);
    let b = &pose_model(&model, &Frame::default(), &standing())[0];
    // ModelPart.Cube with mirror: same rect, u runs the other way.
    assert_eq!(b.faces[FACE_NORTH].as_ref().unwrap().uv, [16.0, 8.0, 8.0, 16.0]);
}

#[test]
fn single_tex_uses_one_square() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = synthetic(vec![raw_cube(10, 1)], vec![RawEffect { kind: 4, payload: vec![10] }]);
    let b = &pose_model(&model, &Frame::default(), &standing())[0];
    // createTexturedSingle: every face uses (u, v)..(u + max(dx,dy,dz), v + same) = 0..4.
    for f in b.faces.iter().flatten() {
        assert_eq!(f.uv, [0.0, 0.0, 4.0, 4.0]);
    }
}

#[test]
fn per_face_uv_maps_cpm_directions() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // PER_FACE_UV payload: cube varint, mask (CPM order UP, DOWN, NORTH, SOUTH, EAST, WEST),
    // then per face u0 v0 u1 v1 rot. Faces: UP (1,2,3,4) rot 0, EAST (10,11,12,13) rot 1.
    let payload = vec![10, 0b01_0001, 1, 2, 3, 4, 0, 10, 11, 12, 13, 1];
    let model = synthetic(vec![raw_cube(10, 1)], vec![RawEffect { kind: 5, payload }]);
    assert_eq!(
        model.cubes[0].face_uv.unwrap()[0],
        Some(FaceUv { u0: 1, v0: 2, u1: 3, v1: 4, rot: 0, auto_uv: false })
    );
    let b = &pose_model(&model, &Frame::default(), &standing())[0];
    let present: Vec<usize> = (0..6).filter(|i| b.faces[*i].is_some()).collect();
    assert_eq!(present, vec![FACE_UP, FACE_EAST]);
    let up = b.faces[FACE_UP].as_ref().unwrap();
    assert_eq!(up.uv, [1.0, 2.0, 3.0, 4.0]);
    // UP quad (ptv4, ptv3, ptv7, ptv) gets (ex,sy),(sx,sy),(sx,ey),(ex,ey): TL = ptv3 = (x, y, ez).
    assert_eq!(up.corners[0], [0.0, 0.0, 4.0]);
    let east = b.faces[FACE_EAST].as_ref().unwrap();
    assert_eq!(east.uv, [10.0, 11.0, 12.0, 13.0]);
    // EAST quad (ptv4, ptv, ptv1, ptv5), rotated once: vertex i takes slot (i + 1) % 4,
    // so TL (slot 1, (sx,sy)) is vertex 0 = ptv4 = (ex, y, ez).
    assert_eq!(east.corners[0], [2.0, 0.0, 4.0]);
}

#[test]
fn colored_cube_is_untextured_and_tinted() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut c = raw_cube(10, 1);
    c.tex_size = 0;
    c.rgb = 0xFF11_2233;
    let model = synthetic(vec![c], vec![]);
    let boxes = pose_model(&model, &Frame::default(), &standing());
    assert_eq!(boxes.len(), 1);
    assert!(!boxes[0].textured);
    assert_eq!(boxes[0].tint, 0x112233);
    // Animated colour overrides it.
    let mut f = Frame::default();
    f.parts.insert(10, PartDelta { color: Some([255.0, 0.0, 16.0]), ..Default::default() });
    assert_eq!(pose_model(&model, &f, &standing())[0].tint, 0xFF0010);
}

#[test]
fn inflate_and_mesh_scale_move_corners() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut c = raw_cube(10, 1);
    c.mc_scale = 0.5;
    c.mesh_scale = [2.0, 1.0, 1.0];
    let model = synthetic(vec![c], vec![]);
    let b = &pose_model(&model, &Frame::default(), &standing())[0];
    let pts: Vec<Vec3> = b.faces.iter().flatten().flat_map(|f| f.corners.map(Vec3::from_array)).collect();
    let (lo, hi) = bounds(&pts);
    assert_eq!(lo, Vec3::new(-0.5, -0.5, -0.5));
    assert_eq!(hi, Vec3::new(4.5, 3.5, 4.5));
    // UVs keep the unscaled layout (dx = ceil(2) = 2, dy = 3, dz = 4).
    assert_eq!(b.faces[FACE_NORTH].as_ref().unwrap().uv, [4.0, 4.0, 6.0, 7.0]);
}

#[test]
fn kept_vanilla_parts_are_drawn() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.cubes.clear();
    raw.effects.clear();
    raw.legacy_anim = None;
    raw.keep_vanilla = [true; 6];
    raw.skin_type = SkinType::Slim;
    raw.texture.as_mut().unwrap().declared = [64, 64];
    let model = Model::build(raw).unwrap();
    let boxes = pose_model(&model, &Frame::default(), &standing());
    // Each part and its second layer.
    assert_eq!(boxes.len(), 12);
    let head = boxes.iter().find(|b| b.source == BoxSource::Vanilla(0)).unwrap();
    assert_eq!(head.faces[FACE_NORTH].as_ref().unwrap().uv, [8.0, 8.0, 16.0, 16.0]);
    // Same placement as Reggie's head cube (same offset/size, same bone).
    let (lo, hi) = bounds(&world_corners(head));
    assert!(near(lo.y, HEAD_LO) && near(hi.y, HEAD_HI), "{lo} {hi}");
    // Slim left arm: 3 px wide (local box space; the idle arm bob tilts it in the draw
    // frame), offset (-1, -2, -2), uv (32, 48).
    let arm = boxes.iter().find(|b| b.source == BoxSource::Vanilla(2)).unwrap();
    let pts: Vec<Vec3> = arm.faces.iter().flatten().flat_map(|f| f.corners.map(Vec3::from_array)).collect();
    let (lo, hi) = bounds(&pts);
    assert_eq!((lo, hi), (Vec3::new(-1.0, -2.0, -2.0), Vec3::new(2.0, 10.0, 2.0)));
    assert_eq!(arm.faces[FACE_NORTH].as_ref().unwrap().uv, [36.0, 52.0, 39.0, 64.0]);
}

fn local_bounds(b: &PosedBox) -> (Vec3, Vec3) {
    let pts: Vec<Vec3> = b.faces.iter().flatten().flat_map(|f| f.corners.map(Vec3::from_array)).collect();
    bounds(&pts)
}

fn kept_vanilla(skin: SkinType, declared: [u32; 2]) -> Vec<PosedBox> {
    let mut raw = reggie_file().data;
    raw.cubes.clear();
    raw.effects.clear();
    raw.legacy_anim = None;
    raw.keep_vanilla = [true; 6];
    raw.skin_type = skin;
    raw.texture.as_mut().unwrap().declared = declared;
    pose_model(&Model::build(raw).unwrap(), &Frame::default(), &standing())
}

#[test]
fn kept_vanilla_parts_have_their_second_layer() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let boxes = kept_vanilla(SkinType::Default, [64, 64]);
    let part = |s: BoxSource| boxes.iter().find(|b| b.source == s).unwrap_or_else(|| panic!("{s:?}"));
    // PlayerPartValues (u2, v2) and PlayerModel.createMesh's CubeDeformation: hat 0.5, the
    // others 0.25. The layer shares its part's transform and box, inflated.
    for (bone, uv2, inflate) in [
        (0, [32.0, 0.0], 0.5),
        (1, [16.0, 32.0], 0.25),
        (2, [48.0, 48.0], 0.25),
        (3, [40.0, 32.0], 0.25),
        (4, [0.0, 48.0], 0.25),
        (5, [0.0, 32.0], 0.25),
    ] {
        let base = part(BoxSource::Vanilla(bone));
        let layer = part(BoxSource::VanillaLayer(bone));
        assert_eq!(base.transform, layer.transform, "bone {bone}");
        assert!(layer.textured && !layer.glow && layer.tint == 0xFF_FFFF);
        let (blo, bhi) = local_bounds(base);
        let (llo, lhi) = local_bounds(layer);
        assert!(((lhi - bhi) - Vec3::splat(inflate)).abs().max_element() < 1e-6, "bone {bone}");
        assert!(((blo - llo) - Vec3::splat(inflate)).abs().max_element() < 1e-6, "bone {bone}");
        // The part's box layout, from (u2, v2): the west face starts at (u2, v2 + depth).
        let depth = bhi.z - blo.z;
        let west = layer.faces[FACE_WEST].as_ref().unwrap().uv;
        assert_eq!([west[0], west[1]], [uv2[0], uv2[1] + depth], "bone {bone}");
    }
    // Slim sleeves keep their 3 px width under the layer.
    let slim = kept_vanilla(SkinType::Slim, [64, 64]);
    let sleeve = slim.iter().find(|b| b.source == BoxSource::VanillaLayer(2)).unwrap();
    let (lo, hi) = local_bounds(sleeve);
    assert_eq!((lo, hi), (Vec3::new(-1.25, -2.25, -2.25), Vec3::new(2.25, 10.25, 2.25)));
    assert_eq!(sleeve.faces[FACE_NORTH].as_ref().unwrap().uv, [52.0, 52.0, 55.0, 64.0]);
}

#[test]
fn kept_vanilla_uvs_follow_the_texture_size() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // A 128x64 texture: u doubles, v stays (each axis by its size / 64).
    let boxes = kept_vanilla(SkinType::Default, [128, 64]);
    let head = boxes.iter().find(|b| b.source == BoxSource::Vanilla(0)).unwrap();
    assert_eq!(head.faces[FACE_NORTH].as_ref().unwrap().uv, [16.0, 8.0, 32.0, 16.0]);
    let hat = boxes.iter().find(|b| b.source == BoxSource::VanillaLayer(0)).unwrap();
    assert_eq!(hat.faces[FACE_NORTH].as_ref().unwrap().uv, [80.0, 8.0, 96.0, 16.0]);
    // A 128x128 (HD) texture: both double.
    let boxes = kept_vanilla(SkinType::Default, [128, 128]);
    let head = boxes.iter().find(|b| b.source == BoxSource::Vanilla(0)).unwrap();
    assert_eq!(head.faces[FACE_NORTH].as_ref().unwrap().uv, [16.0, 16.0, 32.0, 32.0]);
}

#[test]
fn animated_colour_on_a_textured_cube_follows_java() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // RenderedCube.setColor only takes a colour when the cube is colour-only (texSize 0) or
    // has the RECOLOR effect; an animated colour on a plain textured cube is dropped, as in Java.
    let model = synthetic(vec![raw_cube(10, 1)], vec![]);
    let mut f = Frame::default();
    f.parts.insert(10, PartDelta { color: Some([255.0, 0.0, 16.0]), ..Default::default() });
    let b = &pose_model(&model, &f, &standing())[0];
    assert!(b.textured);
    assert_eq!(b.tint, 0xFF_FFFF);
    // With RECOLOR (effect 3: cube, r, g, b) the textured cube is tinted by the effect colour,
    // and an animated colour replaces it.
    let recolor = RawEffect { kind: 3, payload: vec![10, 0x11, 0x22, 0x33] };
    let model = synthetic(vec![raw_cube(10, 1)], vec![recolor]);
    let plain = &pose_model(&model, &Frame::default(), &standing())[0];
    assert!(plain.textured);
    assert_eq!(plain.tint, 0x11_2233);
    assert_eq!(pose_model(&model, &f, &standing())[0].tint, 0xFF0010);
}

#[test]
fn skulldog_loads_and_poses() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = load(&sample("SkullDog")).unwrap();
    assert!(m.texture_px[0] > 0 && m.png_px[0] > 0);
    let boxes = m.pose(&standing());
    assert!(!boxes.is_empty());
    assert!(boxes.iter().all(|b| matches!(b.source, BoxSource::Cube(_))));
    for b in &boxes {
        assert!(b.transform.is_finite());
        assert!(b.faces.iter().flatten().all(|f| f.corners.iter().flatten().all(|v| v.is_finite())));
    }
    let _: Mat4 = boxes[0].transform;
}

// ---------------------------------------------------------------------------------------------
// The first-person arm

/// Where Horizon's `first_person_arm` puts an arm-local point `v` (model px) before its `m`:
/// `mobs::render_model` (crates/horizon-client/src/mobs.rs:3488) visits the arm part with
/// `parent * T(pose.xyz / 16) * R(EulerRot::ZYX, z, y, x) * S(pose.scale)` and places each vertex at
/// that times `v / 16`; `first_person_arm` (game.rs:13501) keeps the part's offset (the pivot),
/// zeroes x/y rotation and sets z to -0.1 (left) or 0.1 (right); the arm part's scale is 1.
fn render_model_place(pivot: [f32; 3], left: bool, v: [f32; 3]) -> Vec3 {
    let z = if left { -0.1 } else { 0.1 };
    let part = Mat4::from_translation(Vec3::from_array(pivot) / 16.0)
        * Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::ZYX, z, 0.0, 0.0));
    part.transform_point3(Vec3::from_array(v) / 16.0)
}

fn assert_placed(b: &PosedBox, pivot: [f32; 3], left: bool) {
    for f in b.faces.iter().flatten() {
        for c in f.corners {
            let got = world(b, c);
            let want = render_model_place(pivot, left, c);
            assert!((got - want).abs().max_element() < 1e-6, "{:?} {c:?}: {got} != {want}", b.source);
        }
    }
}

#[test]
fn first_person_right_arm_is_cubes_28_and_29() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let m = load(&sample("ReggieTheRat")).unwrap();
    let arm = m.pose_arm(false);
    let ids: Vec<BoxSource> = arm.iter().map(|b| b.source).collect();
    assert_eq!(ids, [BoxSource::Cube(28), BoxSource::Cube(29)]);
    // Box layouts (u, v; dx, dy, dz): 28 (17, 54; 3, 6, 4), 29 (40, 28; 3, 12, 4). North is
    // (u + dz, v + dz) to (u + dz + dx, v + dz + dy).
    assert_eq!(arm[0].faces[FACE_NORTH].unwrap().uv, [21.0, 58.0, 24.0, 64.0]);
    assert_eq!(arm[1].faces[FACE_NORTH].unwrap().uv, [44.0, 32.0, 47.0, 44.0]);
    assert_eq!(arm[1].faces[FACE_UP].unwrap().uv, [44.0, 28.0, 47.0, 32.0]);
    // Reggie's right arm bone: vanilla (-5, 2, 0) + PLAYER_PARTPOS (0.5, 0, 0).
    for b in &arm {
        assert_placed(b, [-4.5, 2.0, 0.0], false);
        assert!(b.textured && !b.glow && b.tint == 0xFF_FFFF);
    }
}

#[test]
fn first_person_left_arm_is_cubes_26_and_27() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let m = load(&sample("ReggieTheRat")).unwrap();
    let arm = m.pose_arm(true);
    let ids: Vec<BoxSource> = arm.iter().map(|b| b.source).collect();
    assert_eq!(ids, [BoxSource::Cube(26), BoxSource::Cube(27)]);
    // 26 (40, 11; 3, 12, 4), 27 (51, 45; 3, 6, 4).
    assert_eq!(arm[0].faces[FACE_NORTH].unwrap().uv, [44.0, 15.0, 47.0, 27.0]);
    assert_eq!(arm[1].faces[FACE_NORTH].unwrap().uv, [55.0, 49.0, 58.0, 55.0]);
    for b in &arm {
        assert_placed(b, [4.5, 2.0, 0.0], true);
    }
}

#[test]
fn first_person_arm_is_static() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // No walk swing, bob, sneak or animation: the same boxes whatever was posed before.
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let still = m.pose_arm(false);
    m.pose(&PoseInput { time_ms: 5000, walk_position: 3.0, walk_speed: 1.0, age: 77.0, sneaking: true, ..standing() });
    assert_eq!(m.pose_arm(false), still);
}

#[test]
fn first_person_arm_skips_hidden_cubes() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = reggie_file().data;
    raw.legacy_anim = None;
    raw.cubes.iter_mut().find(|c| c.id == 29).unwrap().hidden = true;
    let arm = pose_arm(&Model::build(raw).unwrap(), false);
    assert_eq!(arm.iter().map(|b| b.source).collect::<Vec<_>>(), [BoxSource::Cube(28)]);
}

#[test]
fn first_person_kept_vanilla_arm_lands_on_the_vanilla_arm() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Every part kept, no cubes, no part positions: the arm is exactly vanilla's right_arm
    // (offset (-5, 2, 0), box (-3, -2, -2) 4x12x4) and its sleeve, and nothing else.
    let mut raw = reggie_file().data;
    raw.cubes.clear();
    raw.effects.clear();
    raw.legacy_anim = None;
    raw.part_pos.clear();
    raw.keep_vanilla = [true; 6];
    raw.texture.as_mut().unwrap().declared = [64, 64];
    let model = Model::build(raw).unwrap();
    for (left, bone, pivot, lo) in [(false, 3, [-5.0, 2.0, 0.0], -3.0), (true, 2, [5.0, 2.0, 0.0], -1.0)] {
        let arm = pose_arm(&model, left);
        let ids: Vec<BoxSource> = arm.iter().map(|b| b.source).collect();
        assert_eq!(ids, [BoxSource::Vanilla(bone), BoxSource::VanillaLayer(bone)]);
        assert_eq!(local_bounds(&arm[0]), (Vec3::new(lo, -2.0, -2.0), Vec3::new(lo + 4.0, 10.0, 2.0)));
        for b in &arm {
            assert_placed(b, pivot, left);
        }
    }
    // Vanilla's right arm UVs (40, 16): north (44, 20)..(48, 32).
    assert_eq!(pose_arm(&model, false)[0].faces[FACE_NORTH].unwrap().uv, [44.0, 20.0, 48.0, 32.0]);
}

// ---------------------------------------------------------------------------------------------
// COPY_TRANSFORM (CopyTransform.java)

fn varint(mut v: u32, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}
fn copy_fx(from: u32, to: u32, copy: u32, multiply: Option<f32>) -> RawEffect {
    let mut p = Vec::new();
    varint(from, &mut p);
    varint(to, &mut p);
    varint(copy | if multiply.is_some() { 1 << 14 } else { 0 }, &mut p);
    if let Some(m) = multiply {
        p.extend_from_slice(&m.to_be_bytes());
    }
    RawEffect { kind: 14, payload: p }
}
fn cube_at(id: u32, pos: [f32; 3], rot: [f32; 3], scale: [f32; 3]) -> RawCube {
    RawCube { pos, rotation: rot, scale, ..raw_cube(id, 1) }
}
fn near_mat(a: Mat4, b: Mat4, eps: f32) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < eps)
}

#[test]
fn copy_transform_decodes() {
    use cpm_core::effects::{decode_effects, Effect, TransformCopy};
    let e = decode_effects(&[copy_fx(10, 11, 0x80F, Some(2.0)), copy_fx(3, 12, 0x9, None)]);
    assert_eq!(e[0], Effect::CopyTransform(TransformCopy { from: 10, to: 11, copy: 0x480F, multiply: 2.0 }));
    assert_eq!(e[1], Effect::CopyTransform(TransformCopy { from: 3, to: 12, copy: 0x9, multiply: 1.0 }));
}

#[test]
fn copy_transform_set_additive_and_multiply() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    const PX: u32 = 1;
    const PY: u32 = 2;
    const PZ: u32 = 4;
    const RX: u32 = 8;
    const SX: u32 = 1 << 6;
    const SY: u32 = 1 << 7;
    const SZ: u32 = 1 << 8;
    const ADD: u32 = 1 << 11;
    let src = cube_at(10, [1.0, 2.0, 3.0], [10.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
    let base = |id| cube_at(id, [5.0, 6.0, 7.0], [0.0, 20.0, 0.0], [3.0, 3.0, 3.0]);
    let copied = synthetic(
        vec![src.clone(), base(11), base(12), base(13)],
        vec![
            // Set: copied axes take the source's value, the rest keep their own.
            copy_fx(10, 11, PX | RX, None),
            // Additive, times 2.
            copy_fx(10, 12, PX | PY | PZ | RX | ADD, Some(2.0)),
            // Additive scale multiplies.
            copy_fx(10, 13, SX | SY | SZ | ADD, None),
        ],
    );
    let expected = synthetic(
        vec![
            src,
            cube_at(11, [1.0, 6.0, 7.0], [10.0, 20.0, 0.0], [3.0, 3.0, 3.0]),
            cube_at(12, [7.0, 10.0, 13.0], [20.0, 20.0, 0.0], [3.0, 3.0, 3.0]),
            cube_at(13, [5.0, 6.0, 7.0], [0.0, 20.0, 0.0], [6.0, 3.0, 3.0]),
        ],
        vec![],
    );
    let a = pose_model(&copied, &Frame::default(), &standing());
    let b = pose_model(&expected, &Frame::default(), &standing());
    for id in [10, 11, 12, 13] {
        let (x, y) = (by_cube(&a, id).unwrap(), by_cube(&b, id).unwrap());
        assert!(near_mat(x.transform, y.transform, 1e-5), "cube {id}: {:?} vs {:?}", x.transform, y.transform);
    }
}

#[test]
fn copy_transform_follows_animated_source_and_hides() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    const PX: u32 = 1;
    const VIS: u32 = 1 << 9;
    let model = synthetic(
        vec![cube_at(10, [0.0; 3], [0.0; 3], [1.0; 3]), cube_at(11, [0.0; 3], [0.0; 3], [1.0; 3]), raw_cube(12, 11)],
        vec![copy_fx(10, 11, PX | VIS, None)],
    );
    let mut frame = Frame::default();
    frame.parts.insert(10, PartDelta { pos: Some([4.0, 0.0, 0.0]), ..Default::default() });
    let boxes = pose_model(&model, &frame, &standing());
    let (a, b) = (by_cube(&boxes, 10).unwrap(), by_cube(&boxes, 11).unwrap());
    assert!(near_mat(a.transform, b.transform, 1e-5));
    frame.parts.get_mut(&10).unwrap().visible = Some(false);
    let boxes = pose_model(&model, &frame, &standing());
    // The hidden source hides the target and its children.
    assert!(boxes.is_empty(), "{:?}", boxes.iter().map(|b| b.source).collect::<Vec<_>>());
}

#[test]
fn copy_transform_from_a_root() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Additive position copy from the left arm bone (pivot 5,2,0 plus its part position).
    let model = synthetic(vec![cube_at(10, [0.0; 3], [0.0; 3], [1.0; 3])], vec![copy_fx(2, 10, 7 | (1 << 11), None)]);
    let plain = synthetic(vec![cube_at(10, [0.0; 3], [0.0; 3], [1.0; 3])], vec![]);
    let a = by_cube(&pose_model(&model, &Frame::default(), &standing()), 10).unwrap().transform;
    let b = by_cube(&pose_model(&plain, &Frame::default(), &standing()), 10).unwrap().transform;
    let shift = a.transform_point3(Vec3::ZERO) - b.transform_point3(Vec3::ZERO);
    // Arm pivot in pixels, through the root transform (1/16 block, x/y flipped, 15/16 player scale).
    let p = model.bones[2].pivot;
    let s = 0.9375 / 16.0;
    let expect = Mat4::from_rotation_y(std::f32::consts::PI)
        .transform_vector3(Vec3::new(-p[0] * s, -p[1] * s, p[2] * s));
    assert!((shift - expect).length() < 1e-4, "{shift:?} vs {expect:?}");
}

#[test]
fn copy_transform_attaches_rainwing_head() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    use cpm_core::export::{element_ids, export_model};
    use cpm_core::project::{read_project, ProjectElement};
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/rainwing-old.cpmproject")).unwrap();
    let p = read_project(&bytes).unwrap();
    fn find<'a>(es: &'a [ProjectElement], path: &[&str]) -> &'a ProjectElement {
        let e = es.iter().find(|e| e.name == path[0]).unwrap_or_else(|| panic!("no {}", path[0]));
        if path.len() == 1 { e } else { find(&e.children, &path[1..]) }
    }
    let root = |part: usize| &p.roots.iter().find(|r| !r.duplicated && r.store_id == part as i64).unwrap().children;
    let ids = element_ids(&p);
    let id = |e: &ProjectElement| ids[&e.store_id][0];
    let neck = find(root(0), &["Pelvis", "Waist", "Chest", "Neck 1"]);
    let chest = find(root(1), &["Pelvis", "Waist", "Chest"]);
    let pelvis = find(root(1), &["Pelvis"]);
    let mut m = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    assert!(m.model.copies.len() >= 3, "{:?}", m.model.copies);
    let node = &m.model.cubes[m.model.node_index(id(neck)).unwrap()];
    let local = Mat4::from_translation(Vec3::from_array(node.pos))
        * Mat4::from_rotation_z(node.rotation[2].to_radians())
        * Mat4::from_rotation_y(node.rotation[1].to_radians())
        * Mat4::from_rotation_x(node.rotation[0].to_radians())
        * Mat4::from_scale(Vec3::from_array(node.render_scale.map(|s| s.max(0.01))));
    // Rest pose, no animation: the head chain (Pelvis / Waist / Chest copying the body's) puts
    // the neck exactly where it would sit under the body's chest.
    let rest = pose_model(&m.model, &Frame::default(), &PoseInput::default());
    let c = by_cube(&rest, id(chest)).expect("body chest drawn").transform;
    let n = by_cube(&rest, id(neck)).expect("neck drawn").transform;
    assert!(near_mat(n, c * local, 1e-3), "neck {n:?}\nexpected {:?}", c * local);
    // The body pelvis is drawn (the source of the first copy).
    assert!(by_cube(&rest, id(pelvis)).is_some());
    // Without the copies the neck sits at the head root, detached (the reported bug).
    let mut detached = m.model.clone();
    detached.copies.clear();
    let bare = pose_model(&detached, &Frame::default(), &PoseInput::default());
    let n0 = by_cube(&bare, id(neck)).unwrap().transform;
    assert!(!near_mat(n0, c * local, 1e-3));
    // Animated pose: same relation holds (the copies read the animated body chain).
    let posed = m.pose(&PoseInput::default());
    let c = by_cube(&posed, id(chest)).unwrap().transform;
    let n = by_cube(&posed, id(neck)).unwrap().transform;
    assert!(near_mat(n, c * local, 1e-3), "animated: neck {n:?}
expected {:?}", c * local);
}

#[test]
fn icon_decodes_reggie() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let file = reggie_file();
    let (w, h, rgba) = cpm_core::mesh::icon_rgba(&file).expect("Reggie has an icon");
    assert!(w > 0 && h > 0);
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    // Not all transparent.
    assert!(rgba.chunks_exact(4).any(|p| p[3] > 0));
    // No icon block: none.
    let empty = ModelFile { icon_png: Vec::new(), ..reggie_file() };
    assert!(cpm_core::mesh::icon_rgba(&empty).is_none());
    let junk = ModelFile { icon_png: b"not a png".to_vec(), ..reggie_file() };
    assert!(cpm_core::mesh::icon_rgba(&junk).is_none());
}

#[test]
fn crawling_lies_flat_whatever_the_pitch() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // AvatarRenderer.setupRotations: rotX(isInWater ? -90 - xRot : -90). Out of water (crawling in
    // a low gap) the body does not tilt with the view; only the head follows the pitch.
    let mut m = load(&sample("ReggieTheRat")).unwrap();
    let crawl = |pitch: f32, in_water: bool| PoseInput { swimming: true, crawling: !in_water, in_water, pitch, ..standing() };
    let same = |a: &[PosedBox], b: &[PosedBox]| {
        a.iter().zip(b).filter(|(x, y)| x.transform.abs_diff_eq(y.transform, 1e-5)).count()
    };
    let (flat, up) = (m.pose(&crawl(0.0, false)), m.pose(&crawl(-60.0, false)));
    assert!(same(&flat, &up) > flat.len() / 2, "the body moved with the pitch");
    // Swimming in water the whole body follows the pitch.
    let (flat, up) = (m.pose(&crawl(0.0, true)), m.pose(&crawl(-60.0, true)));
    assert_eq!(same(&flat, &up), 0);
}
