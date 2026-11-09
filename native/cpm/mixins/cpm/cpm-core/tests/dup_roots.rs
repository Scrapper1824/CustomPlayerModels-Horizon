//! Duplicated player roots: `ROOT_INFO` with `ROOT_CREATE` alone and the legacy `DUP_ROOT`
//! (part 14) turn a `CUSTOM_PART` child into a second root of a player part
//! (`ModelDefinition.addRoot`), posed as that part plus the cube's position and rotation.

use cpm_core::anim::engine::{CapeState, Frame, PartDelta, PoseInput};
use cpm_core::format::{read_definition, RawCube, RawEffect, RawModel};
use cpm_core::mesh::pose_model;
use cpm_core::model::{Model, RootType, CUSTOM_PART};
use cpm_core::write::{checksummed, Writer};
use cpm_core::{BoxSource, PosedBox};
use glam::{Mat4, Vec3};

const ROOT_HIDDEN: u8 = 1;
const ROOT_CREATE: u8 = 2;
const ROOT_DISABLE_VANILLA: u8 = 16;

fn cube(id: u32, parent: u32, pos: [f32; 3], size: [f32; 3]) -> RawCube {
    RawCube {
        id, parent, size, pos, offset: [0.0; 3], rotation: [0.0; 3], tex_size: 1, u: 0, v: 0, rgb: 0,
        mesh_scale: [1.0; 3], scale: [1.0; 3], mc_scale: 0.0, hidden: false,
    }
}

/// A definition holding the one part `ty` with `payload`; cube 10 (a `CUSTOM_PART` child at
/// (0, -8, 0), no mesh) with cube 11 (a 2³ box) under it.
fn raw_with(ty: u8, payload: &Writer) -> RawModel {
    let mut def = Writer::new();
    def.varint(0).object_block(ty, payload).object_block(0, &Writer::new());
    let mut body = Writer::new();
    body.object_block(3, &def).object_block(0, &Writer::new());
    let mut raw = read_definition(&checksummed(0x53, &body.into_bytes())).unwrap();
    raw.cubes = vec![cube(10, CUSTOM_PART, [0.0, -8.0, 0.0], [0.0; 3]), cube(11, 10, [0.0; 3], [2.0; 3])];
    raw
}

/// `ROOT_INFO` with `flags` for the head (part 0), made from cube 10.
fn root_info(flags: u8) -> RawModel {
    let mut p = Writer::new();
    p.u8(flags).u8(0).varint(10);
    raw_with(22, &p)
}

/// Legacy `DUP_ROOT`: cube 10 becomes a second head root.
fn legacy() -> RawModel {
    let mut p = Writer::new();
    p.varint(10).u8(0);
    raw_with(14, &p)
}

fn turned() -> PoseInput {
    PoseInput { on_ground: true, head_yaw: 30.0, pitch: 10.0, ..Default::default() }
}

fn of(boxes: &[PosedBox], source: BoxSource) -> Vec<Mat4> {
    boxes.iter().filter(|b| b.source == source).map(|b| b.transform).collect()
}

fn near(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// The head's rotation for [`turned`] (`translateRotate`: Z · Y · X).
fn head_rot() -> Mat4 {
    Mat4::from_rotation_y(30f32.to_radians()) * Mat4::from_rotation_x(10f32.to_radians())
}

/// The transform the duplicate at (0, -8, 0) on the head has, from the main head's `main`.
fn dup_of(main: Mat4, pos: Vec3, rot: Mat4) -> Mat4 {
    main * head_rot().inverse() * Mat4::from_translation(pos) * rot
}

/// Checks a model whose duplicated head follows the head: two head boxes and two hats, the
/// second pair at the duplicate's pose, and cube 11 drawn under it.
fn assert_follows_head(model: &Model) {
    assert_eq!(model.dup_roots.len(), 1);
    assert!(!model.roots_by_bone[1].contains(&model.dup_roots[0].node), "the fake cube left the body");
    let boxes = pose_model(model, &Frame::default(), &turned());
    let heads = of(&boxes, BoxSource::Vanilla(0));
    let hats = of(&boxes, BoxSource::VanillaLayer(0));
    assert_eq!((heads.len(), hats.len()), (2, 2));
    let want = dup_of(heads[0], Vec3::new(0.0, -8.0, 0.0), head_rot());
    assert!(near(heads[1], want), "{:?} vs {want:?}", heads[1]);
    assert!(near(hats[1], want));
    let cubes = of(&boxes, BoxSource::Cube(11));
    assert_eq!(cubes.len(), 1);
    assert!(near(cubes[0], want), "the duplicate's cubes are drawn under it");
}

#[test]
fn dup_root_info_follows_part() {
    let raw = root_info(ROOT_CREATE);
    assert_eq!(raw.dup_roots.len(), 1);
    assert!(raw.extra_roots.is_empty());
    assert_follows_head(&Model::build(raw).unwrap());

    // ROOT_HIDDEN: no vanilla box, the cubes still.
    let hidden = Model::build(root_info(ROOT_CREATE | ROOT_HIDDEN)).unwrap();
    let boxes = pose_model(&hidden, &Frame::default(), &turned());
    assert_eq!(of(&boxes, BoxSource::Vanilla(0)).len(), 1);
    assert_eq!(of(&boxes, BoxSource::VanillaLayer(0)).len(), 1);
    assert_eq!(of(&boxes, BoxSource::Cube(11)).len(), 1);

    // ROOT_DISABLE_VANILLA: the part's default position plus posN, no head turn.
    let still = Model::build(root_info(ROOT_CREATE | ROOT_DISABLE_VANILLA)).unwrap();
    let boxes = pose_model(&still, &Frame::default(), &turned());
    let heads = of(&boxes, BoxSource::Vanilla(0));
    assert!(near(heads[1], dup_of(heads[0], Vec3::new(0.0, -8.0, 0.0), Mat4::IDENTITY)));
}

#[test]
fn legacy_dup_root_follows_part() {
    let raw = legacy();
    assert_eq!(raw.dup_roots.len(), 1);
    assert_follows_head(&Model::build(raw.clone()).unwrap());

    // ModelPartDupRoot takes its fake cube's hidden state after the effects (the legacy HIDE).
    let mut hidden = raw;
    hidden.effects = vec![RawEffect { kind: 2, payload: vec![10] }];
    let model = Model::build(hidden).unwrap();
    assert!(model.dup_roots[0].hidden);
    let boxes = pose_model(&model, &Frame::default(), &turned());
    assert_eq!(of(&boxes, BoxSource::Vanilla(0)).len(), 1);
}

/// `RootModelElement.setVisible` is empty: an animation's visibility never shows or hides a root's
/// vanilla box (only `ROOT_HIDDEN` / the hidden fake cube does); position still animates.
#[test]
fn dup_root_animation_visibility_does_nothing() {
    let model = Model::build(root_info(ROOT_CREATE)).unwrap();
    let frame = |d: PartDelta| Frame { parts: [(10, d)].into_iter().collect() };
    // "Hidden" by an animation: both heads still drawn, and the cubes.
    let off = pose_model(&model, &frame(PartDelta { visible: Some(false), ..Default::default() }), &turned());
    assert_eq!(of(&off, BoxSource::Vanilla(0)).len(), 2);
    assert_eq!(of(&off, BoxSource::Cube(11)).len(), 1);
    // "Shown" and moved by one though ROOT_HIDDEN: still no second head.
    let hidden = Model::build(root_info(ROOT_CREATE | ROOT_HIDDEN)).unwrap();
    let shown = PartDelta { visible: Some(true), pos: Some([3.0, 0.0, 0.0]), pos_additive: true, ..Default::default() };
    let on = pose_model(&hidden, &frame(shown), &turned());
    assert_eq!(of(&on, BoxSource::Vanilla(0)).len(), 1);
    assert_eq!(of(&on, BoxSource::VanillaLayer(0)).len(), 1);
    // Moved by one (additive position).
    let moved = PartDelta { pos: Some([3.0, 0.0, 0.0]), pos_additive: true, ..Default::default() };
    let boxes = pose_model(&model, &frame(moved), &turned());
    let heads = of(&boxes, BoxSource::Vanilla(0));
    assert!(near(heads[1], dup_of(heads[0], Vec3::new(3.0, -8.0, 0.0), head_rot())));
}

/// The same for a non-player root: a hidden cape root (legacy `MODEL_ROOT`, its fake cube hidden)
/// that an animation shows and moves draws no vanilla cape.
#[test]
fn hidden_cape_root_stays_hidden_under_animation() {
    let mut p = Writer::new();
    p.varint(10).u8(0); // MODEL_ROOT: cube 10 becomes the cape root
    let mut raw = raw_with(12, &p);
    raw.cubes[0].hidden = true;
    let model = Model::build(raw).unwrap();
    assert!(model.extra_roots[0].hidden);
    let caped = PoseInput { cape: Some(CapeState::default()), ..turned() };
    let d = PartDelta { visible: Some(true), pos: Some([1.0, 0.0, 0.0]), pos_additive: true, ..Default::default() };
    let frame = Frame { parts: [(10, d)].into_iter().collect() };
    let boxes = pose_model(&model, &frame, &caped);
    assert!(of(&boxes, BoxSource::RootVanilla(RootType::Cape)).is_empty());
    assert_eq!(of(&boxes, BoxSource::Cube(11)).len(), 1, "its cubes are still drawn");
}

/// `ModelPartDefinition`: a `SKIN_LINK` or `DEFINITION_LINK` among a definition's own parts is
/// refused, before anything is fetched.
#[test]
fn links_inside_a_definition_are_refused() {
    use cpm_core::format::read_cpmmodel_resolving;
    for ty in [4u8, 6] {
        let mut lp = Writer::new();
        lp.u8(6).bytes(b"pb:abc");
        let mut def = Writer::new();
        def.varint(0).object_block(ty, &lp).object_block(0, &Writer::new());
        let mut body = Writer::new();
        body.object_block(3, &def).object_block(0, &Writer::new());
        let mut d = Writer::new();
        d.bytes(&checksummed(0x53, &body.into_bytes()));
        let mut w = Writer::new();
        w.utf("D").utf("").block(&d).varint(0).block(&Writer::new());
        let file = checksummed(0x53, &w.into_bytes());
        let err = read_cpmmodel_resolving(&file, &|_| panic!("no fetch expected")).unwrap_err();
        assert!(err.to_string().contains("invalid tag in definition"), "{ty}: {err}");
        assert!(cpm_core::format::read_cpmmodel(&file).is_err(), "{ty}: no NeedsLink either");
    }
}
