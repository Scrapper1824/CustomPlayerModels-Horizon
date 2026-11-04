//! `ElementsLoaderV1`: root parts and the element tree of `config.json`.

use std::collections::HashSet;

use serde_json::Value;

use super::{boolean, float, hex_color, int, long, obj, string, vec3, JMap};
use crate::model::RootType;

/// `PlayerModelParts` names, by ordinal (`CUSTOM_PART` is never a root in the editor).
const PLAYER_PARTS: [&str; 6] = ["head", "body", "left_arm", "right_arm", "left_leg", "right_leg"];
/// `ItemSlot` names, by ordinal.
const ITEM_SLOTS: [&str; 8] = [
    "left_hand",
    "right_hand",
    "head",
    "any_slot",
    "tool_slot",
    "left_shoulder",
    "right_shoulder",
    "api_slot",
];
/// `Direction` names, by ordinal (the order of [`FaceUv::faces`]).
const DIRECTIONS: [&str; 6] = ["up", "down", "north", "south", "east", "west"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootPart {
    /// `PlayerModelParts` ordinal 0..=5.
    Vanilla(u8),
    /// A `RootModelType` root (cape, elytra, armour).
    Model(RootType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRoot {
    pub part: RootPart,
    /// The part ordinal for an original player root; the file's `storeID` for a duplicate or a
    /// [`RootPart::Model`] root.
    pub store_id: i64,
    /// `dup`: a duplicated root (exported as a fake cube plus `ROOT_INFO` by id, as every
    /// [`RootPart::Model`] root is).
    pub duplicated: bool,
    pub pos: [f32; 3],
    /// Degrees.
    pub rot: [f32; 3],
    /// `!show`.
    pub hidden: bool,
    pub disable_vanilla_anim: bool,
    pub children: Vec<ProjectElement>,
}

/// A cube (`ModelElement` of type `NORMAL`), as `ElementsLoaderV1.loadElement` reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectElement {
    pub name: String,
    /// `show` (editor visibility, not exported).
    pub show_in_editor: bool,
    pub texture: bool,
    /// `textureSize`.
    pub tex_size: i32,
    pub offset: [f32; 3],
    pub pos: [f32; 3],
    /// Degrees.
    pub rotation: [f32; 3],
    pub size: [f32; 3],
    /// Render scale (`rscale` in the file).
    pub scale: [f32; 3],
    /// `meshScale` (`scale` in the file).
    pub mesh_scale: [f32; 3],
    pub u: i32,
    pub v: i32,
    /// `rgb`, parsed from the hex string.
    pub color: u32,
    pub mirror: bool,
    pub mc_scale: f32,
    pub glow: bool,
    pub recolor: bool,
    pub hidden: bool,
    pub single_tex: bool,
    pub extrude: bool,
    pub face_uv: Option<FaceUv>,
    /// `itemRenderer`: `ItemSlot` ordinal (slot id 0).
    pub item_render: Option<u8>,
    pub copy_transform: Option<CopyTransform>,
    pub store_id: i64,
    pub children: Vec<ProjectElement>,
}

/// `PerFaceUV`.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceUv {
    /// By `Direction` ordinal: up, down, north, south, east, west.
    pub faces: [Option<Face>; 6],
}

/// `PerFaceUV.Face`.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub sx: i32,
    pub sy: i32,
    pub ex: i32,
    pub ey: i32,
    /// `Rot` ordinal: 0 = 0°, 1 = 90°, 2 = 180°, 3 = 270°.
    pub rot: u8,
    pub auto_uv: bool,
}

/// `CopyTransformEffect`. `store_id` names the source element (resolved at export).
#[derive(Debug, Clone, PartialEq)]
pub struct CopyTransform {
    pub store_id: i64,
    pub copy_px: bool,
    pub copy_py: bool,
    pub copy_pz: bool,
    pub copy_rx: bool,
    pub copy_ry: bool,
    pub copy_rz: bool,
    pub copy_sx: bool,
    pub copy_sy: bool,
    pub copy_sz: bool,
    /// `cv`.
    pub copy_vis: bool,
    /// Read from key `r`, as Java's `load` does (its `toMap` writes `color`).
    pub copy_color: bool,
    pub additive: bool,
    pub multiply: f32,
}

pub(super) struct Roots {
    pub roots: Vec<ProjectRoot>,
    /// Any `elements` entry (player or not) is a duplicate (default of `removeArmorOffset`).
    pub any_duplicated: bool,
}

/// A root's name, as `dup` entries and player roots refer to it.
fn root_name(r: &ProjectRoot) -> &'static str {
    match r.part {
        RootPart::Vanilla(i) => PLAYER_PARTS[i as usize],
        RootPart::Model(t) => t.name(),
    }
}

fn new_root(part: u8) -> ProjectRoot {
    new_root_of(RootPart::Vanilla(part), part as i64)
}

fn new_root_of(part: RootPart, store_id: i64) -> ProjectRoot {
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

pub(super) fn load_roots(config: &JMap, warnings: &mut Vec<String>) -> Roots {
    // Editor.loadDefaultPlayerModel: every player part except CUSTOM_PART.
    let mut roots: Vec<ProjectRoot> = (0..6).map(new_root).collect();
    let mut any_duplicated = false;
    let mut replaced_ids = 0i64;
    let list = config.get("elements").and_then(Value::as_array);
    for map in list.into_iter().flatten().filter_map(Value::as_object) {
        let m = Some(map);
        let key = string(m, "id").unwrap_or_default();
        let dup = boolean(m, "dup", false);
        any_duplicated |= dup;
        let idx = if boolean(m, "customPart", false) {
            // ElementsLoaderV1: a RootModelType root, its storeID and dup flag from the file.
            let Some(t) = RootType::from_name(&key) else {
                warnings.push(format!("unknown custom root part {key}: skipped"));
                continue;
            };
            let mut root = new_root_of(RootPart::Model(t), long(m, "storeID", 0));
            root.duplicated = dup;
            roots.push(root);
            roots.len() - 1
        } else if dup {
            let Some(src) = roots.iter().position(|r| root_name(r).eq_ignore_ascii_case(&key)) else {
                continue;
            };
            let r = &roots[src];
            let mut root = new_root_of(r.part.clone(), long(m, "storeID", 0));
            root.duplicated = true;
            if let RootPart::Vanilla(part) = r.part
                && root.store_id == part as i64
            {
                // Java: "Fix duplicated parts broken storeID" with a random positive long.
                // A fixed, unlikely value keeps the reader deterministic.
                replaced_ids += 1;
                root.store_id = 0x5EED_0000_0000_0000 + replaced_ids;
            }
            roots.push(root);
            roots.len() - 1
        } else {
            match roots.iter().position(|r| root_name(r).eq_ignore_ascii_case(&key)) {
                Some(i) => i,
                None => continue,
            }
        };
        load_root(&mut roots[idx], map);
    }
    Roots { roots, any_duplicated }
}

/// The root fields every kind of root element reads.
fn load_root(root: &mut ProjectRoot, map: &JMap) {
    let m = Some(map);
    root.hidden = !boolean(m, "show", true);
    if let Some(children) = map.get("children") {
        load_children(children, &mut root.children);
    }
    root.pos = vec3(map.get("pos"), [0.0; 3]);
    root.rot = vec3(map.get("rotation"), [0.0; 3]);
    root.disable_vanilla_anim = boolean(m, "disableVanillaAnim", false);
}

fn load_children(list: &Value, out: &mut Vec<ProjectElement>) {
    for map in list.as_array().into_iter().flatten().filter_map(Value::as_object) {
        out.push(load_element(map));
    }
}

fn load_element(map: &JMap) -> ProjectElement {
    let m = Some(map);
    let mut children = Vec::new();
    if let Some(c) = map.get("children") {
        load_children(c, &mut children);
    }
    ProjectElement {
        name: string(m, "name").unwrap_or_default(),
        show_in_editor: boolean(m, "show", true),
        texture: boolean(m, "texture", false),
        tex_size: int(m, "textureSize", 1),
        offset: vec3(map.get("offset"), [0.0; 3]),
        pos: vec3(map.get("pos"), [0.0; 3]),
        rotation: vec3(map.get("rotation"), [0.0; 3]),
        size: vec3(map.get("size"), [1.0; 3]),
        scale: vec3(map.get("rscale"), [1.0; 3]),
        mesh_scale: vec3(map.get("scale"), [1.0; 3]),
        u: int(m, "u", 0),
        v: int(m, "v", 0),
        color: hex_color(m, "color"),
        mirror: boolean(m, "mirror", false),
        mc_scale: float(m, "mcScale", 0.0),
        glow: boolean(m, "glow", false),
        recolor: boolean(m, "recolor", false),
        hidden: boolean(m, "hidden", false),
        single_tex: boolean(m, "singleTex", false),
        extrude: boolean(m, "extrude", false),
        face_uv: obj(map.get("faceUV")).map(load_face_uv),
        item_render: string(m, "itemRenderer")
            .and_then(|n| ITEM_SLOTS.iter().position(|s| s.eq_ignore_ascii_case(&n)))
            .map(|i| i as u8),
        copy_transform: obj(map.get("copyTransform")).map(load_copy_transform),
        store_id: long(m, "storeID", 0),
        children,
    }
}

fn load_face_uv(map: &JMap) -> FaceUv {
    let faces = DIRECTIONS.map(|d| {
        obj(map.get(d)).map(|f| {
            let f = Some(f);
            let rot = match string(f, "rot").as_deref() {
                Some("90") => 1,
                Some("180") => 2,
                Some("270") => 3,
                _ => 0,
            };
            Face {
                sx: int(f, "sx", 0),
                sy: int(f, "sy", 0),
                ex: int(f, "ex", 0),
                ey: int(f, "ey", 0),
                rot,
                auto_uv: boolean(f, "autoUV", false),
            }
        })
    });
    FaceUv { faces }
}

fn load_copy_transform(map: &JMap) -> CopyTransform {
    let m = Some(map);
    CopyTransform {
        store_id: long(m, "storeID", -1),
        copy_px: boolean(m, "px", false),
        copy_py: boolean(m, "py", false),
        copy_pz: boolean(m, "pz", false),
        copy_rx: boolean(m, "rx", false),
        copy_ry: boolean(m, "ry", false),
        copy_rz: boolean(m, "rz", false),
        copy_sx: boolean(m, "sx", false),
        copy_sy: boolean(m, "sy", false),
        copy_sz: boolean(m, "sz", false),
        copy_vis: boolean(m, "cv", false),
        copy_color: boolean(m, "r", false),
        additive: boolean(m, "additive", false),
        multiply: float(m, "multiply", 1.0),
    }
}

/// Every storeID an animation component can refer to (roots and all elements).
pub(super) fn collect_store_ids(roots: &[ProjectRoot], out: &mut HashSet<i64>) {
    fn walk(e: &[ProjectElement], out: &mut HashSet<i64>) {
        for e in e {
            out.insert(e.store_id);
            walk(&e.children, out);
        }
    }
    for r in roots {
        out.insert(r.store_id);
        walk(&r.children, out);
    }
}
