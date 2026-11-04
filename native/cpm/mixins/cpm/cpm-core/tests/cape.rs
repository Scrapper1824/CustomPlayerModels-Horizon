//! Capes (`CapeLayer` / `CapeLayerMixin`): a model's cape root posed on the body with the vanilla
//! cape's swing, the vanilla cape on the posed body for a model without one, the elytra,
//! chestplate and invisibility rules, and the converter keeping cape roots and the cape sheet.

mod common;

use std::sync::OnceLock;

use cpm_core::anim::engine::Frame;
use cpm_core::export::{element_ids, export_model};
use cpm_core::format::read_cpmmodel;
use cpm_core::mesh::pose_model;
use cpm_core::model::{Model, RootType};
use cpm_core::project::{read_project, Project, ProjectElement, ProjectRoot, RootPart};
use cpm_core::{BoxSource, BoxTexture, CapeState, Equipment, LoadedModel, PoseInput, PosedBox};
use glam::{Mat4, Quat, Vec3};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn rainwing_project() -> &'static Project {
    static P: OnceLock<Project> = OnceLock::new();
    P.get_or_init(|| read_project(&sample("rainwing-old.cpmproject")).unwrap())
}

fn near_mat(a: Mat4, b: Mat4) -> bool {
    a.to_cols_array().iter().zip(b.to_cols_array()).all(|(x, y)| (x - y).abs() < 1e-4)
}

fn caped(cape: CapeState) -> PoseInput {
    PoseInput { time_ms: 1000, on_ground: true, cape: Some(cape), ..Default::default() }
}

/// `PlayerCapeModel`'s cape on the body, turned by the flap and leans (the cape part's
/// `(0, 0, 2)`, `rotY(PI)`, then `rotateBy`).
fn cape_on_body(c: CapeState) -> Mat4 {
    use std::f32::consts::PI;
    let by = Quat::from_rotation_y(-PI)
        * Quat::from_rotation_x((6.0 + c.lean / 2.0 + c.flap).to_radians())
        * Quat::from_rotation_z((c.lean2 / 2.0).to_radians())
        * Quat::from_rotation_y((180.0 - c.lean2 / 2.0).to_radians());
    Mat4::from_rotation_translation(Quat::from_rotation_y(PI) * by, Vec3::new(0.0, 0.0, 2.0))
}

fn find(boxes: &[PosedBox], s: BoxSource) -> &PosedBox {
    boxes.iter().find(|b| b.source == s).unwrap_or_else(|| panic!("no {s:?}"))
}

fn capes(boxes: &[PosedBox]) -> Vec<&PosedBox> {
    boxes.iter().filter(|b| matches!(b.texture, BoxTexture::Cape | BoxTexture::PlayerCape)).collect()
}

fn cube(store_id: i64) -> ProjectElement {
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

/// rainwing's header with its six player roots shown and empty, plus a cape root (store id 900)
/// holding one cube (901), and a 128x64 cape sheet.
fn cape_project() -> Project {
    let mut p = rainwing_project().clone();
    p.anims.clear();
    p.anim_enc = None;
    p.roots.truncate(6);
    for r in &mut p.roots {
        r.children.clear();
        r.hidden = false;
        r.disable_vanilla_anim = false;
    }
    let mut cape = ProjectRoot {
        part: RootPart::Model(RootType::Cape),
        store_id: 900,
        duplicated: false,
        pos: [0.0; 3],
        rot: [0.0; 3],
        hidden: false,
        disable_vanilla_anim: false,
        children: Vec::new(),
    };
    cape.children.push(cube(901));
    p.roots.push(cape);
    p.cape_png = Some(png(128, 64));
    p.cape_size = [128, 64];
    p
}

fn cape_model() -> LoadedModel {
    cpm_core::load(&export_model(&cape_project()).unwrap()).unwrap()
}

/// ReggieTheRat's header with every vanilla part kept and no cubes, roots or animation: no cape
/// root.
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

/// The model-space → draw-frame transform of a standing player (`LivingEntityRenderer.submit`:
/// rotY(180) · scale(-1, -1, 1) · scale(15/16) · translate(0, -1.501, 0), in pixels), worked out
/// here independently of the mesh code.
fn standing_base() -> Mat4 {
    Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(0.9375))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0))
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// The vanilla body (`HumanoidModel.setupAnim`): at rest, or crouched (xRot 0.5, 3.2 px down).
fn vanilla_body(sneaking: bool) -> Mat4 {
    if sneaking {
        Mat4::from_translation(Vec3::new(0.0, 3.2, 0.0)) * Mat4::from_rotation_x(0.5)
    } else {
        Mat4::IDENTITY
    }
}

/// Where vanilla's (and CPM's, at the cape root's base pose) cape is, in the draw frame.
fn vanilla_cape(sneaking: bool, c: CapeState) -> Mat4 {
    standing_base() * vanilla_body(sneaking) * cape_on_body(c)
}

#[test]
fn cape_root_posed_on_body() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let ids = element_ids(&cape_project());
    let mut m = cape_model();
    for input in [
        caped(CapeState::default()),
        PoseInput { sneaking: true, walk_speed: 0.7, walk_position: 2.0, ..caped(CapeState::default()) },
    ] {
        let boxes = m.pose(&input);
        let vanilla = find(&boxes, BoxSource::RootVanilla(RootType::Cape));
        assert_eq!(vanilla.texture, BoxTexture::Cape);
        assert_eq!(vanilla.texture_px, [64, 32]);
        let want = vanilla_cape(input.sneaking, CapeState::default());
        assert!(near_mat(vanilla.transform, want), "sneaking {}", input.sneaking);
        // Its cube is in the model's cape sheet, under the root.
        let c = find(&boxes, BoxSource::Cube(ids[&901][0]));
        assert_eq!(c.texture, BoxTexture::Cape);
        assert_eq!(c.texture_px, [128, 64]);
        assert!(near_mat(c.transform, vanilla.transform));
    }
    // No cape layer, no cape.
    assert!(capes(&m.pose(&PoseInput { time_ms: 1000, on_ground: true, ..Default::default() })).is_empty());
}

/// CapeLayerMixin combines the cape with the cape model's own vanilla body: moving, turning,
/// animating or un-animating the CPM body bone does not move a cape root.
#[test]
fn cape_root_ignores_the_cpm_body_bone() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let c = CapeState { flap: 4.0, lean: 20.0, lean2: 3.0 };
    let mut p = cape_project();
    p.roots[1].pos = [0.0, -4.0, 1.0];
    p.roots[1].rot = [15.0, 0.0, 0.0];
    let mut moved = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    for sneaking in [false, true] {
        let boxes = moved.pose(&PoseInput { sneaking, ..caped(c) });
        let cape = find(&boxes, BoxSource::RootVanilla(RootType::Cape)).transform;
        assert!(near_mat(cape, vanilla_cape(sneaking, c)), "sneaking {sneaking}");
        // The body bone itself did move.
        assert!(!near_mat(find(&boxes, BoxSource::Vanilla(1)).transform, standing_base() * vanilla_body(sneaking)));
    }
    // An animation on the body bone leaves the cape where it is.
    let mut frame = Frame::default();
    let delta = cpm_core::PartDelta { pos: Some([0.0, 5.0, 0.0]), pos_additive: true, ..Default::default() };
    frame.parts.insert(1, delta);
    let boxes = pose_model(&moved.model, &frame, &caped(c));
    assert!(near_mat(find(&boxes, BoxSource::RootVanilla(RootType::Cape)).transform, vanilla_cape(false, c)));
    // A body with its vanilla animation off (the model's own crouch): the cape still crouches.
    p.roots[1].disable_vanilla_anim = true;
    let mut still = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    let boxes = still.pose(&PoseInput { sneaking: true, ..caped(c) });
    assert!(near_mat(find(&boxes, BoxSource::RootVanilla(RootType::Cape)).transform, vanilla_cape(true, c)));
}

#[test]
fn cape_flap_applied() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = cape_model();
    let still = find(&m.pose(&caped(CapeState::default())), BoxSource::RootVanilla(RootType::Cape)).transform;
    for c in [
        CapeState { flap: 20.0, lean: 0.0, lean2: 0.0 },
        CapeState { flap: 5.0, lean: 60.0, lean2: -12.0 },
    ] {
        let cape = find(&m.pose(&caped(c)), BoxSource::RootVanilla(RootType::Cape)).transform;
        assert!(near_mat(cape, vanilla_cape(false, c)), "{c:?}");
        assert!(!near_mat(cape, still), "{c:?} did not move the cape");
    }
}

#[test]
fn cape_root_animations_and_part_position_apply() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // The root's part position (posN) moves it off the vanilla cape's pose (in model space).
    let mut p = cape_project();
    p.roots[6].pos = [0.0, 3.0, 0.0];
    let mut m = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    let c = CapeState { flap: 10.0, ..Default::default() };
    let at = |shift: Vec3| standing_base() * Mat4::from_translation(shift) * cape_on_body(c);
    let cape = find(&m.pose(&caped(c)), BoxSource::RootVanilla(RootType::Cape)).transform;
    assert!(near_mat(cape, at(Vec3::new(0.0, 3.0, 0.0))));
    // The root's own animation moves it too: a frame position delta on the root.
    let id = m.model.cubes[m.model.extra_roots[0].node].id;
    let mut frame = Frame::default();
    let delta = cpm_core::PartDelta { pos: Some([1.0, 0.0, 0.0]), pos_additive: true, ..Default::default() };
    frame.parts.insert(id, delta);
    let boxes = pose_model(&m.model, &frame, &caped(c));
    let animated = find(&boxes, BoxSource::RootVanilla(RootType::Cape)).transform;
    assert!(near_mat(animated, at(Vec3::new(1.0, 3.0, 0.0))));
}

#[test]
fn cape_hidden_under_elytra() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let wings = Equipment { chest_has_wings: true, elytra: true, ..Default::default() };
    let mut m = cape_model();
    let boxes = m.pose(&PoseInput { equipment: wings, ..caped(CapeState::default()) });
    assert!(capes(&boxes).is_empty());
    let plain = plain_model();
    let boxes = pose_model(&plain, &Frame::default(), &PoseInput { equipment: wings, ..caped(CapeState::default()) });
    assert!(capes(&boxes).is_empty());
}

#[test]
fn cape_offset_over_chestplate() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let chest = Equipment { chest: true, chest_humanoid: true, ..Default::default() };
    // In the draw frame: model pixels (0, -0.85, 1.1) are (0, 0.85, -1.1) / 16 blocks at 15/16 scale.
    let shift = Vec3::new(0.0, 0.85, -1.1) * 0.9375 / 16.0;
    let mut m = cape_model();
    let plain = plain_model();
    let check = |bare: &[PosedBox], over: &[PosedBox]| {
        let (a, b) = (capes(bare), capes(over));
        assert!(!a.is_empty() && a.len() == b.len());
        for (a, b) in a.iter().zip(b) {
            assert!(near_mat(b.transform, Mat4::from_translation(shift) * a.transform), "{:?}", a.source);
        }
    };
    let bare = m.pose(&caped(CapeState::default()));
    let over = m.pose(&PoseInput { equipment: chest, ..caped(CapeState::default()) });
    check(&bare, &over);
    let bare = pose_model(&plain, &Frame::default(), &caped(CapeState::default()));
    let over = pose_model(&plain, &Frame::default(), &PoseInput { equipment: chest, ..caped(CapeState::default()) });
    check(&bare, &over);
}

/// Vanilla's CapeLayer draws no cape on an invisible player; CapeLayerMixin draws a cape root
/// before that check, so a model's own cape still shows (as in CPM).
#[test]
fn cape_hidden_when_invisible() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let plain = plain_model();
    assert_eq!(capes(&pose_model(&plain, &Frame::default(), &caped(CapeState::default()))).len(), 1);
    let input = PoseInput { invisible: true, ..caped(CapeState::default()) };
    assert!(capes(&pose_model(&plain, &Frame::default(), &input)).is_empty());
    let mut m = cape_model();
    let shown = capes(&m.pose(&caped(CapeState::default()))).len();
    assert!(shown > 0);
    assert_eq!(capes(&m.pose(&input)).len(), shown);
}

/// A model with no cape root wears vanilla's cape, drawn by vanilla's CapeLayer: on the vanilla
/// body, whatever the CPM body bone does, and without the model's render scale.
#[test]
fn vanilla_cape_on_vanilla_body() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut raw = read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    raw.cubes.clear();
    raw.effects.clear();
    raw.extra_roots.clear();
    raw.legacy_anim = None;
    raw.new_anim = None;
    raw.keep_vanilla = [true; 6];
    let plain = Model::build(raw.clone()).unwrap();
    raw.part_pos.retain(|pp| pp.part != 1);
    raw.part_pos.push(cpm_core::format::PartPos { part: 1, pos: [0.0, 2.0, 1.0], rot: [20.0, 0.0, 0.0] });
    let moved = Model::build(raw).unwrap();
    let c = CapeState { flap: 8.0, lean: 30.0, lean2: 5.0 };
    for model in [&plain, &moved] {
        for sneaking in [false, true] {
            let boxes = pose_model(model, &Frame::default(), &PoseInput { sneaking, ..caped(c) });
            let cape = capes(&boxes);
            assert_eq!(cape.len(), 1);
            assert_eq!(cape[0].source, BoxSource::RootVanilla(RootType::Cape));
            assert_eq!(cape[0].texture, BoxTexture::PlayerCape);
            assert_eq!(cape[0].texture_px, [64, 32]);
            assert!(near_mat(cape[0].transform, vanilla_cape(sneaking, c)), "sneaking {sneaking}");
        }
    }
    // The model's render scale does not reach vanilla's cape.
    let mut scaled = plain.clone();
    scaled.render_transform =
        Some(cpm_core::model::PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [2.0; 3] });
    let boxes = pose_model(&scaled, &Frame::default(), &caped(c));
    assert!(near_mat(capes(&boxes)[0].transform, vanilla_cape(false, c)));
}

/// A cape root's sheet that is not usable (no image) leaves its cubes in the default 64x32 space.
#[test]
fn cape_cubes_without_a_usable_sheet_are_64x32() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let ids = element_ids(&cape_project());
    let mut m = cape_model();
    m.model.cape_texture.as_mut().unwrap().png.clear();
    let boxes = m.pose(&caped(CapeState::default()));
    assert_eq!(find(&boxes, BoxSource::Cube(ids[&901][0])).texture_px, [64, 32]);
}

#[test]
fn converter_keeps_cape_root_and_texture() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Expie: a hidden cape root with no cubes, and a 128x128 cape sheet.
    let expie = read_project(&sample("Expie_by_Clouddy.cpmproject")).unwrap();
    let bytes = export_model(&expie).unwrap();
    let raw = read_cpmmodel(&bytes).unwrap().data;
    assert_eq!(raw.cape_texture.as_ref().unwrap().declared, [128, 128]);
    let mut m = cpm_core::load(&bytes).unwrap();
    let r: Vec<(RootType, bool)> = m.model.extra_roots.iter().map(|r| (r.kind, r.hidden)).collect();
    assert_eq!(r, vec![(RootType::Cape, true)]);
    let sheet = m.cape.as_ref().unwrap();
    assert_eq!(sheet.texture_px, [128, 128]);
    assert_eq!(sheet.png_px.map(|v| v > 0), [true; 2]);
    // Hidden with nothing on it: Expie draws no cape at all.
    assert!(capes(&m.pose(&caped(CapeState::default()))).is_empty());

    // A cape root with a cube round trips with its sheet.
    let p = cape_project();
    let ids = element_ids(&p);
    let m = cape_model();
    assert_eq!(m.model.extra_roots.len(), 1);
    let root = &m.model.extra_roots[0];
    assert_eq!((root.kind, root.hidden), (RootType::Cape, false));
    assert_eq!(m.model.cubes[root.node].id, ids[&900][0]);
    assert_eq!(m.model.cubes[root.node].children.len(), 1);
    let sheet = m.cape.as_ref().unwrap();
    assert_eq!((sheet.texture_px, sheet.png_px), ([128, 64], [128, 64]));
    assert!(m.elytra.is_none());
}
