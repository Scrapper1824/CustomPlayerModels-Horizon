//! Staged animations (`StagedAnimation`): setup / finish stages attached to their pose or
//! gesture by both decoders, and played around the main stage by the engine.

mod common;

use std::sync::OnceLock;

use cpm_core::anim::engine::{Engine, Frame, PoseInput};
use cpm_core::anim::{
    decode_legacy, decode_new, AnimSet, Clip, Gesture, GestureKind, Interp, Pose, Stages, Target, CHANNELS, GROUPS,
};
use cpm_core::export::export_model;
use cpm_core::format::read_cpmmodel;
use cpm_core::project::read_project;
use cpm_core::LoadedModel;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// rainwing exported and loaded, plus its decoded animation set.
fn rainwing() -> (LoadedModel, AnimSet) {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = BYTES.get_or_init(|| export_model(&read_project(&sample("rainwing-old.cpmproject")).unwrap()).unwrap());
    let loaded = cpm_core::load(bytes).unwrap();
    let raw = read_cpmmodel(bytes).unwrap().data.new_anim.expect("ANIMATION_NEW written");
    let set = decode_new(&raw, &loaded.model).unwrap();
    (loaded, set)
}

#[test]
fn rainwing_jumping_has_its_finish_stage() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let (_, set) = rainwing();
    let st = set.pose_stages(&Pose::Jumping).expect("JUMPING is staged");
    assert!(st.setup.is_empty());
    assert_eq!(st.finish.len(), 1);
    let f = &st.finish[0];
    assert_eq!(f.duration_ms, 500);
    assert!(f.additive);
    assert_eq!(f.interp, Interp::PolySingle);
    assert!(!f.components.is_empty());
    // The main stage is still the pose's own clip.
    let jumping: Vec<_> = set.poses.iter().filter(|(p, _)| *p == Pose::Jumping).collect();
    assert_eq!(jumping.len(), 1);
    assert_eq!(jumping[0].1.duration_ms, 550);
}

fn input(t: u64, on_ground: bool) -> PoseInput {
    PoseInput { time_ms: t, on_ground, ..Default::default() }
}

/// Samples both engines every 50 ms: on the ground until 1000, airborne 1000..1300 (the jump
/// starts at 1000, so JUMPING lasts until 1500), then on the ground again.
fn jump_frames(e: &mut Engine) -> Vec<(u64, Frame)> {
    (0..=60).map(|k| k * 50).map(|t| (t, e.sample(&input(t, !(1000..1300).contains(&t))))).collect()
}

#[test]
fn rainwing_finish_plays_after_the_jump() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let (_, set) = rainwing();
    let finish = set.pose_stages(&Pose::Jumping).unwrap().finish[0].clone();
    let mut without = set.clone();
    without.pose_stages.retain(|(p, _)| *p != Pose::Jumping);
    let with = jump_frames(&mut Engine::new(set));
    let base = jump_frames(&mut Engine::new(without));
    let differs = |i: usize| finish.components.iter().any(|id| with[i].1.parts.get(id) != base[i].1.parts.get(id));
    for (i, (t, f)) in with.iter().enumerate() {
        match t {
            // Before and during the jump the finish stage waits.
            0..1500 => assert_eq!(*f, base[i].1, "t={t}"),
            // Then it plays once over the next pose for its 500 ms...
            1500..2000 => {}
            // ...and stops.
            _ => assert_eq!(*f, base[i].1, "t={t}"),
        }
    }
    let playing = (0..with.len()).filter(|&i| (1500..2000).contains(&with[i].0) && differs(i)).count();
    assert!(playing >= 8, "finish animated in {playing} of 10 frames");
    assert!(differs(with.iter().position(|(t, _)| *t == 1750).unwrap()));
}

// ---------------------------------------------------------------------------------------------
// Hand-built gesture: setup (rot 10, 300 ms) + main (rot 20) + finish (rot 30, 400 ms).

const CUBE: u32 = 10;
const ROT: [bool; GROUPS] = [false, true, false, false, false];

fn const_clip(v: f32, dur: u32) -> Clip {
    let mut ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0.0]);
    ch[3] = vec![v];
    Clip {
        components: vec![CUBE],
        targets: vec![Target::Cube(0)],
        frames: 1,
        duration_ms: dur,
        priority: 0,
        additive: false,
        must_finish: false,
        interp: Interp::NoInterpolate,
        dynamic: false,
        channels: vec![ch],
        show: vec![vec![true]],
        present: vec![ROT],
    }
}

fn staged_gesture(must_finish: bool) -> Engine {
    let mut main = const_clip(20.0, 1000);
    main.must_finish = must_finish;
    let g = Gesture {
        name: "wave".into(),
        kind: GestureKind::Gesture,
        looping: true,
        must_finish,
        clips: vec![main],
        stages: Stages { setup: vec![const_clip(10.0, 300)], finish: vec![const_clip(30.0, 400)] },
        ..Default::default()
    };
    Engine::new(AnimSet { gestures: vec![g], ..Default::default() })
}

fn rot(e: &mut Engine, t: u64) -> Option<f32> {
    e.sample(&input(t, true)).parts.get(&CUBE).and_then(|p| p.rot).map(|r| r[0])
}

#[test]
fn gesture_plays_setup_then_main_then_finish() {
    let mut e = staged_gesture(false);
    e.play_gesture(0, 0);
    let mut got = Vec::new();
    for t in (0..=1600).step_by(50) {
        if t == 1000 {
            e.stop_gesture();
        }
        got.push((t, rot(&mut e, t)));
    }
    for (t, r) in got {
        let want = match t {
            0..300 => Some(10.0),
            300..1000 => Some(20.0),
            // Finish: from the frame the gesture stops, for its 400 ms (offset = previous frame).
            1000..1350 => Some(30.0),
            _ => None,
        };
        assert_eq!(r, want, "t={t}");
    }
}

#[test]
fn must_finish_main_delays_the_finish_stage() {
    let mut e = staged_gesture(true);
    e.play_gesture(0, 0);
    let mut got = Vec::new();
    for t in (0..=1800).step_by(50) {
        if t == 1200 {
            e.stop_gesture();
        }
        got.push((t, rot(&mut e, t)));
    }
    for (t, r) in got {
        // The main stage began at 250 (the frame before setup ended): its cycle ends at 1250.
        let want = match t {
            0..300 => Some(10.0),
            300..1250 => Some(20.0),
            1250 => None,
            1300..1650 => Some(30.0),
            _ => None,
        };
        assert_eq!(r, want, "t={t}");
    }
}

#[test]
fn pose_returning_cuts_the_finish_short() {
    // Standing staged with a finish; walking in between; standing again restarts its stages.
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(20.0, 1000))],
        pose_stages: vec![(Pose::Standing, Stages { setup: vec![], finish: vec![const_clip(30.0, 400)] })],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    let walk = |t: u64| PoseInput { time_ms: t, on_ground: true, walk_speed: 1.0, ..Default::default() };
    let r = |f: Frame| f.parts.get(&CUBE).and_then(|p| p.rot).map(|r| r[0]);
    assert_eq!(r(e.sample(&input(0, true))), Some(20.0));
    assert_eq!(r(e.sample(&walk(50))), Some(30.0));
    assert_eq!(r(e.sample(&walk(100))), Some(30.0));
    assert_eq!(r(e.sample(&input(150, true))), Some(20.0));
    assert_eq!(r(e.sample(&walk(200))), Some(30.0));
    assert_eq!(r(e.sample(&walk(700))), None);
}

// ---------------------------------------------------------------------------------------------
// Legacy `$pre$` / `$post$` names.

fn push_block(out: &mut Vec<u8>, ty: u8, payload: &[u8]) {
    out.push(ty);
    assert!(payload.len() < 128);
    out.push(payload.len() as u8);
    out.extend_from_slice(payload);
}

fn gesture_block(out: &mut Vec<u8>, id: u8, name: &str, flags: u8) {
    let mut p = vec![id, name.len() as u8];
    p.extend_from_slice(name.as_bytes());
    p.push(flags);
    push_block(out, 2, &p);
}

/// INFO: no components, one frame, `ms` long.
fn info(out: &mut Vec<u8>, id: u8, ms: u16) {
    push_block(out, 7, &[id, 0, 1, (ms >> 8) as u8, ms as u8]);
}

fn reggie() -> cpm_core::model::Model {
    let raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    cpm_core::model::Model::build(raw).unwrap()
}

#[test]
fn legacy_stage_names_attach_to_their_owner() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let m = reggie();
    let mut b = Vec::new();
    push_block(&mut b, 1, &[17, 0, 1]); // POSE Jumping id 0, additive
    info(&mut b, 0, 550);
    gesture_block(&mut b, 1, "$post$p:jumping", 1);
    info(&mut b, 1, 500);
    gesture_block(&mut b, 2, "wave", 2);
    info(&mut b, 2, 1000);
    gesture_block(&mut b, 3, "$pre$g:wave", 0);
    info(&mut b, 3, 300);
    gesture_block(&mut b, 4, "$post$g:wave", 0);
    info(&mut b, 4, 400);
    // A custom pose by `p:` (no vanilla pose of that name) and by `c:`.
    let mut p = vec![0, 5, 0, 4];
    p.extend_from_slice(b"Idle");
    push_block(&mut b, 1, &p); // POSE custom "Idle" id 5
    info(&mut b, 5, 1000);
    gesture_block(&mut b, 6, "$pre$p:Idle", 0);
    info(&mut b, 6, 200);
    gesture_block(&mut b, 7, "$post$c:Idle", 0);
    info(&mut b, 7, 250);
    // Names that attach to nothing stay as unplayable stage entries.
    gesture_block(&mut b, 8, "$post$p:walking", 0);
    info(&mut b, 8, 100);
    gesture_block(&mut b, 9, "$pre$g:nope", 0);
    info(&mut b, 9, 100);
    b.push(0);
    let s = decode_legacy(&b, &m).unwrap();

    let jump = s.pose_stages(&Pose::Jumping).expect("jumping staged");
    assert!(jump.setup.is_empty());
    assert_eq!(jump.finish.iter().map(|c| c.duration_ms).collect::<Vec<_>>(), [500]);
    let idle = s.pose_stages(&Pose::Custom("Idle".into())).expect("Idle staged");
    assert_eq!(idle.setup.iter().map(|c| c.duration_ms).collect::<Vec<_>>(), [200]);
    assert_eq!(idle.finish.iter().map(|c| c.duration_ms).collect::<Vec<_>>(), [250]);
    assert!(s.pose_stages(&Pose::Walking).is_none());

    let names: Vec<_> = s.gestures.iter().map(|g| (g.name.as_str(), g.kind)).collect();
    assert_eq!(
        names,
        [("wave", GestureKind::Gesture), ("p:walking", GestureKind::Finish), ("g:nope", GestureKind::Setup)]
    );
    let wave = &s.gestures[0];
    assert_eq!(wave.clips[0].duration_ms, 1000);
    assert_eq!(wave.stages.setup.iter().map(|c| c.duration_ms).collect::<Vec<_>>(), [300]);
    assert_eq!(wave.stages.finish.iter().map(|c| c.duration_ms).collect::<Vec<_>>(), [400]);
}

// ---------------------------------------------------------------------------------------------
// Non-looping staged gestures end by themselves, then play their finish; a replay starts over.

fn one_shot(setup: bool, must_finish: bool) -> Engine {
    let mut main = const_clip(20.0, 1000);
    main.must_finish = must_finish;
    let g = Gesture {
        name: "bow".into(),
        kind: GestureKind::Gesture,
        looping: false,
        must_finish,
        clips: vec![main],
        stages: Stages {
            setup: if setup { vec![const_clip(10.0, 300)] } else { vec![] },
            finish: vec![const_clip(30.0, 400)],
        },
        ..Default::default()
    };
    Engine::new(AnimSet { gestures: vec![g], ..Default::default() })
}

#[test]
fn one_shot_staged_gestures_finish_and_replay() {
    for setup in [false, true] {
        for must_finish in [false, true] {
            let mut e = one_shot(setup, must_finish);
            for start in [0u64, 2500] {
                e.play_gesture(0, start);
                assert_eq!(e.playing_gesture(), Some(0));
                for t in (start..start + 2000).step_by(50) {
                    let d = t - start;
                    let want = match d {
                        0..300 if setup => Some(10.0),
                        // The main clip ends 1000 ms after the gesture started (Java measures a
                        // clip's end from when it was added, so a setup shortens it).
                        0..1000 => Some(20.0),
                        // The finish starts in the sample the gesture ends, no blank sample.
                        1000..1400 => Some(30.0),
                        _ => None,
                    };
                    assert_eq!(rot(&mut e, t), want, "setup={setup} must_finish={must_finish} t={t}");
                }
                assert_eq!(e.playing_gesture(), None);
            }
        }
    }
}

#[test]
fn replay_while_finishing_starts_over() {
    let mut e = staged_gesture(true);
    e.play_gesture(0, 0);
    for t in (0..=1200).step_by(50) {
        if t == 1200 {
            e.stop_gesture();
        }
        rot(&mut e, t);
    }
    // The must-finish main is still finishing (cycle ends at 1250): replaying restarts the setup.
    e.play_gesture(0, 1250);
    assert_eq!(rot(&mut e, 1250), Some(10.0));
    assert_eq!(rot(&mut e, 1500), Some(10.0));
    assert_eq!(rot(&mut e, 1550), Some(20.0));
    assert_eq!(rot(&mut e, 2500), Some(20.0));
    e.stop_gesture();
    // Stopped again: the must-finish main (begun at 1500) runs to its cycle end at 3500, then
    // the finish plays.
    assert_eq!(rot(&mut e, 2550), Some(20.0));
    assert_eq!(rot(&mut e, 3450), Some(20.0));
    assert_eq!(rot(&mut e, 3500), None);
    assert_eq!(rot(&mut e, 3550), Some(30.0));
}
