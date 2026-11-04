// The local player's action, hurt and dying state as the probe reads it, mapped into the pose
// input the way CPM's `PlayerProfile.updateFromPlayer` / `PlayerAnimUpdater` fill
// `AnimationState`.
use cpm_core::{Hand, PoseInput, UseKind};
use horizon_mod_cpm::{self as cpm, ActionState, Pull, Use};

fn mapped(s: &ActionState) -> PoseInput {
    let mut i = PoseInput::default();
    cpm::action_input(s, &mut i);
    i
}

#[test]
fn nothing_happening_is_all_off() {
    let i = mapped(&ActionState { max_health: 20.0, health: 20.0, food: 20, light: [0, 15], ..Default::default() });
    assert_eq!((i.attack, i.using, i.holding), (None, None, [false; 2]));
    assert!(!(i.on_ladder || i.climbing || i.crawling || i.trident_spin || i.in_water || i.hurt || i.dying));
    assert_eq!(i.freezing, 0.0);
    assert_eq!((i.health, i.hunger, i.light), (Some(1.0), Some(1.0), Some(1.0)));
}

#[test]
fn swing_is_the_attack() {
    let i = mapped(&ActionState { swing: Some((Hand::Off, 0.4)), ..Default::default() });
    assert_eq!(i.attack, Some((Hand::Off, 0.4)));
}

#[test]
fn pulls_follow_use_ticks() {
    let bow = |ticks| ActionState {
        holding: [true, false],
        using: Some(Use { hand: Hand::Main, kind: Some(UseKind::Bow), ticks, pull: Pull::Bow }),
        ..Default::default()
    };
    // bowPullback = clamp(ticks, 0, 20) / 20.
    assert_eq!(mapped(&bow(5)).using, Some((Hand::Main, UseKind::Bow, 0.25)));
    assert_eq!(mapped(&bow(60)).using, Some((Hand::Main, UseKind::Bow, 1.0)));
    // crossbowPullback = clamp(ticks, 0, charge) / charge.
    let crossbow = ActionState {
        holding: [false, true],
        using: Some(Use { hand: Hand::Off, kind: Some(UseKind::Crossbow), ticks: 10, pull: Pull::Crossbow(25) }),
        ..Default::default()
    };
    assert_eq!(mapped(&crossbow).using, Some((Hand::Off, UseKind::Crossbow, 0.4)));
    // Anything else pulls nothing; an item with no use animation is no use.
    let eat = Use { hand: Hand::Main, kind: Some(UseKind::Eat), ticks: 12, pull: Pull::None };
    assert_eq!(mapped(&ActionState { using: Some(eat), ..Default::default() }).using, Some((Hand::Main, UseKind::Eat, 0.0)));
    let none = Use { kind: None, ..eat };
    assert_eq!(mapped(&ActionState { using: Some(none), ..Default::default() }).using, None);
}

#[test]
fn charged_crossbow_is_held_unless_swinging() {
    let s = ActionState { holding: [true, true], charged: [false, true], ..Default::default() };
    assert_eq!((mapped(&s).crossbow_charged, mapped(&s).using), ([false, true], None));
    let both = ActionState { charged: [true, true], ..s };
    assert_eq!(mapped(&both).crossbow_charged, [true, true]);
    assert_eq!(mapped(&ActionState { swinging: true, ..both }).crossbow_charged, [false, false]);
    // Each hand on its own: the other hand's use is reported beside it.
    let shield = Use { hand: Hand::Main, kind: Some(UseKind::Block), ticks: 3, pull: Pull::None };
    let i = mapped(&ActionState { using: Some(shield), ..s });
    assert_eq!((i.crossbow_charged, i.using), ([false, true], Some((Hand::Main, UseKind::Block, 0.0))));
}

#[test]
fn movement_states() {
    // CLIMBING_ON_LADDER needs |moveAmountY| > 0.05.
    let ladder = ActionState { on_climbable: true, dy: 0.05, ..Default::default() };
    let i = mapped(&ladder);
    assert!(i.on_ladder && !i.climbing);
    assert!(mapped(&ActionState { dy: -0.1, ..ladder }).climbing);
    // isVisuallyCrawling: visually swimming out of water.
    let crawl = mapped(&ActionState { visually_swimming: true, ..Default::default() });
    assert!(crawl.crawling);
    let swim = mapped(&ActionState { visually_swimming: true, in_water: true, ..Default::default() });
    assert!(!swim.crawling && swim.in_water);
    assert!(mapped(&ActionState { spin_attack: true, ..Default::default() }).trident_spin);
}

#[test]
fn values_and_damage() {
    let s = ActionState {
        ticks_frozen: 70,
        health: 5.0,
        max_health: 20.0,
        food: 10,
        light: [12, 3],
        hurt_time: 7,
        ..Default::default()
    };
    let i = mapped(&s);
    // getPercentFrozen: min(ticks, 140) / 140.
    assert_eq!(i.freezing, 0.5);
    assert_eq!(mapped(&ActionState { ticks_frozen: 500, ..s }).freezing, 1.0);
    assert_eq!((i.health, i.hunger, i.light), (Some(0.25), Some(0.5), Some(0.8)));
    assert!(i.hurt && !i.dying);
    assert!(mapped(&ActionState { dead: true, ..s }).dying);
    // No max health reads as full.
    assert_eq!(mapped(&ActionState { max_health: 0.0, ..s }).health, None);
}
