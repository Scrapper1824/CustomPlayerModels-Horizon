//! Animation data shared by the legacy (`ANIMATION_DATA`) and new (`ANIMATION_NEW`) decoders.
//!
//! All angles in this crate are degrees. Java's `IOHelper.readAngle` returns radians;
//! `Reader::angle` and every rotation channel here are degrees instead.

pub mod legacy;
pub mod new;
pub mod engine;
pub mod buttons;

pub use legacy::decode_legacy;
pub use new::decode_new;
pub use buttons::{Action, Button, ButtonKind, ButtonMeta, ButtonState, Cond, Controls};

use crate::model::{Model, BONE_COUNT};

/// `VanillaPose` ordinal -> pose. Ordinals without a named variant are `Other`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pose {
    Standing,
    Walking,
    Running,
    Sneaking,
    Swimming,
    Falling,
    Sleeping,
    Riding,
    Flying,
    Dying,
    CreativeFlying,
    Jumping,
    SneakWalk,
    PunchLeft,
    PunchRight,
    Hurt,
    OnFire,
    /// A named custom pose (`VanillaPose.CUSTOM` plus a name).
    Custom(String),
    /// Any other `VanillaPose` ordinal (e.g. 12 = GLOBAL, 65/66 = head rotation).
    Other(u8),
}

impl Pose {
    pub fn from_ordinal(o: u8) -> Pose {
        match o {
            1 => Pose::Standing,
            2 => Pose::Walking,
            3 => Pose::Running,
            4 => Pose::Sneaking,
            5 => Pose::Swimming,
            6 => Pose::Falling,
            7 => Pose::Sleeping,
            8 => Pose::Riding,
            9 => Pose::Flying,
            10 => Pose::Dying,
            13 => Pose::CreativeFlying,
            17 => Pose::Jumping,
            18 => Pose::SneakWalk,
            19 => Pose::PunchLeft,
            20 => Pose::PunchRight,
            44 => Pose::Hurt,
            45 => Pose::OnFire,
            o => Pose::Other(o),
        }
    }

    /// True for poses whose progress comes from game state rather than time
    /// (`VanillaPose.hasStateGetter`): punch, bow, crossbow charge, speaking,
    /// health/hunger/air, light, head yaw/pitch.
    pub fn is_dynamic(&self) -> bool {
        match self {
            Pose::PunchLeft | Pose::PunchRight => true,
            Pose::Other(o) => matches!(o, 26 | 27 | 30 | 31 | 49 | 59 | 60 | 61 | 64 | 65 | 66),
            _ => false,
        }
    }
}

/// `InterpolatorType` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interp {
    #[default]
    PolyLoop,
    PolySingle,
    LinearLoop,
    LinearSingle,
    NoInterpolate,
    TrigLoop,
    TrigSingle,
}

impl Interp {
    pub fn from_ordinal(o: u8) -> Option<Interp> {
        Some(match o {
            0 => Interp::PolyLoop,
            1 => Interp::PolySingle,
            2 => Interp::LinearLoop,
            3 => Interp::LinearSingle,
            4 => Interp::NoInterpolate,
            5 => Interp::TrigLoop,
            6 => Interp::TrigSingle,
            _ => return None,
        })
    }
}

/// What an animated component id refers to in the model (resolved at decode time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Vanilla bone index 0..6.
    Bone(usize),
    /// Index into `Model::cubes`.
    Cube(usize),
    /// The model has no such element: data is kept, but the engine must skip it.
    Missing,
}

/// Channel order: pos xyz (0..3), rot xyz (3..6, degrees), colour rgb (6..9, 0..255), scale xyz (9..12).
pub const CHANNELS: usize = 12;

/// Channel groups per component (see [`Clip::present`]).
pub const GROUPS: usize = 5;
pub const GROUP_POS: usize = 0;
pub const GROUP_ROT: usize = 1;
pub const GROUP_COLOR: usize = 2;
pub const GROUP_SCALE: usize = 3;
pub const GROUP_VIS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Component ids as stored in the file.
    pub components: Vec<u32>,
    /// Resolved element per component (same length as `components`).
    pub targets: Vec<Target>,
    pub frames: u32,
    pub duration_ms: u32,
    pub priority: i8,
    pub additive: bool,
    /// The clip plays to its end before another may replace it (`MUST_FINISH`).
    pub must_finish: bool,
    pub interp: Interp,
    /// Progress is driven by game state, not time (`duration_ms` is then
    /// `VanillaPose.DYNAMIC_DURATION_DIV` = 1001).
    pub dynamic: bool,
    /// `[component][channel][frame]`, every vector `frames` long.
    pub channels: Vec<[Vec<f32>; CHANNELS]>,
    /// `[component][frame]` visibility.
    pub show: Vec<Vec<bool>>,
    /// `[component][group]`: whether the clip drives each channel group, in the order
    /// [`GROUP_POS`], [`GROUP_ROT`], [`GROUP_COLOR`], [`GROUP_SCALE`], [`GROUP_VIS`].
    /// The engine must skip absent groups and leave those values untouched, so lower-priority
    /// clips or the model's defaults still apply. Their `channels` / `show` entries hold
    /// filler values and must be ignored. Legacy clips have every group present because
    /// legacy CPM fills missing channels. New-format clips mark a group present only when
    /// the file has frame data for it.
    pub present: Vec<[bool; GROUPS]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GestureKind {
    #[default]
    Gesture,
    Pose,
    Layer,
    Value,
    /// `$pre$` staged setup animation that names no pose or gesture of the set (legacy only:
    /// attached stages live in [`Stages`]). Never played.
    Setup,
    /// `$post$` staged finish animation that names no pose or gesture of the set (see `Setup`).
    Finish,
}

/// The setup and finish stages of a pose or gesture (`StagedAnimation`): the setup clips play
/// once before its own clips, the finish clips play once after it stops. Its own clips are the
/// main (PLAY) stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stages {
    /// `$pre$` / `StageType.SETUP` clips.
    pub setup: Vec<Clip>,
    /// `$post$` / `StageType.FINISH` clips.
    pub finish: Vec<Clip>,
}

impl Stages {
    pub fn is_empty(&self) -> bool {
        self.setup.is_empty() && self.finish.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gesture {
    /// Name with the type prefix removed.
    pub name: String,
    pub kind: GestureKind,
    pub looping: bool,
    pub must_finish: bool,
    pub layer: bool,
    pub default_value: u8,
    pub clips: Vec<Clip>,
    /// When the entry plays. Set by the new-format decoder; `None` in a legacy set, where the
    /// engine assigns it as `LegacyAnimationParser` does.
    pub cond: Option<Cond>,
    /// Legacy button metadata (unused for new-format sets, whose buttons are in `controls`).
    pub meta: ButtonMeta,
    /// Setup / finish stages around `clips` (empty when the gesture is not staged).
    pub stages: Stages,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimSet {
    pub poses: Vec<(Pose, Clip)>,
    pub gestures: Vec<Gesture>,
    /// New format: the file's buttons and parameters. `None` for legacy sets: the engine builds
    /// them from `gestures` / `pose_meta` (`LegacyAnimationParser`).
    pub controls: Option<Controls>,
    /// Legacy button metadata of custom poses, by name.
    pub pose_meta: Vec<(String, ButtonMeta)>,
    /// Setup / finish stages of poses (one entry per staged pose); the pose's clips in `poses`
    /// are the main stage.
    pub pose_stages: Vec<(Pose, Stages)>,
}

impl AnimSet {
    /// The stages of `pose`, if it is staged.
    pub fn pose_stages(&self, pose: &Pose) -> Option<&Stages> {
        self.pose_stages.iter().find(|(p, _)| p == pose).map(|(_, s)| s)
    }

    pub(crate) fn pose_stages_mut(&mut self, pose: Pose) -> &mut Stages {
        let i = match self.pose_stages.iter().position(|(p, _)| *p == pose) {
            Some(i) => i,
            None => {
                self.pose_stages.push((pose, Stages::default()));
                self.pose_stages.len() - 1
            }
        };
        &mut self.pose_stages[i].1
    }
}

const LAYER_PREFIX: &str = "$layer$";
const VALUE_LAYER_PREFIX: &str = "$value$";
const SETUP_PREFIX: &str = "$pre$";
const FINISH_PREFIX: &str = "$post$";

/// `AnimationsLoaderV1.getType` + `cleanName`.
pub fn gesture_kind_and_name(raw: &str) -> (GestureKind, String) {
    for (prefix, kind) in [
        (LAYER_PREFIX, GestureKind::Layer),
        (VALUE_LAYER_PREFIX, GestureKind::Value),
        (SETUP_PREFIX, GestureKind::Setup),
        (FINISH_PREFIX, GestureKind::Finish),
    ] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            return (kind, rest.to_string());
        }
    }
    (GestureKind::Gesture, raw.to_string())
}

/// `VanillaPose.DYNAMIC_DURATION_DIV`.
pub(crate) const DYNAMIC_DURATION: u32 = 1001;

/// Resolves component `cid` and builds its channel and visibility vectors (`frames` long).
/// Channels with `None` data are filled the way `ModelPartAnimation.apply` does: the element's
/// own position / rotation / colour, scale 1 and own visibility; additive clips fill position,
/// rotation and scale with 0 instead (colour is never additive).
pub(crate) fn fill_component(
    model: &Model,
    cid: u32,
    add: bool,
    frames: usize,
    data: &[Option<Vec<f32>>; CHANNELS],
    show: Option<Vec<bool>>,
) -> (Target, [Vec<f32>; CHANNELS], Vec<bool>) {
    let (target, own_pos, own_rot, own_rgb, own_visible) = if (cid as usize) < BONE_COUNT {
        let b = &model.bones[cid as usize];
        (Target::Bone(cid as usize), b.pivot, b.rotation, None, true)
    } else if let Some((n, r)) = model.node_index(cid).and_then(|n| model.extra_root(n).map(|r| (n, r))) {
        // A non-player root, like a bone: its default position plus its part position; its
        // visibility and colour are not animatable (RootModelElement.setVisible / setColor).
        let c = &model.cubes[n];
        let p = r.kind.values().pos;
        (Target::Cube(n), [0, 1, 2].map(|k| p[k] + c.pos[k]), c.rotation, None, true)
    } else if let Some(n) = model.node_index(cid) {
        let c = &model.cubes[n];
        (Target::Cube(n), c.pos, c.rotation, c.color, !c.hidden)
    } else {
        (Target::Missing, [0.0; 3], [0.0; 3], None, true)
    };
    let ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|i| {
        if let Some(v) = &data[i] {
            return v.clone();
        }
        let k = i % 3;
        let fill = match i / 3 {
            0 if !add => own_pos[k],
            1 if !add => own_rot[k],
            2 => own_rgb.map_or(0.0, |rgb| ((rgb >> (16 - 8 * k as u32)) & 0xff) as f32),
            3 if !add => 1.0,
            _ => 0.0,
        };
        vec![fill; frames]
    });
    (target, ch, show.unwrap_or_else(|| vec![own_visible; frames]))
}
