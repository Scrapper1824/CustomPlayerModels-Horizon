//! `ANIMATION_NEW` export: a port of `ModelPartAnimationNew(Editor)` / `.write`,
//! `AnimLoaderState.loadFromEditor` / `writeInfo`, `AnimationExporter`, `SerializedTrigger.write`,
//! `SerializedAnimation.write`, `ConstantTimeFloat.write`, `ConstantTimeBool.write`,
//! `ParameterDetails.ParameterAllocator` and the gesture button `write`s
//! (Java paths under `com/tom/cpm/shared/`).
//!
//! Deliberate equivalences with Java (behaviour, not byte order, is unspecified there):
//! - Animations are processed in [`Project::anims`] order (Java: the project zip's `HashMap`).
//! - Components of an animation (`EditorAnim.getComponentsFiltered`) are ordered by frame, then
//!   file order of the frame's components, then walk order of the elements sharing a storeID
//!   (Java: `HashMap` order).
//! - Inside an animation, float frame data is written before boolean frame data (Java iterates a
//!   `HashMap` keyed by `AnimationFrameDataType`).
//!
//! Java behaviours ported as they are:
//! - Triggers never get `layerCtrl` (`AnimationExporter` only sets it on buttons), so
//!   `LayerEncondingHelper.handleLayers` never changes anything and is not ported; pose / gesture
//!   buttons with `layerControlled` are written with `gid` 0.
//! - Dropdown defaults (`linkStagingAnims`' `paramAlloc.setDefaultValue`) are set after
//!   `paramAlloc.finish()` built the parameter block, so they never reach the file; not ported.

use std::collections::HashMap;

use serde_json::Value;

use super::{walk, Node};
use crate::anim::{Pose, DYNAMIC_DURATION};
use crate::io::{CpmError, Result};
use crate::project::{AnimKind, FrameComponent, Project, ProjectAnim, VANILLA_POSES};
use crate::write::Writer;

/// `ModelPartType.ANIMATION_NEW`.
pub(super) const ANIMATION_NEW: u8 = 23;

/// `TagType` ordinals.
mod tag {
    pub const END: u8 = 0;
    pub const NEW_ANIM: u8 = 1;
    pub const NEW_TRIGGER: u8 = 2;
    pub const INIT_BUILTIN_TRIGGER: u8 = 3;
    pub const INIT_NAMED_TRIGGER: u8 = 4;
    pub const INIT_PARAMETER_TRIGGER: u8 = 5;
    pub const INIT_STAGED_TRIGGER: u8 = 6;
    pub const CONSTANT_FRAME_TIME_FLOAT: u8 = 7;
    pub const CONSTANT_FRAME_TIME_BOOLEAN: u8 = 8;
    pub const CUBES_TO_CHANNELS: u8 = 9;
    pub const CONTROL_INFO: u8 = 10;
    pub const GESTURE_BUTTON: u8 = 11;
    pub const PARAMETERS: u8 = 12;
    pub const INIT_STAGED_ANIM: u8 = 13;
}

// SerializedTrigger flags (LAYER_CTRL, 1 << 0, is never set by the exporter).
const LOOPING: u8 = 1 << 1;
const BITMASK: u8 = 1 << 2;
const PARAM_INTERPOLATE: u8 = 1 << 3;
const MUST_FINISH: u8 = 1 << 4;

/// `SerializedAnimation.ADDITIVE`.
const ADDITIVE: u32 = 1 << 0;

// AbstractCommandTriggerableData flags.
const BUTTON_LAYER_CTRL: u8 = 1 << 0;
const BUTTON_COMMAND: u8 = 1 << 1;
const BUTTON_PROPERTY: u8 = 1 << 2;
const BUTTON_HIDDEN: u8 = 1 << 4;

// StageType ordinals.
const STAGE_SETUP: u8 = 0;
const STAGE_PLAY: u8 = 1;
const STAGE_FINISH: u8 = 2;

/// `InterpolatorChannel.VALUES.length`; each cube owns these plus one visibility channel.
const FLOAT_CHANNELS: u32 = 12;
const CUBE_CHANNELS: u32 = FLOAT_CHANNELS + 1;

/// `PlayerSkinLayer` names, by ordinal.
const SKIN_LAYERS: [&str; 6] = ["hat", "jacket", "left_pants_leg", "right_pants_leg", "left_sleeve", "right_sleeve"];

/// `AnimationType` ordinal.
fn kind_ordinal(k: AnimKind) -> u8 {
    match k {
        AnimKind::Pose => 0,
        AnimKind::CustomPose => 1,
        AnimKind::Gesture => 2,
        AnimKind::Layer => 3,
        AnimKind::ValueLayer => 4,
        AnimKind::Setup => 5,
        AnimKind::Finish => 6,
    }
}

fn is_layer(k: AnimKind) -> bool {
    matches!(k, AnimKind::Layer | AnimKind::ValueLayer)
}

/// `EditorAnim.getId`: the display name up to the first `#`.
fn anim_id(display_name: &str) -> &str {
    display_name.split('#').next().unwrap_or("")
}

/// The `ModelElement` fields `AnimFrame.FrameData` and `AnimationExporter` read.
struct Elem {
    pos: [f32; 3],
    /// Degrees.
    rot: [f32; 3],
    rgb: u32,
    texture: bool,
    recolor: bool,
    hidden: bool,
}

impl Elem {
    fn color(&self) -> [f32; 3] {
        [((self.rgb >> 16) & 0xff) as f32, ((self.rgb >> 8) & 0xff) as f32, (self.rgb & 0xff) as f32]
    }
}

/// `(float) Math.toRadians(deg)` (`IElem.part`).
fn rad(deg: f32) -> f32 {
    (deg as f64).to_radians() as f32
}

// `AnimFrame.FrameData.has*Changes` (editor/anim/AnimFrame.java:136-166).

fn moved(v: [f32; 3], base: [f32; 3], add: bool) -> bool {
    (0..3).any(|i| (if add { v[i] } else { v[i] - base[i] }).abs() > 0.01)
}

fn pos_changes(e: &Elem, d: &FrameComponent, add: bool) -> bool {
    moved(d.pos, e.pos, add)
}

fn rot_changes(e: &Elem, d: &FrameComponent, add: bool) -> bool {
    moved(d.rotation, e.rot, add)
}

fn scale_changes(d: &FrameComponent) -> bool {
    d.scale.iter().any(|s| (s - 1.0).abs() > 0.01)
}

fn color_changes(e: &Elem, d: &FrameComponent) -> bool {
    if e.texture && !e.recolor {
        return false;
    }
    let rgb = ((d.color[0] as i32) << 16) | ((d.color[1] as i32) << 8) | (d.color[2] as i32);
    (e.rgb & 0xffffff) as i32 != rgb & 0xffffff
}

fn vis_changes(e: &Elem, d: &FrameComponent) -> bool {
    e.hidden == d.show
}

fn has_changes(e: &Elem, d: &FrameComponent, add: bool) -> bool {
    pos_changes(e, d, add) || rot_changes(e, d, add) || color_changes(e, d) || vis_changes(e, d) || scale_changes(d)
}

#[derive(Debug, Clone)]
struct Trigger {
    stage: Option<u8>,
    staging_id: i32,
    anim: Option<AnimKind>,
    /// `VanillaPose` ordinal.
    pose: Option<u8>,
    looping: bool,
    parameter: i32,
    value: i32,
    bit_mask: bool,
    parameter_interpolate: bool,
    must_finish: bool,
}

impl Trigger {
    fn new() -> Trigger {
        Trigger {
            stage: None,
            staging_id: -1,
            anim: None,
            pose: None,
            looping: false,
            parameter: -1,
            value: 0,
            bit_mask: false,
            parameter_interpolate: false,
            must_finish: false,
        }
    }

    /// `SerializedTrigger.equals` (`looping` and `parameterInterpolate` are not part of it).
    fn same(&self, o: &Trigger) -> bool {
        self.anim == o.anim
            && self.staging_id == o.staging_id
            && self.stage == o.stage
            && self.parameter == o.parameter
            && self.value == o.value
            && self.bit_mask == o.bit_mask
            && self.must_finish == o.must_finish
            && self.pose == o.pose
    }

    /// `SerializedTrigger.write` (parts/anim/SerializedTrigger.java:91-135).
    fn write(&self, out: &mut Writer) {
        out.object_block(tag::NEW_TRIGGER, &Writer::new());
        let finish = if self.must_finish { MUST_FINISH } else { 0 };
        let mut d = Writer::new();
        if let Some(pose) = self.pose {
            d.u8(pose).u8(finish);
            out.object_block(tag::INIT_BUILTIN_TRIGGER, &d);
        } else if let Some(anim @ (AnimKind::Pose | AnimKind::Gesture)) = self.anim {
            let mut flags = finish;
            if self.bit_mask {
                flags |= BITMASK;
            }
            if self.looping {
                flags |= LOOPING;
            }
            // layerCtrl is never set by the exporter, so no gid follows.
            d.u8(kind_ordinal(anim)).u8(flags).u8(self.value as u8);
            out.object_block(tag::INIT_NAMED_TRIGGER, &d);
        } else if let (true, Some(anim)) = (self.parameter != -1, self.anim) {
            let mut flags = finish;
            if self.bit_mask {
                flags |= BITMASK;
            }
            if self.looping {
                flags |= LOOPING;
            }
            if self.parameter_interpolate {
                flags |= PARAM_INTERPOLATE;
            }
            d.u8(kind_ordinal(anim)).u8(flags).varint(self.parameter as u32).u8(self.value as u8);
            out.object_block(tag::INIT_PARAMETER_TRIGGER, &d);
        } else if let Some(stage) = self.stage {
            d.u8(stage).varint(self.staging_id as u32).u8(finish);
            out.object_block(tag::INIT_STAGED_TRIGGER, &d);
        }
    }
}

/// `SerializedAnimation` with its channels' frame data.
struct SerializedAnim {
    trigger: usize,
    priority: i32,
    duration: u32,
    additive: bool,
    cubes: Vec<u32>,
    interp: u8,
    frames: usize,
    floats: Vec<(u32, Vec<f32>)>,
    bools: Vec<(u32, Vec<bool>)>,
}

impl SerializedAnim {
    /// `SerializedAnimation.write` (parts/anim/SerializedAnimation.java:310-345).
    fn write(&self, out: &mut Writer) {
        let mut d = Writer::new();
        d.varint(self.trigger as u32).svarint(self.priority).varint(self.duration);
        out.object_block(tag::NEW_ANIM, &d);
        if !self.cubes.is_empty() {
            let mut d = Writer::new();
            d.varint(self.cubes.len() as u32).varint(FLOAT_CHANNELS);
            d.varint(if self.additive { ADDITIVE } else { 0 });
            for &c in &self.cubes {
                d.varint(c);
            }
            out.object_block(tag::CUBES_TO_CHANNELS, &d);
        }
        // ConstantTimeFloat.write: one group, every channel shares the interpolator and length.
        if !self.floats.is_empty() {
            let mut d = Writer::new();
            d.u8(self.interp).varint(self.frames as u32).varint(self.floats.len() as u32);
            for (ch, values) in &self.floats {
                d.varint(*ch);
                for &v in values {
                    d.var_float(v);
                }
            }
            out.object_block(tag::CONSTANT_FRAME_TIME_FLOAT, &d);
        }
        // ConstantTimeBool.write + IOHelper.writeBoolArray.
        if !self.bools.is_empty() {
            let mut d = Writer::new();
            d.varint(self.frames as u32).varint(self.bools.len() as u32);
            for (ch, values) in &self.bools {
                d.varint(*ch);
                for chunk in values.chunks(8) {
                    d.u8(chunk.iter().enumerate().fold(0u8, |m, (j, &b)| m | ((b as u8) << j)));
                }
            }
            out.object_block(tag::CONSTANT_FRAME_TIME_BOOLEAN, &d);
        }
    }
}

#[derive(Debug)]
enum ButtonKind {
    /// `CustomPoseGestureButtonData(true)`.
    Pose { id: i32 },
    /// `CustomPoseGestureButtonData(false)`.
    Gesture { id: i32, timeout: i32 },
    /// `BoolParameterToggleButtonData`.
    Bool { parameter: i32, mask: i32 },
    /// `ValueParameterButtonData`.
    Value { parameter: i32, max: i32 },
    /// `DropdownButtonData`.
    Dropdown { parameter: i32, options: Vec<String> },
}

#[derive(Debug)]
struct Button {
    name: String,
    layer_ctrl: bool,
    command: bool,
    is_property: bool,
    hidden: bool,
    kind: ButtonKind,
}

impl Button {
    fn new(name: &str, kind: ButtonKind) -> Button {
        Button { name: name.into(), layer_ctrl: false, command: false, is_property: false, hidden: false, kind }
    }

    /// `GESTURE_BUTTON`: `GestureButtonType` ordinal, then the button's `write`.
    fn write(&self, out: &mut Writer) {
        let mut d = Writer::new();
        d.u8(match self.kind {
            ButtonKind::Pose { .. } => 0,
            ButtonKind::Gesture { .. } => 1,
            ButtonKind::Bool { .. } => 2,
            ButtonKind::Value { .. } => 3,
            ButtonKind::Dropdown { .. } => 4,
        });
        d.utf(&self.name);
        let mut flags = 0;
        for (b, f) in [
            (self.layer_ctrl, BUTTON_LAYER_CTRL),
            (self.command, BUTTON_COMMAND),
            (self.is_property, BUTTON_PROPERTY),
            (self.hidden, BUTTON_HIDDEN),
        ] {
            if b {
                flags |= f;
            }
        }
        d.u8(flags);
        match &self.kind {
            ButtonKind::Pose { id } | ButtonKind::Gesture { id, .. } => {
                d.u8(*id as u8);
                if self.layer_ctrl {
                    d.u8(0); // gid: never assigned (see the module docs)
                }
                if let ButtonKind::Gesture { timeout, .. } = self.kind {
                    d.varint(timeout as u32);
                }
            }
            ButtonKind::Bool { parameter, mask } => {
                d.varint(*parameter as u32).u8(*mask as u8);
            }
            ButtonKind::Value { parameter, max } => {
                d.varint(*parameter as u32).u8(*max as u8);
            }
            ButtonKind::Dropdown { parameter, options } => {
                d.varint(*parameter as u32).u8(options.len() as u8);
                for o in options {
                    d.utf(o);
                }
            }
        }
        out.object_block(tag::GESTURE_BUTTON, &d);
    }
}

/// `AnimationExporter.ParameterInfo`.
#[derive(Debug, Clone, Copy)]
struct ParamInfo {
    button: usize,
    parameter: i32,
    value: i32,
    bit_mask: bool,
    interpolate: bool,
}

impl ParamInfo {
    fn apply(&self, t: &mut Trigger) {
        t.parameter = self.parameter;
        t.value = self.value;
        t.bit_mask = self.bit_mask;
        t.parameter_interpolate = self.interpolate;
    }
}

/// `ParameterDetails.ParameterAllocator`.
struct Alloc {
    sync: Vec<i32>,
    pose_counter: i32,
    gesture_counter: i32,
    bit_count: i32,
    bit_mask: i32,
    bit_param: usize,
}

impl Alloc {
    fn new() -> Alloc {
        Alloc { sync: vec![0, 0], pose_counter: 1, gesture_counter: 1, bit_count: 0, bit_mask: 0, bit_param: 0 }
    }

    fn byte(&mut self, default: u8) -> i32 {
        self.sync.push(default as i32);
        self.sync.len() as i32 - 1
    }

    fn bit(&mut self, default: bool) -> (i32, i32) {
        if self.bit_count == 0 {
            if self.bit_param != 0 {
                self.sync[self.bit_param] = self.bit_mask;
            }
            self.bit_param = self.byte(0) as usize;
            self.bit_mask = 0;
            self.bit_count = 8;
        }
        self.bit_count -= 1;
        self.bit_mask |= (default as i32) << self.bit_count;
        (self.bit_param as i32, 1 << self.bit_count)
    }

    fn finish(&self) -> Vec<u8> {
        let mut s = self.sync.clone();
        if self.bit_param != 0 {
            s[self.bit_param] = self.bit_mask;
        }
        s.into_iter().map(|b| b as u8).collect()
    }
}

/// `AnimationExporter.Staging`.
struct Staging {
    id: usize,
    play: Vec<usize>,
    staged: Vec<usize>,
    trigger: usize,
}

struct Exporter<'a> {
    project: &'a Project,
    elems: HashMap<i64, Vec<(u32, Elem)>>,
    alloc: Alloc,
    buttons: Vec<(i32, Button)>,
    all_buttons: HashMap<String, ParamInfo>,
    triggers: Vec<Trigger>,
    anim_triggers: Vec<usize>,
    anims: Vec<SerializedAnim>,
    staging_map: HashMap<usize, usize>,
    stagings: Vec<Staging>,
    max_duration: i32,
}

impl<'a> Exporter<'a> {
    fn new(project: &'a Project) -> Exporter<'a> {
        // AnimationExporter.processElements + findPartRef: every element (roots included) by storeID.
        let mut elems: HashMap<i64, Vec<(u32, Elem)>> = HashMap::new();
        walk(project, |node| {
            let (sid, id, e) = match node {
                Node::Root { root, id } => (
                    root.store_id,
                    id,
                    // ElementType.ROOT_PART: no texture, rgb 0.
                    Elem { pos: root.pos, rot: root.rot, rgb: 0, texture: false, recolor: false, hidden: root.hidden },
                ),
                Node::Element { el, id, .. } => (
                    el.store_id,
                    id,
                    Elem {
                        pos: el.pos,
                        rot: el.rotation,
                        rgb: el.color,
                        texture: el.texture,
                        recolor: el.recolor,
                        hidden: el.hidden,
                    },
                ),
            };
            elems.entry(sid).or_default().push((id, e));
        });
        Exporter {
            project,
            elems,
            alloc: Alloc::new(),
            buttons: Vec::new(),
            all_buttons: HashMap::new(),
            triggers: Vec::new(),
            anim_triggers: Vec::new(),
            anims: Vec::new(),
            staging_map: HashMap::new(),
            stagings: Vec::new(),
            max_duration: project.anims.iter().map(|a| a.duration).max().unwrap_or(-1),
        }
    }

    /// `triggers.computeIfAbsent(tr, an::newTrigger)`.
    fn trigger_id(&mut self, tr: Trigger) -> usize {
        match self.triggers.iter().position(|t| t.same(&tr)) {
            Some(i) => i,
            None => {
                self.triggers.push(tr);
                self.triggers.len() - 1
            }
        }
    }

    fn add_button(&mut self, order: i32, b: Button) -> usize {
        self.buttons.push((order, b));
        self.buttons.len() - 1
    }

    /// `AnimationExporter.makeButtonInfo` (editor/AnimationExporter.java:216-282).
    fn make_button_info(&mut self, a: &ProjectAnim) -> Result<ParamInfo> {
        let id = anim_id(&a.display_name);
        match a.kind {
            AnimKind::CustomPose | AnimKind::Gesture => {
                let pose = a.kind == AnimKind::CustomPose;
                let n = if pose {
                    self.alloc.pose_counter += 1;
                    self.alloc.pose_counter - 1
                } else {
                    self.alloc.gesture_counter += 1;
                    self.alloc.gesture_counter - 1
                };
                let kind = if pose { ButtonKind::Pose { id: n } } else { ButtonKind::Gesture { id: n, timeout: 0 } };
                let mut b = Button::new(id, kind);
                b.layer_ctrl = a.layer_controlled;
                b.hidden = a.hidden;
                let button = self.add_button(a.order, b);
                let info = ParamInfo { button, parameter: 0, value: n, bit_mask: false, interpolate: false };
                self.all_buttons.insert(id.into(), info);
                Ok(info)
            }
            AnimKind::Layer => {
                if let Some(group) = &a.group {
                    if !self.all_buttons.contains_key(group) {
                        let parameter = self.alloc.byte(0);
                        let b = Button::new(group, ButtonKind::Dropdown { parameter, options: vec![String::new()] });
                        let button = self.add_button(a.order, b);
                        let info = ParamInfo { button, parameter, value: 0, bit_mask: false, interpolate: false };
                        self.all_buttons.insert(group.clone(), info);
                    }
                    let info = self.all_buttons[group];
                    let ButtonKind::Dropdown { options, parameter } = &mut self.buttons[info.button].1.kind else {
                        return Err(CpmError::Invalid(format!("Animation name conflict {group}")));
                    };
                    options.push(id.into());
                    let pr = ParamInfo {
                        button: info.button,
                        parameter: *parameter,
                        value: options.len() as i32 - 1,
                        bit_mask: false,
                        interpolate: false,
                    };
                    self.all_buttons.insert(id.into(), pr);
                    Ok(pr)
                } else {
                    let (parameter, mask) = self.alloc.bit(a.layer_default > 0.5);
                    let mut b = Button::new(id, ButtonKind::Bool { parameter, mask });
                    b.hidden = a.hidden;
                    let button = self.add_button(a.order, b);
                    let info = ParamInfo { button, parameter, value: mask, bit_mask: true, interpolate: false };
                    self.all_buttons.insert(id.into(), info);
                    Ok(info)
                }
            }
            AnimKind::ValueLayer => {
                // (byte) (layerDefault * 255): float -> int (saturating) -> low 8 bits.
                let parameter = self.alloc.byte((a.layer_default * 255.0) as i32 as u8);
                let mut b = Button::new(id, ButtonKind::Value { parameter, max: a.max_value });
                b.hidden = a.hidden;
                let button = self.add_button(a.order, b);
                let info = ParamInfo { button, parameter, value: 0, bit_mask: false, interpolate: a.interpolate_value };
                self.all_buttons.insert(id.into(), info);
                Ok(info)
            }
            AnimKind::Pose | AnimKind::Setup | AnimKind::Finish => {
                Err(CpmError::Invalid(format!("animation {:?} has no button", a.display_name)))
            }
        }
    }

    /// `EditorAnim.findLinkedAnims`: `p:<pose>`, `c:<custom pose>` or `g:<gesture id>`.
    fn linked(&self, a: &ProjectAnim) -> Vec<usize> {
        let Some((kind, name)) = a.display_name.split_once(':') else { return Vec::new() };
        let all = self.project.anims.iter().enumerate();
        match kind {
            "p" => match VANILLA_POSES.iter().position(|p| *p == name) {
                Some(o) => all.filter(|(_, e)| e.kind == AnimKind::Pose && e.pose == Some(o as u8)).map(|(i, _)| i).collect(),
                None => Vec::new(),
            },
            "c" => all.filter(|(_, e)| e.kind == AnimKind::CustomPose && e.display_name == name).map(|(i, _)| i).collect(),
            "g" => all
                .filter(|(_, e)| !matches!(e.kind, AnimKind::Pose | AnimKind::CustomPose) && anim_id(&e.display_name) == name)
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `AnimationExporter.processAnimation` (editor/AnimationExporter.java:63-151).
    fn process(&mut self, index: usize) -> Result<()> {
        let project = self.project;
        let a = &project.anims[index];
        let mut tr = Trigger::new();
        match (a.kind, a.pose) {
            (AnimKind::Pose, Some(pose)) => {
                tr.pose = Some(pose);
                tr.must_finish = a.must_finish;
            }
            (AnimKind::CustomPose | AnimKind::Gesture | AnimKind::Layer | AnimKind::ValueLayer, _) => {
                tr.looping = if a.kind == AnimKind::Gesture { a.loop_ } else { true };
                let id = anim_id(&a.display_name);
                if let Some(info) = self.all_buttons.get(id) {
                    info.apply(&mut tr);
                } else {
                    let info = self.make_button_info(a)?;
                    let b = &mut self.buttons[info.button].1;
                    if !is_layer(a.kind) {
                        b.layer_ctrl = a.layer_controlled;
                    }
                    b.command = a.command;
                    b.is_property = a.is_property;
                    info.apply(&mut tr);
                    if a.kind == AnimKind::Gesture && !a.loop_ && self.max_duration > 0 {
                        if let ButtonKind::Gesture { timeout, .. } = &mut b.kind {
                            *timeout = (self.max_duration as f32 / 50.0).ceil() as i32 + 5;
                        }
                    }
                }
                tr.anim = Some(a.kind);
                tr.must_finish = a.must_finish;
            }
            (AnimKind::Setup | AnimKind::Finish, _) => {
                let linked = self.linked(a);
                let st = match linked.iter().find_map(|e| self.staging_map.get(e)) {
                    Some(&s) => s,
                    None => {
                        let s = self.stagings.len();
                        self.stagings.push(Staging { id: s, play: Vec::new(), staged: Vec::new(), trigger: 0 });
                        self.staging_map.insert(index, s);
                        s
                    }
                };
                for &e in &linked {
                    self.staging_map.insert(e, st);
                }
                self.staging_map.insert(index, st);
                let s = &mut self.stagings[st];
                for e in linked {
                    if !s.play.contains(&e) {
                        s.play.push(e);
                    }
                }
                s.staged.push(index);
                tr.stage = Some(if a.kind == AnimKind::Setup { STAGE_SETUP } else { STAGE_FINISH });
                tr.staging_id = s.id as i32;
            }
            (AnimKind::Pose, None) => {}
        }
        let trigger = self.trigger_id(tr);
        self.anim_triggers.push(trigger);

        let dynamic = a.kind == AnimKind::Pose && a.pose.is_some_and(|p| Pose::from_ordinal(p).is_dynamic());
        let duration = if dynamic || a.kind == AnimKind::ValueLayer { DYNAMIC_DURATION } else { a.duration as u32 };
        let anim = self.serialize(a, trigger, duration);
        self.anims.push(anim);
        Ok(())
    }

    /// The channel part of `processAnimation` and `addChannel` (AnimationExporter.java:114-173).
    fn serialize(&self, a: &ProjectAnim, trigger: usize, duration: u32) -> SerializedAnim {
        let add = a.additive;
        // AnimFrame.components: element id -> data; a later component for the same element wins.
        let frames: Vec<HashMap<u32, &FrameComponent>> = a
            .frames
            .iter()
            .map(|f| {
                let mut m = HashMap::new();
                for c in &f.components {
                    for (id, _) in self.elems.get(&c.store_id).into_iter().flatten() {
                        m.insert(*id, c);
                    }
                }
                m
            })
            .collect();
        // getComponentsFiltered: elements whose data changes something in some frame.
        let mut comps: Vec<(u32, &Elem)> = Vec::new();
        for (f, m) in a.frames.iter().zip(&frames) {
            for c in &f.components {
                for (id, e) in self.elems.get(&c.store_id).into_iter().flatten() {
                    if !comps.iter().any(|(i, _)| i == id) && has_changes(e, m[id], add) {
                        comps.push((*id, e));
                    }
                }
            }
        }

        let mut out = SerializedAnim {
            trigger,
            priority: a.priority,
            duration,
            additive: add,
            cubes: Vec::new(),
            interp: a.interpolator as u8,
            frames: frames.len(),
            floats: Vec::new(),
            bools: Vec::new(),
        };
        for (k, &(id, e)) in comps.iter().enumerate() {
            let base = k as u32 * CUBE_CHANNELS;
            out.cubes.push(id);
            let any = |test: &dyn Fn(&FrameComponent) -> bool| frames.iter().any(|m| m.get(&id).is_some_and(|d| test(d)));
            // One float track: frame data, else `empty` (additive) or the element's own value.
            let mut track = |ch: u32, part: &dyn Fn(&FrameComponent) -> f32, own: f32, empty: f32| {
                let values = frames.iter().map(|m| m.get(&id).map_or(if add { empty } else { own }, |d| part(d))).collect();
                out.floats.push((base + ch, values));
            };
            if any(&|d| pos_changes(e, d, add)) {
                for i in 0..3 {
                    track(i as u32, &|d| d.pos[i], e.pos[i], 0.0);
                }
            }
            if any(&|d| rot_changes(e, d, add)) {
                for i in 0..3 {
                    track(3 + i as u32, &|d| rad(d.rotation[i]), rad(e.rot[i]), 0.0);
                }
            }
            if any(&|d| color_changes(e, d)) {
                let own = e.color();
                for i in 0..3 {
                    track(6 + i as u32, &|d| d.color[i], own[i], 0.0);
                }
            }
            let vis = any(&|d| vis_changes(e, d));
            if any(&|d| scale_changes(d)) {
                // ModelElement.getScale is always 1.
                for i in 0..3 {
                    track(9 + i as u32, &|d| d.scale[i], 1.0, 1.0);
                }
            }
            if vis {
                let values = frames.iter().map(|m| m.get(&id).map_or(!e.hidden, |d| d.show)).collect();
                out.bools.push((base + FLOAT_CHANNELS, values));
            }
        }
        out
    }

    /// `AnimationExporter.linkStagingAnims` (editor/AnimationExporter.java:179-214), staging part.
    fn link_stagings(&mut self) -> Result<Vec<u32>> {
        let mut staged = Vec::new();
        for s in 0..self.stagings.len() {
            let mut tr = Trigger::new();
            tr.staging_id = self.stagings[s].id as i32;
            tr.stage = Some(STAGE_PLAY);
            tr.must_finish = self.stagings[s].play.iter().any(|&e| self.project.anims[e].must_finish);
            let id = self.trigger_id(tr);
            let mut mains: Vec<usize> = Vec::new();
            for &e in &self.stagings[s].play {
                let t = self.anim_triggers[e];
                if !mains.contains(&t) {
                    mains.push(t);
                }
                self.anims[e].trigger = id;
            }
            let names: Vec<&str> =
                self.stagings[s].staged.iter().map(|&e| self.project.anims[e].display_name.as_str()).collect();
            match mains.as_slice() {
                [main] => {
                    self.stagings[s].trigger = *main;
                    self.triggers[*main].must_finish = false;
                }
                [] => {
                    return Err(CpmError::Invalid(format!("staged animations {names:?} have no main animation")));
                }
                _ => {
                    return Err(CpmError::Invalid(format!(
                        "staged animations {names:?} are linked to several main animations"
                    )));
                }
            }
            staged.push(self.stagings[s].trigger as u32);
        }
        Ok(staged)
    }
}

/// `PlayerSkinLayer.encode` of the `anim_enc.json` lists (`AnimationsLoaderV1.loadEnc`):
/// returns (free layers, layers whose default is on). Unknown layer names are ignored.
fn layer_masks(enc: Option<&Value>) -> (u8, u8) {
    let bit = |n: &str| SKIN_LAYERS.iter().position(|l| *l == n).map_or(0, |i| 1u8 << i);
    let Some(enc) = enc else { return (0, 0) };
    let free = enc
        .get("freeLayers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .fold(0, |m, n| m | bit(n));
    let on = enc
        .get("defaultValues")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, v)| v.as_bool() == Some(true))
        .fold(0, |m, (n, _)| m | bit(n));
    (free, on)
}

/// Writes the `ANIMATION_NEW` part for `project.anims` (`ExporterImpl.java:89-91`).
pub(super) fn write_animations(project: &Project, out: &mut Writer) -> Result<()> {
    // Exporter.check (editor/Exporter.java:369-372): custom animations need the skin-layer
    // encoding chosen in the editor's AnimEncConfigPopup before CPM exports.
    if project.anim_enc.is_none() && project.anims.iter().any(|a| a.kind != AnimKind::Pose) {
        return Err(CpmError::Invalid(
            "the project has custom animations but no animation encoding (anim_enc.json); \
             set one in the CPM editor (Animation encoding settings) and save the project"
                .into(),
        ));
    }

    // AnimLoaderState.loadInfo (parts/anim/AnimLoaderState.java:216-224).
    let (val_mask, def_layers) = layer_masks(project.anim_enc.as_ref());
    let def_mask = def_layers & !val_mask;
    let reset_id = def_mask | val_mask;
    let blank_id = def_mask;

    // AnimLoaderState.loadFromEditor (AnimLoaderState.java:226-235).
    let mut ex = Exporter::new(project);
    for i in 0..project.anims.len() {
        ex.process(i)?;
    }
    let params = ex.alloc.finish();
    let staged = ex.link_stagings()?;
    // sortButtons: stable sort by `order`.
    ex.buttons.sort_by_key(|(order, _)| *order);

    // ModelPartAnimationNew.write (parts/anim/ModelPartAnimationNew.java:41-67).
    let mut w = Writer::new();
    let mut d = Writer::new();
    d.u8(blank_id).u8(reset_id);
    match &project.model_id {
        Some(id) => d.utf(id),
        None => d.varint(0),
    };
    w.object_block(tag::CONTROL_INFO, &d);
    if params != [0, 0] {
        let mut d = Writer::new();
        d.varint(params.len() as u32).bytes(&params).varint(0); // sync defaults, no local ones
        w.object_block(tag::PARAMETERS, &d);
    }
    for t in &ex.triggers {
        t.write(&mut w);
    }
    for a in &ex.anims {
        a.write(&mut w);
    }
    for (_, b) in &ex.buttons {
        b.write(&mut w);
    }
    for id in staged {
        w.object_block(tag::INIT_STAGED_ANIM, Writer::new().varint(id));
    }
    w.object_block(tag::END, &Writer::new());
    out.object_block(ANIMATION_NEW, &w);
    Ok(())
}
