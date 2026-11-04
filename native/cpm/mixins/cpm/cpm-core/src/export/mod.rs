//! `.cpmproject` → `.cpmmodel` export: a port of CPM's current editor export
//! (`Exporter.exportModel` → `exportSkin0` → `ExporterImpl.prepareExport` → `ModelWriter.finish`).
//!
//! Part order (`exportSkin0` + `ExporterImpl.prepareExport`): `SKIN_TYPE`, `CUBES`, `ROOT_INFO`s,
//! per-element `RENDER_EFFECT`s, copy-transform effects, `ANIMATION_NEW` ([`anim`]), `TEXTURE`,
//! render scale (`SCALING` + `MODEL_SCALE`), project effects (hide skull, armor offset, bed
//! offset, invisible glow), first-person hand, copy protection, `END`.
//! Parts whose data the [`Project`] does not carry (templates, gameplay scaling, tags, animated
//! textures, extra texture sheets) are not written; the project reader warns about them.
//!
//! Images are written as the project stores them (Java re-encodes them through `ImageIO`), so PNG
//! bytes may differ from the editor's while decoding to the same pixels.

mod anim;

use std::collections::HashMap;

use crate::format::{SkinType, HEADER, SHEET_CAPE, SHEET_ELYTRA};
use crate::io::{CpmError, Result};
use crate::project::{Project, ProjectElement, ProjectRoot, RootPart};
use crate::write::{checksummed, Writer};

/// `ModelPartType` ordinals.
mod part {
    pub const END: u8 = 0;
    pub const RENDER_EFFECT: u8 = 8;
    pub const SKIN_TYPE: u8 = 11;
    pub const CLONEABLE: u8 = 15;
    pub const TEXTURE: u8 = 17;
    pub const CUBES: u8 = 21;
    pub const ROOT_INFO: u8 = 22;
}

/// `RenderEffects` ordinals.
mod fx {
    pub const GLOW: u8 = 0;
    pub const COLOR: u8 = 3;
    pub const SINGLE_TEX: u8 = 4;
    pub const PER_FACE_UV: u8 = 5;
    pub const ITEM: u8 = 7;
    pub const HIDE_SKULL: u8 = 8;
    pub const REMOVE_ARMOR_OFFSET: u8 = 9;
    pub const EXTRUDE: u8 = 10;
    pub const MODEL_SCALE: u8 = 12;
    pub const SCALING: u8 = 13;
    pub const COPY_TRANSFORM: u8 = 14;
    pub const FIRST_PERSON_HAND: u8 = 15;
    pub const REMOVE_BED_OFFSET: u8 = 17;
    pub const INVIS_GLOW: u8 = 18;
}

/// `ModelPartRootInfo` flags.
const ROOT_HIDDEN: u8 = 1 << 0;
const ROOT_CREATE: u8 = 1 << 1;
const ROOT_MODEL: u8 = 1 << 2;
const ROOT_TRANSFORM: u8 = 1 << 3;
const ROOT_DISABLE_VANILLA: u8 = 1 << 4;

/// `Cube` V2 flags.
const HAS_MESH: u8 = 1 << 0;
const HAS_TEXTURE: u8 = 1 << 1;
const HIDDEN: u8 = 1 << 2;
const MESH_SCALED: u8 = 1 << 3;
const UV_SCALED: u8 = 1 << 4;
const MC_SCALED: u8 = 1 << 5;
const SCALED: u8 = 1 << 6;

/// `PlayerModelParts.CUSTOM_PART.ordinal()`: the parent of a duplicated root's fake cube.
const CUSTOM_PART: u32 = 6;

/// storeID → every element id the exporter assigned to an element with that storeID, in walk
/// order. Vanilla roots map to their part ordinal, duplicated and non-player (elytra, armour)
/// roots to their fake cube's id.
/// Several elements may share a storeID; animation frames apply to all of them.
pub type ElementIds = HashMap<i64, Vec<u32>>;

/// The ids `ExportHelper.flattenElements` assigns (cubes from 10, pre-order over the roots).
pub fn element_ids(project: &Project) -> ElementIds {
    let mut ids = ElementIds::new();
    walk(project, |node| {
        ids.entry(node.store_id()).or_default().push(node.id());
    });
    ids
}

/// Exports `project` as a complete `.cpmmodel` file (`ModelWriter.finish`). The description is
/// written as given.
pub fn export_model(project: &Project) -> Result<Vec<u8>> {
    let data = export_definition(project)?;
    let mut w = Writer::new();
    w.utf(&project.name);
    w.utf(&project.desc);
    w.varint(data.len() as u32).bytes(&data);
    w.varint(0); // no overflow
    match &project.icon_png {
        Some(icon) => {
            w.varint(icon.len() as u32).bytes(icon);
        }
        None => {
            w.varint(0);
        }
    }
    Ok(checksummed(HEADER, &w.into_bytes()))
}

/// The model data block: `0x53`, the parts, `END`, checksum (`exportSkin0`).
fn export_definition(project: &Project) -> Result<Vec<u8>> {
    let ids = element_ids(project);
    let mut w = Writer::new();

    // ModelPartSkinType: SkinType ordinal (SLIM, DEFAULT, UNKNOWN).
    let skin = match project.skin_type {
        SkinType::Slim => 0,
        SkinType::Default => 1,
        SkinType::Unknown => 2,
    };
    w.object_block(part::SKIN_TYPE, Writer::new().u8(skin));

    // ModelPartCubes (ExporterImpl.java:49-51).
    let mut cubes = Writer::new();
    let mut count = 0u32;
    walk(project, |node| {
        if let Some(c) = node.cube() {
            write_cube(&mut cubes, &c);
            count += 1;
        }
    });
    let mut cw = Writer::new();
    cw.varint(count).bytes(&cubes.into_bytes());
    w.object_block(part::CUBES, &cw);

    // ModelPartRootInfo (ExporterImpl.java:52-62): a non-duplicated player root by its part
    // position; a duplicated or RootModelType root (ROOT_MODEL) as "create from" its fake cube.
    let mut infos = Vec::new();
    walk(project, |node| {
        if let Node::Root { root, id } = node {
            infos.push((root, id));
        }
    });
    for (r, id) in infos {
        let (part, model) = match r.part {
            RootPart::Vanilla(p) => (p, false),
            RootPart::Model(t) => (t.ordinal(), true),
        };
        if r.duplicated || model {
            write_root_info(&mut w, part, model, [0.0; 3], [0.0; 3], r.hidden, r.disable_vanilla_anim, id);
        } else if r.hidden || !eps(r.pos, 0.1) || !eps(r.rot, 0.1) || r.disable_vanilla_anim {
            write_root_info(&mut w, part, false, r.pos, r.rot, r.hidden, r.disable_vanilla_anim, 0);
        }
    }

    // Element effects (ExporterImpl.java:63-90), copy transforms after all of them.
    let mut copies = Vec::new();
    walk(project, |node| {
        let Node::Element { el, id, .. } = node else { return };
        if el.glow {
            effect(&mut w, fx::GLOW, Writer::new().varint(id));
        }
        if el.recolor {
            let c = el.color;
            effect(&mut w, fx::COLOR, Writer::new().varint(id).u8((c >> 16) as u8).u8((c >> 8) as u8).u8(c as u8));
        }
        if el.single_tex {
            effect(&mut w, fx::SINGLE_TEX, Writer::new().varint(id));
        }
        if el.extrude {
            effect(&mut w, fx::EXTRUDE, Writer::new().varint(id));
        }
        if let Some(uv) = &el.face_uv {
            // PerFaceUV.writeFaces.
            let mut e = Writer::new();
            e.varint(id);
            let mask = uv.faces.iter().enumerate().fold(0u8, |m, (i, f)| m | ((f.is_some() as u8) << i));
            e.u8(mask);
            for f in uv.faces.iter().flatten() {
                e.varint(f.sx as u32).varint(f.sy as u32).varint(f.ex as u32).varint(f.ey as u32).u8(f.rot);
            }
            effect(&mut w, fx::PER_FACE_UV, &e);
        }
        if let Some(slot) = el.item_render {
            // EffectRenderItem: id, ItemSlot ordinal, slot id (always 0 in projects).
            effect(&mut w, fx::ITEM, Writer::new().varint(id).u8(slot).u8(0));
        }
        if let Some(ct) = &el.copy_transform {
            copies.push((ct, id));
        }
    });
    for (ct, to) in copies {
        // CopyTransformEffect.load(Editor): the last element in walk order with that storeID.
        let Some(from) = ids.get(&ct.store_id).and_then(|v| v.last()) else { continue };
        let mult = (ct.multiply - 1.0).abs() > 0.01;
        let bits = [
            ct.copy_px, ct.copy_py, ct.copy_pz, ct.copy_rx, ct.copy_ry, ct.copy_rz, ct.copy_sx, ct.copy_sy,
            ct.copy_sz, ct.copy_vis, ct.copy_color, ct.additive,
        ];
        let mut copy = bits.iter().enumerate().fold(0u32, |c, (i, b)| c | ((*b as u32) << i));
        if mult {
            copy |= 1 << 14;
        }
        let mut e = Writer::new();
        e.varint(*from).varint(to).varint(copy);
        if mult {
            e.bytes(&ct.multiply.to_be_bytes());
        }
        effect(&mut w, fx::COPY_TRANSFORM, &e);
    }

    // ModelPartAnimationNew (ExporterImpl.java:89-91), before the texture.
    if !project.anims.is_empty() {
        anim::write_animations(project, &mut w)?;
    }

    // ModelPartTexture for SKIN (TextureSheetType ordinal 0): write2s(size), image block.
    // A declared size of 0 in either axis would make every UV degenerate, so the PNG's own size
    // is used instead (the editor never saves one, but hand-edited projects might).
    let size = if project.skin_size[0] == 0 || project.skin_size[1] == 0 {
        project.skin_png_size
    } else {
        project.skin_size
    };
    let mut t = Writer::new();
    t.u8(0).i16(size[0] as u16 as i16).i16(size[1] as u16 as i16);
    t.varint(project.skin_png.len() as u32).bytes(&project.skin_png);
    w.object_block(part::TEXTURE, &t);
    // The CAPE and ELYTRA sheets, when the project has its own (ExporterImpl.java:92-98 writes
    // every editable sheet; Java's map order is unspecified, SKIN first here, then by ordinal).
    for (sheet, png, size) in [
        (SHEET_CAPE, &project.cape_png, project.cape_size),
        (SHEET_ELYTRA, &project.elytra_png, project.elytra_size),
    ] {
        let Some(png) = png else { continue };
        let mut t = Writer::new();
        t.u8(sheet).i16(size[0] as u16 as i16).i16(size[1] as u16 as i16);
        t.varint(png.len() as u32).bytes(png);
        w.object_block(part::TEXTURE, &t);
    }

    // Render scale (ExporterImpl.java:102-106): SCALING makes the ScaleData MODEL_SCALE sets.
    // Its gameplay values are not carried, so it is written empty, and only with a transform.
    if let Some(t) = &project.render_transform {
        effect(&mut w, fx::SCALING, &Writer::new());
        let mut e = Writer::new();
        part_transform(&mut e, t);
        effect(&mut w, fx::MODEL_SCALE, &e);
    }

    // Project effects (ExporterImpl.java:107-110).
    if !project.hide_head_if_skull {
        effect(&mut w, fx::HIDE_SKULL, Writer::new().bool(false));
    }
    if project.remove_armor_offset {
        effect(&mut w, fx::REMOVE_ARMOR_OFFSET, Writer::new().bool(true));
    }
    if project.remove_bed_offset {
        effect(&mut w, fx::REMOVE_BED_OFFSET, &Writer::new());
    }
    if project.invis_glow {
        effect(&mut w, fx::INVIS_GLOW, &Writer::new());
    }
    // EffectFirstPersonHandPos (ExporterImpl.java:111-112): the left hand, then the right.
    if let Some(hands) = &project.fp_hand {
        let mut e = Writer::new();
        for h in hands {
            part_transform(&mut e, h);
        }
        effect(&mut w, fx::FIRST_PERSON_HAND, &e);
    }

    // Copy protection (ExporterImpl.java:115-129; CopyProtection.lookup is case-insensitive).
    match project.copy_protection.to_ascii_lowercase().as_str() {
        "normal" => {}
        "cloneable" => {
            // ModelPartCloneable: field mask, then name, desc, icon when present.
            let mut c = Writer::new();
            let mut fields = 0u8;
            if !project.name.is_empty() {
                fields |= 1;
            }
            if !project.desc.is_empty() {
                fields |= 2;
            }
            if project.icon_png.is_some() {
                fields |= 4;
            }
            c.u8(fields);
            if !project.name.is_empty() {
                c.utf(&project.name);
            }
            if !project.desc.is_empty() {
                c.utf(&project.desc);
            }
            if let Some(icon) = &project.icon_png {
                c.varint(icon.len() as u32).bytes(icon);
            }
            w.object_block(part::CLONEABLE, &c);
        }
        "uuid_lock" => {
            // Java locks to the exporting player's UUID, which a converter cannot know.
            return Err(CpmError::Invalid("UUID-locked copy protection cannot be exported".into()));
        }
        other => {
            // Java's lookup returns null and the export throws.
            return Err(CpmError::Invalid(format!("unknown copy protection {other:?}")));
        }
    }

    w.object_block(part::END, &Writer::new());
    Ok(checksummed(HEADER, &w.into_bytes()))
}

/// A node of `Editor.elements` in walk order, with the id `flattenElements` gives it.
enum Node<'a> {
    Root { root: &'a ProjectRoot, id: u32 },
    Element { el: &'a ProjectElement, id: u32, parent: u32 },
}

/// The fields `Cube.saveDefinitionCubeV2` reads.
struct Cube {
    parent: u32,
    pos: [f32; 3],
    rotation: [f32; 3],
    size: [f32; 3],
    offset: [f32; 3],
    tex_size: i32,
    u: i32,
    v: i32,
    rgb: u32,
    mc_scale: f32,
    hidden: bool,
    /// `None` for a fake cube (Java leaves `meshScale` / `scale` null).
    mesh_scale: Option<[f32; 3]>,
    scale: Option<[f32; 3]>,
}

impl Node<'_> {
    fn id(&self) -> u32 {
        match *self {
            Node::Root { id, .. } | Node::Element { id, .. } => id,
        }
    }

    fn store_id(&self) -> i64 {
        match self {
            Node::Root { root, .. } => root.store_id,
            Node::Element { el, .. } => el.store_id,
        }
    }

    /// The cube `flattenElements` adds for this node, if any.
    fn cube(&self) -> Option<Cube> {
        match *self {
            // Cube.newFakeCube with the root's pos / rotation (ExportHelper.flattenElements:
            // duplicated and RootModelType roots).
            Node::Root { root, .. } if root.duplicated || matches!(root.part, RootPart::Model(_)) => Some(Cube {
                parent: CUSTOM_PART,
                pos: root.pos,
                rotation: root.rot,
                size: [0.0; 3],
                offset: [0.0; 3],
                tex_size: 0,
                u: 0,
                v: 0,
                rgb: 0,
                mc_scale: 0.0,
                hidden: false,
                mesh_scale: None,
                scale: None,
            }),
            Node::Root { .. } => None,
            // ElementType.NORMAL.preRenderUpdate sets texSize (Java's int negation wraps).
            Node::Element { el, parent, .. } => Some(Cube {
                parent,
                pos: el.pos,
                rotation: el.rotation,
                size: el.size,
                offset: el.offset,
                tex_size: if el.texture { if el.mirror { el.tex_size.wrapping_neg() } else { el.tex_size } } else { 0 },
                u: el.u,
                v: el.v,
                rgb: el.color,
                mc_scale: el.mc_scale,
                hidden: el.hidden,
                mesh_scale: Some(el.mesh_scale),
                scale: Some(el.scale),
            }),
        }
    }
}

/// `ExportHelper.flattenElements` / `walkElements`: pre-order over the roots, ids from 10.
fn walk<'a>(project: &'a Project, mut f: impl FnMut(Node<'a>)) {
    fn children<'a>(els: &'a [ProjectElement], parent: u32, next: &mut u32, f: &mut impl FnMut(Node<'a>)) {
        for el in els {
            let id = *next;
            *next += 1;
            f(Node::Element { el, id, parent });
            children(&el.children, id, next, f);
        }
    }
    let mut next = 10u32;
    for root in &project.roots {
        let id = match root.part {
            RootPart::Vanilla(p) if !root.duplicated => p as u32,
            _ => {
                next += 1;
                next - 1
            }
        };
        f(Node::Root { root, id });
        children(&root.children, id, &mut next, &mut f);
    }
}

/// `Vec3f.epsilon`.
fn eps(v: [f32; 3], e: f32) -> bool {
    v.iter().all(|x| x.abs() < e)
}

/// `Vec3f.sub(1).epsilon(e)`.
fn eps_one(v: [f32; 3], e: f32) -> bool {
    eps(v.map(|x| x - 1.0), e)
}

/// `ActionBuilder.limitVec(rot, 0, 360, true)` then `IOHelper.writeAngle`. Unlike
/// [`Writer::angle`], exactly 360 is left alone (and written as 65535), as Java does.
fn java_angle(w: &mut Writer, v: [f32; 3]) {
    for mut x in v {
        if !(0.0..=360.0).contains(&x) {
            x %= 360.0;
            if x < 0.0 {
                x += 360.0;
            }
        }
        w.i16(((x / 360.0 * 65535.0) as i32).clamp(0, 65535) as u16 as i16);
    }
}

/// `writeVec6b(pos)`, `writeAngle(rotation in degrees)`, `writeVec6b(scale)`
/// (`EffectModelScale.write`, each hand of `EffectFirstPersonHandPos.write`). `writeAngle` here
/// clamps into 0..360 degrees, without the wrap the editor's cube rotations get.
fn part_transform(w: &mut Writer, t: &crate::effects::PartTransform) {
    w.vec6b(t.pos);
    for x in t.rot {
        w.i16(((x / 360.0 * 65535.0) as i32).clamp(0, 65535) as u16 as i16);
    }
    w.vec6b(t.scale);
}

/// `Cube.saveDefinitionCubeV2`.
fn write_cube(w: &mut Writer, c: &Cube) {
    let has_mesh = !eps(c.size, 0.001);
    let texture = has_mesh && c.tex_size != 0;
    let mesh_scaled = c.mesh_scale.is_some_and(|s| !eps_one(s, 0.001));
    let scaled = c.scale.is_some_and(|s| !eps_one(s, 0.001));
    let uv_scaled = texture && c.tex_size != 1;
    let mc_scaled = has_mesh && c.mc_scale.abs() > 0.0001;
    let mut flags = 0;
    for (b, f) in [
        (has_mesh, HAS_MESH),
        (texture, HAS_TEXTURE),
        (c.hidden, HIDDEN),
        (mesh_scaled, MESH_SCALED),
        (uv_scaled, UV_SCALED),
        (mc_scaled, MC_SCALED),
        (scaled, SCALED),
    ] {
        if b {
            flags |= f;
        }
    }
    w.u8(flags).varint(c.parent).var_vec3(c.pos);
    java_angle(w, c.rotation);
    if has_mesh {
        w.var_vec3(c.size).var_vec3(c.offset);
        if texture {
            if uv_scaled {
                w.u8(c.tex_size as u8); // writeByte: low 8 bits
            }
            w.varint(c.u as u32).varint(c.v as u32);
        } else {
            w.u8((c.rgb >> 16) as u8).u8((c.rgb >> 8) as u8).u8(c.rgb as u8);
        }
        if mc_scaled {
            w.float2(c.mc_scale);
        }
    }
    if let (true, Some(s)) = (mesh_scaled, c.mesh_scale) {
        w.var_vec3(s);
    }
    if let (true, Some(s)) = (scaled, c.scale) {
        w.var_vec3(s);
    }
}

/// `ModelPartRootInfo.write`: `part` is a `PlayerModelParts` ordinal, or a `RootModelType` one
/// when `model` (`ROOT_MODEL`).
#[allow(clippy::too_many_arguments)]
fn write_root_info(
    w: &mut Writer,
    part: u8,
    model: bool,
    pos: [f32; 3],
    rot: [f32; 3],
    hidden: bool,
    disable: bool,
    create_from: u32,
) {
    let transform = !eps(pos, 0.1) || !eps(rot, 0.1);
    let mut flags = 0;
    if model {
        flags |= ROOT_MODEL;
    }
    if transform {
        flags |= ROOT_TRANSFORM;
    }
    if hidden {
        flags |= ROOT_HIDDEN;
    }
    if create_from != 0 {
        flags |= ROOT_CREATE;
    }
    if disable {
        flags |= ROOT_DISABLE_VANILLA;
    }
    let mut b = Writer::new();
    b.u8(flags).u8(part);
    if transform {
        b.var_vec3(pos);
        java_angle(&mut b, rot);
    }
    if create_from != 0 {
        b.varint(create_from);
    }
    w.object_block(part::ROOT_INFO, &b);
}

/// A `RENDER_EFFECT` part: effect ordinal, then its payload.
fn effect(w: &mut Writer, kind: u8, payload: &Writer) {
    let mut b = Writer::new();
    b.u8(kind).bytes(&payload.clone().into_bytes());
    w.object_block(part::RENDER_EFFECT, &b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_matches_limit_vec() {
        let mut w = Writer::new();
        java_angle(&mut w, [360.0, -90.0, 450.0]);
        // 360 stays 360 (65535); -90 wraps to 270; 450 wraps to 90.
        assert_eq!(w.into_bytes(), [0xFF, 0xFF, 0xBF, 0xFF, 0x3F, 0xFF]);
    }
}
