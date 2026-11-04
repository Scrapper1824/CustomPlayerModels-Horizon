//! Port of `ModelPartAnimation(IOHelper, ModelDefinition)` (decode) and `ModelPartAnimation.apply`
//! (fill missing channels) for the legacy `ANIMATION_DATA` part.

use std::collections::BTreeMap;

use super::{
    fill_component, gesture_kind_and_name, AnimSet, ButtonMeta, Clip, Gesture, GestureKind, Interp, Pose,
    CHANNELS, DYNAMIC_DURATION, GROUPS,
};
use crate::io::{CpmError, Reader, Result};
use crate::model::Model;
use crate::project::VANILLA_POSES;

const END: u8 = 0;
const POSE: u8 = 1;
const GESTURE: u8 = 2;
const ROTATION: u8 = 3;
const POS: u8 = 4;
const VIS: u8 = 5;
const COLOR: u8 = 6;
const INFO: u8 = 7;
const EXTRA: u8 = 10;
const INTERPOLATION: u8 = 11;
const SCALE: u8 = 12;
const LAYER_DEFAULT: u8 = 13;
const ORDER: u8 = 14;
const IS_PROPERTY: u8 = 15;
const GROUP: u8 = 16;
const COMMAND: u8 = 18;
const MUST_FINISH: u8 = 20;
const SLIDER_OPT: u8 = 21;
const BUTTON_HIDDEN: u8 = 22;
const LAST_TYPE: u8 = BUTTON_HIDDEN;

enum Kind {
    Pose(Pose),
    Gesture(String),
}

struct Rd {
    kind: Kind,
    add: bool,
    looping: bool,
    components: Vec<u32>,
    pos: Vec<Option<Vec<[f32; 3]>>>,
    rot: Vec<Option<Vec<[f32; 3]>>>,
    scale: Vec<Option<Vec<[f32; 3]>>>,
    color: Vec<Option<Vec<[f32; 3]>>>,
    show: Vec<Option<Vec<bool>>>,
    frames: usize,
    duration: u32,
    priority: i8,
    interp: Interp,
    default_value: u8,
    finish: bool,
    meta: ButtonMeta,
}

fn new_rd(kind: Kind, add: bool, looping: bool) -> Rd {
    Rd {
        kind,
        add,
        looping,
        components: Vec::new(),
        pos: Vec::new(),
        rot: Vec::new(),
        scale: Vec::new(),
        color: Vec::new(),
        show: Vec::new(),
        frames: 0,
        duration: 0,
        priority: 0,
        interp: Interp::PolyLoop,
        default_value: 0,
        finish: false,
        meta: ButtonMeta::default(),
    }
}

pub fn decode_legacy(bytes: &[u8], model: &Model) -> Result<AnimSet> {
    let mut r = Reader::new(bytes);
    // Keyed by animation id; Java's HashMap iterates small int keys in ascending order.
    let mut data: BTreeMap<u8, Rd> = BTreeMap::new();

    loop {
        let t = r.u8()?;
        if t == END {
            break;
        }
        let mut b = r.block()?;
        if t > LAST_TYPE {
            continue; // unknown type: skipped by block length (Java: readEnum -> null)
        }
        match t {
            GESTURE => {
                let id = b.u8()?;
                let name = b.utf()?;
                let flags = b.u8()?;
                data.insert(id, new_rd(Kind::Gesture(name), flags & 1 != 0, flags & 2 != 0));
            }
            POSE => {
                let ord = b.u8()?;
                let id = b.u8()?;
                let flags = b.u8()?;
                let pose = if ord == 0 { Pose::Custom(b.utf()?) } else { Pose::from_ordinal(ord) };
                data.insert(id, new_rd(Kind::Pose(pose), flags & 1 != 0, false));
            }
            INFO => {
                let id = b.u8()?;
                let Some(rd) = data.get_mut(&id) else { continue };
                let count = b.u8()? as usize;
                let frames = b.u8()? as usize;
                // Java readShort is signed; durations are non-negative in practice.
                let duration = b.i16()? as u16 as u32;
                rd.components = (0..count).map(|_| b.varint()).collect::<Result<_>>()?;
                rd.pos = vec![None; count];
                rd.rot = vec![None; count];
                rd.scale = vec![None; count];
                rd.color = vec![None; count];
                rd.show = vec![None; count];
                rd.frames = frames;
                rd.duration = duration;
            }
            POS | ROTATION | SCALE | COLOR | VIS => {
                let id = b.u8()?;
                let Some(rd) = data.get_mut(&id) else { continue };
                let cid = b.u8()? as usize;
                if cid >= rd.components.len() {
                    return Err(CpmError::Invalid("legacy animation: component index out of range".into()));
                }
                let frames = rd.frames;
                if t == VIS {
                    let mut v = vec![false; frames];
                    let mut i = 0;
                    while i < frames {
                        let dt = b.u8()?;
                        for j in 0..8.min(frames - i) {
                            v[i + j] = dt & (1 << j) != 0;
                        }
                        i += 8;
                    }
                    rd.show[cid] = Some(v);
                } else {
                    let mut v = Vec::with_capacity(frames);
                    for _ in 0..frames {
                        v.push(match t {
                            POS | SCALE => b.vec6b()?,
                            ROTATION => b.angle()?, // degrees
                            _ => [b.u8()? as f32, b.u8()? as f32, b.u8()? as f32],
                        });
                    }
                    match t {
                        POS => rd.pos[cid] = Some(v),
                        ROTATION => rd.rot[cid] = Some(v),
                        SCALE => rd.scale[cid] = Some(v),
                        _ => rd.color[cid] = Some(v),
                    }
                }
            }
            EXTRA | INTERPOLATION | LAYER_DEFAULT | MUST_FINISH | ORDER | IS_PROPERTY | GROUP | COMMAND
            | SLIDER_OPT | BUTTON_HIDDEN => {
                let id = b.u8()?;
                let Some(rd) = data.get_mut(&id) else { continue };
                match t {
                    EXTRA => rd.priority = b.i8()?,
                    INTERPOLATION => {
                        if let Some(i) = Interp::from_ordinal(b.u8()?) {
                            rd.interp = i;
                        }
                    }
                    LAYER_DEFAULT => rd.default_value = b.u8()?,
                    ORDER => rd.meta.order = b.i8()? as i32,
                    IS_PROPERTY => rd.meta.property = true,
                    GROUP => rd.meta.group = Some(b.utf()?),
                    COMMAND => rd.meta.command = true,
                    SLIDER_OPT => {
                        b.u8()?; // interpolateVal: value smoothing between network updates
                        rd.meta.max_value = b.u8()?;
                    }
                    BUTTON_HIDDEN => rd.meta.hidden = true,
                    _ => rd.finish = true,
                }
            }
            // ENCODING, CTRL_IDS, DATA_ENC, MODEL_PROFILES_ID: layer-encoding data, not needed.
            _ => {}
        }
    }

    let mut set = AnimSet::default();
    for rd in data.into_values() {
        let clip = build_clip(&rd, model);
        match rd.kind {
            Kind::Pose(p) => {
                if let Pose::Custom(n) = &p {
                    // CustomPose objects are shared by name; the first one's metadata is kept.
                    if !set.pose_meta.iter().any(|(m, _)| m == n) {
                        set.pose_meta.push((n.clone(), rd.meta.clone()));
                    }
                }
                set.poses.push((p, clip))
            }
            Kind::Gesture(raw) => {
                let (kind, name) = gesture_kind_and_name(&raw);
                // Java groups by raw name (prefix included), the first clip's properties win.
                match set.gestures.iter().position(|g| g.name == name && g.kind == kind) {
                    Some(i) => set.gestures[i].clips.push(clip),
                    None => set.gestures.push(Gesture {
                        name,
                        kind,
                        looping: rd.looping,
                        must_finish: rd.finish,
                        layer: matches!(kind, GestureKind::Layer | GestureKind::Value),
                        default_value: rd.default_value,
                        clips: vec![clip],
                        cond: None,
                        meta: rd.meta.clone(),
                        stages: Default::default(),
                    }),
                }
            }
        }
    }
    attach_stages(&mut set);
    Ok(set)
}

/// `LegacyAnimationParser.finishLoading` (parts/anim/LegacyAnimationParser.java:208-255): each
/// `$pre$` / `$post$` gesture named `p:<vanilla pose>`, `c:<custom pose>` or `g:<gesture>`
/// becomes the setup / finish stage of that pose or gesture. A `p:` name that is no vanilla pose
/// is looked up as a custom pose. Stages naming nothing that has clips stay in `gestures` as
/// unplayable `Setup` / `Finish` entries, as Java drops them.
fn attach_stages(set: &mut AnimSet) {
    let mut i = 0;
    while i < set.gestures.len() {
        let g = &set.gestures[i];
        let setup = match g.kind {
            GestureKind::Setup => true,
            GestureKind::Finish => false,
            _ => {
                i += 1;
                continue;
            }
        };
        let Some((kind, rest)) = g.name.split_once(':') else {
            i += 1;
            continue;
        };
        enum Owner {
            Pose(Pose),
            Gesture(usize),
        }
        let owner = match kind {
            "p" | "c" => {
                let vanilla = (kind == "p")
                    .then(|| VANILLA_POSES.iter().position(|p| *p == rest))
                    .flatten()
                    .map(|o| Pose::from_ordinal(o as u8));
                // A vanilla name wins even if that pose has no clips (Java: computeIfPresent).
                let pose = vanilla.or_else(|| {
                    set.pose_meta.iter().any(|(n, _)| n == rest).then(|| Pose::Custom(rest.to_string()))
                });
                pose.filter(|p| set.poses.iter().any(|(q, _)| q == p)).map(Owner::Pose)
            }
            // gesturesMap: the last non-staged gesture of that name wins.
            "g" => set
                .gestures
                .iter()
                .rposition(|o| o.name == rest && !matches!(o.kind, GestureKind::Setup | GestureKind::Finish))
                .map(Owner::Gesture),
            _ => None,
        };
        let Some(owner) = owner else {
            i += 1;
            continue;
        };
        let clips = set.gestures.remove(i).clips;
        let stages = match owner {
            Owner::Pose(p) => set.pose_stages_mut(p),
            Owner::Gesture(o) => &mut set.gestures[if o > i { o - 1 } else { o }].stages,
        };
        if setup {
            stages.setup.extend(clips);
        } else {
            stages.finish.extend(clips);
        }
    }
}

/// `ModelPartAnimation.apply`: resolve components and fill channels not stored in the file.
fn build_clip(rd: &Rd, model: &Model) -> Clip {
    let frames = rd.frames;
    let mut targets = Vec::new();
    let mut channels = Vec::new();
    let mut show = Vec::new();
    for (i, &cid) in rd.components.iter().enumerate() {
        let groups = [&rd.pos[i], &rd.rot[i], &rd.color[i], &rd.scale[i]];
        let data: [Option<Vec<f32>>; CHANNELS] = std::array::from_fn(|c| {
            groups[c / 3].as_ref().map(|v| v.iter().map(|f| f[c % 3]).collect())
        });
        let (t, ch, sh) = fill_component(model, cid, rd.add, frames, &data, rd.show[i].clone());
        targets.push(t);
        channels.push(ch);
        show.push(sh);
    }
    let dynamic = matches!(&rd.kind, Kind::Pose(p) if p.is_dynamic());
    Clip {
        components: rd.components.clone(),
        targets,
        frames: frames as u32,
        duration_ms: if dynamic { DYNAMIC_DURATION } else { rd.duration },
        priority: rd.priority,
        additive: rd.add,
        must_finish: rd.finish,
        interp: rd.interp,
        dynamic,
        present: vec![[true; GROUPS]; rd.components.len()],
        channels,
        show,
    }
}
