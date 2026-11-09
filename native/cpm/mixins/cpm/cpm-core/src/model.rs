//! The renderable model tree: vanilla bones plus a forest of custom cubes.

use std::collections::HashMap;

pub use crate::effects::PartTransform;
use crate::anim::texture::AnimTex;
use crate::effects::{decode_effects, Effect, FaceUv, TransformCopy};
use crate::format::{RawModel, RawTexture, SkinType};
use crate::io::{CpmError, Result};

/// Bone indices: head, body, left arm, right arm, left leg, right leg.
pub const BONE_COUNT: usize = 6;

/// `PlayerModelParts.CUSTOM_PART.ordinal()`: the parent of the fake cube a created root
/// (`ModelPartRootInfo` with `ROOT_CREATE`) starts as.
pub const CUSTOM_PART: u32 = 6;

/// `RootModelType`, in ordinal order: the non-player roots (cape, elytra wings, armour pieces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RootType {
    Cape,
    ElytraLeft,
    ElytraRight,
    ArmorHelmet,
    ArmorBody,
    ArmorLeftArm,
    ArmorRightArm,
    ArmorLeggingsBody,
    ArmorLeftLeg,
    ArmorRightLeg,
    ArmorLeftFoot,
    ArmorRightFoot,
}

/// `RootModelValues`: a root type's vanilla part (the box CPM draws for it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RootValues {
    pub pos: [f32; 3],
    pub offset: [f32; 3],
    pub size: [f32; 3],
    pub uv: [u32; 2],
    /// Inflation (`mcscale`).
    pub inflate: f32,
    pub mirror: bool,
}

impl RootType {
    /// By ordinal.
    pub const ALL: [RootType; 12] = [
        RootType::Cape,
        RootType::ElytraLeft,
        RootType::ElytraRight,
        RootType::ArmorHelmet,
        RootType::ArmorBody,
        RootType::ArmorLeftArm,
        RootType::ArmorRightArm,
        RootType::ArmorLeggingsBody,
        RootType::ArmorLeftLeg,
        RootType::ArmorRightLeg,
        RootType::ArmorLeftFoot,
        RootType::ArmorRightFoot,
    ];

    pub fn from_ordinal(o: u8) -> Option<RootType> {
        Self::ALL.get(o as usize).copied()
    }

    pub fn ordinal(self) -> u8 {
        self as u8
    }

    /// `getName()`: the enum name in lower case (the project's root `id`).
    pub fn name(self) -> &'static str {
        [
            "cape",
            "elytra_left",
            "elytra_right",
            "armor_helmet",
            "armor_body",
            "armor_left_arm",
            "armor_right_arm",
            "armor_leggings_body",
            "armor_left_leg",
            "armor_right_leg",
            "armor_left_foot",
            "armor_right_foot",
        ][self as usize]
    }

    pub fn from_name(name: &str) -> Option<RootType> {
        Self::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(name))
    }

    /// `getCopyFrom()`: the player part (bone index) an armour root follows.
    pub fn copy_from(self) -> Option<usize> {
        use RootType::*;
        match self {
            Cape | ElytraLeft | ElytraRight => None,
            ArmorHelmet => Some(0),
            ArmorBody | ArmorLeggingsBody => Some(1),
            ArmorLeftArm => Some(2),
            ArmorRightArm => Some(3),
            ArmorLeftLeg | ArmorLeftFoot => Some(4),
            ArmorRightLeg | ArmorRightFoot => Some(5),
        }
    }

    /// `RootModelValues.getFor` (RootModelValues.java:7-19).
    pub fn values(self) -> RootValues {
        use RootType::*;
        #[rustfmt::skip]
        let (pos, offset, size, uv, inflate, mirror) = match self {
            Cape =>              ([0.0, 0.0, 0.0],   [-5.0, 0.0, -1.0],  [10.0, 16.0, 1.0], [0, 0],   0.0, false),
            ElytraLeft =>        ([0.0, 0.0, 0.0],   [-10.0, 0.0, 0.0],  [10.0, 20.0, 2.0], [22, 0],  1.0, false),
            ElytraRight =>       ([0.0, 0.0, 0.0],   [0.0, 0.0, 0.0],    [10.0, 20.0, 2.0], [22, 0],  1.0, true),
            ArmorHelmet =>       ([0.0, 0.0, 0.0],   [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0],   [0, 0],   1.0, false),
            ArmorBody =>         ([0.0, 0.0, 0.0],   [-4.0, 0.0, -2.0],  [8.0, 12.0, 4.0],  [16, 16], 1.0, false),
            ArmorLeftArm =>      ([5.0, 2.0, 0.0],   [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0],  [40, 16], 1.0, true),
            ArmorRightArm =>     ([-5.0, 2.0, 0.0],  [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0],  [40, 16], 1.0, false),
            ArmorLeggingsBody => ([0.0, 0.0, 0.0],   [-4.0, 0.0, -2.0],  [8.0, 12.0, 4.0],  [16, 16], 0.5, false),
            ArmorLeftLeg =>      ([1.9, 12.0, 0.0],  [-2.0, 0.0, -2.0],  [4.0, 12.0, 4.0],  [0, 16],  0.5, true),
            ArmorRightLeg =>     ([-1.9, 12.0, 0.0], [-2.0, 0.0, -2.0],  [4.0, 12.0, 4.0],  [0, 16],  0.5, false),
            ArmorLeftFoot =>     ([1.9, 12.0, 0.0],  [-2.0, 0.0, -2.0],  [4.0, 12.0, 4.0],  [0, 16],  1.0, true),
            ArmorRightFoot =>    ([-1.9, 12.0, 0.0], [-2.0, 0.0, -2.0],  [4.0, 12.0, 4.0],  [0, 16],  1.0, false),
        };
        RootValues { pos, offset, size, uv, inflate, mirror }
    }
}

/// A non-player root (`ModelPartRootInfo` with `ROOT_MODEL | ROOT_CREATE`): the fake cube
/// `node` it was created from becomes a `RootModelElement` of type `kind`; the cube's position
/// and rotation are its part position (`posN` / `rotN`), its children are the root's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtraRoot {
    pub kind: RootType,
    /// Index into [`Model::cubes`] of the fake cube (no mesh, never drawn itself).
    pub node: usize,
    /// `ROOT_HIDDEN`: the vanilla part is not drawn (the root's cubes still are).
    pub hidden: bool,
    /// `ROOT_DISABLE_VANILLA`.
    pub disable_vanilla: bool,
}

/// A duplicated player root (`ModelPartRootInfo` with `ROOT_CREATE` alone, legacy
/// `ModelPartDupRoot`): the fake cube `node` becomes a second `RootModelElement` of player part
/// `bone`, posed from that part's vanilla pose plus the cube's position and rotation (`posN` /
/// `rotN`); it draws the part's vanilla box unless hidden, then its children.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DupRoot {
    pub bone: usize,
    /// Index into [`Model::cubes`] of the fake cube (no mesh, never drawn itself).
    pub node: usize,
    pub hidden: bool,
    pub disable_vanilla: bool,
}

/// `ItemSlot` of an item part (`ItemRenderer.slot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemPart {
    LeftHand,
    RightHand,
    Head,
    /// `LEFT_SHOULDER` / `RIGHT_SHOULDER`: where a shoulder parrot sits.
    LeftShoulder,
    RightShoulder,
    /// Another `ItemSlot` (any slot, tool, API) by ordinal: still an item part (no mesh, children
    /// not drawn), but nothing is drawn there (as in CPM).
    Other(u8),
}

impl ItemPart {
    /// By `ItemSlot` ordinal.
    pub fn from_ordinal(o: u8) -> ItemPart {
        match o {
            0 => ItemPart::LeftHand,
            1 => ItemPart::RightHand,
            2 => ItemPart::Head,
            5 => ItemPart::LeftShoulder,
            6 => ItemPart::RightShoulder,
            o => ItemPart::Other(o),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bone {
    pub pivot: [f32; 3],
    /// Degrees.
    pub rotation: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u32,
    pub pos: [f32; 3],
    /// Degrees.
    pub rotation: [f32; 3],
    pub offset: [f32; 3],
    pub size: [f32; 3],
    /// Mesh scale.
    pub scale: [f32; 3],
    /// Render scale (CPM `Cube.scale`): scales the cube's transform, children included.
    pub render_scale: [f32; 3],
    /// Inflate (CPM `mcScale`).
    pub mc_scale: f32,
    pub uv: [u32; 2],
    pub tex_scale: u8,
    pub color: Option<u32>,
    pub hidden: bool,
    pub glow: bool,
    pub single_tex: bool,
    /// `EXTRUDE`: drawn as `BoxRender.createTexturedExtruded`'s sliced texture, not a box.
    pub extrude: bool,
    pub face_uv: Option<[Option<FaceUv>; 6]>,
    /// An item part (`RenderedCube.itemRenderer`): it draws nothing, nor do its children; the
    /// item in that slot is drawn at its transform.
    pub item: Option<ItemPart>,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub skin_type: SkinType,
    pub bones: [Bone; BONE_COUNT],
    pub cubes: Vec<Node>,
    pub roots_by_bone: [Vec<usize>; BONE_COUNT],
    pub texture_size: [u32; 2],
    pub texture_png: Vec<u8>,
    /// The model's own elytra sheet (`TEXTURE` of type `ELYTRA`), if any.
    pub elytra_texture: Option<RawTexture>,
    /// The model's own cape sheet (`TEXTURE` of type `CAPE`), if any.
    pub cape_texture: Option<RawTexture>,
    /// The `ANIMATED_TEX` parts, in file order.
    pub anim_textures: Vec<AnimTex>,
    /// Non-player roots (armour, elytra, cape), in file order.
    pub extra_roots: Vec<ExtraRoot>,
    /// Duplicated player roots, in file order (drawn after their part's main root).
    pub dup_roots: Vec<DupRoot>,
    pub keep_vanilla: [bool; BONE_COUNT],
    pub vanilla_anim_disabled: [bool; BONE_COUNT],
    /// The render transform of the model's `ScaleData` (`MODEL_SCALE`): applied to the
    /// third-person player only, before every part (`ModelRenderManager.setupTransform` in
    /// `AnimationMode.PLAYER`). `None`: the identity.
    pub render_transform: Option<PartTransform>,
    /// `ModelDefinition.fpLeftHand` / `fpRightHand` (`FIRST_PERSON_HAND`), as `[left, right]`:
    /// applied to that arm in first person (`AnimationMode.HAND`).
    pub fp_hand: [Option<PartTransform>; 2],
    /// `ModelDefinition.hideHeadIfSkull`: the head is not drawn while a skull is worn. On unless
    /// a `HIDE_SKULL` effect turns it off.
    pub hide_head_if_skull: bool,
    /// `COPY_TRANSFORM` effects whose both ends exist, in file order (applied in that order).
    pub copies: Vec<TransformCopy>,
    index: HashMap<u32, usize>,
    /// Cube index -> index into `extra_roots` for the fake cubes that became roots.
    root_of: HashMap<usize, usize>,
    /// Cube index -> index into `dup_roots`.
    dup_of: HashMap<usize, usize>,
}

/// The deepest cube nesting a model may have (a cube on a bone is depth 1). Deeper trees
/// are refused by [`Model::build`] with [`CpmError::Invalid`].
pub const MAX_DEPTH: usize = 256;

pub(crate) const VANILLA_PIVOTS: [[f32; 3]; BONE_COUNT] = [
    [0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0],
    [5.0, 2.0, 0.0],
    [-5.0, 2.0, 0.0],
    [1.9, 12.0, 0.0],
    [-1.9, 12.0, 0.0],
];

impl Model {
    pub fn build(raw: RawModel) -> Result<Model> {
        // Vanilla pivots are identical for slim and default.
        let mut bones = [Bone { pivot: [0.0; 3], rotation: [0.0; 3] }; BONE_COUNT];
        for (b, p) in bones.iter_mut().zip(VANILLA_PIVOTS) {
            b.pivot = p;
        }
        for pp in &raw.part_pos {
            if let Some(b) = bones.get_mut(pp.part as usize) {
                for i in 0..3 {
                    b.pivot[i] += pp.pos[i];
                }
                b.rotation = pp.rot;
            }
        }

        let mut index = HashMap::new();
        let mut cubes = Vec::with_capacity(raw.cubes.len());
        for (i, c) in raw.cubes.iter().enumerate() {
            if index.insert(c.id, i).is_some() {
                return Err(CpmError::Invalid(format!("duplicate cube id {}", c.id)));
            }
            cubes.push(Node {
                id: c.id,
                pos: c.pos,
                rotation: c.rotation,
                offset: c.offset,
                size: c.size,
                scale: c.mesh_scale,
                render_scale: c.scale,
                mc_scale: c.mc_scale,
                uv: [c.u, c.v],
                tex_scale: c.tex_size,
                color: if c.tex_size == 0 { Some(c.rgb & 0xFF_FFFF) } else { None },
                hidden: c.hidden,
                glow: false,
                single_tex: false,
                extrude: false,
                face_uv: None,
                item: None,
                children: Vec::new(),
            });
        }

        // Link parents. Parent < 10 is a vanilla bone (6 = CUSTOM_PART, treated as body).
        let mut roots_by_bone: [Vec<usize>; BONE_COUNT] = Default::default();
        let mut parent_of: Vec<Option<usize>> = vec![None; cubes.len()];
        for (i, c) in raw.cubes.iter().enumerate() {
            if c.parent < 10 {
                let bone = if c.parent as usize >= BONE_COUNT { 1 } else { c.parent as usize };
                roots_by_bone[bone].push(i);
            } else {
                let p = *index.get(&c.parent).ok_or_else(|| {
                    CpmError::Invalid(format!("cube {} has unknown parent {}", c.id, c.parent))
                })?;
                parent_of[i] = Some(p);
                cubes[p].children.push(i);
            }
        }
        // ModelPartRootInfo.apply (ROOT_CREATE): the CUSTOM_PART child with id `createFrom` leaves
        // CUSTOM_PART and becomes a root of its type; a second ROOT_INFO for it finds nothing.
        let mut extra_roots = Vec::new();
        let mut root_of = HashMap::new();
        for r in &raw.extra_roots {
            let Some(kind) = RootType::from_ordinal(r.root) else { continue };
            let Some(&i) = index.get(&r.create_from) else { continue };
            if raw.cubes[i].parent != CUSTOM_PART || root_of.contains_key(&i) {
                continue;
            }
            roots_by_bone[1].retain(|&c| c != i);
            root_of.insert(i, extra_roots.len());
            extra_roots.push(ExtraRoot { kind, node: i, hidden: r.hidden, disable_vanilla: r.disable_vanilla });
        }
        // ModelPartRootInfo.apply / ModelPartDupRoot.apply for a player part: the same, as a
        // second root of that part (`addRoot(createFrom, PlayerModelParts)`).
        let mut dup_roots = Vec::new();
        let mut dup_of = HashMap::new();
        let mut dup_hidden_from_cube = Vec::new();
        for r in &raw.dup_roots {
            let bone = r.part as usize;
            if bone >= BONE_COUNT {
                continue;
            }
            let Some(&i) = index.get(&r.create_from) else { continue };
            if raw.cubes[i].parent != CUSTOM_PART || root_of.contains_key(&i) || dup_of.contains_key(&i) {
                continue;
            }
            roots_by_bone[1].retain(|&c| c != i);
            dup_of.insert(i, dup_roots.len());
            dup_hidden_from_cube.push(r.hidden_from_cube);
            dup_roots.push(DupRoot { bone, node: i, hidden: r.hidden, disable_vanilla: r.disable_vanilla });
        }

        // Depth and cycle check in one pass (each cube's depth is computed once): a chain that
        // comes back to a cube on it is a cycle, one longer than MAX_DEPTH is refused, which
        // also bounds the recursion of the mesh walk.
        let mut depth: Vec<Option<usize>> = vec![None; cubes.len()];
        let mut on_chain = vec![false; cubes.len()];
        for start in 0..cubes.len() {
            let mut chain = Vec::new();
            let mut cur = start;
            let base = loop {
                if let Some(d) = depth[cur] {
                    break d;
                }
                if on_chain[cur] {
                    return Err(CpmError::Invalid(format!("cube hierarchy cycle at cube {}", raw.cubes[cur].id)));
                }
                if chain.len() >= MAX_DEPTH {
                    return Err(CpmError::Invalid(format!(
                        "cube {} is nested deeper than {MAX_DEPTH} cubes",
                        raw.cubes[start].id
                    )));
                }
                on_chain[cur] = true;
                chain.push(cur);
                match parent_of[cur] {
                    Some(p) => cur = p,
                    None => break 0,
                }
            };
            for (k, &c) in chain.iter().rev().enumerate() {
                let d = base + k + 1;
                if d > MAX_DEPTH {
                    return Err(CpmError::Invalid(format!(
                        "cube {} is nested deeper than {MAX_DEPTH} cubes",
                        raw.cubes[start].id
                    )));
                }
                depth[c] = Some(d);
                on_chain[c] = false;
            }
        }

        // The model's ScaleData: made by the legacy SCALE part (its value is gameplay scaling,
        // not drawn) or a SCALING effect; MODEL_SCALE sets its render transform, and does
        // nothing without one (EffectModelScale.apply). A later SCALING replaces it.
        let mut scale_data: Option<Option<PartTransform>> = raw.scale.map(|_| None);
        let mut fp_hand = [None; 2];
        let mut copies = Vec::new();
        let mut hide_head_if_skull = true;
        for e in decode_effects(&raw.effects) {
            let target = |cube: u32| index.get(&cube).copied();
            match e {
                Effect::Glow { cube } => {
                    if let Some(i) = target(cube) {
                        cubes[i].glow = true;
                    }
                }
                Effect::Scale { cube, mc_scale, scale } => {
                    if let Some(i) = target(cube) {
                        cubes[i].scale = scale;
                        cubes[i].mc_scale = mc_scale;
                    }
                }
                Effect::Hide { cube } => {
                    if let Some(i) = target(cube) {
                        cubes[i].hidden = true;
                    }
                }
                Effect::Color { cube, rgb } => {
                    if let Some(i) = target(cube) {
                        cubes[i].color = Some(rgb & 0xFF_FFFF);
                    }
                }
                Effect::SingleTex { cube } => {
                    if let Some(i) = target(cube) {
                        cubes[i].single_tex = true;
                    }
                }
                Effect::PerFaceUv { cube, faces } => {
                    if let Some(i) = target(cube) {
                        cubes[i].face_uv = Some(faces);
                    }
                }
                Effect::Scaling => scale_data = Some(None),
                Effect::ModelScale(t) => {
                    if let Some(sd) = &mut scale_data {
                        *sd = Some(t);
                    }
                }
                Effect::FirstPersonHand([left, right]) => fp_hand = [Some(left), Some(right)],
                Effect::CopyTransform(ct) => {
                    // EffectCopyTransform.apply: dropped unless both elements exist.
                    let exists = |id: u32| (id as usize) < BONE_COUNT || index.contains_key(&id);
                    if exists(ct.from) && exists(ct.to) {
                        copies.push(ct);
                    }
                }
                Effect::Item { cube, slot, .. } => {
                    // EffectRenderItem.apply: the cube's mesh is emptied, it holds the item.
                    if let Some(i) = target(cube) {
                        cubes[i].size = [0.0; 3];
                        cubes[i].mc_scale = 0.0;
                        cubes[i].item = Some(ItemPart::from_ordinal(slot));
                    }
                }
                Effect::HideSkull { hide } => hide_head_if_skull = hide,
                Effect::Extrude { cube } => {
                    if let Some(i) = target(cube) {
                        cubes[i].extrude = true;
                    }
                }
                Effect::UvOverflow { .. } | Effect::Other(_) => {}
            }
        }

        // ModelPartRoot.apply (legacy MODEL_ROOT) hides the root as its fake cube is once the
        // effects ran (the legacy HIDE marks it): an empty cape or elytra root then hides the
        // vanilla piece, as the model meant.
        for r in &mut extra_roots {
            r.hidden |= cubes[r.node].hidden;
        }
        // ModelPartDupRoot.apply likewise; ModelPartRootInfo's duplicate takes its own flag.
        for (r, from_cube) in dup_roots.iter_mut().zip(dup_hidden_from_cube) {
            if from_cube {
                r.hidden = cubes[r.node].hidden;
            }
        }

        let elytra_texture = raw.elytra_texture;
        let cape_texture = raw.cape_texture;
        let (texture_size, texture_png) = match raw.texture {
            Some(t) => (t.declared, t.png),
            None => ([0, 0], Vec::new()),
        };
        Ok(Model {
            skin_type: raw.skin_type,
            bones,
            cubes,
            roots_by_bone,
            texture_size,
            texture_png,
            elytra_texture,
            cape_texture,
            anim_textures: raw.anim_textures,
            extra_roots,
            dup_roots,
            keep_vanilla: raw.keep_vanilla,
            vanilla_anim_disabled: raw.vanilla_anim_disabled,
            render_transform: scale_data.flatten(),
            fp_hand,
            hide_head_if_skull,
            copies,
            index,
            root_of,
            dup_of,
        })
    }

    pub fn node_index(&self, id: u32) -> Option<usize> {
        self.index.get(&id).copied()
    }

    /// The non-player root cube `node` was turned into, if it is one.
    pub fn extra_root(&self, node: usize) -> Option<&ExtraRoot> {
        self.root_of.get(&node).map(|&r| &self.extra_roots[r])
    }

    /// The duplicated player root cube `node` was turned into, if it is one.
    pub fn dup_root(&self, node: usize) -> Option<&DupRoot> {
        self.dup_of.get(&node).map(|&r| &self.dup_roots[r])
    }
}
