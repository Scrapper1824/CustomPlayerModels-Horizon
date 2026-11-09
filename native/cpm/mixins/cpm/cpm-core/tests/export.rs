mod common;

use cpm_core::effects::{decode_effects, Effect};
use cpm_core::export::{element_ids, export_model};
use cpm_core::format::{read_cpmmodel, SkinType};
use cpm_core::io::CpmError;
use cpm_core::model::Model;
use cpm_core::project::*;
use cpm_core::write::checksummed;

fn expie() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/Expie_by_Clouddy.cpmproject")).unwrap()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.002
}

/// Angles are quantised to 360/65535 degrees.
fn near_angle(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 0.01)
}

fn near3(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| near(a[i], b[i]))
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

fn root(part: u8) -> ProjectRoot {
    ProjectRoot {
        part: RootPart::Vanilla(part),
        store_id: part as i64,
        duplicated: false,
        pos: [0.0; 3],
        rot: [0.0; 3],
        hidden: false,
        disable_vanilla_anim: false,
        children: Vec::new(),
    }
}

fn elem(store_id: i64) -> ProjectElement {
    ProjectElement {
        name: format!("e{store_id}"),
        show_in_editor: true,
        texture: true,
        tex_size: 1,
        offset: [0.0; 3],
        pos: [0.0; 3],
        rotation: [0.0; 3],
        size: [1.0; 3],
        scale: [1.0; 3],
        mesh_scale: [1.0; 3],
        u: 0,
        v: 0,
        color: 0,
        mirror: false,
        mc_scale: 0.0,
        glow: false,
        recolor: false,
        hidden: false,
        single_tex: false,
        extrude: false,
        face_uv: None,
        item_render: None,
        copy_transform: None,
        store_id,
        children: Vec::new(),
    }
}

fn project(roots: Vec<ProjectRoot>, skin_png: Vec<u8>) -> Project {
    Project {
        name: "N".into(),
        desc: String::new(),
        copy_protection: "normal".into(),
        icon_png: None,
        skin_type: SkinType::Default,
        skin_png,
        skin_size: [64, 64],
        skin_png_size: [64, 64],
        elytra_png: None,
        elytra_size: [64, 32],
        cape_png: None,
        cape_size: [64, 32],
        anim_textures: Vec::new(),
        hide_head_if_skull: true,
        remove_armor_offset: false,
        remove_bed_offset: false,
        invis_glow: false,
        render_transform: None,
        fp_hand: None,
        model_id: None,
        roots,
        anims: Vec::new(),
        anim_enc: None,
        warnings: Vec::new(),
    }
}

fn six_roots() -> Vec<ProjectRoot> {
    (0..6).map(root).collect()
}

fn count_elements(e: &[ProjectElement]) -> usize {
    e.iter().map(|e| 1 + count_elements(&e.children)).sum()
}

#[test]
fn expie_exports_and_loads() {
    if common::missing(&["Expie_by_Clouddy.cpmproject"]) {
        return;
    }
    let p = read_project(&expie()).unwrap();
    let bytes = export_model(&p).unwrap();
    let loaded = cpm_core::load(&bytes).unwrap();
    // Java's flattenElements: one cube per NORMAL element plus one fake cube per duplicated or
    // non-player root (Expie's cape).
    let fake = |r: &ProjectRoot| (r.duplicated || matches!(r.part, RootPart::Model(_))) as usize;
    let expected = p.roots.iter().map(|r| count_elements(&r.children) + fake(r)).sum::<usize>();
    assert_eq!(loaded.model.cubes.len(), expected);
    assert!(expected >= 152, "{expected}");
    assert_eq!(loaded.texture_px, p.skin_size);
    assert_eq!(loaded.name, p.name);
    let file = read_cpmmodel(&bytes).unwrap();
    assert_eq!(file.desc, p.desc);
    assert_eq!(file.name, p.name);
    assert_eq!(file.data.skin_type, p.skin_type);
    assert_eq!(file.icon_png, p.icon_png.clone().unwrap_or_default());
    // Expie has animations (see expie_animations_roundtrip).
    assert!(file.data.new_anim.is_some());
}

#[test]
fn minimal_project_is_byte_exact() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let p = project(roots, b"PNG".to_vec());
    let got = export_model(&p).unwrap();

    let one = [0xAA, 0x0A]; // svarint(1 * DIV = 682)
    let mut cube = vec![1u8, 0b11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    for _ in 0..3 {
        cube.extend_from_slice(&one);
    }
    cube.extend_from_slice(&[0, 0, 0, 0, 0]);
    let mut def = vec![11u8, 1, 1, 21, cube.len() as u8];
    def.extend_from_slice(&cube);
    def.extend_from_slice(&[17, 9, 0, 0, 64, 0, 64, 3, b'P', b'N', b'G']);
    def.extend_from_slice(&[0, 0]);
    let data = checksummed(0x53, &def);
    let mut body = vec![1u8, b'N', 0, data.len() as u8];
    body.extend_from_slice(&data);
    body.extend_from_slice(&[0, 0]);
    assert_eq!(got, checksummed(0x53, &body));
}

#[test]
fn element_fields_roundtrip() {
    let mut roots = six_roots();
    roots[0].pos = [1.0, 2.0, 3.0];
    roots[0].rot = [10.0, 0.0, 370.0];
    roots[1].hidden = true;
    roots[2].disable_vanilla_anim = true;

    let mut a = elem(100);
    a.tex_size = 2;
    a.u = 3;
    a.v = 300;
    a.pos = [1.5, -2.0, 0.25];
    a.rotation = [-90.0, 45.0, 0.0];
    a.offset = [-1.0, -2.0, -3.0];
    a.size = [2.0, 3.0, 4.0];
    let mut child = elem(101);
    child.texture = false;
    child.color = 0x123456;
    a.children.push(child);
    let mut mc = elem(102);
    mc.mc_scale = 0.5;
    let mut ms = elem(103);
    ms.mesh_scale = [2.0, 1.0, 1.0];
    let mut sc = elem(104);
    sc.scale = [1.5, 1.0, 1.0];
    let mut hid = elem(105);
    hid.hidden = true;
    let mut glow = elem(106);
    glow.glow = true;
    let mut rec = elem(107);
    rec.recolor = true;
    rec.color = 0xABCDEF;
    let mut st = elem(108);
    st.single_tex = true;
    let mut fu = elem(109);
    let mut faces: [Option<Face>; 6] = Default::default();
    faces[2] = Some(Face { sx: 1, sy: 2, ex: 5, ey: 7, rot: 1, auto_uv: false });
    fu.face_uv = Some(FaceUv { faces });
    let mut mir = elem(110);
    mir.mirror = true;
    roots[0].children.extend([a, mc, ms, sc]);
    roots[3].children.extend([hid, glow, rec, st, fu, mir]);
    let p = project(roots, png(64, 64));

    let ids = element_ids(&p);
    assert_eq!(ids[&0], vec![0]);
    assert_eq!(ids[&100], vec![10]);
    assert_eq!(ids[&101], vec![11]);
    assert_eq!(ids[&110], vec![20]);

    let bytes = export_model(&p).unwrap();
    let raw = read_cpmmodel(&bytes).unwrap().data;
    let effects = decode_effects(&raw.effects);
    assert!(effects.contains(&Effect::Glow { cube: 16 }));
    assert!(effects.contains(&Effect::Color { cube: 17, rgb: 0xABCDEF }));
    assert!(effects.contains(&Effect::SingleTex { cube: 18 }));
    // Effects follow the element walk order (ExporterImpl): glow, color, single tex, per-face UV.
    let order: Vec<u32> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Glow { cube } | Effect::Color { cube, .. } | Effect::SingleTex { cube } => Some(*cube),
            Effect::PerFaceUv { cube, .. } => Some(*cube),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec![16, 17, 18, 19]);
    // Mirrored: texSize = -textureSize, written as a signed byte.
    let mir = raw.cubes.iter().find(|c| c.id == 20).unwrap();
    assert_eq!(mir.tex_size as i8, -1);

    let m = Model::build(raw.clone()).unwrap();
    let n = |id: u32| &m.cubes[m.node_index(id).unwrap()];
    let a = n(10);
    assert_eq!(a.tex_scale, 2);
    assert_eq!(a.uv, [3, 300]);
    assert!(near3(a.pos, [1.5, -2.0, 0.25]));
    assert!(near_angle(a.rotation, [-90.0, 45.0, 0.0]), "{:?}", a.rotation);
    assert!(near3(a.offset, [-1.0, -2.0, -3.0]));
    assert!(near3(a.size, [2.0, 3.0, 4.0]));
    assert_eq!(a.children, vec![m.node_index(11).unwrap()]);
    assert_eq!(n(11).color, Some(0x123456));
    assert!(near(n(12).mc_scale, 0.5));
    assert!(near3(n(13).scale, [2.0, 1.0, 1.0]));
    assert!(near3(n(14).render_scale, [1.5, 1.0, 1.0]));
    assert!(n(15).hidden);
    assert!(n(16).glow);
    assert_eq!(n(17).color, Some(0xABCDEF));
    assert!(n(18).single_tex);
    let fuv = n(19).face_uv.unwrap();
    let north = fuv[2].unwrap();
    assert_eq!((north.u0, north.v0, north.u1, north.v1, north.rot), (1, 2, 5, 7, 1));
    assert!(fuv.iter().enumerate().all(|(i, f)| i == 2 || f.is_none()));
    assert_eq!(m.roots_by_bone[0].len(), 4);
    assert_eq!(m.roots_by_bone[3].len(), 6);

    // Roots.
    let head = raw.part_pos.iter().find(|p| p.part == 0).unwrap();
    assert!(near3(head.pos, [1.0, 2.0, 3.0]));
    assert!(near_angle(head.rot, [10.0, 0.0, 10.0]), "{:?}", head.rot);
    assert!(!raw.keep_vanilla[1]);
    assert!(raw.vanilla_anim_disabled[2]);
    assert_eq!(raw.keep_vanilla.iter().filter(|k| !**k).count(), 1);
    assert_eq!(raw.vanilla_anim_disabled.iter().filter(|k| **k).count(), 1);
    assert_eq!(raw.part_pos.len(), 3);
}

#[test]
fn root_info_written_only_when_needed() {
    let mut roots = six_roots();
    roots[4].pos = [0.05, -0.05, 0.0]; // inside Vec3f.epsilon(0.1)
    roots[5].children.push(elem(9));
    let p = project(roots, png(64, 64));
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    assert!(raw.part_pos.is_empty());
    assert_eq!(raw.keep_vanilla, [true; 6]);
}

#[test]
fn duplicated_root_is_fake_cube_plus_root_info() {
    let mut roots = six_roots();
    roots[0].children.push(elem(20));
    let mut dup = root(2);
    dup.duplicated = true;
    dup.store_id = 555;
    dup.pos = [1.0, 0.0, 0.0];
    dup.children.push(elem(21));
    roots.push(dup);
    let p = project(roots, png(64, 64));
    let ids = element_ids(&p);
    assert_eq!(ids[&20], vec![10]);
    assert_eq!(ids[&555], vec![11]);
    assert_eq!(ids[&21], vec![12]);
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    let fake = raw.cubes.iter().find(|c| c.id == 11).unwrap();
    assert_eq!(fake.parent, 6);
    assert!(near3(fake.pos, [1.0, 0.0, 0.0]));
    assert_eq!(fake.size, [0.0; 3]);
    assert_eq!(raw.cubes.iter().find(|c| c.id == 12).unwrap().parent, 11);
}

#[test]
fn shared_store_ids_keep_every_element() {
    let mut roots = six_roots();
    roots[0].children.push(elem(77));
    roots[1].children.push(elem(77));
    let p = project(roots, png(64, 64));
    assert_eq!(element_ids(&p)[&77], vec![10, 11]);
}

#[test]
fn project_level_effects() {
    let mut p = project(six_roots(), png(64, 64));
    p.hide_head_if_skull = false;
    p.remove_armor_offset = true;
    p.remove_bed_offset = true;
    p.invis_glow = true;
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    let kinds: Vec<(u8, Vec<u8>)> = raw.effects.iter().map(|e| (e.kind, e.payload.clone())).collect();
    assert_eq!(kinds, vec![(8, vec![0]), (9, vec![1]), (17, vec![]), (18, vec![])]);
}

#[test]
fn zero_skin_size_uses_png_size() {
    let mut p = project(six_roots(), png(128, 64));
    p.skin_size = [0, 64];
    p.skin_png_size = [128, 64];
    let loaded = cpm_core::load(&export_model(&p).unwrap()).unwrap();
    assert_eq!(loaded.texture_px, [128, 64]);
}

#[test]
fn uuid_lock_is_refused() {
    let mut p = project(six_roots(), png(64, 64));
    p.copy_protection = "uuid_lock".into();
    assert!(matches!(export_model(&p), Err(CpmError::Invalid(_))));
}

#[test]
fn copy_transform_and_item_effects_after_element_effects() {
    let mut roots = six_roots();
    let mut src = elem(40);
    src.glow = true;
    let mut dst = elem(41);
    dst.copy_transform = Some(CopyTransform {
        store_id: 40,
        copy_px: true,
        copy_py: false,
        copy_pz: false,
        copy_rx: false,
        copy_ry: false,
        copy_rz: false,
        copy_sx: false,
        copy_sy: false,
        copy_sz: false,
        copy_vis: true,
        copy_color: false,
        additive: false,
        multiply: 2.0,
    });
    dst.item_render = Some(1);
    let mut dangling = elem(42);
    dangling.copy_transform = Some(CopyTransform { store_id: 999, ..dst.copy_transform.clone().unwrap() });
    let mut late = elem(43);
    late.extrude = true;
    roots[0].children.extend([src, dst, dangling, late]);
    let p = project(roots, png(64, 64));
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data;
    let kinds: Vec<(u8, Vec<u8>)> = raw.effects.iter().map(|e| (e.kind, e.payload.clone())).collect();
    let mut ct = vec![10u8, 11, 0x81, 0x84, 0x01]; // from 10, to 11, copy = PX | VIS | MULTIPLY = 0x4201
    ct.extend_from_slice(&2.0f32.to_be_bytes());
    assert_eq!(
        kinds,
        vec![(0, vec![10]), (7, vec![11, 1, 0]), (10, vec![13]), (14, ct)]
    );
}

#[test]
fn cloneable_part_written() {
    let mut p = project(six_roots(), png(64, 64));
    p.copy_protection = "cloneable".into();
    p.desc = "D".into();
    let bytes = export_model(&p).unwrap();
    // CLONEABLE (15): fields = name | desc, then both strings; just before END.
    let tail = [15u8, 5, 3, 1, b'N', 1, b'D', 0, 0];
    let data = read_cpmmodel(&bytes).unwrap();
    assert_eq!(data.desc, "D");
    assert!(bytes.windows(tail.len()).any(|w| w == tail));
}

// ---------------------------------------------------------------------------------------------
// Animations (Task 5)

use cpm_core::anim::engine::PoseInput;
use cpm_core::anim::{decode_new, GestureKind, Interp, Pose};
use std::io::Read;

fn at(t: u64) -> PoseInput {
    PoseInput { time_ms: t, on_ground: true, ..Default::default() }
}

/// One `animations/*.json` of the Expie sample, parsed independently of the project reader.
fn expie_anim_json(prefix: &str) -> serde_json::Value {
    let bytes = expie();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let name = zip.file_names().filter_map(Result::ok).find(|n| n.starts_with(prefix)).unwrap().to_string();
    let mut s = String::new();
    zip.by_name(&name).unwrap().read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn comp(store_id: i64, rot: [f32; 3]) -> FrameComponent {
    FrameComponent { store_id, pos: [0.0; 3], rotation: rot, color: [0.0; 3], show: true, scale: [1.0; 3] }
}

fn anim(file: &str, name: &str, kind: AnimKind, pose: Option<u8>, frames: Vec<ProjectFrame>) -> ProjectAnim {
    ProjectAnim {
        file_name: file.into(),
        display_name: name.into(),
        kind,
        pose,
        additive: true,
        duration: 1000,
        priority: 0,
        loop_: false,
        interpolator: Interp::LinearLoop,
        layer_default: 0.0,
        order: 0,
        is_property: false,
        group: None,
        command: false,
        layer_controlled: true,
        max_value: 100,
        interpolate_value: true,
        must_finish: false,
        hidden: false,
        frames,
    }
}

fn rot_frames(store_id: i64) -> Vec<ProjectFrame> {
    vec![
        ProjectFrame { components: vec![comp(store_id, [0.0; 3])] },
        ProjectFrame { components: vec![comp(store_id, [45.0, 0.0, 0.0])] },
    ]
}

fn all_layers_enc() -> serde_json::Value {
    serde_json::json!({
        "freeLayers": ["hat", "jacket", "left_pants_leg", "right_pants_leg", "left_sleeve", "right_sleeve"],
        "defaultValues": {"hat": true}
    })
}

fn decoded(bytes: &[u8]) -> (cpm_core::LoadedModel, cpm_core::anim::AnimSet) {
    let loaded = cpm_core::load(bytes).unwrap();
    let raw = read_cpmmodel(bytes).unwrap().data.new_anim.expect("ANIMATION_NEW written");
    let set = decode_new(&raw, &loaded.model).unwrap();
    (loaded, set)
}

#[test]
fn expie_animations_roundtrip() {
    if common::missing(&["Expie_by_Clouddy.cpmproject"]) {
        return;
    }
    let p = read_project(&expie()).unwrap();
    let bytes = export_model(&p).unwrap();
    let (mut loaded, set) = decoded(&bytes);
    assert!(loaded.engine.custom_poses().contains(&"MenuPose".to_string()), "{:?}", loaded.engine.custom_poses());

    // Global Tail: frame count and duration straight from the project JSON.
    let tail_json = expie_anim_json("animations/v_global_Tail");
    let frames = tail_json["frames"].as_array().unwrap();
    let duration = tail_json["duration"].as_u64().unwrap() as u32;
    let (_, tail) = set.poses.iter().find(|(p, _)| *p == Pose::Other(12)).expect("GLOBAL clip");
    assert_eq!(tail.frames as usize, frames.len());
    assert_eq!(tail.duration_ms, duration);
    assert_eq!(tail.additive, tail_json["additive"].as_bool().unwrap());

    // Every element id carrying a Tail component's storeID is a clip component.
    let ids = element_ids(&p);
    let mut tail_ids: Vec<u32> = Vec::new();
    for f in frames {
        for c in f["components"].as_array().unwrap() {
            for id in &ids[&c["storeID"].as_i64().unwrap()] {
                if !tail_ids.contains(id) {
                    tail_ids.push(*id);
                }
            }
        }
    }
    assert!(!tail_ids.is_empty());
    for id in &tail_ids {
        assert!(tail.components.contains(id), "tail id {id} missing from {:?}", tail.components);
    }

    // The tail moves: rotation differs between t=0 and t=duration/2 for at least one tail cube.
    let f0 = loaded.engine.sample(&at(0));
    let f1 = loaded.engine.sample(&at(duration as u64 / 2));
    let moved = tail_ids.iter().any(|id| {
        let a = f0.parts.get(id).and_then(|d| d.rot);
        let b = f1.parts.get(id).and_then(|d| d.rot);
        matches!((a, b), (Some(a), Some(b)) if !near_angle(a, b))
    });
    assert!(moved, "no tail cube rotated between t=0 and t={}", duration / 2);

    // MenuPose: a custom pose, one frame per JSON frame.
    let menu_json = expie_anim_json("animations/c_MenuPose");
    let (_, menu) = set.poses.iter().find(|(p, _)| *p == Pose::Custom("MenuPose".into())).unwrap();
    assert_eq!(menu.frames as usize, menu_json["frames"].as_array().unwrap().len());
}

#[test]
fn anim_groups_only_when_changed() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anims.push(anim("v_global_Spin.json", "Spin", AnimKind::Pose, Some(12), rot_frames(7)));
    let (_, set) = decoded(&export_model(&p).unwrap());
    let (_, clip) = set.poses.iter().find(|(p, _)| *p == Pose::Other(12)).unwrap();
    assert_eq!(clip.components, vec![10]);
    assert_eq!(clip.present, vec![[false, true, false, false, false]]);
    assert_eq!(clip.frames, 2);
    // Degrees in the project, radians on the wire, degrees again after decoding.
    assert!((clip.channels[0][3][1] - 45.0).abs() < 0.1, "{}", clip.channels[0][3][1]);
}

#[test]
fn shared_store_id_animates_every_element() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    roots[1].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anims.push(anim("v_global_Spin.json", "Spin", AnimKind::Pose, Some(12), rot_frames(7)));
    let (_, set) = decoded(&export_model(&p).unwrap());
    assert_eq!(set.poses[0].1.components, vec![10, 11]);
}

#[test]
fn custom_anims_need_anim_enc() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anims.push(anim("g_Wave.json", "Wave", AnimKind::Gesture, None, rot_frames(7)));
    assert!(matches!(export_model(&p), Err(CpmError::Invalid(_))));
    // Vanilla poses alone need no encoding data.
    p.anims[0] = anim("v_global_Spin.json", "Spin", AnimKind::Pose, Some(12), rot_frames(7));
    assert!(export_model(&p).is_ok());
}

#[test]
fn gestures_and_layers_get_buttons() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anim_enc = Some(all_layers_enc());
    p.anims.push(anim("g_Wave.json", "Wave", AnimKind::Gesture, None, rot_frames(7)));
    let mut layer = anim("g_Glow.json", "Glow", AnimKind::Layer, None, rot_frames(7));
    layer.layer_default = 1.0;
    p.anims.push(layer);
    p.anims.push(anim("g_Dial.json", "Dial", AnimKind::ValueLayer, None, rot_frames(7)));
    p.anims.push(anim("c_Sit.json", "Sit", AnimKind::CustomPose, None, rot_frames(7)));
    let (loaded, _) = decoded(&export_model(&p).unwrap());
    assert_eq!(loaded.engine.custom_poses(), vec!["Sit".to_string()]);
    let g = loaded.engine.gestures();
    let find = |n: &str| g.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n} missing: {g:?}"));
    assert_eq!(find("Wave").kind, GestureKind::Gesture);
    let glow = find("Glow");
    assert_eq!(glow.kind, GestureKind::Layer);
    assert_eq!(glow.default_value, 255, "layerDefault 1.0 turns the layer on");
    let dial = find("Dial");
    assert_eq!(dial.kind, GestureKind::Value);
    assert_eq!(dial.clips[0].duration_ms, 1001);
}

/// Exact ANIMATION_NEW bytes for one rotation-only GLOBAL pose (no anim_enc, no model id).
#[test]
fn global_pose_bytes_are_exact() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anims.push(anim("v_global_Spin.json", "Spin", AnimKind::Pose, Some(12), rot_frames(7)));
    let raw = read_cpmmodel(&export_model(&p).unwrap()).unwrap().data.new_anim.unwrap();
    let rad45 = (45f32.to_radians() * 682.0) as i32; // 535
    assert_eq!(rad45, 535);
    let mut want = vec![
        10, 3, 0, 0, 0, // CONTROL_INFO: blank 0, reset 0, model id ""
        2, 0, // NEW_TRIGGER
        3, 2, 12, 0, // INIT_BUILTIN_TRIGGER GLOBAL, flags 0
        1, 4, 0, 0, 0xE8, 0x07, // NEW_ANIM trigger 0, priority 0, duration 1000
        9, 4, 1, 12, 1, 10, // CUBES_TO_CHANNELS: 1 cube, 12 channels, additive, id 10
    ];
    // CONSTANT_FRAME_TIME_FLOAT: linear_loop, 2 frames, 3 channels (rot x/y/z = 3, 4, 5).
    // svarint(535): 535 & 0x3F = 23 | 0x80, 535 >> 6 = 8.
    let body = [2u8, 2, 3, 3, 0, 23 | 0x80, 8, 4, 0, 0, 5, 0, 0];
    want.push(7);
    want.push(body.len() as u8);
    want.extend_from_slice(&body);
    want.extend_from_slice(&[0, 0]); // END
    assert_eq!(raw, want);
}

#[test]
fn staged_animations_link_to_their_main_trigger() {
    let mut roots = six_roots();
    roots[0].children.push(elem(7));
    let mut p = project(roots, png(64, 64));
    p.anim_enc = Some(all_layers_enc());
    p.anims.push(anim("g_Wave.json", "Wave", AnimKind::Gesture, None, rot_frames(7)));
    p.anims.push(anim("g_pre.json", "g:Wave", AnimKind::Setup, None, rot_frames(7)));
    let (loaded, _) = decoded(&export_model(&p).unwrap());
    let wave = loaded.engine.gestures().iter().find(|g| g.name == "Wave").expect("main gesture kept");
    assert_eq!(wave.clips.len(), 1);
    // A setup stage naming no animation has no main trigger (error.cpm.stagingMissingMain).
    p.anims[1].display_name = "g:Nope".into();
    assert!(matches!(export_model(&p), Err(CpmError::Invalid(_))));
}

/// `-textureSize` of a mirrored element wraps as Java's int negation does (no overflow panic for
/// i32::MIN); only its low byte is written.
#[test]
fn mirrored_extreme_texture_size_wraps() {
    let mut roots = six_roots();
    let mut e = elem(100);
    e.mirror = true;
    e.tex_size = i32::MIN;
    let mut f = elem(101);
    f.mirror = true;
    f.tex_size = 3;
    roots[0].children.extend([e, f]);
    let bytes = export_model(&project(roots, png(64, 64))).unwrap();
    let raw = read_cpmmodel(&bytes).unwrap().data;
    let tex: Vec<i8> = raw.cubes.iter().filter(|c| c.id >= 10).map(|c| c.tex_size as i8).collect();
    assert_eq!(tex, [0, -3]);
}

#[test]
fn animated_textures_exported_after_their_sheet() {
    use cpm_core::anim::texture::AnimTex;
    let t = AnimTex {
        sheet: 0,
        uv_start: [1, 2],
        uv_size: [3, 4],
        anim_start: [5, 600],
        frame_count: 4,
        frame_time: 120,
        an_x: true,
        interpolate: false,
    };
    let c = AnimTex { sheet: 2, an_x: false, interpolate: true, ..t };
    let mut p = project(six_roots(), png(64, 64));
    p.cape_png = Some(png(64, 32));
    p.anim_textures = vec![c, t];
    let bytes = export_model(&p).unwrap();
    let f = read_cpmmodel(&bytes).unwrap();
    // Per sheet in the exporter's order (skin, then cape), each after its TEXTURE.
    assert_eq!(f.data.anim_textures, vec![t, c]);
}
