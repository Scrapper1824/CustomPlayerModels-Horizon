// A player head showing the model's head (`AnimationMode.SKULL`): the head subtree only, posed by
// the SKULL_RENDER and GLOBAL poses, at the skull part's origin.
mod common;

use cpm_core::anim::engine::{Engine, Frame, PartDelta, PoseInput};
use cpm_core::anim::{AnimSet, Clip, Interp, Pose, Target, CHANNELS, GROUPS};
use cpm_core::format::{read_cpmmodel, PartPos, RawCube, RawModel};
use cpm_core::mesh::{load, pose_model, pose_skull, BoxSource, PosedBox, FACE_NORTH};
use cpm_core::model::Model;
use glam::Vec3;

const SAMPLES: [&str; 3] = ["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"];

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}.cpmmodel", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Reggie's header (skin type, texture) with the given cubes, no part positions, effects or
/// animations, and the vanilla parts as `keep`.
fn raw(cubes: Vec<RawCube>, keep: bool) -> RawModel {
    let mut raw: RawModel = read_cpmmodel(&sample("ReggieTheRat")).unwrap().data;
    raw.cubes = cubes;
    raw.effects.clear();
    raw.part_pos.clear();
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [keep; 6];
    raw.texture.as_mut().unwrap().declared = [64, 64];
    raw
}

fn cube(id: u32, parent: u32, pos: [f32; 3]) -> RawCube {
    RawCube {
        id,
        parent,
        size: [2.0, 2.0, 2.0],
        pos,
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

fn corners(b: &PosedBox) -> Vec<Vec3> {
    b.faces.iter().flatten().flat_map(|f| f.corners.map(|c| b.transform.transform_point3(Vec3::from_array(c)))).collect()
}

fn bounds(b: &PosedBox) -> (Vec3, Vec3) {
    let pts = corners(b);
    let lo = pts.iter().fold(Vec3::splat(f32::MAX), |a, p| a.min(*p));
    let hi = pts.iter().fold(Vec3::splat(f32::MIN), |a, p| a.max(*p));
    (lo, hi)
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() < 1e-5
}

fn by_cube(boxes: &[PosedBox], id: u32) -> &PosedBox {
    boxes.iter().find(|b| b.source == BoxSource::Cube(id)).unwrap_or_else(|| panic!("cube {id}"))
}

/// Cubes 10 (on the head), 11 (on the body), 12 (on cube 10) and 13 (on the right arm).
fn four_cubes() -> Vec<RawCube> {
    vec![cube(10, 0, [1.0, -3.0, 0.0]), cube(11, 1, [0.0; 3]), cube(12, 10, [0.0, -2.0, 0.0]), cube(13, 3, [0.0; 3])]
}

#[test]
fn skull_pose_head_subtree_only() {
    if common::missing(&SAMPLES) {
        return;
    }
    let model = Model::build(raw(four_cubes(), false)).unwrap();
    let skull = pose_skull(&model, &Frame::default());
    let ids: Vec<BoxSource> = skull.iter().map(|b| b.source).collect();
    assert_eq!(ids, [BoxSource::Cube(10), BoxSource::Cube(12)]);
    // The player model has all four.
    assert_eq!(pose_model(&model, &Frame::default(), &PoseInput { on_ground: true, ..Default::default() }).len(), 4);
    // With the vanilla parts kept: the head and its hat, nothing of the body or limbs.
    let model = Model::build(raw(four_cubes(), true)).unwrap();
    let ids: Vec<BoxSource> = pose_skull(&model, &Frame::default()).iter().map(|b| b.source).collect();
    assert_eq!(ids, [BoxSource::Vanilla(0), BoxSource::VanillaLayer(0), BoxSource::Cube(10), BoxSource::Cube(12)]);
    // Every sample model's skull is finite and only has head boxes.
    for name in ["ReggieTheRat", "SkullDog"] {
        let m = load(&sample(name)).unwrap();
        for b in m.pose_skull(0) {
            assert!(matches!(b.source, BoxSource::Cube(_) | BoxSource::Vanilla(0) | BoxSource::VanillaLayer(0)), "{name} {:?}", b.source);
            assert!(corners(&b).iter().all(|p| p.is_finite()));
        }
    }
}

const ROT_X: [bool; GROUPS] = [false, true, false, false, false];

/// A clip turning element `id` about x by `deg`, constantly.
fn turn(id: u32, target: Target, deg: f32, priority: i8) -> Clip {
    let mut ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0.0]);
    ch[3] = vec![deg];
    Clip {
        components: vec![id],
        targets: vec![target],
        frames: 1,
        duration_ms: 1000,
        priority,
        additive: false,
        must_finish: false,
        interp: Interp::NoInterpolate,
        dynamic: false,
        channels: vec![ch],
        show: vec![vec![true]],
        present: vec![ROT_X],
    }
}

fn rot_x(f: &Frame, id: u32) -> Option<f32> {
    f.parts.get(&id).and_then(|p| p.rot).map(|r| r[0])
}

#[test]
fn skull_render_pose_applied() {
    // The engine: SKULL_RENDER (11) and GLOBAL (12) play, by priority; the player's poses do not.
    let set = AnimSet {
        poses: vec![
            (Pose::Standing, turn(10, Target::Cube(0), 30.0, 5)),
            (Pose::Other(11), turn(10, Target::Cube(0), 45.0, 0)),
            (Pose::Other(12), turn(12, Target::Cube(1), 20.0, 0)),
        ],
        ..Default::default()
    };
    let mut engine = Engine::new(set);
    let skull = engine.sample_skull(250);
    assert_eq!(rot_x(&skull, 10), Some(45.0));
    assert_eq!(rot_x(&skull, 12), Some(20.0));
    // The player's frame is unchanged by it, and has its standing pose.
    let player = engine.sample(&PoseInput { time_ms: 250, on_ground: true, ..Default::default() });
    assert_eq!(rot_x(&player, 10), Some(30.0));
    assert_eq!(engine.sample_skull(250), skull);
    // A higher priority SKULL_RENDER clip wins over GLOBAL on the same part.
    let set = AnimSet {
        poses: vec![(Pose::Other(12), turn(10, Target::Cube(0), 10.0, 0)), (Pose::Other(11), turn(10, Target::Cube(0), 60.0, 1))],
        ..Default::default()
    };
    assert_eq!(rot_x(&Engine::new(set).sample_skull(0), 10), Some(60.0));

    if common::missing(&SAMPLES) {
        return;
    }
    // The mesh: the frame moves the head's cubes, but not the head root (skipTransform).
    let model = Model::build(raw(four_cubes(), true)).unwrap();
    let still = pose_skull(&model, &Frame::default());
    let mut frame = Frame::default();
    frame.parts.insert(0, PartDelta { rot: Some([0.0, 90.0, 0.0]), pos: Some([3.0, 3.0, 3.0]), ..Default::default() });
    assert_eq!(pose_skull(&model, &frame), still);
    frame.parts.insert(10, PartDelta { rot: Some([90.0, 0.0, 0.0]), ..Default::default() });
    let turned = pose_skull(&model, &frame);
    let (a, b) = (by_cube(&still, 10), by_cube(&turned, 10));
    assert!(!near(bounds(a).0, bounds(b).0) || !near(bounds(a).1, bounds(b).1), "cube 10 did not turn");
    // Its child turns with it; the vanilla head does not move.
    assert_ne!(by_cube(&still, 12).transform, by_cube(&turned, 12).transform);
    assert_eq!(still[0], turned[0]);
}

#[test]
fn skull_placement_matches_part_values() {
    if common::missing(&SAMPLES) {
        return;
    }
    // The head moved by a part position (PLAYER_PARTPOS) and its pivot: the skull ignores both.
    let mut moved = raw(four_cubes(), true);
    moved.part_pos.push(PartPos { part: 0, pos: [2.0, -5.0, 1.0], rot: [0.0, 30.0, 0.0] });
    for raw in [raw(four_cubes(), true), moved] {
        let model = Model::build(raw).unwrap();
        let skull = pose_skull(&model, &Frame::default());
        // PlayerPartValues.HEAD: offset (-4, -8, -4), 8 a side, in blocks at the part's origin
        // (PLAYER_SKULL, Y down: half a block wide, half a block up from the part).
        let head = &skull[0];
        assert_eq!(head.source, BoxSource::Vanilla(0));
        let (lo, hi) = bounds(head);
        assert!(near(lo, Vec3::new(-0.25, -0.5, -0.25)) && near(hi, Vec3::new(0.25, 0.0, 0.25)), "{lo} {hi}");
        assert_eq!(head.faces[FACE_NORTH].unwrap().uv, [8.0, 8.0, 16.0, 16.0]);
        // The skull's hat: (u2, v2) = (32, 0), inflated 0.25 (SkullModel), not the player's 0.5.
        let hat = &skull[1];
        assert_eq!(hat.source, BoxSource::VanillaLayer(0));
        let (lo, hi) = bounds(hat);
        let d = 0.25 / 16.0;
        assert!(near(lo, Vec3::new(-0.25 - d, -0.5 - d, -0.25 - d)) && near(hi, Vec3::new(0.25 + d, d, 0.25 + d)), "{lo} {hi}");
        assert_eq!(hat.faces[FACE_NORTH].unwrap().uv, [40.0, 8.0, 48.0, 16.0]);
        // Cube 10 at its own position from the part's origin: (1, -3, 0) px, 2 a side.
        let (lo, hi) = bounds(by_cube(&skull, 10));
        assert!(near(lo, Vec3::new(1.0, -3.0, 0.0) / 16.0) && near(hi, Vec3::new(3.0, -1.0, 2.0) / 16.0), "{lo} {hi}");
    }
}
