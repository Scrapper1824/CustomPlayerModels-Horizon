//! Held and head items: the item parts (`ITEM` effect, `itemRenderer`) and the arm / head
//! fallbacks the mesh gives the game as [`ItemAnchor`]s, and hide-skull.

mod common;

use std::sync::OnceLock;

use cpm_core::anim::engine::{Frame, PartDelta};
use cpm_core::export::{element_ids, export_model};
use cpm_core::format::{read_cpmmodel, RawCube, RawEffect, RawRoot, SkinType};
use cpm_core::mesh::{pose_model_with_items, ItemAnchor, ItemSlot};
use cpm_core::model::{Model, RootType, CUSTOM_PART};
use cpm_core::project::{read_project, Project, ProjectElement, RootPart};
use cpm_core::{BoxSource, Equipment, LoadedModel, PoseInput, PosedBox};
use glam::{Mat4, Vec3};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn rainwing_project() -> &'static Project {
    static P: OnceLock<Project> = OnceLock::new();
    P.get_or_init(|| read_project(&sample("rainwing-old.cpmproject")).unwrap())
}

fn rainwing() -> LoadedModel {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    cpm_core::load(BYTES.get_or_init(|| export_model(rainwing_project()).unwrap())).unwrap()
}

/// The element id of the element at `path` under root `part`.
fn id_at(p: &Project, part: &RootPart, path: &[&str]) -> u32 {
    let root = p.roots.iter().find(|r| &r.part == part).unwrap();
    let mut els: &[ProjectElement] = &root.children;
    let mut found = None;
    for name in path {
        let e = els.iter().find(|e| e.name == *name).unwrap_or_else(|| panic!("no {name}"));
        found = Some(e);
        els = &e.children;
    }
    element_ids(p)[&found.unwrap().store_id][0]
}

fn standing() -> PoseInput {
    PoseInput { on_ground: true, ..Default::default() }
}

fn anchor(items: &[ItemAnchor], slot: ItemSlot) -> &ItemAnchor {
    items.iter().find(|a| a.slot == slot).unwrap_or_else(|| panic!("no {slot:?} anchor"))
}

fn origin(m: Mat4) -> Vec3 {
    m.transform_point3(Vec3::ZERO)
}

fn near_mat(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-4)
}

fn raw_cube(id: u32, parent: u32) -> RawCube {
    RawCube {
        id,
        parent,
        size: [2.0, 3.0, 4.0],
        pos: [0.0; 3],
        offset: [0.0; 3],
        rotation: [0.0; 3],
        tex_size: 1,
        u: 0,
        v: 0,
        rgb: 0,
        mesh_scale: [1.0; 3],
        scale: [1.0; 3],
        mc_scale: 0.0,
        hidden: false,
    }
}

/// Reggie's header with these cubes and effects, vanilla parts kept as `keep`.
fn synthetic(cubes: Vec<RawCube>, effects: Vec<RawEffect>, keep: bool, skin: SkinType) -> Model {
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.cubes = cubes;
    raw.effects = effects;
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [keep; 6];
    raw.part_pos.clear();
    raw.skin_type = skin;
    raw.vanilla_anim_disabled = [false; 6];
    raw.scale = None;
    Model::build(raw).unwrap()
}

/// As [`synthetic`], the vanilla motion off: every part at rest, unturned.
fn still(skin: SkinType) -> Model {
    let mut m = synthetic(Vec::new(), Vec::new(), true, skin);
    m.vanilla_anim_disabled = [true; 6];
    m
}

/// `EffectRenderItem`: cube id, `ItemSlot` ordinal (0 left hand, 1 right hand, 2 head), slot id.
fn item_effect(cube: u32, slot: u8) -> RawEffect {
    RawEffect { kind: 7, payload: vec![cube as u8, slot, 0] }
}

fn vanilla(boxes: &[PosedBox], bone: usize) -> &PosedBox {
    boxes.iter().find(|b| b.source == BoxSource::Vanilla(bone)).unwrap()
}

const BLOCKS: f32 = 16.0;

#[test]
fn rainwing_item_parts_anchor_items() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let p = rainwing_project();
    let right = id_at(p, &RootPart::Vanilla(3), &["Right Hand Item"]);
    let left = id_at(p, &RootPart::Vanilla(2), &["Left Hand Item"]);
    let mut m = rainwing();
    let posed = m.pose_with_items(&PoseInput { time_ms: 1000, ..standing() });
    let main = anchor(&posed.items, ItemSlot::MainHand);
    assert_eq!(main.part, Some(right), "the main hand (right) is drawn at Right Hand Item");
    assert!(!main.left);
    let off = anchor(&posed.items, ItemSlot::OffHand);
    assert_eq!(off.part, Some(left));
    assert!(off.left);
    // Item parts draw nothing themselves.
    assert!(!posed.boxes.iter().any(|b| b.source == BoxSource::Cube(right) || b.source == BoxSource::Cube(left)));
    // The anchor is in blocks: a unit along its x is a block of the model, scaled (15/16 and
    // no render transform here), not a pixel.
    let x = main.transform.transform_vector3(Vec3::X).length();
    let s = 0.9375;
    assert_eq!(m.model.render_transform, None);
    assert!((x - s).abs() < 1e-3, "{x} vs {s}");
    // The hands are either side of the body, the right at -X (the draw frame's +X is the left).
    assert!(origin(main.transform).x < origin(off.transform).x);
}

#[test]
fn item_follows_animated_part() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = synthetic(vec![raw_cube(10, 3)], vec![item_effect(10, 1)], false, SkinType::Default);
    let still = pose_model_with_items(&model, &Frame::default(), &standing());
    let mut frame = Frame::default();
    frame.parts.insert(10, PartDelta { pos: Some([0.0, 5.0, 0.0]), ..Default::default() });
    let moved = pose_model_with_items(&model, &frame, &standing());
    let a = anchor(&still.items, ItemSlot::MainHand);
    let b = anchor(&moved.items, ItemSlot::MainHand);
    assert_eq!(a.part, Some(10));
    // 5 pixels down the (slightly bobbed) arm: 5/16 blocks at 15/16 size, mostly downwards.
    let d = origin(b.transform) - origin(a.transform);
    assert!((d.length() - 5.0 / 16.0 * 0.9375).abs() < 1e-4, "{d}");
    assert!(d.y < -0.28, "{d}");
    // Hidden, the part holds nothing.
    let mut hide = Frame::default();
    hide.parts.insert(10, PartDelta { visible: Some(false), ..Default::default() });
    let hidden = pose_model_with_items(&model, &hide, &standing());
    assert!(hidden.items.iter().all(|a| a.slot != ItemSlot::MainHand));
    // The off hand, with no part of its own, still falls back to its arm.
    assert_eq!(anchor(&hidden.items, ItemSlot::OffHand).part, None);
}

#[test]
fn item_part_offset_and_rotation_apply() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Offset (pixels) and rotation as CPM stores them: translateRotate, then offset / 16.
    let mut c = raw_cube(10, 3);
    c.pos = [1.0, 2.0, 3.0];
    c.rotation = [0.0, 90.0, 0.0];
    c.offset = [0.0, 4.0, 0.0];
    let model = synthetic(vec![c], vec![item_effect(10, 1)], true, SkinType::Default);
    let posed = pose_model_with_items(&model, &Frame::default(), &standing());
    let arm = vanilla(&posed.boxes, 3).transform;
    let expect = arm
        * Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
        * Mat4::from_rotation_y(90f32.to_radians())
        * Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0))
        * Mat4::from_scale(Vec3::splat(BLOCKS));
    assert!(near_mat(anchor(&posed.items, ItemSlot::MainHand).transform, expect));
}

#[test]
fn items_fall_back_to_arms() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    for (skin, shift) in [(SkinType::Default, 0.0), (SkinType::Slim, 0.5)] {
        let model = still(skin);
        let posed = pose_model_with_items(&model, &Frame::default(), &standing());
        // PlayerModel.translateToHand: the arm's pose, half a pixel out for slim arms (here
        // unturned, so the shift is along the box's own x).
        let right = vanilla(&posed.boxes, 3).transform * Mat4::from_translation(Vec3::new(shift, 0.0, 0.0));
        let left = vanilla(&posed.boxes, 2).transform * Mat4::from_translation(Vec3::new(-shift, 0.0, 0.0));
        let main = anchor(&posed.items, ItemSlot::MainHand);
        assert_eq!(main.part, None);
        assert!(!main.left);
        assert!(near_mat(main.transform, right * Mat4::from_scale(Vec3::splat(BLOCKS))), "{skin:?}");
        let off = anchor(&posed.items, ItemSlot::OffHand);
        assert!(off.left);
        assert!(near_mat(off.transform, left * Mat4::from_scale(Vec3::splat(BLOCKS))), "{skin:?}");
    }
}

#[test]
fn left_handed_main_hand() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = synthetic(vec![raw_cube(10, 3)], vec![item_effect(10, 1)], true, SkinType::Default);
    let input = PoseInput { left_handed: true, ..standing() };
    let posed = pose_model_with_items(&model, &Frame::default(), &input);
    // The main hand is the left arm (no part there: its fallback), the off hand the right
    // hand's part.
    let main = anchor(&posed.items, ItemSlot::MainHand);
    assert!(main.left);
    assert_eq!(main.part, None);
    assert!(near_mat(main.transform, vanilla(&posed.boxes, 2).transform * Mat4::from_scale(Vec3::splat(BLOCKS))));
    let off = anchor(&posed.items, ItemSlot::OffHand);
    assert!(!off.left);
    assert_eq!(off.part, Some(10));
}

#[test]
fn head_item_anchor() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Rainwing: its Head Item part.
    let p = rainwing_project();
    let head_path = ["Pelvis", "Waist", "Chest", "Neck 1", "Neck 2", "Neck 3", "Neck 4", "Head", "Head Item"];
    let id = id_at(p, &RootPart::Vanilla(0), &head_path);
    let posed = rainwing().pose_with_items(&standing());
    assert_eq!(anchor(&posed.items, ItemSlot::Head).part, Some(id));
    // Without one: the head bone (CustomHeadLayer's translateToHead).
    let model = synthetic(Vec::new(), Vec::new(), true, SkinType::Default);
    let posed = pose_model_with_items(&model, &Frame::default(), &standing());
    let head = anchor(&posed.items, ItemSlot::Head);
    assert_eq!(head.part, None);
    assert!(near_mat(head.transform, vanilla(&posed.boxes, 0).transform * Mat4::from_scale(Vec3::splat(BLOCKS))));
}

#[test]
fn hide_skull_only_with_flag_and_skull() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let head_boxes = |boxes: &[PosedBox]| {
        boxes
            .iter()
            .filter(|b| matches!(b.source, BoxSource::Vanilla(0) | BoxSource::VanillaLayer(0) | BoxSource::Cube(10)))
            .count()
    };
    let skull = PoseInput { equipment: Equipment { skull: true, ..Default::default() }, ..standing() };
    // The flag is on by default (`hideHeadIfSkull = true`).
    let model = synthetic(vec![raw_cube(10, 0), raw_cube(11, 1)], Vec::new(), true, SkinType::Default);
    let bare = pose_model_with_items(&model, &Frame::default(), &standing());
    assert_eq!(head_boxes(&bare.boxes), 3);
    let worn = pose_model_with_items(&model, &Frame::default(), &skull);
    assert_eq!(head_boxes(&worn.boxes), 0, "a worn skull hides the head");
    assert!(worn.boxes.iter().any(|b| b.source == BoxSource::Cube(11)), "the body still draws");
    assert!(worn.boxes.iter().any(|b| b.source == BoxSource::Vanilla(1)));
    // The skull itself is still placed at the head.
    assert!(worn.items.iter().any(|a| a.slot == ItemSlot::Head));
    // HIDE_SKULL false (written only when the project turns the flag off) keeps the head.
    let off = RawEffect { kind: 8, payload: vec![0] };
    let model = synthetic(vec![raw_cube(10, 0), raw_cube(11, 1)], vec![off], true, SkinType::Default);
    assert!(!model.hide_head_if_skull);
    let worn = pose_model_with_items(&model, &Frame::default(), &skull);
    assert_eq!(head_boxes(&worn.boxes), 3);
}

/// [`synthetic`] with non-player roots.
fn with_roots(cubes: Vec<RawCube>, effects: Vec<RawEffect>, roots: Vec<RawRoot>) -> Model {
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.cubes = cubes;
    raw.effects = effects;
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [true; 6];
    raw.part_pos.clear();
    raw.skin_type = SkinType::Default;
    raw.vanilla_anim_disabled = [false; 6];
    raw.scale = None;
    raw.extra_roots = roots;
    Model::build(raw).unwrap()
}

#[test]
fn item_part_under_armour_root_anchors_only_while_worn() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Right Hand Item inside the Armor Right Arm root (fake cube 20, a CUSTOM_PART child).
    let root = RawRoot { root: RootType::ArmorRightArm.ordinal(), create_from: 20, hidden: false, disable_vanilla: false };
    let model = with_roots(vec![raw_cube(20, CUSTOM_PART), raw_cube(21, 20)], vec![item_effect(21, 1)], vec![root]);
    // Not worn: the root is not drawn, its part never stored: the arm holds the item.
    let bare = pose_model_with_items(&model, &Frame::default(), &standing());
    let main = anchor(&bare.items, ItemSlot::MainHand);
    assert_eq!(main.part, None);
    assert!(near_mat(main.transform, vanilla(&bare.boxes, 3).transform * Mat4::from_scale(Vec3::splat(BLOCKS))));
    // A chestplate worn: the root draws, the item is at its part.
    let chest = PoseInput { equipment: Equipment { chest: true, ..Default::default() }, ..standing() };
    let worn = pose_model_with_items(&model, &Frame::default(), &chest);
    assert_eq!(anchor(&worn.items, ItemSlot::MainHand).part, Some(21));
}

#[test]
fn item_part_nested_in_an_item_part_is_never_reached() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // A head item part with a right hand item part under it: render() does not go below an item
    // part, so the right hand falls back to the arm.
    let model = synthetic(vec![raw_cube(10, 0), raw_cube(11, 10)], vec![item_effect(10, 2), item_effect(11, 1)], true, SkinType::Default);
    let posed = pose_model_with_items(&model, &Frame::default(), &standing());
    assert_eq!(anchor(&posed.items, ItemSlot::Head).part, Some(10));
    assert_eq!(anchor(&posed.items, ItemSlot::MainHand).part, None);
}

#[test]
fn item_part_under_a_hidden_cube_holds_nothing() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // render() still walks a hidden cube's children (storing their item parts as not drawn): the
    // slot has a part, so no fallback either.
    let model = synthetic(vec![raw_cube(10, 3), raw_cube(11, 10)], vec![item_effect(11, 1)], true, SkinType::Default);
    let mut hide = Frame::default();
    hide.parts.insert(10, PartDelta { visible: Some(false), ..Default::default() });
    let posed = pose_model_with_items(&model, &hide, &standing());
    assert!(posed.items.iter().all(|a| a.slot != ItemSlot::MainHand));
    let shown = pose_model_with_items(&model, &Frame::default(), &standing());
    assert_eq!(anchor(&shown.items, ItemSlot::MainHand).part, Some(11));
}

#[test]
fn head_item_part_still_places_under_a_skull_hidden_head() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = synthetic(vec![raw_cube(10, 0)], vec![item_effect(10, 2)], true, SkinType::Default);
    let skull = PoseInput { equipment: Equipment { skull: true, ..Default::default() }, ..standing() };
    let posed = pose_model_with_items(&model, &Frame::default(), &skull);
    assert!(!posed.boxes.iter().any(|b| b.source == BoxSource::Vanilla(0)));
    assert_eq!(anchor(&posed.items, ItemSlot::Head).part, Some(10));
}
