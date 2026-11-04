//! Armour and elytra roots (`RootModelType`): project reading, export / decode, and the worn
//! equipment the mesh draws (custom cubes in the armour texture, else the vanilla parts).

mod common;

use std::sync::OnceLock;

use cpm_core::anim::engine::{Engine, Frame};
use cpm_core::export::{element_ids, export_model};
use cpm_core::format::{read_cpmmodel, RawRoot};
use cpm_core::mesh::{pose_model, FACE_NORTH};
use cpm_core::model::{Model, RootType, CUSTOM_PART};
use cpm_core::project::{read_project, Project, ProjectElement, ProjectRoot, RootPart};
use cpm_core::{ArmorSlot, BoxSource, BoxTexture, Equipment, LoadedModel, PoseInput, PosedBox};
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

fn wearing(equipment: Equipment) -> PoseInput {
    PoseInput { time_ms: 1000, on_ground: true, equipment, ..Default::default() }
}

const CHEST: Equipment = Equipment { head: false, chest: true, legs: false, feet: false, elytra: false, skull: false, chest_has_wings: false, chest_humanoid: false };

fn textured(boxes: &[PosedBox], t: BoxTexture) -> Vec<&PosedBox> {
    boxes.iter().filter(|b| b.texture == t).collect()
}

fn idx(e: &Engine, name: &str) -> usize {
    e.buttons().iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no button {name:?}"))
}

/// Element ids of the element at `path` (names from the root down) under the root `part`.
fn id_at(p: &Project, part: &RootPart, path: &[&str]) -> Vec<u32> {
    let root = p.roots.iter().find(|r| &r.part == part).unwrap_or_else(|| panic!("no root {part:?}"));
    let mut els: &[ProjectElement] = &root.children;
    let mut found = None;
    for name in path {
        let e = els.iter().find(|e| e.name == *name).unwrap_or_else(|| panic!("no {name} in {path:?}"));
        found = Some(e);
        els = &e.children;
    }
    element_ids(p)[&found.unwrap().store_id].clone()
}

fn near_mat(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn rainwing_keeps_armor_and_elytra_roots() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let p = rainwing_project();
    let kinds: Vec<RootType> = p
        .roots
        .iter()
        .filter_map(|r| match r.part {
            RootPart::Model(t) => Some(t),
            RootPart::Vanilla(_) => None,
        })
        .collect();
    assert_eq!(kinds.len(), 11, "{kinds:?}");
    assert!(kinds.contains(&RootType::ArmorBody) && kinds.contains(&RootType::ElytraLeft));
    let body = p.roots.iter().find(|r| r.part == RootPart::Model(RootType::ArmorBody)).unwrap();
    assert!(!body.children.is_empty());
    assert!(body.hidden, "rainwing's armour roots are hidden (show: false)");
    assert!(!p.warnings.iter().any(|w| w.contains("armor") || w.contains("elytra")), "{:?}", p.warnings);

    let m = rainwing();
    let decoded: Vec<RootType> = m.model.extra_roots.iter().map(|r| r.kind).collect();
    assert_eq!(decoded, kinds);
    let r = m.model.extra_roots.iter().find(|r| r.kind == RootType::ArmorBody).unwrap();
    assert!(!m.model.cubes[r.node].children.is_empty());
    assert!(r.hidden);
    assert!(!m.model.roots_by_bone[1].contains(&r.node), "a root's fake cube is not a body cube");
}

#[test]
fn nothing_worn_draws_no_equipment() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let boxes = m.pose(&wearing(Equipment::default()));
    assert!(!boxes.is_empty());
    assert!(boxes.iter().all(|b| b.texture == BoxTexture::Skin), "only skin boxes");
    assert!(boxes.iter().all(|b| !matches!(b.source, BoxSource::RootVanilla(_))));
}

#[test]
fn chestplate_draws_armor_body_cubes_on_the_dragon_chest() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let p = rainwing_project();
    let mut m = rainwing();
    let boxes = m.pose(&wearing(CHEST));
    let armor = textured(&boxes, BoxTexture::Armor(ArmorSlot::Chestplate));
    assert!(!armor.is_empty());
    assert!(armor.iter().all(|b| b.texture_px == [64, 32] && matches!(b.source, BoxSource::Cube(_))));
    // The roots are hidden: no vanilla chestplate, and no other slot.
    assert!(boxes.iter().all(|b| !matches!(b.source, BoxSource::RootVanilla(_))));
    assert!(boxes.iter().all(|b| matches!(b.texture, BoxTexture::Skin | BoxTexture::Armor(ArmorSlot::Chestplate))));

    // armor_body/Pelvis/Waist/Chest copies body/Pelvis/Waist/Chest (position, rotation, scale x):
    // posed on the same body pose, it lies exactly on the dragon's chest, walking or sneaking.
    let armor_chest = id_at(p, &RootPart::Model(RootType::ArmorBody), &["Pelvis", "Waist", "Chest"]);
    let chest = id_at(p, &RootPart::Vanilla(1), &["Pelvis", "Waist", "Chest"]);
    for input in [wearing(CHEST), PoseInput { walk_speed: 0.8, walk_position: 3.0, sneaking: true, ..wearing(CHEST) }] {
        let boxes = m.pose(&input);
        let find = |id: u32| boxes.iter().find(|b| b.source == BoxSource::Cube(id)).unwrap_or_else(|| panic!("no box {id}"));
        let (a, c) = (find(armor_chest[0]), find(chest[0]));
        assert_eq!(a.texture, BoxTexture::Armor(ArmorSlot::Chestplate));
        assert_eq!(c.texture, BoxTexture::Skin);
        assert!(near_mat(a.transform, c.transform), "{:?}\n{:?}", a.transform, c.transform);
    }
}

#[test]
fn armor_toggle_hides_the_armor_cubes() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let shown = textured(&m.pose(&wearing(CHEST)), BoxTexture::Armor(ArmorSlot::Chestplate)).len();
    assert!(shown > 0);
    let i = idx(&m.engine, "Armor toggle");
    m.engine.press(i, 0);
    let boxes = m.pose(&wearing(CHEST));
    assert_eq!(textured(&boxes, BoxTexture::Armor(ArmorSlot::Chestplate)).len(), 0);
    m.engine.press(i, 0);
    assert_eq!(textured(&m.pose(&wearing(CHEST)), BoxTexture::Armor(ArmorSlot::Chestplate)).len(), shown);
}

/// ReggieTheRat's header (part positions, texture) with every vanilla part kept and no cubes,
/// roots or animation.
fn plain_model() -> Model {
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.cubes.clear();
    raw.effects.clear();
    raw.extra_roots.clear();
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [true; 6];
    Model::build(raw).unwrap()
}

fn input(e: Equipment) -> PoseInput {
    PoseInput { on_ground: true, equipment: e, ..Default::default() }
}

#[test]
fn helmet_without_custom_armor_draws_the_vanilla_helmet_on_the_head() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = plain_model();
    let helmet = Equipment { head: true, ..Default::default() };
    let boxes = pose_model(&model, &Frame::default(), &input(helmet));
    let worn: Vec<&PosedBox> = boxes.iter().filter(|b| b.texture != BoxTexture::Skin).collect();
    assert_eq!(worn.len(), 1);
    let h = worn[0];
    assert_eq!(h.source, BoxSource::RootVanilla(RootType::ArmorHelmet));
    assert_eq!(h.texture, BoxTexture::Armor(ArmorSlot::Helmet));
    assert_eq!(h.texture_px, [64, 32]);
    // On the head's transform; (-4,-8,-4) 8x8x8 inflated by 1; front face from (8, 8).
    let head = boxes.iter().find(|b| b.source == BoxSource::Vanilla(0)).unwrap();
    assert!(near_mat(h.transform, head.transform));
    let xs: Vec<f32> = h.faces.iter().flatten().flat_map(|f| f.corners.map(|c| c[0])).collect();
    let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    assert_eq!((lo, hi), (-5.0, 5.0));
    let north = h.faces[FACE_NORTH].unwrap();
    assert_eq!(north.uv, [8.0, 8.0, 16.0, 16.0]);
    // Nothing worn, nothing drawn.
    let none = pose_model(&model, &Frame::default(), &input(Equipment::default()));
    assert!(none.iter().all(|b| b.texture == BoxTexture::Skin));
}

#[test]
fn every_slot_draws_its_vanilla_parts() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let model = plain_model();
    let all = Equipment { head: true, chest: true, legs: true, feet: true, elytra: true, skull: false, ..Equipment::default() };
    let boxes = pose_model(&model, &Frame::default(), &input(all));
    let count = |t: BoxTexture| boxes.iter().filter(|b| b.texture == t).count();
    assert_eq!(count(BoxTexture::Armor(ArmorSlot::Helmet)), 1);
    assert_eq!(count(BoxTexture::Armor(ArmorSlot::Chestplate)), 3);
    assert_eq!(count(BoxTexture::Armor(ArmorSlot::Leggings)), 3);
    assert_eq!(count(BoxTexture::Armor(ArmorSlot::Boots)), 2);
    assert_eq!(count(BoxTexture::Elytra), 2);
    // Leggings are inflated by 0.5, on the leg's transform.
    let leg = boxes.iter().find(|b| b.source == BoxSource::RootVanilla(RootType::ArmorLeftLeg)).unwrap();
    let skin_leg = boxes.iter().find(|b| b.source == BoxSource::Vanilla(4)).unwrap();
    assert!(near_mat(leg.transform, skin_leg.transform));
    let ys: Vec<f32> = leg.faces.iter().flatten().flat_map(|f| f.corners.map(|c| c[1])).collect();
    assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -0.5);
    // Elytra wings: 64x32, behind the body.
    let wing = boxes.iter().find(|b| b.source == BoxSource::RootVanilla(RootType::ElytraLeft)).unwrap();
    assert_eq!(wing.texture_px, [64, 32]);
}

// ---------------------------------------------------------------------------------------------
// Export / decode

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&vec![255; (w * h * 4) as usize]).unwrap();
    }
    out
}

fn root_of(part: RootPart, store_id: i64) -> ProjectRoot {
    ProjectRoot {
        part,
        store_id,
        duplicated: false,
        pos: [0.0; 3],
        rot: [0.0; 3],
        hidden: false,
        disable_vanilla_anim: false,
        children: Vec::new(),
    }
}

fn cube(store_id: i64) -> ProjectElement {
    // The rainwing project's first armour cube, renamed: a plain textured cube.
    let mut e = rainwing_project()
        .roots
        .iter()
        .find(|r| r.part == RootPart::Model(RootType::ArmorBody))
        .unwrap()
        .children[0]
        .clone();
    e.name = format!("e{store_id}");
    e.store_id = store_id;
    e.children.clear();
    e.copy_transform = None;
    e.pos = [0.0; 3];
    e.rotation = [0.0; 3];
    e.scale = [1.0; 3];
    e
}

#[test]
fn armor_root_roundtrip() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut p = rainwing_project().clone();
    p.anims.clear();
    p.anim_enc = None;
    p.roots.truncate(6);
    for r in &mut p.roots {
        r.children.clear();
    }
    let mut armor = root_of(RootPart::Model(RootType::ArmorHelmet), 900);
    armor.pos = [0.0, -2.0, 0.0];
    armor.children.push(cube(901));
    let mut wing = root_of(RootPart::Model(RootType::ElytraLeft), 902);
    wing.hidden = true;
    p.roots.push(armor);
    p.roots.push(wing);
    p.elytra_png = Some(png(64, 32));
    p.elytra_size = [64, 32];

    let ids = element_ids(&p);
    assert_eq!(ids[&900], vec![10]);
    assert_eq!(ids[&901], vec![11]);
    assert_eq!(ids[&902], vec![12]);

    let bytes = export_model(&p).unwrap();
    let raw = read_cpmmodel(&bytes).unwrap().data;
    // ExportHelper.flattenElements: a fake cube per root, on CUSTOM_PART at the root's position.
    let fake = raw.cubes.iter().find(|c| c.id == 10).unwrap();
    assert_eq!(fake.parent, CUSTOM_PART);
    assert_eq!(fake.size, [0.0; 3]);
    assert!((fake.pos[1] + 2.0).abs() < 1e-3);
    assert_eq!(raw.cubes.iter().find(|c| c.id == 11).unwrap().parent, 10);
    assert_eq!(
        raw.extra_roots,
        vec![
            RawRoot { root: RootType::ArmorHelmet.ordinal(), create_from: 10, hidden: false, disable_vanilla: false },
            RawRoot { root: RootType::ElytraLeft.ordinal(), create_from: 12, hidden: true, disable_vanilla: false },
        ]
    );
    assert_eq!(raw.elytra_texture.as_ref().unwrap().declared, [64, 32]);

    let m = cpm_core::load(&bytes).unwrap();
    let kinds: Vec<(RootType, bool)> = m.model.extra_roots.iter().map(|r| (r.kind, r.hidden)).collect();
    assert_eq!(kinds, vec![(RootType::ArmorHelmet, false), (RootType::ElytraLeft, true)]);
    assert!(m.model.roots_by_bone[1].is_empty());
    assert_eq!(m.elytra.as_ref().unwrap().png_px, [64, 32]);

    // Worn: the helmet root (not hidden) draws the vanilla helmet and its cube; the hidden wing
    // root draws nothing (no cubes), the right wing (no root) its vanilla part.
    let mut m = m;
    let all = Equipment { head: true, elytra: true, ..Default::default() };
    let boxes = m.pose(&PoseInput { on_ground: true, equipment: all, ..Default::default() });
    let src: Vec<BoxSource> = boxes.iter().filter(|b| b.texture != BoxTexture::Skin).map(|b| b.source).collect();
    assert_eq!(
        src,
        vec![
            BoxSource::RootVanilla(RootType::ElytraRight),
            BoxSource::RootVanilla(RootType::ArmorHelmet),
            BoxSource::Cube(11),
        ]
    );
    let helmet = boxes.iter().find(|b| b.source == BoxSource::RootVanilla(RootType::ArmorHelmet)).unwrap();
    let cube = boxes.iter().find(|b| b.source == BoxSource::Cube(11)).unwrap();
    assert_eq!(cube.texture, BoxTexture::Armor(ArmorSlot::Helmet));
    assert!(near_mat(helmet.transform, cube.transform));
    // The root's part position (posN = (0, -2, 0)) moves both off the head's pose.
    let mut moved = p.clone();
    moved.roots[6].pos = [0.0; 3];
    let mut flat = cpm_core::load(&export_model(&moved).unwrap()).unwrap();
    let boxes = flat.pose(&PoseInput { on_ground: true, equipment: all, ..Default::default() });
    let flat_helmet = boxes.iter().find(|b| b.source == BoxSource::RootVanilla(RootType::ArmorHelmet)).unwrap();
    // (The head is not turned at rest, so the offset is a plain translation after its pose.)
    assert!(near_mat(helmet.transform, flat_helmet.transform * Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0))));
}

#[test]
fn root_info_bytes_for_a_model_root() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut p = rainwing_project().clone();
    p.anims.clear();
    p.anim_enc = None;
    p.roots.truncate(6);
    for r in &mut p.roots {
        r.children.clear();
    }
    p.roots.push(root_of(RootPart::Model(RootType::ArmorRightFoot), 5));
    let bytes = export_model(&p).unwrap();
    let file = read_cpmmodel(&bytes).unwrap();
    assert_eq!(file.data.extra_roots.len(), 1);
    // ROOT_INFO part (22), 3 bytes: flags ROOT_MODEL | ROOT_CREATE, RootModelType ordinal 11,
    // create from cube 10.
    let needle = [22u8, 3, 0x06, 11, 10];
    assert!(bytes.windows(needle.len()).any(|w| w == needle), "ROOT_INFO not found");
}

/// An armour root takes its player part's final pose (vanilla motion, part position, animation):
/// `HumanoidArmorLayer` copies the player model's part poses into the armour model.
#[test]
fn armor_roots_follow_their_player_part() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut p = rainwing_project().clone();
    p.anims.clear();
    p.anim_enc = None;
    p.roots.truncate(6);
    for r in &mut p.roots {
        r.children.clear();
        r.hidden = false;
        r.disable_vanilla_anim = false;
    }
    p.roots[0].pos = [0.0, -1.0, 0.0];
    for (t, sid) in [(RootType::ArmorHelmet, 900), (RootType::ArmorBody, 902), (RootType::ArmorRightLeg, 904)] {
        let mut r = root_of(RootPart::Model(t), sid);
        r.children.push(cube(sid + 1));
        p.roots.push(r);
    }
    let ids = element_ids(&p);
    let mut m = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    let worn = Equipment { head: true, chest: true, legs: true, ..Default::default() };
    let input = PoseInput {
        sneaking: true,
        walk_speed: 0.7,
        walk_position: 2.0,
        pitch: 30.0,
        head_yaw: -20.0,
        ..wearing(worn)
    };
    let boxes = m.pose(&input);
    let find = |s: BoxSource| boxes.iter().find(|b| b.source == s).unwrap_or_else(|| panic!("no {s:?}"));
    for (sid, bone, slot) in [(901, 0, ArmorSlot::Helmet), (903, 1, ArmorSlot::Chestplate), (905, 5, ArmorSlot::Leggings)] {
        let c = find(BoxSource::Cube(ids[&sid][0]));
        assert_eq!(c.texture, BoxTexture::Armor(slot));
        assert!(near_mat(c.transform, find(BoxSource::Vanilla(bone)).transform), "bone {bone}");
    }
}

/// A copy transform onto a player bone moves that bone before the armour copies its pose
/// (`copyPropertiesTo` after `applyCopy`): the armour follows the copied pose, as Java does.
#[test]
fn armor_roots_follow_a_copy_onto_their_player_part() {
    if common::missing(&["ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    use cpm_core::effects::TransformCopy;
    let mut p = rainwing_project().clone();
    p.anims.clear();
    p.anim_enc = None;
    p.roots.truncate(6);
    for r in &mut p.roots {
        r.children.clear();
        r.hidden = false;
        r.disable_vanilla_anim = false;
    }
    let mut r = root_of(RootPart::Model(RootType::ArmorBody), 902);
    r.children.push(cube(903));
    p.roots.push(r);
    let ids = element_ids(&p);
    let mut m = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    // Head (0) position and rotation onto the body (1), replacing them.
    m.model.copies.push(TransformCopy { from: 0, to: 1, copy: 0b111_111, multiply: 1.0 });
    let input = PoseInput { pitch: 40.0, head_yaw: -25.0, ..wearing(CHEST) };
    let boxes = m.pose(&input);
    let find = |s: BoxSource| boxes.iter().find(|b| b.source == s).unwrap_or_else(|| panic!("no {s:?}"));
    let head = find(BoxSource::Vanilla(0)).transform;
    let body = find(BoxSource::Vanilla(1)).transform;
    // The copy turned the body with the head.
    assert!(near_mat(body, head), "the copy did not reach the body");
    let armor = find(BoxSource::Cube(ids[&903][0]));
    assert_eq!(armor.texture, BoxTexture::Armor(ArmorSlot::Chestplate));
    assert!(near_mat(armor.transform, body), "armour body does not follow the copied body pose");
    let vanilla = find(BoxSource::RootVanilla(RootType::ArmorBody));
    assert!(near_mat(vanilla.transform, body));
}
