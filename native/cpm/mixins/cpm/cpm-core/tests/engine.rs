mod common;

use cpm_core::anim::engine::{Engine, Frame, PoseInput};
use cpm_core::anim::{decode_legacy, AnimSet, Clip, Gesture, GestureKind, Interp, Pose, Target, CHANNELS, GROUPS};
use cpm_core::format::read_cpmmodel;
use cpm_core::model::Model;

const CUBE: u32 = 10;
const ROT: [bool; GROUPS] = [false, true, false, false, false];

/// One component, rotation-x keyframes only (every other group absent).
fn rot_clip(id: u32, target: Target, rx: &[f32], dur: u32, prio: i8, additive: bool, interp: Interp) -> Clip {
    let n = rx.len();
    let mut ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0.0; n]);
    ch[3] = rx.to_vec();
    Clip {
        components: vec![id],
        targets: vec![target],
        frames: n as u32,
        duration_ms: dur,
        priority: prio,
        additive,
        must_finish: false,
        interp,
        dynamic: false,
        channels: vec![ch],
        show: vec![vec![true; n]],
        present: vec![ROT],
    }
}

/// Constant rotation-x value on [`CUBE`].
fn const_clip(v: f32, prio: i8) -> Clip {
    rot_clip(CUBE, Target::Cube(0), &[v], 1000, prio, false, Interp::NoInterpolate)
}

fn gesture(name: &str, kind: GestureKind, looping: bool, must_finish: bool, clips: Vec<Clip>) -> Gesture {
    Gesture {
        name: name.into(),
        kind,
        looping,
        must_finish,
        layer: matches!(kind, GestureKind::Layer | GestureKind::Value),
        default_value: 0,
        clips,
        ..Default::default()
    }
}

fn at(t: u64) -> PoseInput {
    PoseInput { time_ms: t, on_ground: true, ..Default::default() }
}

fn rot_x(f: &Frame, id: u32) -> Option<f32> {
    f.parts.get(&id).and_then(|p| p.rot).map(|r| r[0])
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn standing_samples_frame0() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(CUBE, Target::Cube(0), &[10.0, 20.0], 1000, 0, false, Interp::LinearLoop))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    let f = e.sample(&at(0));
    let p = &f.parts[&CUBE];
    assert_eq!(p.rot, Some([10.0, 0.0, 0.0]));
    // Absent groups stay untouched.
    assert_eq!((p.pos, p.scale, p.color, p.visible), (None, None, None, None));
    assert!(!p.additive);
}

#[test]
fn linear_midpoint() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(CUBE, Target::Cube(0), &[0.0, 20.0], 1000, 0, false, Interp::LinearSingle))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.sample(&at(0));
    let v = rot_x(&e.sample(&at(500)), CUBE).unwrap();
    assert!(near(v, 10.0), "{v}");
}

#[test]
fn loops() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(CUBE, Target::Cube(0), &[0.0, 20.0, 5.0], 1000, 0, false, Interp::PolyLoop))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.sample(&at(0));
    let a = rot_x(&e.sample(&at(500)), CUBE).unwrap();
    let b = rot_x(&e.sample(&at(1500)), CUBE).unwrap();
    assert!(near(a, b), "{a} vs {b}");
    assert!(!near(a, 0.0));
}

#[test]
fn pose_priority() {
    let mut set = AnimSet {
        poses: vec![
            (Pose::Standing, const_clip(1.0, 0)),
            (Pose::Walking, const_clip(2.0, 0)),
            (Pose::Sneaking, const_clip(3.0, 0)),
            (Pose::SneakWalk, const_clip(4.0, 0)),
        ],
        gestures: vec![],
        ..Default::default()
    };
    let input = PoseInput { sneaking: true, walk_speed: 0.5, ..at(0) };
    assert_eq!(rot_x(&Engine::new(set.clone()).sample(&input), CUBE), Some(4.0));
    set.poses.pop();
    assert_eq!(rot_x(&Engine::new(set.clone()).sample(&input), CUBE), Some(3.0));
    // Plain walking and standing.
    let walk = PoseInput { walk_speed: 0.5, ..at(0) };
    assert_eq!(rot_x(&Engine::new(set.clone()).sample(&walk), CUBE), Some(2.0));
    assert_eq!(rot_x(&Engine::new(set).sample(&at(0)), CUBE), Some(1.0));
}

#[test]
fn gesture_overrides_pose() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(10.0, 0))],
        gestures: vec![gesture("wave", GestureKind::Gesture, true, false, vec![const_clip(50.0, 1)])],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    assert_eq!(e.gestures().len(), 1);
    assert_eq!(rot_x(&e.sample(&at(0)), CUBE), Some(10.0));
    e.play_gesture(0, 0);
    assert_eq!(rot_x(&e.sample(&at(16)), CUBE), Some(50.0));
    e.stop_gesture();
    assert_eq!(rot_x(&e.sample(&at(32)), CUBE), Some(10.0));
}

#[test]
fn gesture_wins_priority_tie() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(10.0, 0)), (Pose::Walking, const_clip(20.0, 0))],
        gestures: vec![gesture("wave", GestureKind::Gesture, true, false, vec![const_clip(50.0, 0)])],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.play_gesture(0, 0);
    assert_eq!(rot_x(&e.sample(&at(0)), CUBE), Some(50.0));
    // A pose that starts after the gesture still loses the tie.
    let walk = PoseInput { walk_speed: 0.5, ..at(100) };
    assert_eq!(rot_x(&e.sample(&walk), CUBE), Some(50.0));
}

#[test]
fn gesture_ends() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(10.0, 0))],
        gestures: vec![gesture("bow", GestureKind::Gesture, false, false, vec![const_clip(50.0, 1)])],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.play_gesture(0, 0);
    assert_eq!(rot_x(&e.sample(&at(0)), CUBE), Some(50.0));
    assert_eq!(rot_x(&e.sample(&at(900)), CUBE), Some(50.0));
    assert_eq!(rot_x(&e.sample(&at(1100)), CUBE), Some(10.0));
    assert_eq!(rot_x(&e.sample(&at(2500)), CUBE), Some(10.0));
    // It can be played again and starts over.
    e.play_gesture(0, 3000);
    assert_eq!(rot_x(&e.sample(&at(3000)), CUBE), Some(50.0));
}

#[test]
fn must_finish_gesture_plays_to_cycle_end() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(10.0, 0))],
        gestures: vec![gesture("spin", GestureKind::Gesture, true, true, vec![const_clip(50.0, 1)])],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.play_gesture(0, 0);
    e.sample(&at(0));
    e.sample(&at(1200));
    e.stop_gesture();
    // Still inside the second cycle: keeps playing until 2000.
    assert_eq!(rot_x(&e.sample(&at(1500)), CUBE), Some(50.0));
    assert_eq!(rot_x(&e.sample(&at(2100)), CUBE), Some(10.0));
}

#[test]
fn additive_adds_on_top() {
    let set = AnimSet {
        poses: vec![
            (Pose::Standing, const_clip(10.0, 0)),
            (Pose::Other(12), rot_clip(CUBE, Target::Cube(0), &[5.0], 1000, 1, true, Interp::NoInterpolate)),
        ],
        gestures: vec![],
        ..Default::default()
    };
    let f = Engine::new(set).sample(&at(0));
    let p = &f.parts[&CUBE];
    assert_eq!(p.rot, Some([15.0, 0.0, 0.0]));
    // Set first, then added: the result is absolute.
    assert!(!p.additive && !p.rot_additive);
}

#[test]
fn additive_only_is_relative() {
    let set = AnimSet {
        poses: vec![(Pose::Other(12), rot_clip(CUBE, Target::Cube(0), &[5.0], 1000, 0, true, Interp::NoInterpolate))],
        gestures: vec![],
        ..Default::default()
    };
    let f = Engine::new(set).sample(&at(0));
    let p = &f.parts[&CUBE];
    assert_eq!(p.rot, Some([5.0, 0.0, 0.0]));
    assert!(p.additive && p.rot_additive);
}

#[test]
fn missing_target_is_skipped() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(999, Target::Missing, &[5.0], 1000, 0, false, Interp::NoInterpolate))],
        gestures: vec![],
        ..Default::default()
    };
    let f = Engine::new(set).sample(&at(0));
    assert!(f.parts.is_empty());
}

#[test]
fn bones_ignore_colour_and_visibility() {
    let mut c = rot_clip(0, Target::Bone(0), &[5.0], 1000, 0, false, Interp::NoInterpolate);
    c.present = vec![[true; GROUPS]];
    let set = AnimSet { poses: vec![(Pose::Standing, c)], gestures: vec![], ..Default::default() };
    let f = Engine::new(set).sample(&at(0));
    let p = &f.parts[&0];
    assert_eq!(p.rot, Some([5.0, 0.0, 0.0]));
    assert!(p.pos.is_some());
    assert_eq!((p.color, p.visible, p.scale), (None, None, None));
}

#[test]
fn layer_toggles() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(10.0, 0))],
        gestures: vec![gesture("hat", GestureKind::Layer, true, false, vec![const_clip(70.0, 1)])],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    assert_eq!(rot_x(&e.sample(&at(0)), CUBE), Some(10.0));
    e.play_gesture(0, 10);
    assert_eq!(rot_x(&e.sample(&at(20)), CUBE), Some(70.0));
    e.play_gesture(0, 30);
    assert_eq!(rot_x(&e.sample(&at(40)), CUBE), Some(10.0));
}

#[test]
fn head_pitch_drives_dynamic_clip() {
    // HEAD_ROTATION_PITCH (66): progress = (pitch + 90) / 180.
    let mut c = rot_clip(CUBE, Target::Cube(0), &[0.0, 90.0], 1001, 0, false, Interp::LinearSingle);
    c.dynamic = true;
    let set = AnimSet { poses: vec![(Pose::Other(66), c)], gestures: vec![], ..Default::default() };
    let mut e = Engine::new(set);
    let down = rot_x(&e.sample(&PoseInput { pitch: -90.0, ..at(0) }), CUBE).unwrap();
    let mid = rot_x(&e.sample(&PoseInput { pitch: 0.0, ..at(5000) }), CUBE).unwrap();
    assert!(near(down, 0.0), "{down}");
    // millis 500 -> step 500/1001*2 -> linear single v = step/2 -> 45 * 1000/1001.
    assert!((mid - 45.0 * 1000.0 / 1001.0).abs() < 0.05, "{mid}");
}

fn reggie() -> AnimSet {
    let b = std::fs::read(format!("{}/tests/samples/ReggieTheRat.cpmmodel", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let raw = read_cpmmodel(&b).unwrap().data;
    let anim = raw.legacy_anim.clone().unwrap();
    let m = Model::build(raw).unwrap();
    decode_legacy(&anim, &m).unwrap()
}

/// Reggie's WALKING clip: additive rotation 14.9966 degrees about x on cube 21, only while walking.
#[test]
fn reggie_walk_rotates_cube_21() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let walk = PoseInput { walk_speed: 0.6, ..at(0) };
    let f = Engine::new(reggie()).sample(&walk);
    let p = &f.parts[&21];
    let r = p.rot.unwrap();
    assert!((r[0] - 14.9966).abs() < 0.002 && r[1] == 0.0 && r[2] == 0.0, "{r:?}");
    assert!(p.rot_additive);
    // Standing: no clip drives cube 21.
    assert!(Engine::new(reggie()).sample(&at(0)).parts.get(&21).is_none());
}

/// The GLOBAL clip (cube 22, 5000 ms PolyLoop) animates the tail over time.
#[test]
fn reggie_global_animates_tail() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let mut e = Engine::new(reggie());
    let walk = |t| PoseInput { walk_speed: 0.6, walk_position: t as f32 / 100.0, ..at(t) };
    let a = e.sample(&walk(0));
    let b = e.sample(&walk(1000));
    let changed = (22..=25).any(|id| {
        let ra = a.parts.get(&id).and_then(|p| p.rot);
        let rb = b.parts.get(&id).and_then(|p| p.rot);
        ra.is_some() && rb.is_some() && ra != rb
    });
    assert!(changed, "{:?} / {:?}", a.parts, b.parts);
}

#[test]
fn poly_loop_hits_keyframes() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(CUBE, Target::Cube(0), &[0.0, 20.0, 5.0], 3000, 0, false, Interp::PolyLoop))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    for (t, want) in [(0, 0.0), (1000, 20.0), (2000, 5.0), (3000, 0.0)] {
        let v = rot_x(&e.sample(&at(t)), CUBE).unwrap();
        assert!(near(v, want), "t={t}: {v}");
    }
}

#[test]
fn rotation_takes_short_way() {
    // RotationInterpolator: 350 -> 10 passes through 360, not 180.
    let set = AnimSet {
        poses: vec![(Pose::Standing, rot_clip(CUBE, Target::Cube(0), &[350.0, 10.0], 1000, 0, false, Interp::LinearSingle))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    e.sample(&at(0));
    let v = rot_x(&e.sample(&at(500)), CUBE).unwrap();
    assert!(near(v, 360.0), "{v}");
}

fn dyn_clip(rx: &[f32]) -> Clip {
    let mut c = rot_clip(CUBE, Target::Cube(0), rx, 1001, 0, false, Interp::NoInterpolate);
    c.dynamic = true;
    c
}

#[test]
fn health_hunger_air_light_default_to_full() {
    // Frames 0..9; progress 1.0 -> millis 1000 -> step 1000/1001*10 = 9.99 -> frame 9.
    let frames: Vec<f32> = (0..10).map(|f| f as f32).collect();
    for ord in [59u8, 60, 61, 64] {
        let set = AnimSet { poses: vec![(Pose::Other(ord), dyn_clip(&frames))], gestures: vec![], ..Default::default() };
        assert_eq!(rot_x(&Engine::new(set.clone()).sample(&at(0)), CUBE), Some(9.0), "ordinal {ord}");
        let zero = PoseInput { health: Some(0.0), hunger: Some(0.0), air: Some(0.0), light: Some(0.0), ..at(0) };
        assert_eq!(rot_x(&Engine::new(set.clone()).sample(&zero), CUBE), Some(0.0), "ordinal {ord}");
        let half = PoseInput { health: Some(0.5), hunger: Some(0.5), air: Some(0.5), light: Some(0.5), ..at(0) };
        assert_eq!(rot_x(&Engine::new(set).sample(&half), CUBE), Some(4.0), "ordinal {ord}");
    }
}

#[test]
fn custom_pose_replaces_main_until_it_changes() {
    let set = AnimSet {
        poses: vec![
            (Pose::Standing, const_clip(1.0, 0)),
            (Pose::Walking, const_clip(2.0, 0)),
            (Pose::Custom("sit".into()), const_clip(7.0, 0)),
            (Pose::Custom("lie".into()), const_clip(8.0, 0)),
            (Pose::Custom("sit".into()), rot_clip(11, Target::Cube(1), &[3.0], 1000, 0, false, Interp::NoInterpolate)),
        ],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    assert_eq!(e.custom_poses(), vec!["sit".to_string(), "lie".to_string()]);
    assert_eq!(rot_x(&e.sample(&at(0)), CUBE), Some(1.0));
    e.set_custom_pose(Some(0));
    let f = e.sample(&at(10));
    assert_eq!((rot_x(&f, CUBE), rot_x(&f, 11)), (Some(7.0), Some(3.0)));
    assert_eq!(rot_x(&e.sample(&at(20)), CUBE), Some(7.0));
    // Vanilla main pose changes -> custom pose clears.
    let walk = PoseInput { walk_speed: 0.5, ..at(30) };
    assert_eq!(rot_x(&e.sample(&walk), CUBE), Some(2.0));
    assert_eq!(rot_x(&e.sample(&at(40)), CUBE), Some(1.0));
    // Selecting the active one again clears it; another index switches.
    e.set_custom_pose(Some(1));
    assert_eq!(rot_x(&e.sample(&at(50)), CUBE), Some(8.0));
    e.set_custom_pose(Some(1));
    assert_eq!(rot_x(&e.sample(&at(60)), CUBE), Some(1.0));
    e.set_custom_pose(Some(0));
    e.set_custom_pose(None);
    assert_eq!(rot_x(&e.sample(&at(70)), CUBE), Some(1.0));
}

#[test]
fn must_finish_pose_resumes_when_reentered() {
    let mut walk_clip = const_clip(20.0, 1);
    walk_clip.must_finish = true;
    let set = AnimSet { poses: vec![(Pose::Walking, walk_clip), (Pose::Standing, const_clip(10.0, 0))], gestures: vec![], ..Default::default() };
    let walk = |t| PoseInput { walk_speed: 0.5, ..at(t) };
    let mut e = Engine::new(set.clone());
    e.sample(&walk(0));
    e.sample(&walk(1100));
    // Stand mid-cycle: walking lingers (priority 1) until 2000.
    assert_eq!(rot_x(&e.sample(&at(1200)), CUBE), Some(20.0));
    // Walking again before the cycle ends: it loops on past 2000.
    e.sample(&walk(1300));
    assert_eq!(rot_x(&e.sample(&walk(2100)), CUBE), Some(20.0));
    assert_eq!(rot_x(&e.sample(&walk(5100)), CUBE), Some(20.0));
    // Lingering clip finishes on a standing sample, then walking is re-entered while the
    // finished clip is still in the list: it restarts instead of staying frozen.
    let mut e = Engine::new(set);
    e.sample(&walk(0));
    e.sample(&walk(1100));
    e.sample(&at(1200));
    assert_eq!(rot_x(&e.sample(&at(2100)), CUBE), Some(10.0));
    assert_eq!(rot_x(&e.sample(&walk(2200)), CUBE), Some(20.0));
    assert_eq!(rot_x(&e.sample(&walk(4300)), CUBE), Some(20.0));
}

#[test]
fn falling_comes_from_the_fall_distance_input() {
    use cpm_core::anim::engine::FALL_DISTANCE;
    assert_eq!(FALL_DISTANCE, 4.0);
    let set = AnimSet {
        poses: vec![
            (Pose::Standing, const_clip(1.0, 0)),
            (Pose::Falling, const_clip(5.0, 0)),
            (Pose::Riding, const_clip(6.0, 0)),
            (Pose::Swimming, const_clip(7.0, 0)),
        ],
        gestures: vec![],
        ..Default::default()
    };
    // Off the ground for many seconds (treading water, on a ladder, a long jump) without the
    // fall-distance flag: never FALLING.
    let mut e = Engine::new(set.clone());
    e.sample(&at(0));
    for t in (100..10_000).step_by(100) {
        let air = PoseInput { on_ground: false, ..at(t) };
        assert_eq!(rot_x(&e.sample(&air), CUBE), Some(1.0), "t={t}");
    }
    let swim = PoseInput { on_ground: false, swimming: true, ..at(10_000) };
    assert_eq!(rot_x(&e.sample(&swim), CUBE), Some(7.0));
    // The flag plays FALLING at once, above riding and swimming as in getMainPose.
    let mut e = Engine::new(set);
    let fall = PoseInput { on_ground: false, falling: true, ..at(0) };
    assert_eq!(rot_x(&e.sample(&fall), CUBE), Some(5.0));
    let fall_riding = PoseInput { riding: true, swimming: true, ..fall };
    assert_eq!(rot_x(&e.sample(&fall_riding), CUBE), Some(5.0));
    // Landing clears it.
    assert_eq!(rot_x(&e.sample(&at(50)), CUBE), Some(1.0));
}

#[test]
fn leaving_the_ground_plays_jumping_for_half_a_second() {
    let set = AnimSet {
        poses: vec![(Pose::Standing, const_clip(1.0, 0)), (Pose::Jumping, const_clip(3.0, 0))],
        gestures: vec![],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    assert_eq!(rot_x(&e.sample(&PoseInput { on_ground: true, ..at(0) }), CUBE), Some(1.0));
    // Java: `jumping + 500 > time` after the jump event.
    let air = |t| PoseInput { on_ground: false, ..at(t) };
    assert_eq!(rot_x(&e.sample(&air(100)), CUBE), Some(3.0));
    assert_eq!(rot_x(&e.sample(&air(550)), CUBE), Some(3.0));
    assert_eq!(rot_x(&e.sample(&air(650)), CUBE), Some(1.0));
    // Standing on the ground the whole time never jumps.
    let mut e2 = Engine::new(AnimSet {
        poses: vec![(Pose::Standing, const_clip(1.0, 0)), (Pose::Jumping, const_clip(3.0, 0))],
        gestures: vec![],
        ..Default::default()
    });
    for t in (0..2000).step_by(100) {
        assert_eq!(rot_x(&e2.sample(&PoseInput { on_ground: true, ..at(t) }), CUBE), Some(1.0));
    }
}
