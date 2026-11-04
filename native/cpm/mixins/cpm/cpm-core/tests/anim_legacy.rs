mod common;

use cpm_core::anim::{decode_legacy, AnimSet, GestureKind, Interp, Pose, Target};
use cpm_core::format::read_cpmmodel;
use cpm_core::model::Model;

fn load(name: &str) -> (Model, Vec<u8>) {
    let b = std::fs::read(format!("{}/tests/samples/{name}.cpmmodel", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let raw = read_cpmmodel(&b).unwrap().data;
    let anim = raw.legacy_anim.clone().expect("legacy anim");
    (Model::build(raw).unwrap(), anim)
}

fn decode(name: &str) -> (Model, AnimSet) {
    let (m, a) = load(name);
    let set = decode_legacy(&a, &m).unwrap();
    (m, set)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.002
}

fn all_clips(s: &AnimSet) -> Vec<&cpm_core::anim::Clip> {
    s.poses.iter().map(|p| &p.1).chain(s.gestures.iter().flat_map(|g| g.clips.iter())).collect()
}

fn check_shapes(s: &AnimSet) {
    for c in all_clips(s) {
        assert_eq!(c.components.len(), c.targets.len());
        assert_eq!(c.components.len(), c.channels.len());
        assert_eq!(c.components.len(), c.show.len());
        for ch in &c.channels {
            assert!(ch.iter().all(|v| v.len() == c.frames as usize));
        }
        assert!(c.show.iter().all(|v| v.len() == c.frames as usize));
    }
}

#[test]
fn reggie_anim_decodes() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (_, s) = decode("ReggieTheRat");
    assert!(!s.poses.is_empty());
    check_shapes(&s);
    // Legacy CPM fills every channel, so every group is driven.
    for c in all_clips(&s) {
        assert_eq!(c.present, vec![[true; 5]; c.components.len()]);
    }
}

#[test]
fn skulldog_anim_decodes() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (_, s) = decode("SkullDog");
    assert!(!s.poses.is_empty());
    check_shapes(&s);
}

// Expected values come from decoding the 142-byte ANIMATION_DATA block by hand/python
// (POSE blocks: [ordinal, id, flags] then INFO [id, count, frames, i16 duration, varint ids]):
//   id0: POSE ord 3 (RUNNING)  flags 1 (additive)  INFO 3 comps [24,21,23] frames 1 dur 1 ms
//   id1: POSE ord 2 (WALKING)  flags 1 (additive)  INFO 1 comp  [21]       frames 1 dur 1 ms
//   id2: POSE ord 12 (GLOBAL)  flags 1 (additive)  INFO 1 comp  [22]       frames 2 dur 5000 ms
// plus ENC blocks, POS/ROT blocks, INTERP 4 (NO_INTERPOLATE) for ids 0 and 1, CTRL_IDS, END.
#[test]
fn reggie_pose_kinds() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (_, s) = decode("ReggieTheRat");
    let got: Vec<_> = s
        .poses
        .iter()
        .map(|(p, c)| (p.clone(), c.components.clone(), c.frames, c.duration_ms, c.additive, c.interp, c.dynamic))
        .collect();
    assert_eq!(
        got,
        vec![
            (Pose::Running, vec![24, 21, 23], 1, 1, true, Interp::NoInterpolate, false),
            (Pose::Walking, vec![21], 1, 1, true, Interp::NoInterpolate, false),
            (Pose::Other(12), vec![22], 2, 5000, true, Interp::PolyLoop, false),
        ]
    );
    assert!(s.gestures.is_empty());
}

#[test]
fn reggie_values() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (m, s) = decode("ReggieTheRat");
    // Walking, component 21: ROT bytes 0x0aaa,0,0 -> 2730/65535*360 = 14.9966 degrees about x.
    let walk = &s.poses[1].1;
    assert!(near(walk.channels[0][3][0], 14.9966), "{}", walk.channels[0][3][0]);
    assert!(near(walk.channels[0][4][0], 0.0));
    // Running, component 24: POS y = 0x0044/682, z = 0xffbc(-68)/682.
    let run = &s.poses[0].1;
    assert!(near(run.channels[0][1][0], 68.0 / 682.0));
    assert!(near(run.channels[0][2][0], -68.0 / 682.0));
    // Additive clips: unset channels stay at zero (scale too).
    assert_eq!(run.channels[0][9][0], 0.0);
    // All Reggie components exist in the model.
    for (_, c) in &s.poses {
        for (id, t) in c.components.iter().zip(&c.targets) {
            assert_eq!(*t, Target::Cube(m.node_index(*id).expect("cube")));
        }
    }
}

#[test]
fn skulldog_poses_and_gesture() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (_, s) = decode("SkullDog");
    let kinds: Vec<_> = s.poses.iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(kinds, vec![Pose::Sneaking, Pose::Other(12), Pose::Other(66), Pose::Other(65)]);
    // 66 = HEAD_ROTATION_PITCH, 65 = HEAD_ROTATION_YAW: state driven.
    assert!(!s.poses[0].1.dynamic);
    assert!(s.poses[2].1.dynamic && s.poses[3].1.dynamic);
    assert_eq!(s.poses[2].1.duration_ms, 1001);
    assert_eq!(s.poses[2].1.interp, Interp::LinearSingle);
    assert_eq!(s.poses[2].1.frames, 3);
    // Gesture "$pre$c:" -> staged setup named "c:", zero frames, zero components.
    assert_eq!(s.gestures.len(), 1);
    let g = &s.gestures[0];
    assert_eq!((g.name.as_str(), g.kind, g.looping, g.layer), ("c:", GestureKind::Setup, false, false));
    assert_eq!(g.clips.len(), 1);
    assert_eq!(g.clips[0].frames, 0);
    assert!(g.clips[0].components.is_empty());
}

fn push_block(out: &mut Vec<u8>, ty: u8, payload: &[u8]) {
    out.push(ty);
    assert!(payload.len() < 128);
    out.push(payload.len() as u8);
    out.extend_from_slice(payload);
}

/// POSE Standing id 0, non-additive, components [999 (absent), 0 (head bone), <first cube>], no channels.
#[test]
fn unknown_component_is_skipped() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (m, _) = load("ReggieTheRat");
    let cube_id = m.cubes[0].id;
    assert!(cube_id < 128);
    let mut b = Vec::new();
    push_block(&mut b, 1, &[1, 0, 0]);
    // id 0, count 3, frames 2, duration 1000 (0x03e8), ids: 999 = e7 07, 0, cube id
    push_block(&mut b, 7, &[0, 3, 2, 0x03, 0xe8, 0xe7, 0x07, 0, cube_id as u8]);
    b.push(0);
    let s = decode_legacy(&b, &m).unwrap();
    assert_eq!(s.poses.len(), 1);
    let c = &s.poses[0].1;
    assert_eq!(c.components, vec![999, 0, cube_id]);
    assert_eq!(c.targets[0], Target::Missing);
    assert_eq!(c.targets[1], Target::Bone(0));
    assert_eq!(c.targets[2], Target::Cube(0));
    assert_eq!(c.duration_ms, 1000);
    // Missing: zeros, scale 1. Bone: own pivot / rotation.
    assert_eq!(c.channels[0][0], vec![0.0, 0.0]);
    assert_eq!(c.channels[0][9], vec![1.0, 1.0]);
    assert_eq!(c.channels[1][1], vec![m.bones[0].pivot[1]; 2]);
    assert_eq!(c.channels[2][0], vec![m.cubes[0].pos[0]; 2]);
    assert_eq!(c.channels[2][3], vec![m.cubes[0].rotation[0]; 2]);
    assert_eq!(c.show[2], vec![!m.cubes[0].hidden; 2]);
}

#[test]
fn truncated_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (m, a) = load("ReggieTheRat");
    assert!(decode_legacy(&a[..a.len() - 5], &m).is_err());
}

/// MUST_FINISH (type 20) marks the clip; clips without it are not must-finish.
#[test]
fn must_finish_sets_clip_flag() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"]) {
        return;
    }
    let (m, _) = load("ReggieTheRat");
    let mut b = Vec::new();
    push_block(&mut b, 1, &[1, 0, 0]); // POSE Standing id 0
    push_block(&mut b, 7, &[0, 0, 1, 0x03, 0xe8]); // INFO id 0, 0 comps, 1 frame, 1000 ms
    push_block(&mut b, 1, &[2, 1, 0]); // POSE Walking id 1
    push_block(&mut b, 7, &[1, 0, 1, 0x03, 0xe8]);
    push_block(&mut b, 20, &[0]); // MUST_FINISH id 0
    b.push(0);
    let s = decode_legacy(&b, &m).unwrap();
    assert!(s.poses[0].1.must_finish);
    assert!(!s.poses[1].1.must_finish);
}
