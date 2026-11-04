//! The model's render transform (`MODEL_SCALE`, `ModelRenderManager.setupTransform` in
//! `AnimationMode.PLAYER`) and the first-person hand (`FIRST_PERSON_HAND`, `HAND` mode).

mod common;

use std::io::{Cursor, Write};

use cpm_core::anim::engine::Frame;
use cpm_core::effects::PartTransform;
use cpm_core::export::export_model;
use cpm_core::format::{read_cpmmodel, RawCube, RawEffect, SkinType};
use cpm_core::mesh::{pose_arm, pose_model, pose_model_with_items, ItemSlot};
use cpm_core::model::Model;
use cpm_core::project::read_project;
use cpm_core::write::Writer;
use cpm_core::{BoxSource, PoseInput, PosedBox};
use glam::{Mat4, Vec3};

const SCALING: u8 = 13;
const MODEL_SCALE: u8 = 12;
const FIRST_PERSON_HAND: u8 = 15;
const OURS: [u8; 3] = [SCALING, MODEL_SCALE, FIRST_PERSON_HAND];

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn cube(id: u32, parent: u32) -> RawCube {
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

/// Reggie's header with a cube on the body (10) and on each arm (11 left, 12 right), the vanilla
/// parts kept, no part positions or legacy scale, and these effects.
fn model(effects: Vec<RawEffect>) -> Model {
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.cubes = vec![cube(10, 1), cube(11, 2), cube(12, 3)];
    raw.effects = effects;
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [true; 6];
    raw.part_pos.clear();
    raw.skin_type = SkinType::Default;
    raw.vanilla_anim_disabled = [false; 6];
    raw.scale = None;
    raw.extra_roots.clear();
    Model::build(raw).unwrap()
}

fn effect(kind: u8, w: &Writer) -> RawEffect {
    RawEffect { kind, payload: w.clone().into_bytes() }
}

fn scaling() -> RawEffect {
    effect(SCALING, &Writer::new())
}

/// `EffectModelScale.write`: vec6b position, angle rotation (degrees on the wire), vec6b scale.
fn model_scale(t: PartTransform) -> RawEffect {
    let mut w = Writer::new();
    w.vec6b(t.pos).angle(t.rot).vec6b(t.scale);
    effect(MODEL_SCALE, &w)
}

fn fp_hand(left: PartTransform, right: PartTransform) -> RawEffect {
    let mut w = Writer::new();
    for t in [left, right] {
        w.vec6b(t.pos).angle(t.rot).vec6b(t.scale);
    }
    effect(FIRST_PERSON_HAND, &w)
}

fn standing() -> PoseInput {
    PoseInput { on_ground: true, ..Default::default() }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn near3(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| near(a[i], b[i]))
}

fn near_mat(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn by_cube(boxes: &[PosedBox], id: u32) -> Mat4 {
    boxes.iter().find(|b| b.source == BoxSource::Cube(id)).unwrap().transform
}

/// `translateRotate(pos px) · rotate(ZYX, degrees) · scale` in model pixels.
fn expected(pos: [f32; 3], rot: [f32; 3], scale: [f32; 3]) -> Mat4 {
    let r = rot.map(f32::to_radians);
    Mat4::from_translation(Vec3::from_array(pos))
        * Mat4::from_rotation_z(r[2])
        * Mat4::from_rotation_y(r[1])
        * Mat4::from_rotation_x(r[0])
        * Mat4::from_scale(Vec3::from_array(scale))
}

const T: PartTransform = PartTransform { pos: [1.0, -2.0, 3.0], rot: [10.0, 90.0, 30.0], scale: [2.0, 0.5, 1.5] };

#[test]
fn render_scale_parses_full_transform() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let m = model(vec![scaling(), model_scale(T)]);
    let t = m.render_transform.expect("render transform");
    assert!(near3(t.pos, T.pos), "{:?}", t.pos);
    assert!(near3(t.rot, T.rot), "{:?}", t.rot);
    assert!(near3(t.scale, T.scale), "{:?}", t.scale);
    // EffectModelScale.apply needs a ScaleData: without SCALING (or the legacy SCALE part) first,
    // it does nothing.
    assert_eq!(model(vec![model_scale(T)]).render_transform, None);
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.scale = Some(0.5);
    raw.effects = vec![model_scale(T)];
    let legacy = Model::build(raw).unwrap();
    assert!(near3(legacy.render_transform.expect("legacy scale makes a ScaleData").scale, T.scale));
    // The legacy SCALE part alone is gameplay (entity) scaling: no render transform.
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.scale = Some(0.5);
    raw.effects.clear();
    assert_eq!(Model::build(raw).unwrap().render_transform, None);
    // A later SCALING starts a new ScaleData, dropping the transform.
    assert_eq!(model(vec![scaling(), model_scale(T), scaling()]).render_transform, None);
}

#[test]
fn render_scale_applies_in_third_person() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let plain = model(vec![]);
    let scaled = model(vec![scaling(), model_scale(T)]);
    let r = expected(T.pos, T.rot, T.scale);
    // The body cube sits at the model origin at rest, so its transform gains exactly `r`.
    let a = by_cube(&pose_model(&plain, &Frame::default(), &standing()), 10);
    let b = by_cube(&pose_model(&scaled, &Frame::default(), &standing()), 10);
    assert!(near_mat(b, a * r), "{b:?} vs {:?}", a * r);
    // Uniform: only a scale given doubles the model about its origin.
    let uniform = model(vec![scaling(), model_scale(PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [2.0; 3] })]);
    let c = by_cube(&pose_model(&uniform, &Frame::default(), &standing()), 10);
    assert!(near_mat(c, a * Mat4::from_scale(Vec3::splat(2.0))));
    // An item part's anchor follows (it is stored under CPM's matrix stack)...
    let mut raw_items = vec![cube(13, 0)];
    raw_items[0].size = [0.0; 3];
    let with_part = |effects: Vec<RawEffect>| {
        let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
        raw.cubes = raw_items.clone();
        raw.effects = effects;
        raw.legacy_anim = None;
        raw.new_anim = None;
        raw.part_pos.clear();
        raw.vanilla_anim_disabled = [false; 6];
        raw.scale = None;
        raw.extra_roots.clear();
        Model::build(raw).unwrap()
    };
    let item = RawEffect { kind: 7, payload: vec![13, 2, 0] };
    let head = |m: &Model| {
        let posed = pose_model_with_items(m, &Frame::default(), &standing());
        posed.items.iter().find(|i| i.slot == ItemSlot::Head).unwrap().transform
    };
    let (p_plain, p_scaled) = (with_part(vec![item.clone()]), with_part(vec![item, scaling(), model_scale(T)]));
    let px = Mat4::from_scale(Vec3::splat(16.0));
    assert!(near_mat(head(&p_scaled), head(&p_plain) * px.inverse() * r * px));
    // ...but the vanilla fallback (no item part) does not: PlayerRenderManager.translateAndRotate
    // uses the plain vanilla part, outside CPM's stack.
    assert!(near_mat(head(&scaled), head(&plain)));
    for slot in [ItemSlot::MainHand, ItemSlot::OffHand] {
        let at = |m: &Model| {
            let posed = pose_model_with_items(m, &Frame::default(), &standing());
            posed.items.iter().find(|i| i.slot == slot).unwrap().transform
        };
        assert!(near_mat(at(&scaled), at(&plain)));
    }
}

#[test]
fn render_scale_clamps() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let plain = model(vec![]);
    // setupTransform: a component below 0.1 or above 10 is 1.
    let t = PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [0.05, 20.0, 2.0] };
    let clamped = model(vec![scaling(), model_scale(t)]);
    let a = by_cube(&pose_model(&plain, &Frame::default(), &standing()), 10);
    let b = by_cube(&pose_model(&clamped, &Frame::default(), &standing()), 10);
    assert!(near_mat(b, a * Mat4::from_scale(Vec3::new(1.0, 1.0, 2.0))));
    // SCALING alone: a zero scale (ScaleData's default), so the identity.
    let zero = model(vec![scaling(), model_scale(PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [0.0; 3] })]);
    assert!(near_mat(by_cube(&pose_model(&zero, &Frame::default(), &standing()), 10), a));
}

#[test]
fn render_scale_not_on_fp_arm() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    // Not on the first-person arm (HAND mode). CPM's own previews do draw it
    // (DirectModelRenderManager.doRender0): they pose as the third-person player
    // (see the mod's preview_fit_includes_render_scale).
    let plain = model(vec![]);
    let scaled = model(vec![scaling(), model_scale(T)]);
    for left in [true, false] {
        assert_eq!(pose_arm(&scaled, left), pose_arm(&plain, left));
    }
}

#[test]
fn fp_hand_per_arm() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let plain = model(vec![]);
    let left = PartTransform { pos: [1.0, 2.0, -3.0], rot: [0.0, 0.0, 45.0], scale: [0.0; 3] };
    let right = PartTransform { pos: [0.0; 3], rot: [20.0, 0.0, 0.0], scale: [1.5, 1.5, 1.5] };
    let hand = model(vec![fp_hand(left, right)]);
    let fp = hand.fp_hand;
    assert!(near3(fp[0].unwrap().pos, left.pos) && near3(fp[0].unwrap().rot, left.rot));
    assert!(near3(fp[1].unwrap().rot, right.rot) && near3(fp[1].unwrap().scale, right.scale));
    assert_eq!(plain.fp_hand, [None, None]);
    // Each arm, in the arm pose's block space: (pos / 16, rotate, scale) before the arm's own
    // pivot; a zero scale is clamped to 1.
    let px = Mat4::from_scale(Vec3::splat(16.0));
    for (is_left, t, scale) in [(true, left, [1.0; 3]), (false, right, right.scale)] {
        let f = px.inverse() * expected(t.pos, t.rot, scale) * px;
        let (with, without) = (pose_arm(&hand, is_left), pose_arm(&plain, is_left));
        assert_eq!(with.len(), without.len());
        assert!(!with.is_empty());
        for (a, b) in with.iter().zip(&without) {
            assert!(near_mat(a.transform, f * b.transform), "left {is_left}");
        }
    }
    // Third person is untouched.
    assert_eq!(pose_model(&hand, &Frame::default(), &standing()), pose_model(&plain, &Frame::default(), &standing()));
}

fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&vec![0u8; (w * h * 4) as usize]).unwrap();
    }
    out
}

#[test]
fn converter_round_trips_scale_and_fp_hand() {
    let config = br#"{"version": 1, "scaling": 1.0,
        "scalingEx": {"render_position": {"x": 1, "y": -2, "z": 3},
                      "render_rotation": {"x": 10, "y": 90, "z": 30},
                      "render_scale": {"x": 2, "y": 0.5, "z": 1.5}},
        "firstPersonHand": {
            "left": {"position": {"x": 1, "y": 2, "z": -3}, "rotation": {"x": 0, "y": 0, "z": 45}, "scale": {"x": 0, "y": 0, "z": 0}},
            "right": {"position": {"x": 0, "y": 0, "z": 0}, "rotation": {"x": 20, "y": 0, "z": 0}, "scale": {"x": 1.5, "y": 1.5, "z": 1.5}}}}"#;
    let skin = png(64, 64);
    let p = read_project(&make_zip(&[("config.json", config), ("skin.png", &skin)])).unwrap();
    assert!(
        !p.warnings.iter().any(|w| w.contains("scal") || w.contains("first person")),
        "{:?}",
        p.warnings
    );
    let rt = p.render_transform.expect("render transform read");
    assert!(near3(rt.pos, T.pos) && near3(rt.rot, T.rot) && near3(rt.scale, T.scale));
    let fp = p.fp_hand.expect("first-person hand read");
    assert!(near3(fp[0].pos, [1.0, 2.0, -3.0]) && near3(fp[0].rot, [0.0, 0.0, 45.0]));
    assert!(near3(fp[1].rot, [20.0, 0.0, 0.0]) && near3(fp[1].scale, [1.5; 3]));

    let m = cpm_core::load(&export_model(&p).unwrap()).unwrap().model;
    let t = m.render_transform.expect("MODEL_SCALE written");
    assert!(near3(t.pos, T.pos) && near3(t.rot, T.rot) && near3(t.scale, T.scale), "{t:?}");
    let [l, r] = m.fp_hand.map(|h| h.expect("FIRST_PERSON_HAND written"));
    assert!(near3(l.pos, fp[0].pos) && near3(l.rot, fp[0].rot) && near3(l.scale, [0.0; 3]));
    assert!(near3(r.pos, fp[1].pos) && near3(r.rot, fp[1].rot) && near3(r.scale, fp[1].scale));

    // Exporter: SCALING (gameplay values dropped), then MODEL_SCALE, then FIRST_PERSON_HAND.
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    // (REMOVE_ARMOR_OFFSET, on by default, comes between them, as in ExporterImpl.)
    let kinds: Vec<u8> = raw.effects.iter().map(|e| e.kind).filter(|k| OURS.contains(k)).collect();
    assert_eq!(kinds, vec![SCALING, MODEL_SCALE, FIRST_PERSON_HAND]);
    assert!(raw.effects[0].payload.is_empty());

    // Scaling off (0), an unchanged transform and unchanged hands write nothing.
    let config = br#"{"version": 1, "scaling": 0,
        "scalingEx": {"render_scale": {"x": 2, "y": 2, "z": 2}},
        "firstPersonHand": {"left": {"scale": {"x": 0, "y": 0, "z": 0}}}}"#;
    let p = read_project(&make_zip(&[("config.json", config), ("skin.png", &skin)])).unwrap();
    assert_eq!((p.render_transform, p.fp_hand), (None, None));
    let config = br#"{"version": 1, "scaling": 1, "scalingEx": {"render_scale": {"x": 1, "y": 0, "z": 1}}}"#;
    let p = read_project(&make_zip(&[("config.json", config), ("skin.png", &skin)])).unwrap();
    assert_eq!(p.render_transform, None);
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    assert!(!raw.effects.iter().any(|e| OURS.contains(&e.kind)));
    // Gameplay scaling is still dropped, with a warning.
    let config = br#"{"version": 1, "scaling": 0.5, "scalingEx": {"eye_height": 2}}"#;
    let p = read_project(&make_zip(&[("config.json", config), ("skin.png", &skin)])).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("scaling")), "{:?}", p.warnings);
}
