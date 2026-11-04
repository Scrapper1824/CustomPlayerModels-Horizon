//! Port of `ModelPartAnimationNew(IOHelper, ModelDefinition)` (decode, via `AnimLoaderState`,
//! `SerializedTrigger`, `SerializedAnimation`, `ConstantTimeFloat`/`Bool`, `ParameterDetails`,
//! gesture buttons) and `ModelPartAnimationNew.apply` (trigger -> pose / gesture), mapped into
//! the shared [`AnimSet`] types.
//!
//! Mapping notes:
//! - Each cube in `CUBES_TO_CHANNELS` owns 12 float channels (`InterpolatorChannel` order, the
//!   same as [`CHANNELS`]) plus one visibility channel. Only cubes with at least one channel
//!   carrying frame data become clip components (Java makes no driver for the others).
//! - Rotation frames are radians in the file; they are converted to degrees.
//! - Channels without frame data are filled like the legacy decoder (`fill_component`).
//! - When all frame data in an animation shares one frame count and interpolator (what the CPM
//!   exporter always writes), it maps 1:1. Otherwise it is resampled onto 20 frames per second
//!   of duration (min 2) with linear interpolation.
//! - Staged animations: PLAY clips join the main trigger's clips; SETUP / FINISH clips become the
//!   owning pose's or gesture's [`Stages`](super::Stages).

use super::buttons::{slider_max, Action, Button as MenuButton, ButtonKind, Cond, Controls};
use super::{
    fill_component, AnimSet, Clip, Gesture, GestureKind, Interp, Pose, CHANNELS, DYNAMIC_DURATION, GROUPS, GROUP_VIS,
};
use crate::io::{CpmError, Reader, Result};
use crate::model::Model;

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
const LAYER_CTRL: u8 = 1 << 0;
const LOOPING: u8 = 1 << 1;
const BITMASK: u8 = 1 << 2;
const MUST_FINISH: u8 = 1 << 4;

// AbstractCommandTriggerableData flags (LAYER_CTRL shared with the trigger flags).
const BUTTON_COMMAND: u8 = 1 << 1;
const BUTTON_PROPERTY: u8 = 1 << 2;
const BUTTON_HIDDEN: u8 = 1 << 4;

/// `SerializedAnimation.ADDITIVE`.
const ADDITIVE: u8 = 1 << 0;

/// `AnimationType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimType {
    Pose,
    CustomPose,
    Gesture,
    Layer,
    ValueLayer,
    Setup,
    Finish,
}

impl AnimType {
    fn from_ordinal(o: u8) -> Option<AnimType> {
        Some(match o {
            0 => AnimType::Pose,
            1 => AnimType::CustomPose,
            2 => AnimType::Gesture,
            3 => AnimType::Layer,
            4 => AnimType::ValueLayer,
            5 => AnimType::Setup,
            6 => AnimType::Finish,
            _ => return None,
        })
    }
}

/// `VanillaPose.VALUES.length`.
const VANILLA_POSE_COUNT: u8 = 72;

// `StageType` ordinals.
const STAGE_SETUP: u8 = 0;
const STAGE_PLAY: u8 = 1;

#[derive(Debug, Default)]
struct Trigger {
    anim: Option<AnimType>,
    /// `VanillaPose` ordinal (builtin triggers only).
    pose: Option<u8>,
    looping: bool,
    must_finish: bool,
    parameter: i64,
    value: u32,
    bit_mask: bool,
    /// `StageType` ordinal (staged triggers only).
    stage: Option<u8>,
    staging_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// `InterpolatorChannel` index 0..12.
    Float(usize),
    Vis,
}

/// `ConstantTimeFloat` (interp `Some`) or `ConstantTimeBool` (interp `None`, values 0 / 1).
#[derive(Debug, Clone)]
struct FrameData {
    interp: Option<Interp>,
    values: Vec<f32>,
}

#[derive(Debug)]
struct Channel {
    cube: u32,
    slot: Slot,
    data: Option<FrameData>,
}

#[derive(Debug)]
struct Anim {
    trigger: u32,
    priority: i32,
    duration: u32,
    additive: Option<bool>,
    channels: Vec<Channel>,
}

#[derive(Debug)]
enum Button {
    /// `CustomPoseGestureButtonData`.
    PoseOrGesture { pose: bool, name: String, id: u8 },
    /// `BoolParameterToggleButtonData`.
    Bool { name: String, parameter: u32, mask: u8 },
    /// `ValueParameterButtonData`.
    Value { name: String, parameter: u32, max: u8 },
    /// `DropdownButtonData`.
    Dropdown { name: String, parameter: u32, options: Vec<String> },
}

#[derive(Default)]
struct State {
    triggers: Vec<Trigger>,
    anims: Vec<Anim>,
    /// Each button with its `GestureGui` hidden state (hidden, command or property flag).
    buttons: Vec<(Button, bool)>,
    sync_defaults: Option<Vec<u8>>,
    staged: Vec<u32>,
}

impl State {
    fn trigger(&mut self) -> Result<&mut Trigger> {
        self.triggers.last_mut().ok_or_else(|| invalid("trigger init before NEW_TRIGGER"))
    }

    fn anim(&mut self) -> Result<&mut Anim> {
        self.anims.last_mut().ok_or_else(|| invalid("animation data before NEW_ANIM"))
    }
}

fn invalid(s: &str) -> CpmError {
    CpmError::Invalid(format!("new animation: {s}"))
}

fn channel<'a>(a: &'a mut Anim, id: u32) -> Result<&'a mut Channel> {
    a.channels.get_mut(id as usize).ok_or_else(|| invalid("frame data for unknown channel"))
}

pub fn decode_new(bytes: &[u8], model: &Model) -> Result<AnimSet> {
    let mut r = Reader::new(bytes);
    let mut st = State::default();
    loop {
        let tag = r.u8()?;
        let mut b = r.block()?;
        match tag {
            END => break,
            NEW_ANIM => {
                let trigger = b.varint()?;
                let priority = b.svarint()?;
                let duration = b.varint()?;
                st.anims.push(Anim { trigger, priority, duration, additive: None, channels: Vec::new() });
            }
            NEW_TRIGGER => st.triggers.push(Trigger { parameter: -1, staging_id: -1, ..Default::default() }),
            INIT_BUILTIN_TRIGGER => {
                let pose = b.u8()?;
                let flags = b.u8()?;
                let t = st.trigger()?;
                // Out-of-range ordinal: Java's readEnum gives null and compile drops the trigger.
                t.pose = (pose < VANILLA_POSE_COUNT).then_some(pose);
                t.anim = Some(AnimType::Pose);
                t.must_finish = flags & MUST_FINISH != 0;
                t.looping = true;
            }
            INIT_NAMED_TRIGGER => {
                let anim = AnimType::from_ordinal(b.u8()?);
                let flags = b.u8()?;
                let value = b.u8()? as u32;
                if flags & LAYER_CTRL != 0 {
                    b.u8()?; // gid: skin-layer encoding, not needed here
                }
                let t = st.trigger()?;
                t.anim = anim;
                t.parameter = if anim == Some(AnimType::Gesture) { 1 } else { 0 };
                t.looping = flags & LOOPING != 0;
                t.must_finish = flags & MUST_FINISH != 0;
                t.value = value;
            }
            INIT_PARAMETER_TRIGGER => {
                let anim = AnimType::from_ordinal(b.u8()?);
                let flags = b.u8()?;
                let parameter = b.varint()? as i64;
                let value = b.u8()? as u32;
                let t = st.trigger()?;
                t.anim = anim;
                t.looping = flags & LOOPING != 0;
                t.bit_mask = flags & BITMASK != 0;
                t.must_finish = flags & MUST_FINISH != 0;
                t.parameter = parameter;
                t.value = value;
            }
            INIT_STAGED_TRIGGER => {
                let stage = b.u8()?;
                let staging_id = b.varint()? as i64;
                let flags = b.u8()?;
                let t = st.trigger()?;
                t.anim = Some(AnimType::Setup);
                t.stage = (stage < 3).then_some(stage);
                t.staging_id = staging_id;
                t.must_finish = flags & MUST_FINISH != 0;
            }
            CONSTANT_FRAME_TIME_FLOAT => {
                let interp = Interp::from_ordinal(b.u8()?).ok_or_else(|| invalid("unknown interpolator"))?;
                let frames = b.varint()? as usize;
                let count = b.varint()?;
                let a = st.anim()?;
                for _ in 0..count {
                    let id = b.varint()?;
                    let mut values = Vec::with_capacity(frames.min(b.remaining()));
                    for _ in 0..frames {
                        values.push(b.var_float()?);
                    }
                    channel(a, id)?.data = Some(FrameData { interp: Some(interp), values });
                }
            }
            CONSTANT_FRAME_TIME_BOOLEAN => {
                let frames = b.varint()? as usize;
                let count = b.varint()?;
                let a = st.anim()?;
                for _ in 0..count {
                    let id = b.varint()?;
                    let packed = b.bytes(frames.div_ceil(8))?;
                    let values = (0..frames).map(|i| (packed[i / 8] >> (i % 8) & 1) as f32).collect();
                    channel(a, id)?.data = Some(FrameData { interp: None, values });
                }
            }
            CUBES_TO_CHANNELS => {
                let count = b.varint()?;
                let interpolated = b.varint()?;
                let flags = b.u8()?;
                let a = st.anim()?;
                a.additive.get_or_insert(flags & ADDITIVE != 0);
                for _ in 0..count {
                    let cube = b.varint()?;
                    if interpolated == CHANNELS as u32 {
                        for i in 0..CHANNELS {
                            a.channels.push(Channel { cube, slot: Slot::Float(i), data: None });
                        }
                    }
                    a.channels.push(Channel { cube, slot: Slot::Vis, data: None });
                }
            }
            GESTURE_BUTTON => {
                let kind = b.u8()?;
                if kind > 4 {
                    continue; // unknown GestureButtonType
                }
                let name = b.utf()?;
                let flags = b.u8()?;
                let hidden = flags & (BUTTON_COMMAND | BUTTON_PROPERTY | BUTTON_HIDDEN) != 0;
                let button = match kind {
                    0 | 1 => {
                        let id = b.u8()?;
                        if flags & LAYER_CTRL != 0 {
                            b.u8()?; // gid
                        }
                        // gestureTimeout (gestures only) is not needed.
                        Button::PoseOrGesture { pose: kind == 0, name, id }
                    }
                    2 => Button::Bool { name, parameter: b.varint()?, mask: b.u8()? },
                    3 => Button::Value { name, parameter: b.varint()?, max: b.u8()? },
                    _ => {
                        let parameter = b.varint()?;
                        // Java readByte: a negative count reads no options.
                        let n = b.i8()?.max(0);
                        let options = (0..n).map(|_| b.utf()).collect::<Result<_>>()?;
                        Button::Dropdown { name, parameter, options }
                    }
                };
                st.buttons.push((button, hidden));
            }
            PARAMETERS => {
                let n = b.varint()? as usize;
                st.sync_defaults = Some(b.bytes(n)?.to_vec());
                // Local parameter defaults follow; unused.
            }
            INIT_STAGED_ANIM => st.staged.push(b.varint()?),
            // CONTROL_INFO: blank / reset gesture ids and model profile id (layer encoding).
            CONTROL_INFO => {}
            // Unknown tags are skipped by block length (Java: readEnum -> null).
            _ => {}
        }
    }
    Ok(compile(st, model))
}

/// What a trigger plays as (`SerializedTrigger.compile`).
enum Owner {
    Pose(Pose),
    Gesture { name: String, kind: GestureKind, default_value: u8, cond: Cond },
}

/// `SerializedTrigger.compile` for a non-staged trigger. `None`: the trigger cannot be played
/// (unknown pose, no button, staged type).
fn resolve(st: &State, owner: &Trigger) -> Option<Owner> {
    let param = usize::try_from(owner.parameter).ok();
    let (name, kind, default_value, cond) = match owner.anim? {
        AnimType::Pose => return Some(Owner::Pose(Pose::from_ordinal(owner.pose?))),
        AnimType::CustomPose => {
            return st.buttons.iter().find_map(|(b, _)| match b {
                Button::PoseOrGesture { pose: true, name, id } if *id as u32 == owner.value => {
                    Some(Owner::Pose(Pose::Custom(name.clone())))
                }
                _ => None,
            })
        }
        AnimType::Gesture => st.buttons.iter().find_map(|(b, _)| match b {
            Button::PoseOrGesture { pose: false, name, id } if *id as u32 == owner.value => {
                Some((name.clone(), GestureKind::Gesture, 0, Cond::Gesture(*id)))
            }
            _ => None,
        })?,
        AnimType::Layer => {
            let def = default_param(st, owner.parameter);
            let cond = param.map(|param| Cond::Layer { param, value: owner.value as u8, bitmask: owner.bit_mask });
            st.buttons.iter().find_map(|(b, _)| match b {
                Button::Bool { name, parameter, mask }
                    if owner.bit_mask && *parameter as i64 == owner.parameter && *mask as u32 == owner.value =>
                {
                    Some((name.clone(), GestureKind::Layer, if def & mask != 0 { 255 } else { 0 }, cond?))
                }
                Button::Dropdown { parameter, options, .. } if !owner.bit_mask && *parameter as i64 == owner.parameter => {
                    let name = options.get(owner.value as usize)?;
                    Some((name.clone(), GestureKind::Layer, if def as u32 == owner.value { 255 } else { 0 }, cond?))
                }
                _ => None,
            })?
        }
        AnimType::ValueLayer => st.buttons.iter().find_map(|(b, _)| match b {
            Button::Value { name, parameter, .. } if *parameter as i64 == owner.parameter => Some((
                name.clone(),
                GestureKind::Value,
                default_param(st, owner.parameter),
                Cond::Value { param: param? },
            )),
            _ => None,
        })?,
        AnimType::Setup | AnimType::Finish => return None,
    };
    Some(Owner::Gesture { name, kind, default_value, cond })
}

/// The index of the gesture `t` plays as, created (without clips) when missing.
fn gesture_index(set: &mut AnimSet, t: &Trigger, name: String, kind: GestureKind, default_value: u8, cond: Cond) -> usize {
    if let Some(i) = set.gestures.iter().position(|g| g.name == name && g.kind == kind && g.cond == Some(cond)) {
        return i;
    }
    set.gestures.push(Gesture {
        name,
        kind,
        // LayerTrigger / ValueTrigger always loop.
        looping: t.looping || matches!(kind, GestureKind::Layer | GestureKind::Value),
        must_finish: t.must_finish,
        layer: matches!(kind, GestureKind::Layer | GestureKind::Value),
        default_value,
        clips: Vec::new(),
        cond: Some(cond),
        meta: Default::default(),
        stages: Default::default(),
    });
    set.gestures.len() - 1
}

/// `ModelPartAnimationNew.apply` (parts/anim/ModelPartAnimationNew.java:71-110) +
/// `SerializedTrigger.compile`. A staging's PLAY clips join its main trigger's clips; its SETUP /
/// FINISH clips become that pose's or gesture's [`Stages`](super::Stages).
fn compile(st: State, model: &Model) -> AnimSet {
    let mut set = AnimSet::default();
    // Main and PLAY clips first, so that stages find their gesture; then SETUP / FINISH.
    for pass_stages in [false, true] {
        for a in &st.anims {
            let Some(t) = st.triggers.get(a.trigger as usize) else { continue };
            let (owner, stage) = match t.stage {
                Some(stage) => {
                    let Some(&main) = usize::try_from(t.staging_id).ok().and_then(|i| st.staged.get(i)) else {
                        continue;
                    };
                    let Some(main) = st.triggers.get(main as usize) else { continue };
                    (main, Some(stage))
                }
                None => (t, None),
            };
            let extra = matches!(stage, Some(s) if s != STAGE_PLAY);
            if extra != pass_stages {
                continue;
            }
            // A main clip with no frames or no driven components does nothing in Java; leave it
            // out. Setup / finish clips are kept: their duration still times the stages.
            let empty = a.channels.iter().all(|c| c.data.as_ref().is_none_or(|d| d.values.is_empty()));
            if empty && !extra {
                continue;
            }
            // No pose or button: the trigger cannot be activated; dropped.
            let Some(res) = resolve(&st, owner) else { continue };
            // Setup / finish clips play on their own time (`StagedAnimation.Anim`).
            let clip = |dynamic: bool| build_clip(a, model, t.must_finish, dynamic && !extra);
            match (res, stage) {
                (Owner::Pose(pose), Some(s)) if s != STAGE_PLAY => {
                    let c = clip(false);
                    let stages = set.pose_stages_mut(pose);
                    if s == STAGE_SETUP {
                        stages.setup.push(c)
                    } else {
                        stages.finish.push(c)
                    }
                }
                (Owner::Pose(pose), _) => {
                    let c = clip(pose.is_dynamic());
                    set.poses.push((pose, c));
                }
                (Owner::Gesture { name, kind, default_value, cond }, stage) => {
                    let i = gesture_index(&mut set, owner, name, kind, default_value, cond);
                    let c = clip(false);
                    let g = &mut set.gestures[i];
                    match stage {
                        Some(STAGE_SETUP) => g.stages.setup.push(c),
                        Some(s) if s != STAGE_PLAY => g.stages.finish.push(c),
                        _ => {
                            g.must_finish |= c.must_finish;
                            g.clips.push(c);
                        }
                    }
                }
            }
        }
    }
    set.controls = Some(controls(&st, &set));
    set
}

/// The file's buttons in file order (`AnimationRegistry.namedActions`, which `GestureGui` lists
/// as is) with their actions, and the sync parameter defaults (`ParameterDetails.DEFAULT`, two
/// zero bytes, without a `PARAMETERS` block).
fn controls(st: &State, set: &AnimSet) -> Controls {
    let mut c = Controls { defaults: st.sync_defaults.clone().unwrap_or_else(|| vec![0, 0]), ..Default::default() };
    for (i, (b, hidden)) in st.buttons.iter().enumerate() {
        let (name, kind, action) = match b {
            Button::PoseOrGesture { pose: true, name, .. } => {
                (name, ButtonKind::CustomPose, Action::CustomPose(name.clone()))
            }
            Button::PoseOrGesture { pose: false, name, id } => {
                let gi = set.gestures.iter().position(|g| g.cond == Some(Cond::Gesture(*id)));
                (name, ButtonKind::Gesture, Action::Gesture(gi))
            }
            Button::Bool { name, parameter, mask } => {
                (name, ButtonKind::Toggle, Action::Bool { param: *parameter as usize, mask: *mask })
            }
            Button::Value { name, parameter, max } => {
                let max = slider_max(*max);
                (name, ButtonKind::Slider { max }, Action::Value { param: *parameter as usize, max })
            }
            Button::Dropdown { name, parameter, options } => (
                name,
                ButtonKind::Dropdown { options: options.clone() },
                Action::Dropdown { param: *parameter as usize },
            ),
        };
        c.buttons.push(MenuButton { name: name.clone(), kind, hidden: *hidden, order: i as i32 });
        c.actions.push(action);
    }
    c
}

/// `ParameterDetails.getDefaultParam`.
fn default_param(st: &State, parameter: i64) -> u8 {
    let defaults = st.sync_defaults.as_deref().unwrap_or(&[]);
    usize::try_from(parameter).ok().and_then(|p| defaults.get(p).copied()).unwrap_or(0)
}

/// Frames per second of duration when frame data has to be resampled.
const RESAMPLE_FPS: u32 = 20;
/// Upper bound on resampled frames (one minute at [`RESAMPLE_FPS`]).
const MAX_RESAMPLED_FRAMES: u32 = 1200;

fn build_clip(a: &Anim, model: &Model, must_finish: bool, dynamic: bool) -> Clip {
    // Additive is per CUBES_TO_CHANNELS block in Java; the exporter writes one block per
    // animation, so the first block's flag is taken for the whole clip.
    let additive = a.additive.unwrap_or(false);
    let with_data = || a.channels.iter().filter(|c| c.data.as_ref().is_some_and(|d| !d.values.is_empty()));

    let mut components: Vec<u32> = Vec::new();
    for c in with_data() {
        if !components.contains(&c.cube) {
            components.push(c.cube);
        }
    }

    // Uniform: one frame count, float slots all share one interpolator, vis slots are booleans.
    let first_len = with_data().next().map(|c| c.data.as_ref().unwrap().values.len());
    let float_interp = with_data().find_map(|c| match c.slot {
        Slot::Float(_) => c.data.as_ref().unwrap().interp,
        Slot::Vis => None,
    });
    let uniform = with_data().all(|c| {
        let d = c.data.as_ref().unwrap();
        Some(d.values.len()) == first_len
            && match c.slot {
                Slot::Float(_) => d.interp.is_some() && d.interp == float_interp,
                Slot::Vis => d.interp.is_none(),
            }
    });

    let (frames, interp, sample_at): (usize, Interp, Option<Box<dyn Fn(usize) -> f32>>) = if uniform {
        (first_len.unwrap_or(0), float_interp.unwrap_or_default(), None)
    } else {
        let n = (a.duration.saturating_mul(RESAMPLE_FPS) / 1000).clamp(2, MAX_RESAMPLED_FRAMES) as usize;
        let single = with_data().any(|c| {
            matches!(
                c.data.as_ref().unwrap().interp,
                Some(Interp::PolySingle | Interp::LinearSingle | Interp::TrigSingle)
            )
        });
        // A single-shot clip reaches its last frame at progress 1; a looping one wraps.
        let div = if single { (n - 1) as f32 } else { n as f32 };
        let interp = if single { Interp::LinearSingle } else { Interp::LinearLoop };
        (n, interp, Some(Box::new(move |k| k as f32 / div)))
    };

    let mut targets = Vec::new();
    let mut channels = Vec::new();
    let mut show = Vec::new();
    let mut present = Vec::new();
    for &cube in &components {
        let mut data: [Option<Vec<f32>>; CHANNELS] = Default::default();
        let mut vis = None;
        for c in with_data().filter(|c| c.cube == cube) {
            let d = c.data.as_ref().unwrap();
            let rotation = matches!(c.slot, Slot::Float(3..=5));
            let mut values: Vec<f32> =
                if rotation { d.values.iter().map(|v| v.to_degrees()).collect() } else { d.values.clone() };
            if let Some(t_of) = &sample_at {
                if rotation {
                    unwrap_degrees(&mut values);
                }
                values = (0..frames).map(|k| sample(d.interp, &values, t_of(k))).collect();
            }
            match c.slot {
                Slot::Float(i) => data[i] = Some(values),
                Slot::Vis => vis = Some(values.iter().map(|v| *v > 0.5).collect()),
            }
        }
        let mut p = [false; GROUPS];
        for (g, slot) in p.iter_mut().enumerate().take(4) {
            *slot = data[g * 3..g * 3 + 3].iter().any(Option::is_some);
        }
        p[GROUP_VIS] = vis.is_some();
        present.push(p);
        let (t, ch, sh) = fill_component(model, cube, additive, frames, &data, vis);
        targets.push(t);
        channels.push(ch);
        show.push(sh);
    }

    Clip {
        components,
        targets,
        frames: frames as u32,
        duration_ms: if dynamic { DYNAMIC_DURATION } else { a.duration },
        priority: a.priority.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
        additive,
        must_finish,
        interp,
        dynamic,
        present,
        channels,
        show,
    }
}

/// Makes successive angles continuous (`RotationInterpolator`, in degrees).
fn unwrap_degrees(v: &mut [f32]) {
    for i in 1..v.len() {
        let prev = v[i - 1];
        while v[i] - prev > 180.0 {
            v[i] -= 360.0;
        }
        while v[i] - prev < -180.0 {
            v[i] += 360.0;
        }
    }
}

/// Value of a frame track at progress `t` in 0..=1 (`ConstantTimeFloatDriver` / `ConstantTimeBoolDriver`).
/// Spline and trigonometric interpolators are approximated linearly.
fn sample(interp: Option<Interp>, v: &[f32], t: f32) -> f32 {
    let len = v.len();
    let x = t * len as f32;
    let lerp = |i: usize, j: usize, f: f32| v[i] + (v[j] - v[i]) * f;
    match interp {
        None | Some(Interp::NoInterpolate) => v[(x.floor() as usize).min(len - 1)],
        Some(Interp::PolyLoop | Interp::LinearLoop | Interp::TrigLoop) => {
            let i = x.floor() as usize;
            lerp(i % len, (i + 1) % len, x - x.floor())
        }
        Some(Interp::PolySingle | Interp::LinearSingle | Interp::TrigSingle) => {
            let p = x / len as f32 * (len - 1) as f32;
            let i = p.floor() as usize;
            lerp(i.min(len - 1), (i + 1).min(len - 1), p - p.floor())
        }
    }
}
