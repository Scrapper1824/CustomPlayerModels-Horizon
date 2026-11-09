//! Animation playback: a port of CPM's `AnimationState.getMainPose` / `collectAnimations`,
//! `AnimationHandler.animate` (stacking, priority, looping, must-finish), `Animation.animate`
//! (frame step, set vs add), `StagedAnimation` (setup / main / finish stages) and the
//! `interpolator` package.
//!
//! [`Engine::sample`] returns a [`Frame`]: the composed result of every playing clip, per
//! element id (bone index 0..6 or cube id). It does not touch the model; Task 8 applies it.

use std::collections::HashMap;
use std::f64::consts::PI;

use super::buttons::{self, byte_to_slider, slider_to_byte, Action, Button, ButtonKind, ButtonState, Cond, Controls};
use super::{
    AnimSet, Clip, GestureKind, Interp, Pose, Stages, Target, CHANNELS, GROUP_COLOR, GROUP_POS, GROUP_ROT, GROUP_SCALE,
    GROUP_VIS,
};

/// What the player wears that CPM draws (`HumanoidArmorLayer` / `WingsLayer`): each worn slot's
/// custom cubes (the model's armour / elytra roots) or, without them, its vanilla boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Equipment {
    /// A helmet (armour with a humanoid layer) in the head slot.
    pub head: bool,
    /// A chestplate in the chest slot.
    pub chest: bool,
    /// Leggings in the legs slot.
    pub legs: bool,
    /// Boots in the feet slot.
    pub feet: bool,
    /// An elytra in the chest slot.
    pub elytra: bool,
    /// A skull or head in the head slot (`AnimationState.hasSkullOnHead`): with the model's
    /// `hideHeadIfSkull`, the head is not drawn.
    pub skull: bool,
    /// The chest item has a `wings` equipment layer (an elytra): `CapeLayer` draws no cape.
    pub chest_has_wings: bool,
    /// The chest item has a `humanoid` equipment layer (a chestplate): `CapeLayer` moves the cape
    /// out by `(0, -0.053125, 0.06875)` blocks.
    pub chest_humanoid: bool,
}

/// The vanilla cape's motion (`AvatarRenderState.capeFlap` / `capeLean` / `capeLean2`, degrees, as
/// `AvatarRenderer.extractCapeState` works them out: Horizon's `AvatarBob::cape`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CapeState {
    pub flap: f32,
    pub lean: f32,
    pub lean2: f32,
}

/// Game state that drives pose selection and state-driven clips. Time comes only from `time_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PoseInput {
    /// Monotonic clock in milliseconds.
    pub time_ms: u64,
    /// Vanilla limb swing (`walkAnimation.position`). Currently unused in practice: it is only
    /// read for WALKING / RUNNING clips marked `dynamic`, which the decoders never produce;
    /// WALKING / RUNNING are time-driven, as in Java.
    pub walk_position: f32,
    /// Horizontal movement amount; above [`MOVE_EPSILON`] the player counts as moving.
    pub walk_speed: f32,
    /// Head yaw relative to the body, degrees (`netHeadYaw`).
    pub head_yaw: f32,
    /// Head pitch, degrees (-90 up .. 90 down).
    pub pitch: f32,
    /// Ticks lived plus partial tick. Currently unused by the engine (no CPM pose reads it).
    pub age: f32,
    pub hurt: bool,
    pub on_ground: bool,
    /// FALLING: Java's `fallDistance > 4` ([`FALL_DISTANCE`] blocks fallen since the player last
    /// stood, was in water or on a ladder: vanilla resets the distance there).
    pub falling: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub swimming: bool,
    /// Elytra flight.
    pub fall_flying: bool,
    pub creative_flying: bool,
    pub sleeping: bool,
    pub riding: bool,
    pub dying: bool,
    pub on_fire: bool,
    /// HEALTH progress 0..1 (health / max health); `None` plays the full-value frame (1.0).
    pub health: Option<f32>,
    /// HUNGER progress 0..1 (food level / 20); `None` = 1.0.
    pub hunger: Option<f32>,
    /// AIR progress 0..1 (air supply / max air); `None` = 1.0.
    pub air: Option<f32>,
    /// LIGHT progress 0..1 (max(sky, block light) / 15); `None` = 1.0.
    pub light: Option<f32>,
    /// Worn armour and elytra (only the mesh reads it).
    pub equipment: Equipment,
    /// The main hand is the left (the player's main hand setting): the mesh's item anchors, and
    /// the side of each hand's pose.
    pub left_handed: bool,
    /// The swing (`swingingHand`, `attackTime`, 0..1): PUNCH_LEFT / RIGHT while above 0.
    pub attack: Option<(Hand, f32)>,
    /// The item in use (`activeHand`, its use animation, its pull: `bowPullback` for a bow,
    /// `crossbowPullback` for a charging crossbow, else 0).
    pub using: Option<(Hand, UseKind, f32)>,
    /// A non-empty stack in the main / off hand (`[Hand::Main, Hand::Off]`).
    pub holding: [bool; 2],
    /// The main / off hand holds a charged crossbow and the player is not swinging
    /// (`ArmPose.CROSSBOW_HOLD`, decided for each hand on its own).
    pub crossbow_charged: [bool; 2],
    /// `onClimbable` (ON_LADDER).
    pub on_ladder: bool,
    /// Moving up or down more than 0.05 blocks a tick (`|moveAmountY| > 0.05`, CLIMBING_ON_LADDER
    /// while on a ladder).
    pub climbing: bool,
    /// `isVisuallyCrawling` (CRAWLING; `swimming` is set too, as Java's `isVisuallySwimming`).
    pub crawling: bool,
    /// Riptide (`Pose.SPIN_ATTACK`, TRIDENT_SPIN).
    pub trident_spin: bool,
    /// `isInWater` (RETRO_SWIMMING when the model has it).
    pub in_water: bool,
    /// Frozen fraction 0..1 (`getPercentFrozen`); FREEZING plays (by time) while above 0.
    pub freezing: f32,
    /// The cape layer's motion; `None`: no cape layer (`CapeLayer` is not run, e.g. a spectator).
    pub cape: Option<CapeState>,
    /// `isInvisible`: `CapeLayer` draws no cape.
    pub invisible: bool,
    /// A parrot sits on the left / right shoulder (`parrotOnLeftShoulder` /
    /// `parrotOnRightShoulder`: PARROT_LEFT / PARROT_RIGHT).
    pub parrot_left: bool,
    pub parrot_right: bool,
}

/// A hand, as the game names it; its side follows [`PoseInput::left_handed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hand {
    #[default]
    Main,
    Off,
}

/// The use animation of the item in use (`HandAnimation`, `ItemUseAnimation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Bow,
    /// Charging.
    Crossbow,
    /// Eat or drink.
    Eat,
    Block,
    Spyglass,
    Trident,
    Horn,
    Brush,
    Spear,
}

/// `PlayerModelSetup.ArmPose` as vanilla's `AvatarRenderer.getArmPose` picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmPose {
    Empty,
    Item,
    Block,
    Bow,
    ThrowSpear,
    CrossbowCharge,
    CrossbowHold,
    Spyglass,
    TootHorn,
    Brush,
    Spear,
}

impl ArmPose {
    fn two_handed(self) -> bool {
        matches!(self, ArmPose::Bow | ArmPose::CrossbowCharge | ArmPose::CrossbowHold)
    }

    /// `getArmPose` for `hand`.
    fn of(i: &PoseInput, hand: Hand) -> ArmPose {
        if !i.holding[hand as usize] {
            return ArmPose::Empty;
        }
        if i.crossbow_charged[hand as usize] {
            return ArmPose::CrossbowHold;
        }
        match i.using {
            Some((h, kind, _)) if h == hand => match kind {
                UseKind::Block => ArmPose::Block,
                UseKind::Bow => ArmPose::Bow,
                UseKind::Trident => ArmPose::ThrowSpear,
                UseKind::Crossbow => ArmPose::CrossbowCharge,
                UseKind::Spyglass => ArmPose::Spyglass,
                UseKind::Horn => ArmPose::TootHorn,
                UseKind::Brush => ArmPose::Brush,
                UseKind::Spear => ArmPose::Spear,
                UseKind::Eat => ArmPose::Item,
            },
            _ => ArmPose::Item,
        }
    }

    /// `AnimationState.getArmPose`: the arm's pose, `left` or right (ordinals).
    fn pose(self, left: bool, i: &PoseInput) -> Option<Pose> {
        let (l, r) = match self {
            ArmPose::Empty => return None,
            ArmPose::Item => (37, 38),
            ArmPose::Block => (40, 41),
            ArmPose::Bow => (26, 27),
            ArmPose::ThrowSpear => (32, 33),
            ArmPose::CrossbowCharge => (30, 31),
            ArmPose::CrossbowHold => (28, 29),
            ArmPose::Spyglass => (35, 36),
            ArmPose::TootHorn => (50, 51),
            ArmPose::Brush => (67, 68),
            // SPEAR while the use animation is SPEAR, else HOLDING.
            ArmPose::Spear if matches!(i.using, Some((_, UseKind::Spear, _))) => (70, 71),
            ArmPose::Spear => (37, 38),
        };
        Some(Pose::from_ordinal(if left { l } else { r }))
    }
}

/// `Hand.of(mainHand, hand)`: true for the left side.
fn left_side(i: &PoseInput, hand: Hand) -> bool {
    i.left_handed == (hand == Hand::Main)
}

/// `walk_speed` above this counts as moving (Java: `|moveAmountX| > 0 || |moveAmountZ| > 0`).
pub const MOVE_EPSILON: f32 = 0.01;
/// `AnimationState.getMainPose`: FALLING once `fallDistance` is above this many blocks.
pub const FALL_DISTANCE: f64 = 4.0;
/// Java: `jumping + 500 > time`.
pub const JUMP_MS: u64 = 500;

/// Composed animation output for one element.
///
/// `None` means no playing clip drives that group: keep the model's own value.
/// Position and rotation (degrees) are either absolute (a non-additive clip set them, later
/// additive clips were added on top) or, when only additive clips touched them, offsets to add
/// to the model's own value. Scale is either absolute or, when only additive clips touched it,
/// a per-axis multiplier of the model's own scale; in an absolute scale a `0` component means
/// "keep the model's own scale on that axis" (Java skips zero scale components).
/// Colour is always absolute (0..255, truncated and clamped like Java). Bones never get colour,
/// visibility or scale (`RootModelElement` ignores them).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PartDelta {
    pub pos: Option<[f32; 3]>,
    pub rot: Option<[f32; 3]>,
    pub scale: Option<[f32; 3]>,
    pub color: Option<[f32; 3]>,
    pub visible: Option<bool>,
    /// True when every driven position / rotation / scale group is relative (see the per-group
    /// flags; use those when a part mixes absolute and relative groups).
    pub additive: bool,
    pub pos_additive: bool,
    pub rot_additive: bool,
    pub scale_additive: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    /// Keyed by element id: bone index 0..6 or cube id.
    pub parts: HashMap<u32, PartDelta>,
}

// ---------------------------------------------------------------------------------------------
// Interpolators

const FULL_ROT: f64 = 360.0;

/// `RotationInterpolator` (degrees): unwraps successive angles so interpolation takes the short way.
struct Setup {
    rot: bool,
    prev: Option<f64>,
    mul: f64,
}

impl Setup {
    fn new(rot: bool) -> Setup {
        Setup { rot, prev: None, mul: 0.0 }
    }

    fn apply(&mut self, value: f64) -> f64 {
        if !self.rot {
            return value;
        }
        let Some(prev) = self.prev else {
            self.prev = Some(value);
            return value;
        };
        let v1 = (value - prev).abs();
        let v2 = (FULL_ROT - prev + value).abs();
        let v3 = (FULL_ROT + prev - value).abs();
        self.prev = Some(value);
        if v1 < v2 && v1 < v3 {
            value + self.mul
        } else if v1 > v2 && v2 < v3 {
            self.mul += FULL_ROT;
            value + self.mul
        } else if v1 > v3 && v2 > v3 {
            self.mul -= FULL_ROT;
            value + self.mul
        } else {
            value
        }
    }
}

enum Curve {
    /// Natural cubic spline over integer knots starting at `x0` (`PolynomialSplineFunction`).
    Spline { x0: f64, polys: Vec<[f64; 4]> },
    /// `Linear(Loop)Interpolator` / `Trigonometric(Loop)Interpolator`.
    Lerp { v: Vec<f32>, looped: bool, trig: bool },
    /// `NoInterpolate`.
    Step(Vec<f32>),
}

impl Curve {
    fn new(interp: Interp, values: &[f32], rot: bool) -> Curve {
        let n = values.len();
        let mut s = Setup::new(rot);
        let at = |i: isize| values[i.rem_euclid(n as isize) as usize] as f64;
        match interp {
            Interp::PolyLoop => {
                s.apply(values[0] as f64);
                s.apply(values[n - 1] as f64);
                let x: Vec<f64> = (0..n + 5).map(|i| i as f64 - 2.0).collect();
                let y: Vec<f64> = (0..n + 5).map(|j| s.apply(at(j as isize + n as isize - 2))).collect();
                Curve::Spline { x0: -2.0, polys: spline(&x, &y) }
            }
            Interp::PolySingle => {
                let x: Vec<f64> = (0..n + 2).map(|i| i as f64 - 1.0).collect();
                let y: Vec<f64> =
                    (0..n + 2).map(|j| s.apply(values[(j as isize - 1).clamp(0, n as isize - 1) as usize] as f64)).collect();
                Curve::Spline { x0: -1.0, polys: spline(&x, &y) }
            }
            Interp::LinearLoop | Interp::TrigLoop => {
                s.apply(at(n as isize - 2));
                s.apply(at(n as isize - 1));
                let v = values.iter().map(|&f| s.apply(f as f64) as f32).collect();
                Curve::Lerp { v, looped: true, trig: interp == Interp::TrigLoop }
            }
            Interp::LinearSingle | Interp::TrigSingle => {
                let v = values.iter().map(|&f| s.apply(f as f64) as f32).collect();
                Curve::Lerp { v, looped: false, trig: interp == Interp::TrigSingle }
            }
            Interp::NoInterpolate => Curve::Step(values.to_vec()),
        }
    }

    fn at(&self, t: f64) -> f32 {
        match self {
            Curve::Spline { x0, polys } => {
                let n = polys.len();
                let t = t.clamp(*x0, *x0 + n as f64);
                let i = ((t - x0).floor() as usize).min(n - 1);
                let [a, b, c, d] = polys[i];
                let u = t - (x0 + i as f64);
                (((d * u + c) * u + b) * u + a) as f32
            }
            Curve::Lerp { v, looped, trig } => {
                let len = v.len();
                let (x, i0, i1) = if *looped {
                    let frm = t.floor().abs() as usize;
                    (t, frm % len, (frm + 1) % len)
                } else {
                    let x = t / len as f64 * (len - 1) as f64;
                    let frm = x.floor().abs() as usize;
                    (x, frm.min(len - 1), (frm + 1).min(len - 1))
                };
                let p = (x - x.floor()) as f32;
                let p = if *trig { (p as f64 * PI / 2.0).sin() as f32 } else { p };
                v[i0] + p * (v[i1] - v[i0])
            }
            Curve::Step(v) => v[(t as i64).unsigned_abs() as usize % v.len()],
        }
    }
}

/// `SplineInterpolator.interpolate`: natural cubic spline, coefficients `[a, b, c, d]` per interval.
fn spline(x: &[f64], y: &[f64]) -> Vec<[f64; 4]> {
    let n = x.len() - 1;
    let h: Vec<f64> = (0..n).map(|i| x[i + 1] - x[i]).collect();
    let mut mu = vec![0.0; n];
    let mut z = vec![0.0; n + 1];
    for i in 1..n {
        let g = 2.0 * (x[i + 1] - x[i - 1]) - h[i - 1] * mu[i - 1];
        mu[i] = h[i] / g;
        z[i] = (3.0 * (y[i + 1] * h[i - 1] - y[i] * (x[i + 1] - x[i - 1]) + y[i - 1] * h[i]) / (h[i - 1] * h[i])
            - h[i - 1] * z[i - 1])
            / g;
    }
    let mut b = vec![0.0; n];
    let mut c = vec![0.0; n + 1];
    let mut d = vec![0.0; n];
    for j in (0..n).rev() {
        c[j] = z[j] - mu[j] * c[j + 1];
        b[j] = (y[j + 1] - y[j]) / h[j] - h[j] * (c[j + 1] + 2.0 * c[j]) / 3.0;
        d[j] = (c[j + 1] - c[j]) / (3.0 * h[j]);
    }
    (0..n).map(|i| [y[i], b[i], c[i], d[i]]).collect()
}

/// Per-clip curves: `[component] -> Some(12 channel curves)`, `None` for missing targets.
struct ClipCurves(Vec<Option<Vec<Curve>>>);

impl ClipCurves {
    fn new(clip: &Clip) -> ClipCurves {
        if clip.frames == 0 {
            return ClipCurves(Vec::new());
        }
        ClipCurves(
            clip.targets
                .iter()
                .zip(&clip.channels)
                .map(|(t, ch)| {
                    (*t != Target::Missing).then(|| {
                        (0..CHANNELS).map(|i| Curve::new(clip.interp, &ch[i], (3..6).contains(&i))).collect()
                    })
                })
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Composition (IModelComponent.setX(add, ...))

#[derive(Default)]
struct Acc {
    pos: Option<([f32; 3], bool)>,
    rot: Option<([f32; 3], bool)>,
    scale: Option<([f32; 3], bool)>,
    color: Option<[f32; 3]>,
    visible: Option<bool>,
}

fn set_or_add(cur: &mut Option<([f32; 3], bool)>, add: bool, v: [f32; 3]) {
    *cur = Some(match (*cur, add) {
        (None, true) => (v, true),
        (Some((c, rel)), true) => ([c[0] + v[0], c[1] + v[1], c[2] + v[2]], rel),
        (_, false) => (v, false),
    });
}

/// `RenderedCube.setRenderScale`: zero components are skipped; additive multiplies.
fn scale(cur: &mut Option<([f32; 3], bool)>, add: bool, v: [f32; 3]) {
    let (mut s, rel) = match (*cur, add) {
        (Some(c), true) => c,
        (None, true) => ([1.0; 3], true),
        (Some((c, false)), false) => (c, false),
        // A relative multiplier cannot be carried into an absolute value without the model.
        (_, false) => ([0.0; 3], false),
    };
    for k in 0..3 {
        if v[k] != 0.0 {
            if add {
                s[k] *= v[k];
            } else {
                s[k] = v[k];
            }
        }
    }
    *cur = Some((s, rel));
}

/// `Animation.animate`.
fn animate(clip: &Clip, curves: &ClipCurves, millis: u64, out: &mut HashMap<u32, Acc>) {
    if clip.frames == 0 || clip.duration_ms == 0 {
        return;
    }
    let dur = clip.duration_ms as u64;
    let step = (millis % dur) as f32 / dur as f32 * clip.frames as f32;
    let t = step as f64;
    let add = clip.additive;
    for (ci, &id) in clip.components.iter().enumerate() {
        let Some(Some(cv)) = curves.0.get(ci) else { continue };
        let bone = matches!(clip.targets[ci], Target::Bone(_));
        let present = clip.present[ci];
        let v3 = |b: usize| [cv[b].at(t), cv[b + 1].at(t), cv[b + 2].at(t)];
        let drives = present[GROUP_POS] || present[GROUP_ROT] || (!bone && present.iter().any(|&p| p));
        if !drives {
            continue;
        }
        let acc = out.entry(id).or_default();
        if present[GROUP_ROT] {
            set_or_add(&mut acc.rot, add, v3(3));
        }
        if present[GROUP_POS] {
            set_or_add(&mut acc.pos, add, v3(0));
        }
        if bone {
            continue;
        }
        if present[GROUP_COLOR] {
            acc.color = Some(v3(6).map(|c| (c as i32).clamp(0, 255) as f32));
        }
        if present[GROUP_VIS] {
            let f = (step as usize).min(clip.frames as usize - 1);
            acc.visible = clip.show[ci].get(f).copied();
        }
        if present[GROUP_SCALE] {
            scale(&mut acc.scale, add, v3(9));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Engine

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Src {
    Pose(usize),
    /// `epoch` makes each `play_gesture` a new animation instance (0 for layers / values).
    Gesture { g: usize, clip: usize, epoch: u64 },
    /// A clip of a staged pose or gesture (`StagedAnimation.Anim`). `clip` indexes `AnimSet::poses`
    /// (pose) or `Gesture::clips` (gesture) for [`Stage::Play`], else the stage's clips. Like
    /// Java's `Anim` objects these are one instance per owner: replaying a gesture whose clips
    /// still play continues them.
    Staged { owner: Owner, stage: Stage, clip: usize },
}

/// The pose (index into `AnimSet::pose_stages`) or gesture a staged clip belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Owner {
    Pose(usize),
    Gesture(usize),
}

/// `StagedAnimation.Stage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    Setup,
    Play,
    Finish,
}

/// `StagedAnimation.AnimData`.
#[derive(Debug, Clone, Copy, Default)]
struct AnimData {
    offset: u64,
    last_frame: u64,
    finished: bool,
    finishing: bool,
}

/// One staged owner's state (`StagedAnimation.currentStage` plus its clips' `AnimData`).
struct Run {
    stage: Stage,
    data: HashMap<(Stage, usize), AnimData>,
    /// Gesture owners: the `play_gesture` activation the stages were prepared for. A new
    /// activation restarts them (setup, main, finish) as a fresh play.
    epoch: u64,
}

impl Run {
    fn data(&mut self, stage: Stage, clip: usize) -> &mut AnimData {
        self.data.entry((stage, clip)).or_default()
    }

    fn finished(&self, stage: Stage, clip: usize) -> bool {
        self.data.get(&(stage, clip)).is_some_and(|d| d.finished)
    }
}

/// `StagedAnimation.Anim.animate` (animation/StagedAnimation.java:110-127). `millis` is the
/// handler time (trigger time for setup / main, raw time for finish). `draw` false skips the
/// clip's own animation (the handler is about to finish it) but keeps the stage bookkeeping.
#[allow(clippy::too_many_arguments)]
fn staged_animate(
    run: &mut Run,
    setups: usize,
    stage: Stage,
    idx: usize,
    clip: &Clip,
    curves: &ClipCurves,
    millis: u64,
    draw: bool,
    acc: &mut HashMap<u32, Acc>,
) {
    let current = run.stage;
    let d = run.data(stage, idx);
    let lf = d.last_frame;
    d.last_frame = millis;
    if current != stage {
        d.offset = millis;
        return;
    }
    let dur = clip.duration_ms as u64;
    let rel = millis.saturating_sub(d.offset);
    match stage {
        Stage::Finish if millis >= d.offset + dur => {}
        Stage::Setup if rel >= dur => {
            d.finished = true;
            if (0..setups).all(|i| run.finished(Stage::Setup, i)) {
                run.stage = Stage::Play;
            }
        }
        // Java divides by the duration here; a zero-length clip just ends.
        Stage::Play if d.finishing && (dur == 0 || rel / dur != lf.saturating_sub(d.offset) / dur) => {
            d.finished = true
        }
        _ => {
            if draw {
                animate(clip, curves, rel, acc);
            }
        }
    }
}

/// Where a clip's time comes from (`AnimationTrigger.getTime`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Progress {
    Time,
    /// State value 0..1, played as `value * DYNAMIC_DURATION_MUL` ms.
    Value(f32),
}

struct Want {
    src: Src,
    /// Staged clips: `StagedAnimation.Anim.mustFinish` (main stage only). The handler-level
    /// `must_finish` of a staged owner is off (the exporter and `LegacyAnimationParser` clear it).
    stage_must_finish: bool,
    looping: bool,
    must_finish: bool,
    rank: u8,
    progress: Progress,
    start: u64,
}

/// `AnimationHandler.PlayingAnim`.
struct Playing {
    src: Src,
    priority: i8,
    /// Tie-break after priority: vanilla poses 0, layers / values 1, gestures 2.
    rank: u8,
    seq: u64,
    start: u64,
    last: u64,
    looping: bool,
    must_finish: bool,
    finished: bool,
    /// Kept only because it is must-finish and no longer wanted.
    lingering: bool,
    progress: Progress,
    /// See [`Want::stage_must_finish`].
    stage_must_finish: bool,
}

#[derive(Debug, Clone, Copy)]
struct Active {
    index: usize,
    start: u64,
    epoch: u64,
}

/// Plays an [`AnimSet`]: one vanilla pose, the always-on poses, layers and one gesture.
pub struct Engine {
    set: AnimSet,
    pose_curves: Vec<ClipCurves>,
    gesture_curves: Vec<Vec<ClipCurves>>,
    /// `[setup, finish]` curves per `AnimSet::pose_stages` entry.
    pose_stage_curves: Vec<[Vec<ClipCurves>; 2]>,
    /// `[setup, finish]` curves per gesture.
    gesture_stage_curves: Vec<[Vec<ClipCurves>; 2]>,
    playing: Vec<Playing>,
    /// Staged owners with playing clips.
    runs: HashMap<Owner, Run>,
    seq: u64,
    epoch: u64,
    active: Option<Active>,
    /// The model's buttons and what they do.
    controls: Controls,
    /// Sync parameter bytes (`AnimationEngine.gestureData`); bytes 0 / 1 (pose / gesture id)
    /// stay at their defaults, see [`Engine::parameters`].
    params: Vec<u8>,
    was_on_ground: Option<bool>,
    jump_at: Option<u64>,
    /// Selected custom pose name (replaces the vanilla main pose until that changes).
    custom: Option<String>,
    /// Vanilla main pose of the previous sample (`PersistentAnimationState.prevPose`).
    prev_main: Option<Pose>,
}

/// Poses Java's `collectAnimations` always (or conditionally) adds, besides the main pose,
/// that this engine can drive. Order matters for equal-priority stacking.
const GLOBAL: Pose = Pose::Other(12);
const HEALTH: Pose = Pose::Other(59);
const HUNGER: Pose = Pose::Other(60);
const AIR: Pose = Pose::Other(61);
const LIGHT: Pose = Pose::Other(64);
const HEAD_YAW: Pose = Pose::Other(65);
const HEAD_PITCH: Pose = Pose::Other(66);
const EATING_LEFT: Pose = Pose::Other(14);
const EATING_RIGHT: Pose = Pose::Other(15);
const RETRO_SWIMMING: Pose = Pose::Other(16);
const TRIDENT_SPIN: Pose = Pose::Other(34);
const FREEZING: Pose = Pose::Other(46);
const ON_LADDER: Pose = Pose::Other(47);
const CLIMBING_ON_LADDER: Pose = Pose::Other(48);
const CRAWLING: Pose = Pose::Other(69);
const ARMOR_HEAD: Pose = Pose::Other(21);
const ARMOR_BODY: Pose = Pose::Other(22);
const ARMOR_LEGS: Pose = Pose::Other(23);
const ARMOR_BOOTS: Pose = Pose::Other(24);
const WEARING_ELYTRA: Pose = Pose::Other(25);
const WEARING_SKULL: Pose = Pose::Other(39);
const PARROT_LEFT: Pose = Pose::Other(42);
const PARROT_RIGHT: Pose = Pose::Other(43);

/// `VanillaPose.DYNAMIC_DURATION_MUL`.
const DYNAMIC_MUL: f32 = 1000.0;
/// Vanilla limb swing frequency (`cos(limbSwing * 0.6662)`): one walk cycle per 2π/0.6662.
const WALK_FREQ: f32 = 0.6662;

impl Engine {
    pub fn new(mut set: AnimSet) -> Engine {
        let pose_curves = set.poses.iter().map(|(_, c)| ClipCurves::new(c)).collect();
        let gesture_curves = set.gestures.iter().map(|g| g.clips.iter().map(ClipCurves::new).collect()).collect();
        let stage_curves =
            |s: &Stages| [s.setup.iter().map(ClipCurves::new).collect(), s.finish.iter().map(ClipCurves::new).collect()];
        let pose_stage_curves = set.pose_stages.iter().map(|(_, s)| stage_curves(s)).collect();
        let gesture_stage_curves = set.gestures.iter().map(|g| stage_curves(&g.stages)).collect();
        // Legacy sets: buttons and parameters as `LegacyAnimationParser` builds them.
        let controls = match set.controls.take() {
            Some(c) => c,
            None => buttons::legacy(&mut set),
        };
        let params = controls.defaults.clone();
        Engine {
            set,
            pose_curves,
            gesture_curves,
            pose_stage_curves,
            gesture_stage_curves,
            playing: Vec::new(),
            runs: HashMap::new(),
            seq: 0,
            epoch: 0,
            active: None,
            controls,
            params,
            was_on_ground: None,
            jump_at: None,
            custom: None,
            prev_main: None,
        }
    }

    pub fn gestures(&self) -> &[super::Gesture] {
        &self.set.gestures
    }

    /// Gesture: start it (or stop it if it is the one playing, like CPM's toggle keybind mode).
    /// Layer: toggle its parameter on / off. Value layers are set through [`Engine::set_value`];
    /// Setup / Finish stages cannot be played on their own. Out-of-range indices are ignored.
    pub fn play_gesture(&mut self, index: usize, now_ms: u64) {
        let Some(g) = self.set.gestures.get(index) else { return };
        match (g.kind, g.cond) {
            (GestureKind::Layer, Some(Cond::Layer { param, value, bitmask })) => {
                let v = self.param(param);
                let nv = if bitmask { v ^ value } else if v == value { 0 } else { value };
                self.set_param(param, nv);
            }
            (GestureKind::Gesture, _) => self.toggle_gesture(index, now_ms),
            // `GestureKind::Pose` is never produced by the decoders (custom poses live in
            // `AnimSet::poses`, see `set_custom_pose`).
            _ => {}
        }
    }

    fn toggle_gesture(&mut self, index: usize, now_ms: u64) {
        if self.active.is_some_and(|a| a.index == index) {
            self.active = None;
        } else {
            self.epoch += 1;
            self.active = Some(Active { index, start: now_ms, epoch: self.epoch });
        }
    }

    /// `AnimationEngine.getGestureValue`: out of range reads 0.
    fn param(&self, p: usize) -> u8 {
        self.params.get(p).copied().unwrap_or(0)
    }

    /// `AnimationEngine.setGestureValue`: out of range is ignored.
    fn set_param(&mut self, p: usize, v: u8) {
        if let Some(b) = self.params.get_mut(p) {
            *b = v;
        }
    }

    /// `LayerTrigger.canPlay`. Java compares the non-bitmask case as signed byte vs int, so
    /// values above 127 never match there; this compares unsigned.
    fn layer_on(&self, cond: Option<Cond>) -> bool {
        match cond {
            Some(Cond::Layer { param, value, bitmask: true }) => self.param(param) & value == value,
            Some(Cond::Layer { param, value, bitmask: false }) => self.param(param) == value,
            _ => false,
        }
    }

    /// The model's gesture-menu buttons, in display order (hidden ones included, flagged).
    pub fn buttons(&self) -> &[Button] {
        &self.controls.buttons
    }

    /// Current state of button `i` (out of range: `Active(false)`).
    pub fn button_state(&self, i: usize) -> ButtonState {
        let Some(a) = self.controls.actions.get(i) else { return ButtonState::Active(false) };
        match a {
            Action::Gesture(gi) => ButtonState::Active(gi.is_some() && self.active.map(|a| a.index) == *gi),
            Action::CustomPose(name) => ButtonState::Active(self.custom.as_ref() == Some(name)),
            // BitmaskParameterValueAction.getValue.
            Action::Bool { param, mask } => ButtonState::Active(self.param(*param) & mask == *mask),
            Action::Value { param, max } => ButtonState::Value(byte_to_slider(self.param(*param), *max)),
            // DropdownButtonData.get: an index past the options shows none (0).
            Action::Dropdown { param } => {
                let v = self.param(*param) as usize;
                let n = match &self.controls.buttons[i].kind {
                    ButtonKind::Dropdown { options } => options.len(),
                    _ => 0,
                };
                ButtonState::Selected(if v < n { v } else { 0 })
            }
            // LegacyDropdownButtonData.get: the first option whose parameter is above 0 (signed).
            Action::LegacyDropdown { params } => ButtonState::Selected(
                params.iter().position(|&p| (self.param(p) as i8) > 0).map_or(0, |k| k + 1),
            ),
        }
    }

    /// Gesture: plays it, or stops it when it is playing. Custom pose: selects it, or clears it
    /// when selected. Toggle: flips it. Sliders and dropdowns ignore it, as do out-of-range indices.
    pub fn press(&mut self, i: usize, now_ms: u64) {
        let Some(a) = self.controls.actions.get(i) else { return };
        match a.clone() {
            Action::Gesture(Some(gi)) => self.toggle_gesture(gi, now_ms),
            Action::CustomPose(name) => {
                self.custom = if self.custom.as_ref() == Some(&name) { None } else { Some(name) };
            }
            // BitmaskParameterValueAction.setValue(-1).
            Action::Bool { param, mask } => self.set_param(param, self.param(param) ^ mask),
            _ => {}
        }
    }

    /// Slider `i` to `v` (clamped to its max; `ValueParameterValueAction.setValue`). Other
    /// buttons ignore it.
    pub fn set_value(&mut self, i: usize, v: u8) {
        if let Some(&Action::Value { param, max }) = self.controls.actions.get(i) {
            self.set_param(param, slider_to_byte(v, max));
        }
    }

    /// Dropdown `i` to `option` (option 0 is "none" for legacy dropdowns and for the exporter's
    /// dropdowns). Out-of-range options and other buttons are ignored.
    pub fn select(&mut self, i: usize, option: usize) {
        let n = match self.controls.buttons.get(i).map(|b| &b.kind) {
            Some(ButtonKind::Dropdown { options }) => options.len(),
            _ => return,
        };
        if option >= n {
            return;
        }
        match self.controls.actions[i].clone() {
            Action::Dropdown { param } => self.set_param(param, option.min(255) as u8),
            Action::LegacyDropdown { params } => {
                for (k, p) in params.into_iter().enumerate() {
                    self.set_param(p, (k + 1 == option) as u8);
                }
            }
            _ => {}
        }
    }

    /// Stops the playing gesture (Reset Gesture).
    pub fn reset_gesture(&mut self) {
        self.stop_gesture();
    }

    /// Clears the selected custom pose (Reset Pose).
    pub fn reset_pose(&mut self) {
        self.custom = None;
    }

    /// The sync parameter bytes in Java's layout (byte 0 pose id, byte 1 gesture id, then the
    /// model's parameters). Bytes 0 and 1 always hold their defaults: the custom pose and the
    /// gesture are not part of the persisted state.
    pub fn parameters(&self) -> Vec<u8> {
        self.params.clone()
    }

    /// Restores [`Engine::parameters`]. Returns false and changes nothing when the length differs
    /// from this model's. Bytes 0 and 1 (pose / gesture id) are not applied.
    pub fn set_parameters(&mut self, p: &[u8]) -> bool {
        if p.len() != self.params.len() {
            return false;
        }
        let keep = self.params.len().min(2);
        self.params[keep..].copy_from_slice(&p[keep..]);
        true
    }

    /// Names of the custom poses (`Pose::Custom` in the set), first occurrence order.
    pub fn custom_poses(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (p, _) in &self.set.poses {
            if let Pose::Custom(n) = p {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }

    /// Selects custom pose `index` (into [`Engine::custom_poses`]); it replaces the vanilla main
    /// pose until the vanilla main pose changes (`AnimationEngine.prepareAnimations`).
    /// `None`, an out-of-range index or the already-selected index clear it.
    pub fn set_custom_pose(&mut self, index: Option<usize>) {
        let name = index.and_then(|i| self.custom_poses().into_iter().nth(i));
        self.custom = if name.is_some() && name == self.custom { None } else { name };
    }

    /// The selected custom pose's name ([`Engine::set_custom_pose`]), if any.
    pub fn custom_pose(&self) -> Option<&str> {
        self.custom.as_deref()
    }

    /// The index (into [`Engine::gestures`]) of the gesture playing, if any.
    pub fn playing_gesture(&self) -> Option<usize> {
        self.active.map(|a| a.index)
    }

    /// Stops the playing gesture. A must-finish gesture plays to the end of its current cycle.
    pub fn stop_gesture(&mut self) {
        self.active = None;
    }

    fn has_pose(&self, p: &Pose) -> bool {
        self.set.poses.iter().any(|(q, _)| q == p)
    }

    /// `AnimationState.getMainPose`.
    fn main_pose(&self, i: &PoseInput) -> Pose {
        let now = i.time_ms;
        let moving = i.walk_speed.abs() > MOVE_EPSILON;
        let jumping = self.jump_at.is_some_and(|j| j + JUMP_MS > now);
        if i.sleeping {
            Pose::Sleeping
        } else if i.dying {
            Pose::Dying
        } else if i.fall_flying {
            Pose::Flying
        } else if i.trident_spin {
            TRIDENT_SPIN
        } else if i.falling {
            Pose::Falling
        } else if i.riding {
            Pose::Riding
        } else if i.creative_flying {
            Pose::CreativeFlying
        } else if i.crawling && self.has_pose(&CRAWLING) {
            CRAWLING
        } else if i.swimming {
            Pose::Swimming
        } else if i.in_water && self.has_pose(&RETRO_SWIMMING) {
            RETRO_SWIMMING
        } else if i.on_ladder && i.climbing && self.has_pose(&CLIMBING_ON_LADDER) {
            CLIMBING_ON_LADDER
        } else if i.on_ladder && self.has_pose(&ON_LADDER) {
            ON_LADDER
        } else if jumping && self.has_pose(&Pose::Jumping) {
            Pose::Jumping
        } else if i.sneaking {
            if moving && self.has_pose(&Pose::SneakWalk) { Pose::SneakWalk } else { Pose::Sneaking }
        } else if i.sprinting {
            Pose::Running
        } else if moving {
            Pose::Walking
        } else {
            Pose::Standing
        }
    }

    /// `AnimationState.collectAnimations` after GLOBAL, up to the always-on value poses: the
    /// swing, what is worn, each arm's pose (left first), eating, hurt, fire and freezing.
    /// Parrots, speaking, GUI, VR, menu and invisibility poses are not driven.
    fn state_poses(i: &PoseInput, out: &mut Vec<Pose>) {
        if let Some((hand, _)) = i.attack.filter(|a| a.1 > 0.0) {
            out.push(if left_side(i, hand) { Pose::PunchLeft } else { Pose::PunchRight });
        }
        // `PlayerProfile`: a skull or head, else any helmet; an elytra (the chest item's glider),
        // else any chest item; leggings; boots.
        let e = &i.equipment;
        if e.skull {
            out.push(WEARING_SKULL);
        } else if e.head {
            out.push(ARMOR_HEAD);
        }
        if e.elytra || e.chest_has_wings {
            out.push(WEARING_ELYTRA);
        } else if e.chest {
            out.push(ARMOR_BODY);
        }
        if e.legs {
            out.push(ARMOR_LEGS);
        }
        if e.feet {
            out.push(ARMOR_BOOTS);
        }
        let main = ArmPose::of(i, Hand::Main);
        let mut off = ArmPose::of(i, Hand::Off);
        // AvatarRenderer.extractRenderState: a two-handed main pose leaves the off hand holding.
        if main.two_handed() {
            off = if i.holding[Hand::Off as usize] { ArmPose::Item } else { ArmPose::Empty };
        }
        let (left, right) = if i.left_handed { (main, off) } else { (off, main) };
        out.extend(left.pose(true, i));
        out.extend(right.pose(false, i));
        if let Some((hand, UseKind::Eat, _)) = i.using {
            out.push(if left_side(i, hand) { EATING_LEFT } else { EATING_RIGHT });
        }
        if i.parrot_left {
            out.push(PARROT_LEFT);
        }
        if i.parrot_right {
            out.push(PARROT_RIGHT);
        }
        if i.hurt {
            out.push(Pose::Hurt);
        }
        if i.on_fire {
            out.push(Pose::OnFire);
        }
        if i.freezing > 0.0 {
            out.push(FREEZING);
        }
    }

    /// `VanillaPose.getTime` for state-driven clips.
    fn pose_progress(pose: &Pose, clip: &Clip, i: &PoseInput) -> Progress {
        if !clip.dynamic {
            return Progress::Time;
        }
        Progress::Value(match pose {
            Pose::Walking | Pose::Running => (i.walk_position * WALK_FREQ / std::f32::consts::TAU).rem_euclid(1.0),
            p if *p == HEAD_YAW => head_yaw_progress(i.head_yaw),
            p if *p == HEAD_PITCH => ((i.pitch + 90.0) / 180.0).clamp(0.0, 1.0),
            p if *p == HEALTH => i.health.unwrap_or(1.0).clamp(0.0, 1.0),
            p if *p == HUNGER => i.hunger.unwrap_or(1.0).clamp(0.0, 1.0),
            p if *p == AIR => i.air.unwrap_or(1.0).clamp(0.0, 1.0),
            p if *p == LIGHT => i.light.unwrap_or(1.0).clamp(0.0, 1.0),
            // attackTime (not clamped: a restarted swing may pass 1, as Java's).
            Pose::PunchLeft | Pose::PunchRight => i.attack.map_or(0.0, |a| a.1),
            Pose::Other(26 | 27) => match i.using {
                Some((_, UseKind::Bow, pull)) => pull,
                _ => 0.0,
            },
            Pose::Other(30 | 31) => match i.using {
                Some((_, UseKind::Crossbow, pull)) => pull,
                _ => 0.0,
            },
            // Speaking: off, progress 0.
            _ => 0.0,
        })
    }

    fn update_ground(&mut self, i: &PoseInput) {
        let airborne = !i.on_ground && !i.creative_flying && !i.fall_flying && !i.swimming && !i.riding && !i.sleeping;
        if airborne && self.was_on_ground == Some(true) {
            self.jump_at = Some(i.time_ms);
        }
        self.was_on_ground = Some(i.on_ground);
    }

    fn wants(&self, i: &PoseInput, main: &Pose) -> Vec<Want> {
        let now = i.time_ms;
        let mut out = Vec::new();
        let mut extra = vec![GLOBAL];
        Self::state_poses(i, &mut extra);
        extra.extend([HEALTH, HUNGER, AIR, LIGHT, HEAD_YAW, HEAD_PITCH]);
        for want in std::iter::once(main).chain(&extra) {
            let Some(k) = self.set.pose_stages.iter().position(|(p, _)| p == want) else {
                for (pi, (p, c)) in self.set.poses.iter().enumerate() {
                    if p == want {
                        out.push(Want {
                            src: Src::Pose(pi),
                            stage_must_finish: false,
                            looping: true,
                            must_finish: c.must_finish,
                            rank: 0,
                            progress: Self::pose_progress(p, c, i),
                            start: now,
                        });
                    }
                }
                continue;
            };
            // Staged pose (`StagedAnimation.getAll`: setup, main, finish): every clip plays while
            // the pose is wanted; the current stage decides which of them animates.
            let owner = Owner::Pose(k);
            let mf = self.play_must_finish(owner);
            let stages = &self.set.pose_stages[k].1;
            let base = Want {
                src: Src::Pose(0),
                stage_must_finish: false,
                looping: true,
                must_finish: false,
                rank: 0,
                progress: Progress::Time,
                start: now,
            };
            for ci in 0..stages.setup.len() {
                out.push(Want { src: Src::Staged { owner, stage: Stage::Setup, clip: ci }, ..base });
            }
            for pi in self.play_clips(owner) {
                let (p, c) = &self.set.poses[pi];
                out.push(Want {
                    src: Src::Staged { owner, stage: Stage::Play, clip: pi },
                    stage_must_finish: mf,
                    progress: Self::pose_progress(p, c, i),
                    ..base
                });
            }
            for ci in 0..stages.finish.len() {
                out.push(Want { src: Src::Staged { owner, stage: Stage::Finish, clip: ci }, ..base });
            }
        }
        for (gi, g) in self.set.gestures.iter().enumerate() {
            let (rank, looping, start, epoch, progress) = match g.kind {
                GestureKind::Layer if self.layer_on(g.cond) => (1, true, now, 0, Progress::Time),
                // ValueTrigger: parameter / 256.
                GestureKind::Value => {
                    let v = match g.cond {
                        Some(Cond::Value { param }) => self.param(param),
                        _ => g.default_value,
                    };
                    (1, true, now, 0, Progress::Value(v as f32 / 256.0))
                }
                GestureKind::Gesture => match self.active {
                    Some(a) if a.index == gi => (2, g.looping, a.start, a.epoch, Progress::Time),
                    _ => continue,
                },
                _ => continue,
            };
            if g.stages.is_empty() {
                for (ci, c) in g.clips.iter().enumerate() {
                    out.push(Want {
                        src: Src::Gesture { g: gi, clip: ci, epoch },
                        stage_must_finish: false,
                        looping,
                        must_finish: g.must_finish || c.must_finish,
                        rank,
                        progress,
                        start,
                    });
                }
                continue;
            }
            let owner = Owner::Gesture(gi);
            let mf = self.play_must_finish(owner);
            let base =
                Want { src: Src::Pose(0), stage_must_finish: false, looping, must_finish: false, rank, progress, start };
            for ci in 0..g.stages.setup.len() {
                out.push(Want { src: Src::Staged { owner, stage: Stage::Setup, clip: ci }, ..base });
            }
            for ci in 0..g.clips.len() {
                out.push(Want { src: Src::Staged { owner, stage: Stage::Play, clip: ci }, stage_must_finish: mf, ..base });
            }
            for ci in 0..g.stages.finish.len() {
                out.push(Want {
                    src: Src::Staged { owner, stage: Stage::Finish, clip: ci },
                    progress: Progress::Time,
                    ..base
                });
            }
        }
        out
    }

    /// `StagedAnimation.Anim.mustFinish` of a staged owner's main clips: the pose's must-finish
    /// flag (`LegacyAnimationParser`: any of its clips; new format: the PLAY trigger's) or the
    /// gesture's.
    fn play_must_finish(&self, owner: Owner) -> bool {
        match owner {
            Owner::Pose(k) => {
                let pose = &self.set.pose_stages[k].0;
                self.set.poses.iter().any(|(p, c)| p == pose && c.must_finish)
            }
            Owner::Gesture(g) => {
                let g = &self.set.gestures[g];
                g.must_finish || g.clips.iter().any(|c| c.must_finish)
            }
        }
    }

    fn stages(&self, owner: Owner) -> &Stages {
        match owner {
            Owner::Pose(k) => &self.set.pose_stages[k].1,
            Owner::Gesture(g) => &self.set.gestures[g].stages,
        }
    }

    /// The main clips of a staged owner (indices into `AnimSet::poses` / `Gesture::clips`).
    fn play_clips(&self, owner: Owner) -> Vec<usize> {
        match owner {
            Owner::Pose(k) => {
                let pose = &self.set.pose_stages[k].0;
                (0..self.set.poses.len()).filter(|&i| self.set.poses[i].0 == *pose).collect()
            }
            Owner::Gesture(g) => (0..self.set.gestures[g].clips.len()).collect(),
        }
    }

    /// `StagedAnimation.Anim.checkAndUpdateRemove` (animation/StagedAnimation.java:130-152):
    /// true when the no-longer-wanted clip stops.
    fn staged_remove(&mut self, owner: Owner, stage: Stage, clip: usize, must_finish: bool) -> bool {
        let play = self.play_clips(owner);
        let mf = self.play_must_finish(owner);
        let finish_dur = match stage {
            Stage::Finish => self.stages(owner).finish[clip].duration_ms as u64,
            _ => 0,
        };
        let Some(run) = self.runs.get_mut(&owner) else { return true };
        match stage {
            Stage::Finish => {
                // The finish stage begins once every must-finish main clip has finished.
                if play.iter().all(|&j| !mf || run.finished(Stage::Play, j)) {
                    run.stage = Stage::Finish;
                    let d = run.data(stage, clip);
                    if d.last_frame >= d.offset + finish_dur {
                        d.offset = 0;
                        return true;
                    }
                }
                false
            }
            Stage::Play if must_finish => {
                let d = run.data(stage, clip);
                d.finishing = true;
                d.finished
            }
            _ => {
                let d = run.data(stage, clip);
                d.finished = false;
                d.offset = 0;
                true
            }
        }
    }

    /// `StagedAnimation.Anim.prepare`: a clip of `owner` (re)started, so the owner starts over
    /// from its setup stage (main stage without one).
    fn staged_prepare(&mut self, owner: Owner) {
        let stage = if self.stages(owner).setup.is_empty() { Stage::Play } else { Stage::Setup };
        let epoch = match owner {
            Owner::Gesture(g) => self.active.filter(|a| a.index == g).map_or(0, |a| a.epoch),
            Owner::Pose(_) => 0,
        };
        let run = self.runs.entry(owner).or_insert_with(|| Run { stage, data: HashMap::new(), epoch });
        run.stage = stage;
        run.epoch = epoch;
        for d in run.data.values_mut() {
            d.offset = 0;
            d.finished = false;
            d.finishing = false;
        }
    }

    /// A staged gesture that just ended by itself starts its finish stage in the same sample (the
    /// removal step of the next sample would otherwise leave one sample with neither stage).
    fn start_finish(&mut self, owner: Owner, now: u64, acc: &mut HashMap<u32, Acc>) {
        let finishes: Vec<(usize, u64, bool)> = self
            .playing
            .iter()
            .filter_map(|p| match p.src {
                Src::Staged { owner: o, stage: Stage::Finish, clip } if o == owner => {
                    Some((clip, now.saturating_sub(p.start), p.stage_must_finish))
                }
                _ => None,
            })
            .collect();
        let mut gone = Vec::new();
        for &(clip, _, mf) in &finishes {
            if self.staged_remove(owner, Stage::Finish, clip, mf) {
                gone.push(clip);
            }
        }
        let setups = self.stages(owner).setup.len();
        let Some(mut run) = self.runs.remove(&owner) else { return };
        if run.stage == Stage::Finish {
            for &(clip, step, _) in finishes.iter().filter(|f| !gone.contains(&f.0)) {
                let (c, curves) = self.clip(Src::Staged { owner, stage: Stage::Finish, clip });
                staged_animate(&mut run, setups, Stage::Finish, clip, c, curves, step, true, acc);
            }
        }
        self.runs.insert(owner, run);
        self.playing.retain(|p| !matches!(p.src, Src::Staged { owner: o, stage: Stage::Finish, clip } if o == owner && gone.contains(&clip)));
    }

    fn clip(&self, src: Src) -> (&Clip, &ClipCurves) {
        match src {
            Src::Pose(i) => (&self.set.poses[i].1, &self.pose_curves[i]),
            Src::Gesture { g, clip, .. } => (&self.set.gestures[g].clips[clip], &self.gesture_curves[g][clip]),
            Src::Staged { owner, stage, clip } => match (owner, stage) {
                (Owner::Pose(_), Stage::Play) => (&self.set.poses[clip].1, &self.pose_curves[clip]),
                (Owner::Gesture(g), Stage::Play) => (&self.set.gestures[g].clips[clip], &self.gesture_curves[g][clip]),
                (_, stage) => {
                    let s = (stage == Stage::Finish) as usize;
                    let stages = self.stages(owner);
                    let clips = if s == 0 { &stages.setup } else { &stages.finish };
                    let curves = match owner {
                        Owner::Pose(k) => &self.pose_stage_curves[k][s],
                        Owner::Gesture(g) => &self.gesture_stage_curves[g][s],
                    };
                    (&clips[clip], &curves[clip])
                }
            },
        }
    }

    /// Advances to `input.time_ms` and returns the composed frame.
    pub fn sample(&mut self, input: &PoseInput) -> Frame {
        let now = input.time_ms;
        self.update_ground(input);
        // A custom pose holds until the vanilla main pose changes.
        let vanilla = self.main_pose(input);
        if self.prev_main.as_ref().is_some_and(|p| *p != vanilla) {
            self.custom = None;
        }
        self.prev_main = Some(vanilla.clone());
        let main = self.custom.clone().map_or(vanilla, Pose::Custom);
        let wants = self.wants(input, &main);

        // A replayed staged gesture starts over (setup, main, finish) even if clips of its
        // previous play are still running (Java would continue them: a must-finish main that
        // already ended would then never restart).
        if let Some(a) = self.active {
            let owner = Owner::Gesture(a.index);
            if self.runs.get(&owner).is_some_and(|r| r.epoch != a.epoch) {
                self.runs.remove(&owner);
                self.playing.retain(|p| !matches!(p.src, Src::Staged { owner: o, .. } if o == owner));
            }
        }

        // Drop what is no longer wanted; must-finish clips stop looping and linger until done.
        let mut kept = Vec::with_capacity(self.playing.len());
        for mut p in std::mem::take(&mut self.playing) {
            if wants.iter().any(|w| w.src == p.src) {
                kept.push(p);
                continue;
            }
            if let Src::Staged { owner, stage, clip } = p.src {
                if !self.staged_remove(owner, stage, clip, p.stage_must_finish) {
                    kept.push(p);
                }
                continue;
            }
            if p.must_finish {
                p.looping = false;
                p.lingering = true;
                if !p.finished {
                    kept.push(p);
                }
            }
        }
        self.playing = kept;
        for w in &wants {
            if let Some(p) = self.playing.iter_mut().find(|p| p.src == w.src) {
                p.progress = w.progress;
                // A staged clip wanted again simply continues (Java: the same `Anim` instance).
                if matches!(p.src, Src::Staged { .. }) {
                    continue;
                }
                // Wanted again while lingering: resume looping (restart if it already ended).
                if p.lingering {
                    p.lingering = false;
                    p.looping = w.looping;
                    if p.finished {
                        p.finished = false;
                        p.start = w.start;
                        p.last = 0;
                    }
                }
                continue;
            }
            self.seq += 1;
            let priority = self.clip(w.src).0.priority;
            self.playing.push(Playing {
                src: w.src,
                priority,
                rank: w.rank,
                seq: self.seq,
                start: w.start,
                last: 0,
                looping: w.looping,
                must_finish: w.must_finish,
                finished: false,
                lingering: false,
                progress: w.progress,
                stage_must_finish: w.stage_must_finish,
            });
            // `PlayingAnim` constructor -> `StagedAnimation.Anim.prepare`.
            if let Src::Staged { owner, .. } = w.src {
                self.staged_prepare(owner);
            }
        }
        self.playing.sort_by_key(|p| (p.priority, p.rank, p.seq));

        let mut acc: HashMap<u32, Acc> = HashMap::new();
        let mut playing = std::mem::take(&mut self.playing);
        let mut runs = std::mem::take(&mut self.runs);
        for p in playing.iter_mut().filter(|p| !p.finished) {
            let (clip, curves) = self.clip(p.src);
            let dur = clip.duration_ms as u64;
            let step = now.saturating_sub(p.start);
            if let Src::Staged { owner, stage, clip: idx } = p.src {
                // A finish clip never ends at the handler level (`getDuration` is
                // `Integer.MAX_VALUE`); the others end like any clip, Java measuring that from
                // when the clip was added, not from when its stage began.
                let hdur = if stage == Stage::Finish { i32::MAX as u64 } else { dur };
                let crossing = !p.looping && hdur > 0 && step / hdur != p.last / hdur;
                let millis = match (stage, p.progress) {
                    (Stage::Finish, _) | (_, Progress::Time) => step,
                    (_, Progress::Value(v)) => (v * DYNAMIC_MUL) as u64,
                };
                let setups = self.stages(owner).setup.len();
                let ended = crossing || (hdur == 0 && !p.looping);
                if let Some(run) = runs.get_mut(&owner) {
                    staged_animate(run, setups, stage, idx, clip, curves, millis, !crossing, &mut acc);
                    // A main clip the handler ends has finished its stage too, so a must-finish
                    // one lets the finish stage begin (Java never marks it and stalls).
                    if ended && stage == Stage::Play {
                        run.data(stage, idx).finished = true;
                    }
                }
                if ended {
                    p.finished = true;
                }
                p.last = step;
                continue;
            }
            if dur == 0 {
                p.finished = !p.looping;
                continue;
            }
            // Java animates first and then marks finished, showing the wrapped frame once;
            // finishing first avoids that one-frame snap back to frame 0.
            if !p.looping && step / dur != p.last / dur {
                p.finished = true;
                p.last = step;
                continue;
            }
            let millis = match p.progress {
                Progress::Time => step,
                Progress::Value(v) => (v * DYNAMIC_MUL) as u64,
            };
            animate(clip, curves, millis, &mut acc);
            p.last = step;
        }
        self.playing = playing;
        runs.retain(|o, _| self.playing.iter().any(|p| matches!(p.src, Src::Staged { owner, .. } if owner == *o)));
        self.runs = runs;

        // A gesture whose clips all finished is over (Java: gesture auto-reset timeout). A
        // staged gesture's finish stage plays after that, so it does not count.
        if let Some(a) = self.active {
            let done = self
                .playing
                .iter()
                .filter(|p| match p.src {
                    Src::Gesture { epoch, .. } => epoch == a.epoch,
                    Src::Staged { owner: Owner::Gesture(g), stage, .. } => g == a.index && stage != Stage::Finish,
                    _ => false,
                })
                .all(|p| p.finished);
            if done {
                self.active = None;
                self.start_finish(Owner::Gesture(a.index), now, &mut acc);
            }
        }

        let parts = acc
            .into_iter()
            .map(|(id, a)| {
                let rel = |g: Option<([f32; 3], bool)>| g.is_some_and(|(_, r)| r);
                let groups = [a.pos, a.rot, a.scale];
                let delta = PartDelta {
                    pos: a.pos.map(|g| g.0),
                    rot: a.rot.map(|g| g.0),
                    scale: a.scale.map(|g| g.0),
                    color: a.color,
                    visible: a.visible,
                    additive: groups.iter().any(|g| g.is_some()) && groups.iter().all(|g| g.is_none_or(|(_, r)| r)),
                    pos_additive: rel(a.pos),
                    rot_additive: rel(a.rot),
                    scale_additive: rel(a.scale),
                };
                (id, delta)
            })
            .collect();
        Frame { parts }
    }
}

/// `VanillaPose.calcHeadYaw` with the body yaw at 0: signed relative yaw mapped to 0..1.
fn head_yaw_progress(head_yaw: f32) -> f32 {
    let mut yaw = head_yaw % 360.0;
    if yaw < 0.0 {
        yaw += 360.0;
    }
    let body = 0.0f32;
    let mut d = (yaw - body).abs();
    if d > 180.0 {
        d = 360.0 - d;
    }
    let cd = (body as f64 - yaw as f64 + 360.0) % 360.0;
    let ccd = (yaw as f64 - body as f64 + 360.0) % 360.0;
    if cd < ccd {
        d = -d;
    }
    ((d + 90.0) / 180.0).clamp(0.0, 1.0)
}
