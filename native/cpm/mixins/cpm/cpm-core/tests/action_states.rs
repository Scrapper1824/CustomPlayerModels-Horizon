//! Action, hurt and dying states: `AnimationState.getMainPose` / `collectAnimations` and the
//! `VanillaPose` state getters, with hand-built sets (one cube per pose so each can be seen).

use cpm_core::anim::engine::{Engine, Frame, Hand, PoseInput, UseKind};
use cpm_core::anim::{AnimSet, Clip, Interp, Pose, Target, CHANNELS, GROUPS};

const ROT: [bool; GROUPS] = [false, true, false, false, false];
/// The cube the main poses write.
const MAIN: u32 = 1;

fn rot_clip(id: u32, rx: &[f32], dur: u32, interp: Interp, dynamic: bool) -> Clip {
    let n = rx.len();
    let mut ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0.0; n]);
    ch[3] = rx.to_vec();
    Clip {
        components: vec![id],
        targets: vec![Target::Cube(id as usize)],
        frames: n as u32,
        duration_ms: dur,
        priority: 0,
        additive: false,
        must_finish: false,
        interp,
        dynamic,
        channels: vec![ch],
        show: vec![vec![true; n]],
        present: vec![ROT],
    }
}

/// A constant pose on cube `id`.
fn on(ordinal: u8, id: u32, v: f32) -> (Pose, Clip) {
    (Pose::from_ordinal(ordinal), rot_clip(id, &[v], 1000, Interp::NoInterpolate, false))
}

/// A state-driven pose on cube `id`: frames 0..9 (`VanillaPose.DYNAMIC_DURATION_DIV`).
fn dynamic(ordinal: u8, id: u32) -> (Pose, Clip) {
    let frames: Vec<f32> = (0..10).map(|f| f as f32).collect();
    (Pose::from_ordinal(ordinal), rot_clip(id, &frames, 1001, Interp::NoInterpolate, true))
}

fn set(poses: Vec<(Pose, Clip)>) -> AnimSet {
    AnimSet { poses, gestures: vec![], ..Default::default() }
}

fn at(t: u64) -> PoseInput {
    PoseInput { time_ms: t, on_ground: true, ..Default::default() }
}

fn rot(f: &Frame, id: u32) -> Option<f32> {
    f.parts.get(&id).and_then(|p| p.rot).map(|r| r[0])
}

fn sample(s: &AnimSet, i: PoseInput) -> Frame {
    Engine::new(s.clone()).sample(&i)
}

// VanillaPose ordinals.
const EATING_LEFT: u8 = 14;
const EATING_RIGHT: u8 = 15;
const RETRO_SWIMMING: u8 = 16;
const PUNCH_LEFT: u8 = 19;
const PUNCH_RIGHT: u8 = 20;
const BOW_LEFT: u8 = 26;
const BOW_RIGHT: u8 = 27;
const CROSSBOW_LEFT: u8 = 28;
const CROSSBOW_RIGHT: u8 = 29;
const CROSSBOW_CH_RIGHT: u8 = 31;
const TRIDENT_RIGHT: u8 = 33;
const TRIDENT_SPIN: u8 = 34;
const SPYGLASS_LEFT: u8 = 35;
const HOLDING_LEFT: u8 = 37;
const HOLDING_RIGHT: u8 = 38;
const BLOCKING_LEFT: u8 = 40;
const BLOCKING_RIGHT: u8 = 41;
const HURT: u8 = 44;
const FREEZING: u8 = 46;
const ON_LADDER: u8 = 47;
const CLIMBING_ON_LADDER: u8 = 48;
const TOOT_HORN_RIGHT: u8 = 51;
const BRUSH_LEFT: u8 = 67;
const CRAWLING: u8 = 69;
const SPEAR_RIGHT: u8 = 71;

#[test]
fn punch_progress() {
    let s = set(vec![dynamic(PUNCH_LEFT, 10), dynamic(PUNCH_RIGHT, 11)]);
    // attackTime 0.5 -> 500 ms -> step 500/1001*10 = 4.99 -> frame 4.
    let f = sample(&s, PoseInput { attack: Some((Hand::Main, 0.5)), ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (None, Some(4.0)));
    let f = sample(&s, PoseInput { attack: Some((Hand::Main, 0.95)), ..at(0) });
    assert_eq!(rot(&f, 11), Some(9.0));
    // The off hand swings the other arm; a left main hand swaps both.
    let f = sample(&s, PoseInput { attack: Some((Hand::Off, 0.5)), ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (Some(4.0), None));
    let f = sample(&s, PoseInput { attack: Some((Hand::Main, 0.5)), left_handed: true, ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (Some(4.0), None));
    // `attackTime > 0` only.
    let f = sample(&s, PoseInput { attack: Some((Hand::Main, 0.0)), ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (None, None));
    assert_eq!(rot(&sample(&s, at(0)), 11), None);
}

#[test]
fn bow_pull_progress() {
    let s = set(vec![
        dynamic(BOW_LEFT, 10),
        dynamic(BOW_RIGHT, 11),
        dynamic(CROSSBOW_CH_RIGHT, 12),
        on(HOLDING_LEFT, 13, 1.0),
        on(HOLDING_RIGHT, 14, 1.0),
    ]);
    // bowPullback = clamp(ticks, 0, 20) / 20, from the probe.
    let bow = PoseInput { holding: [true, false], using: Some((Hand::Main, UseKind::Bow, 0.5)), ..at(0) };
    let f = sample(&s, bow);
    assert_eq!((rot(&f, 10), rot(&f, 11), rot(&f, 14)), (None, Some(4.0), None));
    let full = PoseInput { using: Some((Hand::Main, UseKind::Bow, 1.0)), ..bow };
    assert_eq!(rot(&sample(&s, full), 11), Some(9.0));
    // A bow is two-handed: the other hand holds (or is empty), whatever it does.
    let both = PoseInput { holding: [true, true], ..bow };
    let f = sample(&s, both);
    assert_eq!((rot(&f, 11), rot(&f, 13)), (Some(4.0), Some(1.0)));
    // crossbowPullback for a charging crossbow.
    let ch = PoseInput { holding: [true, false], using: Some((Hand::Main, UseKind::Crossbow, 0.25)), ..at(0) };
    // 250 ms -> 2.497 -> frame 2.
    assert_eq!(rot(&sample(&s, ch), 12), Some(2.0));
}

#[test]
fn offhand_use_picks_left_pose() {
    let s = set(vec![
        on(BLOCKING_LEFT, 10, 1.0),
        on(BLOCKING_RIGHT, 11, 1.0),
        on(HOLDING_LEFT, 12, 1.0),
        on(HOLDING_RIGHT, 13, 1.0),
        on(EATING_LEFT, 14, 1.0),
        on(EATING_RIGHT, 15, 1.0),
        on(SPYGLASS_LEFT, 16, 1.0),
        on(BRUSH_LEFT, 17, 1.0),
    ]);
    let shield = PoseInput { holding: [true, true], using: Some((Hand::Off, UseKind::Block, 0.0)), ..at(0) };
    let f = sample(&s, shield);
    assert_eq!((rot(&f, 10), rot(&f, 11), rot(&f, 12), rot(&f, 13)), (Some(1.0), None, None, Some(1.0)));
    // A left main hand puts the off hand on the right.
    let f = sample(&s, PoseInput { left_handed: true, ..shield });
    assert_eq!((rot(&f, 10), rot(&f, 11), rot(&f, 12)), (None, Some(1.0), Some(1.0)));
    // Eating: EATING on the active hand's side, the arm itself holding.
    let eat = PoseInput { holding: [false, true], using: Some((Hand::Off, UseKind::Eat, 0.0)), ..at(0) };
    let f = sample(&s, eat);
    assert_eq!((rot(&f, 14), rot(&f, 15), rot(&f, 12), rot(&f, 13)), (Some(1.0), None, Some(1.0), None));
    let f = sample(&s, PoseInput { using: Some((Hand::Off, UseKind::Spyglass, 0.0)), ..eat });
    assert_eq!((rot(&f, 16), rot(&f, 12)), (Some(1.0), None));
    let f = sample(&s, PoseInput { using: Some((Hand::Off, UseKind::Brush, 0.0)), ..eat });
    assert_eq!(rot(&f, 17), Some(1.0));
}

#[test]
fn holding_poses() {
    let s = set(vec![
        on(HOLDING_LEFT, 10, 1.0),
        on(HOLDING_RIGHT, 11, 1.0),
        on(CROSSBOW_RIGHT, 12, 1.0),
        on(TRIDENT_RIGHT, 13, 1.0),
        on(TOOT_HORN_RIGHT, 14, 1.0),
        on(SPEAR_RIGHT, 15, 1.0),
    ]);
    let f = sample(&s, at(0));
    assert_eq!((rot(&f, 10), rot(&f, 11)), (None, None));
    let f = sample(&s, PoseInput { holding: [true, false], ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (None, Some(1.0)));
    let f = sample(&s, PoseInput { holding: [false, true], ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (Some(1.0), None));
    let f = sample(&s, PoseInput { holding: [true, false], left_handed: true, ..at(0) });
    assert_eq!((rot(&f, 10), rot(&f, 11)), (Some(1.0), None));
    // A use on a hand with nothing in it is no pose (`getArmPose`: EMPTY first).
    let f = sample(&s, PoseInput { using: Some((Hand::Main, UseKind::Trident, 0.0)), ..at(0) });
    assert_eq!(rot(&f, 13), None);
    let used = |k| PoseInput { holding: [true, true], using: Some((Hand::Main, k, 0.0)), ..at(0) };
    // A charged crossbow is held two-handed: the off hand only holds, whatever it does.
    let charged = PoseInput { crossbow_charged: [true, false], ..used(UseKind::Eat) };
    let f = sample(&s, PoseInput { using: Some((Hand::Off, UseKind::Block, 0.0)), ..charged });
    assert_eq!((rot(&f, 12), rot(&f, 11), rot(&f, 10)), (Some(1.0), None, Some(1.0)));
    let f = sample(&s, used(UseKind::Trident));
    assert_eq!((rot(&f, 13), rot(&f, 11)), (Some(1.0), None));
    let f = sample(&s, used(UseKind::Horn));
    assert_eq!(rot(&f, 14), Some(1.0));
    // A spear in use is SPEAR; eating is the plain holding pose.
    let f = sample(&s, used(UseKind::Spear));
    assert_eq!((rot(&f, 15), rot(&f, 11)), (Some(1.0), None));
    let f = sample(&s, used(UseKind::Eat));
    assert_eq!((rot(&f, 15), rot(&f, 11)), (None, Some(1.0)));
}

#[test]
fn ladder_and_climbing() {
    let base = vec![on(1, MAIN, 1.0), on(4, MAIN, 4.0), on(5, MAIN, 5.0)];
    let mut poses = base.clone();
    poses.extend([on(ON_LADDER, MAIN, 47.0), on(CLIMBING_ON_LADDER, MAIN, 48.0)]);
    let s = set(poses);
    let ladder = PoseInput { on_ladder: true, ..at(0) };
    assert_eq!(rot(&sample(&s, ladder), MAIN), Some(47.0));
    // Moving up or down (`|moveAmountY| > 0.05`, from the probe).
    let climb = PoseInput { climbing: true, ..ladder };
    assert_eq!(rot(&sample(&s, climb), MAIN), Some(48.0));
    // Above sneaking, below swimming.
    assert_eq!(rot(&sample(&s, PoseInput { sneaking: true, ..ladder }), MAIN), Some(47.0));
    assert_eq!(rot(&sample(&s, PoseInput { swimming: true, ..climb }), MAIN), Some(5.0));
    // Climbing without its own animation falls back to ON_LADDER; neither: the usual pose.
    let mut poses = base.clone();
    poses.push(on(ON_LADDER, MAIN, 47.0));
    assert_eq!(rot(&sample(&set(poses), climb), MAIN), Some(47.0));
    assert_eq!(rot(&sample(&set(base.clone()), climb), MAIN), Some(1.0));
    // Climbing alone (off a ladder) is nothing.
    assert_eq!(rot(&sample(&s, PoseInput { climbing: true, ..at(0) }), MAIN), Some(1.0));
}

#[test]
fn crawling_pose() {
    let base = vec![on(1, MAIN, 1.0), on(5, MAIN, 5.0), on(13, MAIN, 13.0)];
    let mut poses = base.clone();
    poses.push(on(CRAWLING, MAIN, 69.0));
    let s = set(poses);
    // Crawling is visually swimming out of water.
    let crawl = PoseInput { crawling: true, swimming: true, ..at(0) };
    assert_eq!(rot(&sample(&s, crawl), MAIN), Some(69.0));
    // Without a CRAWLING animation: SWIMMING.
    assert_eq!(rot(&sample(&set(base), crawl), MAIN), Some(5.0));
    // Creative flight comes first.
    assert_eq!(rot(&sample(&s, PoseInput { creative_flying: true, ..crawl }), MAIN), Some(13.0));
}

#[test]
fn trident_spin_and_retro_swimming() {
    let s = set(vec![
        on(1, MAIN, 1.0),
        on(6, MAIN, 6.0),
        on(9, MAIN, 9.0),
        on(TRIDENT_SPIN, MAIN, 34.0),
        on(RETRO_SWIMMING, MAIN, 16.0),
    ]);
    // Riptide: after elytra flight, before falling.
    let spin = PoseInput { trident_spin: true, falling: true, ..at(0) };
    assert_eq!(rot(&sample(&s, spin), MAIN), Some(34.0));
    assert_eq!(rot(&sample(&s, PoseInput { fall_flying: true, ..spin }), MAIN), Some(9.0));
    // In water without swimming: RETRO_SWIMMING when the model has it.
    assert_eq!(rot(&sample(&s, PoseInput { in_water: true, ..at(0) }), MAIN), Some(16.0));
    let plain = set(vec![on(1, MAIN, 1.0)]);
    assert_eq!(rot(&sample(&plain, PoseInput { in_water: true, ..at(0) }), MAIN), Some(1.0));
}

#[test]
fn freezing_progress() {
    // FREEZING has no state getter: it plays by time while frozen at all.
    let s = set(vec![(Pose::from_ordinal(FREEZING), rot_clip(10, &[0.0, 20.0], 1000, Interp::LinearLoop, false))]);
    assert_eq!(rot(&sample(&s, at(0)), 10), None);
    let mut e = Engine::new(s.clone());
    assert_eq!(rot(&e.sample(&PoseInput { freezing: 0.1, ..at(0) }), 10), Some(0.0));
    // A looped 2-frame clip is halfway to its second frame at a quarter of its length.
    let f = e.sample(&PoseInput { freezing: 1.0, ..at(250) });
    assert!((rot(&f, 10).unwrap() - 10.0).abs() < 1e-3);
    // Thawed: off.
    assert_eq!(rot(&e.sample(&PoseInput { freezing: 0.0, ..at(600) }), 10), None);
}

#[test]
fn hurt_and_dying_from_input() {
    let s = set(vec![on(1, MAIN, 1.0), on(6, MAIN, 6.0), on(7, MAIN, 7.0), on(10, MAIN, 10.0), on(HURT, 20, 44.0)]);
    let f = sample(&s, PoseInput { hurt: true, ..at(0) });
    assert_eq!((rot(&f, MAIN), rot(&f, 20)), (Some(1.0), Some(44.0)));
    assert_eq!(rot(&sample(&s, at(0)), 20), None);
    // DYING: after sleeping, before everything else.
    let dying = PoseInput { dying: true, falling: true, hurt: true, ..at(0) };
    let f = sample(&s, dying);
    assert_eq!((rot(&f, MAIN), rot(&f, 20)), (Some(10.0), Some(44.0)));
    assert_eq!(rot(&sample(&s, PoseInput { sleeping: true, ..dying }), MAIN), Some(7.0));
}

#[test]
fn charged_crossbow_is_held_per_hand() {
    let s = set(vec![
        on(CROSSBOW_LEFT, 10, 1.0),
        on(CROSSBOW_RIGHT, 11, 1.0),
        on(HOLDING_LEFT, 12, 1.0),
        on(BLOCKING_RIGHT, 13, 1.0),
        on(EATING_RIGHT, 14, 1.0),
    ]);
    // Right-handed, a charged crossbow in the off hand while the main hand blocks or eats
    // (`getArmPose` is decided for each hand on its own).
    let block = PoseInput {
        holding: [true, true],
        crossbow_charged: [false, true],
        using: Some((Hand::Main, UseKind::Block, 0.0)),
        ..at(0)
    };
    let f = sample(&s, block);
    assert_eq!((rot(&f, 10), rot(&f, 12), rot(&f, 13)), (Some(1.0), None, Some(1.0)));
    let f = sample(&s, PoseInput { using: Some((Hand::Main, UseKind::Eat, 0.0)), ..block });
    assert_eq!((rot(&f, 10), rot(&f, 14)), (Some(1.0), Some(1.0)));
    // The flag needs the stack in that hand.
    let f = sample(&s, PoseInput { holding: [true, false], ..block });
    assert_eq!(rot(&f, 10), None);
}

/// `collectAnimations`: a skull plays WEARING_SKULL, else a helmet ARMOR_HEAD; an elytra plays
/// WEARING_ELYTRA, else a chest item ARMOR_BODY; leggings ARMOR_LEGS; boots ARMOR_BOOTS.
#[test]
fn wearing_poses_follow_what_is_worn() {
    use cpm_core::anim::engine::Equipment;
    const ARMOR_HEAD: u8 = 21;
    const ARMOR_BODY: u8 = 22;
    const ARMOR_LEGS: u8 = 23;
    const ARMOR_BOOTS: u8 = 24;
    const WEARING_ELYTRA: u8 = 25;
    const WEARING_SKULL: u8 = 39;
    let s = set(vec![
        on(ARMOR_HEAD, 21, 1.0),
        on(ARMOR_BODY, 22, 1.0),
        on(ARMOR_LEGS, 23, 1.0),
        on(ARMOR_BOOTS, 24, 1.0),
        on(WEARING_ELYTRA, 25, 1.0),
        on(WEARING_SKULL, 39, 1.0),
    ]);
    let with = |equipment: Equipment| sample(&s, PoseInput { equipment, ..at(100) });
    let none = with(Equipment::default());
    for id in [21, 22, 23, 24, 25, 39] {
        assert_eq!(rot(&none, id), None, "nothing worn: pose on {id} is off");
    }
    let armour = with(Equipment { head: true, chest: true, legs: true, feet: true, ..Default::default() });
    for id in [21, 22, 23, 24] {
        assert!(rot(&armour, id).is_some(), "armour: pose on {id} plays");
    }
    assert_eq!(rot(&armour, 25), None);
    assert_eq!(rot(&armour, 39), None);
    let elytra = with(Equipment { elytra: true, chest_has_wings: true, skull: true, head: true, ..Default::default() });
    assert!(rot(&elytra, 25).is_some(), "an elytra plays WEARING_ELYTRA");
    assert_eq!(rot(&elytra, 22), None, "not ARMOR_BODY with it");
    assert!(rot(&elytra, 39).is_some(), "a skull plays WEARING_SKULL");
    assert_eq!(rot(&elytra, 21), None, "not ARMOR_HEAD with it");
}

/// `collectAnimations`: PARROT_LEFT / PARROT_RIGHT while a parrot sits on that shoulder (no state
/// getter: by time).
#[test]
fn parrot_poses_play() {
    const PARROT_LEFT: u8 = 42;
    const PARROT_RIGHT: u8 = 43;
    let s = set(vec![on(PARROT_LEFT, 42, 1.0), on(PARROT_RIGHT, 43, 1.0)]);
    let none = sample(&s, at(100));
    assert_eq!((rot(&none, 42), rot(&none, 43)), (None, None));
    let left = sample(&s, PoseInput { parrot_left: true, ..at(100) });
    assert_eq!((rot(&left, 42), rot(&left, 43)), (Some(1.0), None));
    let right = sample(&s, PoseInput { parrot_right: true, ..at(100) });
    assert_eq!((rot(&right, 42), rot(&right, 43)), (None, Some(1.0)));
    let both = sample(&s, PoseInput { parrot_left: true, parrot_right: true, ..at(100) });
    assert_eq!((rot(&both, 42), rot(&both, 43)), (Some(1.0), Some(1.0)));
}
