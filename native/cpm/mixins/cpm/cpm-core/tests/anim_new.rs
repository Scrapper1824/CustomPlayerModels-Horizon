//! New-format (`ANIMATION_NEW`) animation decoding.
//! The `W` writer mirrors `IOHelper.write*`; the helpers below mirror the `write` methods of
//! `ModelPartAnimationNew`, `SerializedTrigger`, `SerializedAnimation`, `ConstantTimeFloat`,
//! `ConstantTimeBool`, `AnimLoaderState.writeInfo`, `ParameterDetails` and the gesture buttons.

use cpm_core::anim::{decode_new, AnimSet, GestureKind, Interp, Pose, Target};
use cpm_core::format::{RawCube, RawModel, SkinType};
use cpm_core::io::DIV;
use cpm_core::model::Model;

use std::f32::consts::PI;

#[derive(Default, Clone)]
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
    /// `writeVarFloat`: `writeSignedVarInt((int) (f * DIV))`.
    fn var_float(&mut self, f: f32) -> &mut Self {
        self.svarint((f * DIV as f32) as i32)
    }
    fn utf(&mut self, s: &str) -> &mut Self {
        self.varint(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
        self
    }
    fn bools(&mut self, v: &[bool]) -> &mut Self {
        for chunk in v.chunks(8) {
            let mut b = 0u8;
            for (j, &x) in chunk.iter().enumerate() {
                if x {
                    b |= 1 << j;
                }
            }
            self.u8(b);
        }
        self
    }
    /// `writeNextObjectBlock(tag)`: enum ordinal byte then a varint-length block.
    fn obj(&mut self, tag: u8, inner: &W) -> &mut Self {
        self.u8(tag);
        self.varint(inner.0.len() as u32);
        self.0.extend_from_slice(&inner.0);
        self
    }
}

// TagType ordinals.
const END: u8 = 0;
const NEW_ANIM: u8 = 1;
const NEW_TRIGGER: u8 = 2;
const INIT_BUILTIN_TRIGGER: u8 = 3;
const INIT_NAMED_TRIGGER: u8 = 4;
const INIT_PARAMETER_TRIGGER: u8 = 5;
const INIT_STAGED_TRIGGER: u8 = 6;
const CONSTANT_FRAME_TIME_FLOAT: u8 = 7;
const CONSTANT_FRAME_TIME_BOOLEAN: u8 = 8;
const CUBES_TO_CHANNELS: u8 = 9;
const CONTROL_INFO: u8 = 10;
const GESTURE_BUTTON: u8 = 11;
const PARAMETERS: u8 = 12;
const INIT_STAGED_ANIM: u8 = 13;

// SerializedTrigger flags.
const LOOPING: u8 = 1 << 1;
const BITMASK: u8 = 1 << 2;
const PARAM_INTERPOLATE: u8 = 1 << 3;
const MUST_FINISH: u8 = 1 << 4;

// AnimationType ordinals.
const AT_CUSTOM_POSE: u8 = 1;
const AT_GESTURE: u8 = 2;
const AT_LAYER: u8 = 3;
const AT_VALUE_LAYER: u8 = 4;

// Channel ids within one cube's 13-channel group (CUBES_TO_CHANNELS with 12 interpolated channels).
const CH_PER_CUBE: u32 = 13;
const POS_Y: u32 = 1;
const ROT_X: u32 = 3;
const ROT_Y: u32 = 4;
const ROT_Z: u32 = 5;
const VIS: u32 = 12;

fn control_info(w: &mut W) {
    w.obj(CONTROL_INFO, W::default().u8(0).u8(0).varint(0));
}

fn end(w: &mut W) {
    w.u8(END).varint(0);
}

fn new_trigger(w: &mut W) {
    w.obj(NEW_TRIGGER, &W::default());
}

fn builtin_trigger(w: &mut W, pose: u8, flags: u8) {
    new_trigger(w);
    w.obj(INIT_BUILTIN_TRIGGER, W::default().u8(pose).u8(flags));
}

fn named_trigger(w: &mut W, anim: u8, flags: u8, value: u8) {
    new_trigger(w);
    w.obj(INIT_NAMED_TRIGGER, W::default().u8(anim).u8(flags).u8(value));
}

fn parameter_trigger(w: &mut W, anim: u8, flags: u8, parameter: u32, value: u8) {
    new_trigger(w);
    w.obj(INIT_PARAMETER_TRIGGER, W::default().u8(anim).u8(flags).varint(parameter).u8(value));
}

fn staged_trigger(w: &mut W, stage: u8, staging_id: u32, flags: u8) {
    new_trigger(w);
    w.obj(INIT_STAGED_TRIGGER, W::default().u8(stage).varint(staging_id).u8(flags));
}

fn new_anim(w: &mut W, trigger: u32, priority: i32, duration: u32) {
    w.obj(NEW_ANIM, W::default().varint(trigger).svarint(priority).varint(duration));
}

fn cubes_to_channels(w: &mut W, cubes: &[u32], additive: bool) {
    let mut d = W::default();
    d.varint(cubes.len() as u32).varint(12).varint(additive as u32);
    for &c in cubes {
        d.varint(c);
    }
    w.obj(CUBES_TO_CHANNELS, &d);
}

fn const_float(w: &mut W, interp: u8, frames: usize, chans: &[(u32, Vec<f32>)]) {
    let mut d = W::default();
    d.u8(interp).varint(frames as u32).varint(chans.len() as u32);
    for (ch, vals) in chans {
        assert_eq!(vals.len(), frames);
        d.varint(*ch);
        for &v in vals {
            d.var_float(v);
        }
    }
    w.obj(CONSTANT_FRAME_TIME_FLOAT, &d);
}

fn const_bool(w: &mut W, frames: usize, chans: &[(u32, Vec<bool>)]) {
    let mut d = W::default();
    d.varint(frames as u32).varint(chans.len() as u32);
    for (ch, vals) in chans {
        d.varint(*ch);
        d.bools(vals);
    }
    w.obj(CONSTANT_FRAME_TIME_BOOLEAN, &d);
}

/// `CustomPoseGestureButtonData` (GestureButtonType POSE = 0, GESTURE = 1), no layer ctrl.
fn pose_or_gesture_button(w: &mut W, pose: bool, name: &str, id: u8) {
    let mut d = W::default();
    d.u8(if pose { 0 } else { 1 }).utf(name).u8(0).u8(id);
    if !pose {
        d.varint(25); // gestureTimeout
    }
    w.obj(GESTURE_BUTTON, &d);
}

fn parameters(w: &mut W, sync: &[u8]) {
    let mut d = W::default();
    d.varint(sync.len() as u32);
    for &b in sync {
        d.u8(b);
    }
    d.varint(0);
    w.obj(PARAMETERS, &d);
}

fn raw_cube(id: u32, parent: u32, pos: [f32; 3], rgb: Option<u32>) -> RawCube {
    RawCube {
        id,
        parent,
        size: [1.0; 3],
        pos,
        offset: [0.0; 3],
        rotation: [0.0, 10.0, 0.0],
        tex_size: if rgb.is_some() { 0 } else { 1 },
        u: 0,
        v: 0,
        rgb: rgb.unwrap_or(0),
        mesh_scale: [1.0; 3],
        scale: [1.0; 3],
        mc_scale: 0.0,
        hidden: false,
    }
}

/// Cubes 10 (on the head, colour 0x102030) and 11 (child of 10, textured).
fn model() -> Model {
    Model::build(RawModel {
        skin_type: SkinType::Default,
        cubes: vec![raw_cube(10, 0, [1.0, 2.0, 3.0], Some(0x102030)), raw_cube(11, 10, [0.0; 3], None)],
        keep_vanilla: [true; 6],
        vanilla_anim_disabled: [false; 6],
        texture: None,
        elytra_texture: None,
        cape_texture: None,
        extra_roots: Vec::new(),
        part_pos: Vec::new(),
        effects: Vec::new(),
        legacy_anim: None,
        new_anim: None,
        scale: None,
        link: None,
    })
    .unwrap()
}

/// `(int)(f * DIV) / DIV`: the value after a var-float round trip.
fn q(f: f32) -> f32 {
    ((f * DIV as f32) as i32) as f32 / DIV as f32
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn check_shapes(s: &AnimSet) {
    let clips = s.poses.iter().map(|p| &p.1).chain(s.gestures.iter().flat_map(|g| g.clips.iter()));
    for c in clips {
        assert_eq!(c.components.len(), c.targets.len());
        assert_eq!(c.components.len(), c.channels.len());
        assert_eq!(c.components.len(), c.show.len());
        for ch in &c.channels {
            assert!(ch.iter().all(|v| v.len() == c.frames as usize));
        }
        assert!(c.show.iter().all(|v| v.len() == c.frames as usize));
    }
}

const ROT: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [0.5, -0.25, 0.1], [1.0, -0.5, 0.2], [0.5, -0.25, 0.1]];

/// One WALKING clip (must finish, priority -3, 1200 ms, LINEAR_LOOP), components cube 10 and the
/// head bone, 4 frames of rotation (radians in the file) on both.
fn walking_bytes() -> Vec<u8> {
    let mut w = W::default();
    control_info(&mut w);
    builtin_trigger(&mut w, 2, MUST_FINISH);
    new_anim(&mut w, 0, -3, 1200);
    cubes_to_channels(&mut w, &[10, 0], false);
    let axis = |a: usize, scale: f32| ROT.iter().map(|r| r[a] * scale).collect::<Vec<f32>>();
    const_float(
        &mut w,
        2,
        4,
        &[
            (ROT_X, axis(0, 1.0)),
            (ROT_Y, axis(1, 1.0)),
            (ROT_Z, axis(2, 1.0)),
            (CH_PER_CUBE + ROT_X, axis(0, -1.0)),
            (CH_PER_CUBE + ROT_Y, axis(1, -1.0)),
            (CH_PER_CUBE + ROT_Z, axis(2, -1.0)),
        ],
    );
    end(&mut w);
    w.0
}

#[test]
fn new_pose_clip_roundtrip() {
    let m = model();
    let s = decode_new(&walking_bytes(), &m).unwrap();
    check_shapes(&s);
    assert!(s.gestures.is_empty());
    assert_eq!(s.poses.len(), 1);
    let (pose, c) = &s.poses[0];
    assert_eq!(*pose, Pose::Walking);
    assert_eq!(c.components, vec![10, 0]);
    assert_eq!(c.targets, vec![Target::Cube(0), Target::Bone(0)]);
    assert_eq!(c.frames, 4);
    assert_eq!(c.duration_ms, 1200);
    assert_eq!(c.priority, -3);
    assert!(!c.additive);
    assert!(c.must_finish);
    assert!(!c.dynamic);
    assert_eq!(c.interp, Interp::LinearLoop);
    for f in 0..4 {
        for a in 0..3 {
            let want = q(ROT[f][a]).to_degrees();
            assert!(near(c.channels[0][3 + a][f], want), "cube f{f} a{a}: {} vs {want}", c.channels[0][3 + a][f]);
            let want = q(-ROT[f][a]).to_degrees();
            assert!(near(c.channels[1][3 + a][f], want), "bone f{f} a{a}");
        }
    }
    // Channels without data: non-additive, so the element's own values.
    assert_eq!(c.channels[0][0], vec![1.0; 4]);
    assert_eq!(c.channels[0][2], vec![3.0; 4]);
    assert_eq!(c.channels[0][6], vec![16.0; 4]);
    assert_eq!(c.channels[0][8], vec![48.0; 4]);
    assert_eq!(c.channels[0][9], vec![1.0; 4]);
    assert_eq!(c.channels[1][1], vec![m.bones[0].pivot[1]; 4]);
    assert_eq!(c.show, vec![vec![true; 4]; 2]);
    // Rotation only: every other group is left to the engine's defaults / lower clips.
    assert_eq!(c.present, vec![[false, true, false, false, false]; 2]);
}

#[test]
fn new_gesture_roundtrip() {
    let m = model();
    let mut w = W::default();
    control_info(&mut w);
    named_trigger(&mut w, AT_GESTURE, LOOPING, 1);
    new_anim(&mut w, 0, 0, 1000);
    cubes_to_channels(&mut w, &[11], true);
    const_float(&mut w, 0, 2, &[(POS_Y, vec![0.0, 2.0])]);
    const_bool(&mut w, 2, &[(VIS, vec![true, false])]);
    pose_or_gesture_button(&mut w, false, "wave", 1);
    end(&mut w);

    let s = decode_new(&w.0, &m).unwrap();
    check_shapes(&s);
    assert!(s.poses.is_empty());
    assert_eq!(s.gestures.len(), 1);
    let g = &s.gestures[0];
    assert_eq!((g.name.as_str(), g.kind, g.looping, g.must_finish, g.layer), ("wave", GestureKind::Gesture, true, false, false));
    assert_eq!(g.clips.len(), 1);
    let c = &g.clips[0];
    assert_eq!(c.components, vec![11]);
    assert_eq!(c.targets, vec![Target::Cube(1)]);
    assert!(c.additive);
    assert!(!c.must_finish);
    assert_eq!(c.interp, Interp::PolyLoop);
    assert_eq!((c.frames, c.duration_ms, c.priority), (2, 1000, 0));
    assert_eq!(c.channels[0][1], vec![0.0, q(2.0)]);
    // Additive: unset channels are zero, scale included.
    assert_eq!(c.channels[0][0], vec![0.0; 2]);
    assert_eq!(c.channels[0][9], vec![0.0; 2]);
    assert_eq!(c.show[0], vec![true, false]);
    assert_eq!(c.present, vec![[true, false, false, false, true]]);
}

#[test]
fn truncated_new_anim_is_error() {
    let m = model();
    let b = walking_bytes();
    for cut in [1, 3, 10, b.len() / 2] {
        assert!(decode_new(&b[..b.len() - cut], &m).is_err(), "cut {cut}");
    }
}

#[test]
fn cubes_without_frame_data_are_left_out() {
    let m = model();
    let mut w = W::default();
    builtin_trigger(&mut w, 1, 0);
    new_anim(&mut w, 0, 0, 500);
    cubes_to_channels(&mut w, &[10, 11], false);
    const_float(&mut w, 4, 1, &[(CH_PER_CUBE + POS_Y, vec![5.0])]);
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    let c = &s.poses[0].1;
    assert_eq!(s.poses[0].0, Pose::Standing);
    assert_eq!(c.components, vec![11]);
    assert_eq!(c.interp, Interp::NoInterpolate);
    assert!(!c.must_finish);
}

#[test]
fn custom_pose_via_parameter_trigger() {
    let m = model();
    let mut w = W::default();
    pose_or_gesture_button(&mut w, true, "sit", 1);
    parameter_trigger(&mut w, AT_CUSTOM_POSE, LOOPING, 0, 1);
    new_anim(&mut w, 0, 2, 800);
    cubes_to_channels(&mut w, &[10], false);
    const_float(&mut w, 2, 2, &[(POS_Y, vec![1.0, 2.0])]);
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    assert!(s.gestures.is_empty());
    assert_eq!(s.poses.len(), 1);
    assert_eq!(s.poses[0].0, Pose::Custom("sit".into()));
    assert_eq!(s.poses[0].1.priority, 2);
}

#[test]
fn layers_and_value_parameters() {
    let m = model();
    let mut w = W::default();
    // Sync params: 0, 1 reserved; 2 = bit layer byte (bit 0x80 on); 3 = dropdown (option 1); 4 = slider 128.
    parameters(&mut w, &[0, 0, 0x80, 1, 128]);
    // BOOL_PARAMETER_TOGGLE "hat": param 2, mask 0x80.
    w.obj(GESTURE_BUTTON, W::default().u8(2).utf("hat").u8(0).varint(2).u8(0x80));
    // DROPDOWN "hats": param 3, options ["", "cap"].
    w.obj(GESTURE_BUTTON, W::default().u8(4).utf("hats").u8(0).varint(3).u8(2).utf("").utf("cap"));
    // VALUE_PARAMETER_SLIDER "tail": param 4, max 255.
    w.obj(GESTURE_BUTTON, W::default().u8(3).utf("tail").u8(0).varint(4).u8(255));
    parameter_trigger(&mut w, AT_LAYER, BITMASK | LOOPING, 2, 0x80);
    parameter_trigger(&mut w, AT_LAYER, LOOPING, 3, 1);
    parameter_trigger(&mut w, AT_VALUE_LAYER, PARAM_INTERPOLATE, 4, 0);
    for t in 0..3 {
        new_anim(&mut w, t, 0, if t == 2 { 1001 } else { 1000 });
        cubes_to_channels(&mut w, &[10], false);
        const_float(&mut w, 3, 3, &[(ROT_X, vec![0.0, 0.5, 1.0])]);
    }
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    check_shapes(&s);
    let got: Vec<_> = s
        .gestures
        .iter()
        .map(|g| (g.name.as_str(), g.kind, g.looping, g.layer, g.default_value, g.clips.len()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("hat", GestureKind::Layer, true, true, 255, 1),
            ("cap", GestureKind::Layer, true, true, 255, 1),
            // Layer and value triggers always loop, whatever the LOOPING flag says.
            ("tail", GestureKind::Value, true, true, 128, 1),
        ]
    );
    assert_eq!(s.gestures[2].clips[0].duration_ms, 1001);
    assert_eq!(s.gestures[2].clips[0].interp, Interp::LinearSingle);
}

#[test]
fn staged_gesture_keeps_main_and_setup_stages() {
    let m = model();
    let mut w = W::default();
    pose_or_gesture_button(&mut w, false, "dance", 1);
    named_trigger(&mut w, AT_GESTURE, 0, 1); // 0: main trigger
    staged_trigger(&mut w, 1, 0, MUST_FINISH); // 1: PLAY of staging 0
    staged_trigger(&mut w, 0, 0, 0); // 2: SETUP of staging 0
    new_anim(&mut w, 1, 0, 1000);
    cubes_to_channels(&mut w, &[10], false);
    const_float(&mut w, 2, 2, &[(POS_Y, vec![7.0, 8.0])]);
    new_anim(&mut w, 2, 0, 300);
    cubes_to_channels(&mut w, &[10], false);
    const_float(&mut w, 2, 2, &[(POS_Y, vec![-7.0, -8.0])]);
    w.obj(INIT_STAGED_ANIM, W::default().varint(0));
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    assert!(s.poses.is_empty());
    assert_eq!(s.gestures.len(), 1);
    let g = &s.gestures[0];
    assert_eq!((g.name.as_str(), g.kind, g.looping), ("dance", GestureKind::Gesture, false));
    assert!(g.must_finish);
    assert_eq!(g.clips.len(), 1);
    assert!(g.clips[0].must_finish);
    assert_eq!(g.clips[0].channels[0][1], vec![q(7.0), q(8.0)]);
    // The SETUP stage is attached to the gesture, not dropped.
    assert_eq!(g.stages.setup.len(), 1);
    assert!(g.stages.finish.is_empty());
    assert_eq!(g.stages.setup[0].duration_ms, 300);
    assert_eq!(g.stages.setup[0].channels[0][1], vec![q(-7.0), q(-8.0)]);
}

#[test]
fn mixed_frame_counts_are_resampled() {
    let m = model();
    let mut w = W::default();
    builtin_trigger(&mut w, 2, 0);
    new_anim(&mut w, 0, 0, 1000);
    cubes_to_channels(&mut w, &[10], true);
    const_float(&mut w, 2, 2, &[(ROT_X, vec![0.0, PI / 2.0])]);
    const_float(&mut w, 4, 4, &[(POS_Y, vec![0.0, 1.0, 2.0, 3.0])]);
    const_bool(&mut w, 2, &[(VIS, vec![true, false])]);
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    check_shapes(&s);
    let c = &s.poses[0].1;
    // 20 frames per second of duration, linear loop.
    assert_eq!(c.frames, 20);
    assert_eq!(c.interp, Interp::LinearLoop);
    // Frame 5 = progress 0.25: rot x halfway through frame 0..1 of 2, pos y at frame 1 of 4.
    assert!(near(c.channels[0][3][5], q(PI / 2.0).to_degrees() / 2.0), "{}", c.channels[0][3][5]);
    assert!(near(c.channels[0][1][5], 1.0));
    assert!(near(c.channels[0][1][15], 3.0));
    assert!(c.show[0][9] && !c.show[0][10]);
}

#[test]
fn unknown_tags_are_skipped() {
    let m = model();
    let mut w = W::default();
    w.obj(99, W::default().u8(1).u8(2).u8(3));
    let mut b = w.0;
    b.extend_from_slice(&walking_bytes());
    let s = decode_new(&b, &m).unwrap();
    assert_eq!(s.poses.len(), 1);
}

#[test]
fn frame_data_for_unknown_channel_is_error() {
    let m = model();
    let mut w = W::default();
    builtin_trigger(&mut w, 2, 0);
    new_anim(&mut w, 0, 0, 1000);
    cubes_to_channels(&mut w, &[10], false);
    const_float(&mut w, 2, 1, &[(13, vec![1.0])]);
    end(&mut w);
    assert!(decode_new(&w.0, &m).is_err());
}

#[test]
fn out_of_range_builtin_pose_is_dropped() {
    let m = model();
    let mut w = W::default();
    builtin_trigger(&mut w, 72, 0); // one past SPEAR_RIGHT
    builtin_trigger(&mut w, 12, 0); // GLOBAL: kept as Other(12)
    for t in 0..2 {
        new_anim(&mut w, t, 0, 1000);
        cubes_to_channels(&mut w, &[10], false);
        const_float(&mut w, 2, 2, &[(POS_Y, vec![1.0, 2.0])]);
    }
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    let poses: Vec<_> = s.poses.iter().map(|p| p.0.clone()).collect();
    assert_eq!(poses, vec![Pose::Other(12)]);
}

#[test]
fn clips_without_frame_data_are_skipped() {
    let m = model();
    let mut w = W::default();
    pose_or_gesture_button(&mut w, false, "wave", 1);
    builtin_trigger(&mut w, 2, 0);
    named_trigger(&mut w, AT_GESTURE, LOOPING, 1);
    new_anim(&mut w, 0, 0, 1000);
    cubes_to_channels(&mut w, &[10], false); // channels but no frame data
    new_anim(&mut w, 1, 0, 1000);
    cubes_to_channels(&mut w, &[10], false);
    const_float(&mut w, 2, 0, &[(POS_Y, vec![])]); // zero frames
    end(&mut w);
    let s = decode_new(&w.0, &m).unwrap();
    assert!(s.poses.is_empty());
    assert!(s.gestures.is_empty());
}
