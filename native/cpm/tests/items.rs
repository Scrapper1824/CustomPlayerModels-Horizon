// Held and head items in the mod's runtime: the vanilla layer transforms after each anchor, the
// display contexts, and the worn skull's own placement.
use cpm_core::glam::{Mat4, Vec3};
use cpm_core::{ItemAnchor, ItemSlot};
use horizon_mod_cpm as cpm;

fn near_mat(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-4)
}

fn at(slot: ItemSlot, left: bool, transform: Mat4) -> ItemAnchor {
    ItemAnchor { slot, left, part: None, transform }
}

#[test]
fn hands_take_item_in_hand_layer_offsets() {
    let t = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
    // ItemInHandLayer: rotate X -90, Y 180, then (±1/16, 2/16, -10/16), + for the right arm.
    let layer = |s: f32| {
        Mat4::from_rotation_x(-90f32.to_radians())
            * Mat4::from_rotation_y(180f32.to_radians())
            * Mat4::from_translation(Vec3::new(s / 16.0, 0.125, -0.625))
    };
    let right = cpm::item_draw(&at(ItemSlot::MainHand, false, t), false);
    assert_eq!(right.display, cpm::ItemDisplay::ThirdPersonRightHand);
    assert_eq!(right.seed, 2);
    assert!(near_mat(right.transform, t * layer(1.0)));
    let left = cpm::item_draw(&at(ItemSlot::OffHand, true, t), false);
    assert_eq!(left.display, cpm::ItemDisplay::ThirdPersonLeftHand);
    assert_eq!(left.seed, 1);
    assert_eq!(left.slot, ItemSlot::OffHand);
    assert!(near_mat(left.transform, t * layer(-1.0)));
    // A left-handed main hand is drawn as a left hand.
    let main_left = cpm::item_draw(&at(ItemSlot::MainHand, true, t), false);
    assert_eq!(main_left.display, cpm::ItemDisplay::ThirdPersonLeftHand);
}

#[test]
fn head_takes_custom_head_layer_placement() {
    let t = Mat4::from_rotation_y(0.3);
    let head = cpm::item_draw(&at(ItemSlot::Head, false, t), false);
    assert!(!head.skull);
    assert_eq!(head.display, cpm::ItemDisplay::Head);
    assert_eq!(head.seed, 5);
    let expect = t
        * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0))
        * Mat4::from_rotation_y(180f32.to_radians())
        * Mat4::from_scale(Vec3::new(0.625, -0.625, -0.625));
    assert!(near_mat(head.transform, expect));
}

#[test]
fn worn_skull_takes_the_skull_path() {
    // CustomHeadLayer's skull: translateToHead, then SKULL_SCALE (1.1875) for the skull renderer.
    let t = Mat4::from_rotation_y(0.3);
    let skull = cpm::item_draw(&at(ItemSlot::Head, false, t), true);
    assert!(skull.skull);
    assert_eq!(skull.display, cpm::ItemDisplay::Head);
    assert!(near_mat(skull.transform, t * Mat4::from_scale(Vec3::splat(1.1875))));
    // The flag is for the head only.
    assert!(!cpm::item_draw(&at(ItemSlot::MainHand, false, t), true).skull);
}

#[test]
fn degenerate_anchors_draw_nothing() {
    // A zero mesh scale on an item part: Java draws nothing visible; no NaN pose reaches the game.
    let flat = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
    let ok = Mat4::IDENTITY;
    let draws = cpm::item_draws(&[at(ItemSlot::MainHand, false, flat), at(ItemSlot::OffHand, true, ok)], false);
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].slot, ItemSlot::OffHand);
}

#[test]
fn degenerate_shoulders_draw_no_parrot() {
    // A zero scale on a shoulder part: no parrot drawn there (and no NaN pose reaches the game).
    use cpm_core::ShoulderAnchor;
    let flat = Mat4::from_scale(Vec3::new(0.0, 1.0, 1.0));
    let anchors = [
        ShoulderAnchor { left: true, part: Some(3), transform: flat },
        ShoulderAnchor { left: false, part: None, transform: Mat4::IDENTITY },
    ];
    let draws = cpm::parrot_draws(&anchors);
    assert_eq!(draws, vec![anchors[1]]);
}
