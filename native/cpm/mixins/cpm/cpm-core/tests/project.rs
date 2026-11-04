mod common;

use std::io::{Cursor, Write};

use cpm_core::anim::Interp;
use cpm_core::format::SkinType;
use cpm_core::io::CpmError;
use cpm_core::model::RootType;
use cpm_core::project::*;
use serde_json::Value;

fn expie() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/Expie_by_Clouddy.cpmproject")).unwrap()
}

/// Reads one entry of a zip, independently of the code under test.
fn zip_entry(zip: &[u8], name: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut out).unwrap();
    out
}

fn count_children(v: &Value) -> usize {
    v.get("children")
        .and_then(Value::as_array)
        .map(|c| c.iter().map(|e| 1 + count_children(e)).sum())
        .unwrap_or(0)
}

fn count_elements(roots: &[ProjectRoot]) -> usize {
    fn walk(e: &[ProjectElement]) -> usize {
        e.iter().map(|e| 1 + walk(&e.children)).sum()
    }
    roots.iter().map(|r| walk(&r.children)).sum()
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

fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn reads_expie() {
    if common::missing(&["Expie_by_Clouddy.cpmproject"]) {
        return;
    }
    let bytes = expie();
    let p = read_project(&bytes).unwrap();

    let desc: Value = serde_json::from_slice(&zip_entry(&bytes, "description.json")).unwrap();
    assert_eq!(p.name, desc["name"].as_str().unwrap());
    assert_eq!(p.desc, desc["desc"].as_str().unwrap());
    assert_eq!(p.name, "Expie | by ClouddyVR");

    assert!(p.skin_png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(p.skin_size, [128, 128]);
    assert_eq!(p.skin_type, SkinType::Default);
    assert!(p.icon_png.as_ref().unwrap().starts_with(b"\x89PNG"));
    assert!(p.hide_head_if_skull);
    assert!(!p.remove_bed_offset);
    assert!(!p.invis_glow);
    assert!(p.remove_armor_offset);

    let config: Value = serde_json::from_slice(&zip_entry(&bytes, "config.json")).unwrap();
    let elems = config["elements"].as_array().unwrap();
    for (i, id) in ["head", "body", "left_arm", "right_arm", "left_leg", "right_leg"].iter().enumerate() {
        assert!(elems.iter().any(|e| e["id"] == *id));
        let r = p.roots.iter().find(|r| r.part == RootPart::Vanilla(i as u8)).expect(id);
        assert_eq!(r.store_id, i as i64);
        assert!(!r.duplicated);
    }
    // The cape root is kept (hidden, no cubes), with the model's cape sheet.
    let capes: Vec<&ProjectRoot> = p.roots.iter().filter(|r| r.part == RootPart::Model(RootType::Cape)).collect();
    assert_eq!(capes.len(), 1);
    assert!(capes[0].hidden && capes[0].children.is_empty());
    assert!(p.cape_png.is_some());
    assert_eq!(p.cape_size, [128, 128]);
    assert!(!p.warnings.iter().any(|w| w.contains("cape")), "{:?}", p.warnings);
    let head = &p.roots[0];
    assert_eq!(head.pos, [0.0, -0.5, 0.0]);
    assert!(head.hidden);

    let expected: usize = elems.iter().map(count_children).sum();
    assert_eq!(expected, 152);
    assert_eq!(count_elements(&p.roots), expected);

    // First head child, field for field.
    let e = &head.children[0];
    assert_eq!(e.name, "head");
    assert!(e.texture);
    assert_eq!(e.tex_size, 1);
    assert_eq!(e.offset, [-2.0, -3.35, -4.8]);
    assert_eq!(e.size, [4.0, 3.3, 2.0]);
    assert_eq!(e.store_id, 6383044859800771584);
    let fu = e.face_uv.as_ref().unwrap();
    let up = fu.faces[0].as_ref().unwrap();
    assert_eq!((up.sx, up.sy, up.ex, up.ey, up.rot), (63, 67, 59, 65, 2));

    assert_eq!(p.anims.len(), 2);
    let menu = p.anims.iter().find(|a| a.display_name == "MenuPose").unwrap();
    assert!(menu.file_name.starts_with("c_MenuPose_"));
    assert_eq!(menu.kind, AnimKind::CustomPose);
    assert_eq!(menu.pose, None);
    assert_eq!(menu.duration, 1000);
    assert!(menu.additive);
    assert_eq!(menu.interpolator, Interp::PolyLoop);
    assert_eq!(menu.frames.len(), 1);
    let c = &menu.frames[0].components[0];
    assert_eq!(c.store_id, 2);
    assert_eq!(c.rotation, [350.0, 20.0, 330.0]);
    assert!(!c.show);

    let tail = p.anims.iter().find(|a| a.display_name == "Tail").unwrap();
    assert!(tail.file_name.starts_with("v_global_Tail_"));
    assert_eq!(tail.kind, AnimKind::Pose);
    assert_eq!(tail.pose, Some(12)); // VanillaPose.GLOBAL
    assert_eq!(tail.duration, 2000);
    assert_eq!(tail.frames.len(), 5);
    assert!(tail.frames.iter().all(|f| !f.components.is_empty()));

    let enc = p.anim_enc.as_ref().unwrap();
    assert_eq!(enc["defaultValues"]["hat"], Value::Bool(true));
}

#[test]
fn unknown_and_missing_keys_tolerated() {
    let config = br#"{"version": 1, "someFutureKey": {"x": 1},
        "elements": [{"id": "head", "children": [{"storeID": 77, "unknown": true}]}]}"#;
    let skin = png(64, 64);
    let zip = make_zip(&[("config.json", config), ("skin.png", &skin)]);
    let p = read_project(&zip).unwrap();

    assert_eq!(p.name, "");
    assert_eq!(p.desc, "");
    assert!(p.icon_png.is_none());
    assert_eq!(p.skin_type, SkinType::Default);
    assert_eq!(p.skin_size, [64, 64]);
    assert!(p.hide_head_if_skull);
    assert!(p.remove_armor_offset);
    assert!(!p.remove_bed_offset);
    assert!(!p.invis_glow);
    assert!(p.anims.is_empty());
    assert!(p.anim_enc.is_none());

    // All six player roots exist even when config.json lists only one.
    assert_eq!(p.roots.len(), 6);
    for (i, r) in p.roots.iter().enumerate() {
        assert_eq!(r.part, RootPart::Vanilla(i as u8));
        assert_eq!(r.pos, [0.0; 3]);
        assert!(!r.hidden);
        assert!(!r.disable_vanilla_anim);
    }
    let e = &p.roots[0].children[0];
    assert_eq!(e.store_id, 77);
    assert_eq!(e.name, "");
    assert!(!e.texture);
    assert_eq!(e.tex_size, 1);
    assert_eq!(e.size, [1.0; 3]);
    assert_eq!(e.scale, [1.0; 3]);
    assert_eq!(e.mesh_scale, [1.0; 3]);
    assert_eq!(e.pos, [0.0; 3]);
    assert_eq!((e.u, e.v, e.color), (0, 0, 0));
    assert_eq!(e.mc_scale, 0.0);
    assert!(!e.mirror && !e.glow && !e.recolor && !e.hidden && !e.single_tex && !e.extrude);
    assert!(e.face_uv.is_none() && e.item_render.is_none() && e.copy_transform.is_none());
    assert!(e.children.is_empty());
}

#[test]
fn ints_and_floats_and_effects() {
    let config = br#"{"version": 1, "skinType": "slim", "hideHeadIfSkull": false,
        "removeBedOffset": true, "enableInvisGlow": true, "modelId": "abc",
        "skinSize": {"x": 128, "y": 64},
        "elements": [
          {"id": "left_arm", "show": false, "pos": {"x": 1, "y": 2.5, "z": -3},
           "rotation": {"x": 0, "y": 90, "z": 0}, "disableVanillaAnim": true},
          {"id": "head", "dup": true, "storeID": 0, "show": true},
          {"id": "body", "children": [{"name": "c", "storeID": 5, "color": "ff00ff",
            "textureSize": 2, "texture": true, "u": 3, "v": 4, "mcScale": 1, "glow": true,
            "itemRenderer": "right_hand",
            "copyTransform": {"storeID": 77, "px": true, "r": true, "multiply": 2},
            "children": [{"storeID": 6}]}]}
        ]}"#;
    let anim = br#"{"name": "$layer$Wings", "duration": 500, "interpolator": "LINEAR_SINGLE",
        "group": "g", "layerDefault": 1, "frames": [{"components": [
          {"storeID": 5, "color": "102030", "show": true, "pos": {"x": 1, "y": 2, "z": 3}},
          {"storeID": 999, "color": "0", "show": true}]}]}"#;
    let skin = png(128, 64);
    let zip = make_zip(&[
        ("config.json", config),
        ("skin.png", &skin),
        ("animations/g_Wings_x.json", anim),
        ("animations/q_bad_x.json", b"{\"frames\": []}"),
    ]);
    let p = read_project(&zip).unwrap();
    assert_eq!(p.skin_type, SkinType::Slim);
    assert!(!p.hide_head_if_skull);
    assert!(p.remove_bed_offset && p.invis_glow);
    assert!(!p.remove_armor_offset, "default is false when a duplicated root exists");
    assert_eq!(p.model_id.as_deref(), Some("abc"));
    assert_eq!(p.skin_size, [128, 64]);

    assert_eq!(p.roots.len(), 7);
    let la = &p.roots[2];
    assert!(la.hidden && la.disable_vanilla_anim);
    assert_eq!(la.pos, [1.0, 2.5, -3.0]);
    assert_eq!(la.rot, [0.0, 90.0, 0.0]);
    let dup = &p.roots[6];
    assert_eq!(dup.part, RootPart::Vanilla(0));
    assert!(dup.duplicated);
    assert_ne!(dup.store_id, 0, "a duplicate's storeID equal to its part ordinal is replaced");

    let c = &p.roots[1].children[0];
    assert_eq!(c.color, 0xff00ff);
    assert_eq!((c.tex_size, c.u, c.v), (2, 3, 4));
    assert_eq!(c.mc_scale, 1.0);
    assert!(c.glow && c.texture);
    assert_eq!(c.item_render, Some(1));
    let ct = c.copy_transform.as_ref().unwrap();
    assert_eq!(ct.store_id, 77);
    assert!(ct.copy_px && ct.copy_color && !ct.copy_py);
    assert_eq!(ct.multiply, 2.0);
    assert_eq!(c.children[0].store_id, 6);

    assert_eq!(p.anims.len(), 1);
    let a = &p.anims[0];
    assert_eq!(a.kind, AnimKind::Layer);
    assert_eq!(a.display_name, "Wings");
    assert_eq!(a.duration, 500);
    assert_eq!(a.interpolator, Interp::LinearSingle);
    assert_eq!(a.group.as_deref(), Some("g"));
    assert_eq!(a.layer_default, 1.0);
    assert!(a.additive);
    // Components for unknown storeIDs are dropped, as findPartRef does.
    assert_eq!(a.frames[0].components.len(), 1);
    let fc = &a.frames[0].components[0];
    assert_eq!(fc.color, [16.0, 32.0, 48.0]);
    assert_eq!(fc.pos, [1.0, 2.0, 3.0]);
    assert_eq!(fc.scale, [1.0; 3]);
}

#[test]
fn missing_config_is_error() {
    let skin = png(64, 64);
    let zip = make_zip(&[("skin.png", &skin)]);
    assert!(matches!(read_project(&zip), Err(CpmError::Invalid(_))));
}

#[test]
fn missing_skin_is_error() {
    let zip = make_zip(&[("config.json", br#"{"version": 1}"#)]);
    assert!(matches!(read_project(&zip), Err(CpmError::Invalid(_))));
}

#[test]
fn not_a_zip_is_error() {
    assert!(read_project(b"definitely not a zip").is_err());
}

#[test]
fn tags_are_warned_about() {
    let config = br#"{"version": 1}"#;
    let skin = png(64, 64);
    let items = br#"{"tags": [{"id": "my:swords", "elements": ["minecraft:iron_sword"]}]}"#;
    let blocks = br#"{"tags": []}"#;
    let zip = make_zip(&[
        ("config.json", config),
        ("skin.png", &skin),
        ("tags/items.json", items),
        ("tags/blocks.json", blocks),
        ("tags/biomes.json", b"not json"),
    ]);
    let p = read_project(&zip).unwrap();
    let tags: Vec<&String> = p.warnings.iter().filter(|w| w.contains("tags")).collect();
    assert_eq!(tags, ["items tags are not converted"]);
}
