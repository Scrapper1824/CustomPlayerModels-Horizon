//! Posing and mesh output: vanilla player limb motion (`HumanoidModel.setupAnim`), CPM part
//! positions and the animation [`Frame`] combined into posed, textured boxes.
//!
//! Ported from CPM's `ModelRenderManager` (`RedirectRenderer.render` / `translateRotate`,
//! `createBox`), `RootModelElement` (vanilla pose + part position, additive vs absolute
//! animation), `RenderedCube` and `BoxRender` (`createTextured`, `createTexturedSingle`,
//! per-face UV, `createColored`), which build quads exactly as Minecraft's `ModelPart.Cube`.
//!
//! Spaces:
//! - *model space*: Minecraft model pixels, Y down, the model origin 1.501 blocks above the feet,
//!   the face looking towards -Z (`ModelPart` space).
//! - *draw frame*: Horizon's mob draw pose (`native_mob_draw::mob_pose`): blocks, feet origin,
//!   Y up, +Z the way the body faces, +X its left.
//!
//! Face directions are CPM's (`PerFaceUV` / `BoxRender`): "up" is the face at the box's minimum
//! model Y (the top of an upright head), "down" the one at maximum Y; north / south are min / max
//! Z (north is the head's front), west / east min / max X (east is towards the player's left).

use glam::{Mat4, Vec3};

use crate::anim::engine::{CapeState, Engine, Frame, PartDelta, PoseInput};
use crate::anim::texture::{AnimatedTexture, SHEET_SKIN};
use crate::anim::{decode_legacy, decode_new, AnimSet};
use crate::effects::{FaceUv, TransformCopy};
use crate::format::{read_cpmmodel, ModelFile, SkinType, SHEET_CAPE, SHEET_ELYTRA};
use crate::io::{CpmError, Result};
use crate::model::{ExtraRoot, ItemPart, Model, Node, PartTransform, RootType, BONE_COUNT};

/// Face indices into [`PosedBox::faces`] (Horizon's `Direction::ALL` order).
pub const FACE_DOWN: usize = 0;
pub const FACE_UP: usize = 1;
pub const FACE_NORTH: usize = 2;
pub const FACE_SOUTH: usize = 3;
pub const FACE_WEST: usize = 4;
pub const FACE_EAST: usize = 5;

/// One box face, ready for `native_draw_quad`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceQuad {
    /// Model pixels in the box's local space (before [`PosedBox::transform`]), ordered top-left,
    /// bottom-left, bottom-right, top-right as seen from outside the face (counter-clockwise from
    /// outside: `(bl - tl) × (br - tl)` points out of the box).
    pub corners: [[f32; 3]; 4],
    /// `[u0, v0, u1, v1]` in texture pixels of the box's [`PosedBox::texture_px`] (for skin
    /// boxes, the model's *declared* texture size):
    /// TL ↔ (u0, v0), BL ↔ (u0, v1), BR ↔ (u1, v1), TR ↔ (u1, v0). `u0 > u1` (or `v0 > v1`)
    /// for mirrored / flipped faces.
    pub uv: [f32; 4],
}

/// An armour slot, for [`BoxTexture::Armor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmorSlot {
    Helmet,
    Chestplate,
    Leggings,
    Boots,
}

/// The texture a [`PosedBox`] samples; its UVs are in that texture's pixels, [`PosedBox::texture_px`]
/// in size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxTexture {
    /// The model's skin ([`LoadedModel::texture_rgba`]).
    Skin,
    /// The armour item worn in that slot: its humanoid layer, the humanoid leggings layer for
    /// [`ArmorSlot::Leggings`] (CPM's `ARMOR1` / `ARMOR2` sheets, 64x32).
    Armor(ArmorSlot),
    /// The model's own elytra sheet ([`LoadedModel::elytra`]) when it has one, else the worn
    /// elytra's texture (64x32).
    Elytra,
    /// A cape root's boxes: the model's own cape sheet ([`LoadedModel::cape`]) when it has one, else
    /// the player's cape (64x32); nothing without either (`CapeLayerMixin`; Java's template cape
    /// is not shipped).
    Cape,
    /// The vanilla cape of a model with no cape root: the player's cape only (vanilla's
    /// `CapeLayer`, 64x32).
    PlayerCape,
}

/// What a [`PosedBox`] was made from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSource {
    /// A model cube (its id).
    Cube(u32),
    /// A kept vanilla part (bone index 0..6), drawn from `PlayerPartValues`.
    Vanilla(usize),
    /// The second skin layer of a kept vanilla part (hat, jacket, sleeves, pants), at
    /// `PlayerPartValues` `(u2, v2)`, inflated as `PlayerModel.createMesh` does.
    VanillaLayer(usize),
    /// The vanilla armour / elytra part of a root type (`RootModelValues`), drawn for a worn slot
    /// on a root of that type that is not hidden, or on the player part when the model has no
    /// root of that type.
    RootVanilla(RootType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PosedBox {
    /// Box local space (model pixels, Y down) → draw frame (blocks, feet origin, Y up, +Z forward).
    pub transform: Mat4,
    /// In [`FACE_DOWN`], [`FACE_UP`], [`FACE_NORTH`], [`FACE_SOUTH`], [`FACE_WEST`], [`FACE_EAST`]
    /// order; `None` for a face the box does not have.
    pub faces: [Option<FaceQuad>; 6],
    /// 0xRRGGBB multiplier, 0xFFFFFF for none.
    pub tint: u32,
    pub glow: bool,
    /// False for a colour-only cube (CPM `texSize == 0`): draw it untextured (a white texel)
    /// times `tint`; its `uv`s are CPM's colour-box layout from (0, 0) and mean nothing.
    pub textured: bool,
    pub source: BoxSource,
    /// The texture the face UVs sample.
    pub texture: BoxTexture,
    /// The size of the pixel space the face UVs are in: the model's declared skin size for
    /// [`BoxTexture::Skin`]; 64x32 for armour and for vanilla elytra boxes; the elytra sheet's
    /// declared size (64x32 without one) for elytra cubes. UV / `texture_px` is the 0..1 position
    /// on the texture.
    pub texture_px: [u32; 2],
}

/// What an [`ItemAnchor`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemSlot {
    /// The main hand's stack (the right arm unless [`PoseInput::left_handed`]).
    MainHand,
    OffHand,
    /// The head slot's stack (a pumpkin, skull, banner, ...).
    Head,
}

/// Where an item is drawn: CPM's `ItemTransform` for an item part (`ModelRenderManager`
/// `render`: the part's transform, its offset and mesh scale), else the transform vanilla's layer
/// gets from the part it is drawn on (`PlayerModel.translateToHand`: the arm, half a pixel out for
/// slim arms; `translateToHead`: the head). The layer's own offsets (`ItemInHandLayer`'s hand
/// offset, `CustomHeadLayer`'s placement) come after it, as do the display context's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemAnchor {
    pub slot: ItemSlot,
    /// On the left arm (vanilla's `THIRD_PERSON_LEFT_HAND`); false for the right arm and the head.
    pub left: bool,
    /// The item part's cube id; `None` at the arm or head bone (the model has no part for it).
    pub part: Option<u32>,
    /// Blocks (vanilla's model space: Y down, the face towards -Z) to the draw frame.
    pub transform: Mat4,
}

/// Where a shoulder parrot is drawn (`ParrotOnShoulderLayer.submitOnShoulder` with CPM's
/// `ParrotOnShoulderLayerMixin`): the layer's translate (±0.4, -1.5 or -1.3 crouching, 0) on the
/// model root, then the model's `LEFT_SHOULDER` / `RIGHT_SHOULDER` item part transform (relative
/// to the model root) and, crouching, (0, -0.2, 0); without a part, the translate alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShoulderAnchor {
    /// The left shoulder (`ShoulderEntityLeft`).
    pub left: bool,
    /// The shoulder part's cube id; `None` where vanilla puts the parrot.
    pub part: Option<u32>,
    /// The parrot model's space (blocks, Y down, as `ParrotModel` draws after the layer's
    /// translate) to the draw frame.
    pub transform: Mat4,
}

/// A posed model: its boxes and where its held and head items and shoulder parrots go.
#[derive(Debug, Clone, PartialEq)]
pub struct Posed {
    pub boxes: Vec<PosedBox>,
    /// At most one per slot. A slot whose item parts are all hidden has none (nothing drawn).
    pub items: Vec<ItemAnchor>,
    /// At most one per shoulder; none for a shoulder whose parts are all hidden.
    pub shoulders: Vec<ShoulderAnchor>,
}

/// An extra texture sheet of the model, decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// RGBA8, `png_px` in size.
    pub rgba: Vec<u8>,
    /// The size the model declares: the space its cubes' UVs are in.
    pub texture_px: [u32; 2],
    /// The decoded PNG's real size.
    pub png_px: [u32; 2],
}

/// A decoded model ready to pose.
pub struct LoadedModel {
    pub name: String,
    pub model: Model,
    pub engine: Engine,
    /// The decoded texture, RGBA8, `png_px` in size (empty when the model has no texture).
    pub texture_rgba: Vec<u8>,
    /// The texture size the model declares (`SKIN` block): the space every UV is in.
    pub texture_px: [u32; 2],
    /// The decoded PNG's real size (may differ from `texture_px`).
    pub png_px: [u32; 2],
    /// The model's own elytra sheet, if it has one ([`BoxTexture::Elytra`]).
    pub elytra: Option<Sheet>,
    /// The model's own cape sheet, if it has one ([`BoxTexture::Cape`]).
    pub cape: Option<Sheet>,
    /// The model's animated textures ([`Model::anim_textures`]), with their state; applied to
    /// `texture_rgba`, `cape` and `elytra` by [`LoadedModel::tick_textures`].
    pub anim_textures: Vec<AnimatedTexture>,
}

/// Decodes a `.cpmmodel` file.
pub fn load(bytes: &[u8]) -> Result<LoadedModel> {
    LoadedModel::from_file(read_cpmmodel(bytes)?)
}

impl LoadedModel {
    pub fn from_file(file: ModelFile) -> Result<LoadedModel> {
        if let Some(link) = &file.data.link {
            return Err(CpmError::NeedsLink(link.clone()));
        }
        let raw = file.data;
        let legacy = raw.legacy_anim.clone();
        let new = raw.new_anim.clone();
        let model = Model::build(raw)?;
        let set = match (legacy, new) {
            (Some(a), _) => decode_legacy(&a, &model)?,
            (None, Some(a)) => decode_new(&a, &model)?,
            (None, None) => AnimSet::default(),
        };
        let (png_px, texture_rgba) =
            if model.texture_png.is_empty() { ([0, 0], Vec::new()) } else { decode_png(&model.texture_png)? };
        let sheet = |t: &Option<crate::format::RawTexture>| -> Result<Option<Sheet>> {
            match t {
                Some(t) if !t.png.is_empty() => {
                    let (png_px, rgba) = decode_png(&t.png)?;
                    Ok(Some(Sheet { rgba, texture_px: t.declared, png_px }))
                }
                _ => Ok(None),
            }
        };
        let elytra = sheet(&model.elytra_texture)?;
        let cape = sheet(&model.cape_texture)?;
        let anim_textures = model.anim_textures.iter().map(|&t| AnimatedTexture::new(t)).collect();
        Ok(LoadedModel {
            elytra,
            cape,
            anim_textures,
            name: file.name,
            texture_px: model.texture_size,
            png_px,
            texture_rgba,
            engine: Engine::new(set),
            model,
        })
    }

    /// `AnimationRegistry.tickAnimated` at `time_ms`: copies each animated texture's frame into
    /// its sheet (the skin, cape or elytra; another sheet, or one the model lacks, is skipped).
    /// True when a sheet's pixels changed (the image needs uploading again).
    pub fn tick_textures(&mut self, time_ms: u64) -> bool {
        let mut changed = false;
        for a in &mut self.anim_textures {
            let (rgba, size) = match a.tex.sheet {
                SHEET_SKIN => (&mut self.texture_rgba, self.png_px),
                SHEET_CAPE => match &mut self.cape {
                    Some(s) => (&mut s.rgba, s.png_px),
                    None => continue,
                },
                SHEET_ELYTRA => match &mut self.elytra {
                    Some(s) => (&mut s.rgba, s.png_px),
                    None => continue,
                },
                _ => continue,
            };
            changed |= a.update(time_ms, rgba, size);
        }
        changed
    }

    /// Advances the animation to `input.time_ms` and returns the posed boxes (worn armour and
    /// elytra per `input.equipment`).
    pub fn pose(&mut self, input: &PoseInput) -> Vec<PosedBox> {
        self.pose_with_items(input).boxes
    }

    /// As [`pose`](Self::pose), with the item anchors ([`pose_model_with_items`]).
    pub fn pose_with_items(&mut self, input: &PoseInput) -> Posed {
        let frame = self.engine.sample(input);
        pose_model_with_items(&self.model, &frame, input)
    }

    /// The first-person arm ([`pose_arm`]).
    pub fn pose_arm(&self, left: bool) -> Vec<PosedBox> {
        pose_arm(&self.model, left)
    }
}

/// The model's icon (the file's icon block) as RGBA: (width, height, pixels). `None` when it has
/// none or it is not a PNG that decodes.
pub fn icon_rgba(file: &ModelFile) -> Option<(u32, u32, Vec<u8>)> {
    if file.icon_png.is_empty() {
        return None;
    }
    let ([w, h], rgba) = decode_png(&file.icon_png).ok()?;
    (w > 0 && h > 0).then_some((w, h, rgba))
}

fn decode_png(bytes: &[u8]) -> Result<([u32; 2], Vec<u8>)> {
    let err = |e: png::DecodingError| CpmError::Png(e.to_string());
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().map_err(err)?;
    let size = r.output_buffer_size().ok_or_else(|| CpmError::Png("image too large".into()))?;
    let mut buf = vec![0; size];
    let info = r.next_frame(&mut buf).map_err(err)?;
    buf.truncate(info.buffer_size());
    let px = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(CpmError::Png("palette not expanded".into())),
    };
    if rgba.len() != px * 4 {
        return Err(CpmError::Png("unexpected decoded size".into()));
    }
    Ok(([info.width, info.height], rgba))
}

// ---------------------------------------------------------------------------------------------
// Vanilla motion

/// `PlayerPartValues`: pivot, box offset, size, uv and second-layer uv (`u2`, `v2`) per bone;
/// slim arms for `SkinType::Slim`.
struct PartValues {
    pos: [f32; 3],
    offset: [f32; 3],
    size: [f32; 3],
    uv: [u32; 2],
    uv2: [u32; 2],
}

fn part_values(bone: usize, skin: SkinType) -> PartValues {
    let slim = skin == SkinType::Slim;
    let (pos, offset, size, uv, uv2) = match bone {
        0 => ([0.0, 0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0, 0], [32, 0]),
        1 => ([0.0, 0.0, 0.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16, 16], [16, 32]),
        2 if slim => ([5.0, 2.0, 0.0], [-1.0, -2.0, -2.0], [3.0, 12.0, 4.0], [32, 48], [48, 48]),
        2 => ([5.0, 2.0, 0.0], [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], [32, 48], [48, 48]),
        3 if slim => ([-5.0, 2.0, 0.0], [-2.0, -2.0, -2.0], [3.0, 12.0, 4.0], [40, 16], [40, 32]),
        3 => ([-5.0, 2.0, 0.0], [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], [40, 16], [40, 32]),
        4 => ([1.9, 12.0, 0.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16, 48], [0, 48]),
        _ => ([-1.9, 12.0, 0.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0, 16], [0, 32]),
    };
    PartValues { pos, offset, size, uv, uv2 }
}

/// `PlayerModel.createMesh`'s `CubeDeformation` for each bone's second layer: the hat 0.5,
/// the jacket, sleeves and pants 0.25.
fn layer_inflate(bone: usize) -> f32 {
    if bone == HEAD { 0.5 } else { 0.25 }
}

/// The vanilla player model's UVs are in a 64×64 sheet; the model's texture may be larger or
/// smaller (`texture_px`), so each axis is scaled by its size / 64 (0 keeps the 64 layout).
fn sheet_scale(texture_px: [u32; 2]) -> [f32; 2] {
    texture_px.map(|t| if t == 0 { 1.0 } else { t as f32 / 64.0 })
}

fn scale_uvs(mut faces: [Option<FaceQuad>; 6], [su, sv]: [f32; 2]) -> [Option<FaceQuad>; 6] {
    for f in faces.iter_mut().flatten() {
        f.uv = [f.uv[0] * su, f.uv[1] * sv, f.uv[2] * su, f.uv[3] * sv];
    }
    faces
}

const HEAD: usize = 0;
const BODY: usize = 1;
const LEFT_ARM: usize = 2;
const RIGHT_ARM: usize = 3;
const LEFT_LEG: usize = 4;
const RIGHT_LEG: usize = 5;

/// A vanilla `ModelPart` pose: position in pixels, rotation in radians.
#[derive(Debug, Clone, Copy, Default)]
struct Limb {
    pos: Vec3,
    rot: Vec3,
}

/// `Mth.rotLerpRad`.
fn rot_lerp_rad(a: f32, from: f32, to: f32) -> f32 {
    use std::f32::consts::PI;
    let mut diff = to - from;
    while diff < -PI {
        diff += PI * 2.0;
    }
    while diff >= PI {
        diff -= PI * 2.0;
    }
    from + a * diff
}

/// `HumanoidModel.setupAnim` (Minecraft 26.x, as Horizon's `mobs::anim::humanoid` ports it) for
/// the state a [`PoseInput`] carries: empty hands, no attack swing, swim amount 0 or 1.
fn vanilla_limbs(i: &PoseInput) -> [Limb; BONE_COUNT] {
    use std::f32::consts::{FRAC_PI_4, PI};
    let mut l: [Limb; BONE_COUNT] = std::array::from_fn(|b| Limb {
        pos: Vec3::from_array(part_values(b, SkinType::Default).pos),
        rot: Vec3::ZERO,
    });
    l[HEAD].rot.x = i.pitch.to_radians();
    l[HEAD].rot.y = i.head_yaw.to_radians();
    if i.fall_flying {
        l[HEAD].rot.x = -FRAC_PI_4;
    }
    let (pos, sp) = (i.walk_position, i.walk_speed);
    l[RIGHT_ARM].rot.x = (pos * 0.6662 + PI).cos() * 2.0 * sp * 0.5;
    l[LEFT_ARM].rot.x = (pos * 0.6662).cos() * 2.0 * sp * 0.5;
    l[RIGHT_LEG].rot.x = (pos * 0.6662).cos() * 1.4 * sp;
    l[LEFT_LEG].rot.x = (pos * 0.6662 + PI).cos() * 1.4 * sp;
    l[RIGHT_LEG].rot.y = 0.005;
    l[LEFT_LEG].rot.y = -0.005;
    l[RIGHT_LEG].rot.z = 0.005;
    l[LEFT_LEG].rot.z = -0.005;
    if i.riding {
        l[RIGHT_ARM].rot.x += -PI / 5.0;
        l[LEFT_ARM].rot.x += -PI / 5.0;
        l[RIGHT_LEG].rot = Vec3::new(-1.413_716_7, PI / 10.0, 0.078_539_82);
        l[LEFT_LEG].rot = Vec3::new(-1.413_716_7, -PI / 10.0, -0.078_539_82);
    }
    if i.sneaking {
        l[BODY].rot.x = 0.5;
        l[RIGHT_ARM].rot.x += 0.4;
        l[LEFT_ARM].rot.x += 0.4;
        l[RIGHT_LEG].pos.z += 4.0;
        l[LEFT_LEG].pos.z += 4.0;
        l[HEAD].pos.y += 4.2;
        l[BODY].pos.y += 3.2;
        l[LEFT_ARM].pos.y += 3.2;
        l[RIGHT_ARM].pos.y += 3.2;
    }
    // AnimationUtils.bobModelPart
    for (arm, s) in [(RIGHT_ARM, 1.0), (LEFT_ARM, -1.0)] {
        l[arm].rot.z += s * ((i.age * 0.09).cos() * 0.05 + 0.05);
        l[arm].rot.x += s * ((i.age * 0.067).sin() * 0.05);
    }
    if i.swimming {
        // HumanoidModel swim animation with swimAmount = 1 (targets reached exactly).
        if !i.fall_flying {
            l[HEAD].rot.x = rot_lerp_rad(1.0, l[HEAD].rot.x, -FRAC_PI_4);
        }
        let swim_pos = pos.rem_euclid(26.0);
        let quadratic = |x: f32| -65.0 * x + x * x;
        let targets = if swim_pos < 14.0 {
            let stroke = 1.870_796_4 * quadratic(swim_pos) / quadratic(14.0);
            Some(([0.0, PI, PI + stroke], [0.0, PI, PI - stroke]))
        } else if swim_pos < 22.0 {
            let t = (swim_pos - 14.0) / 8.0;
            let x = PI / 2.0 * t;
            Some(([x, PI, 5.012_389 - 1.870_796_4 * t], [x, PI, 1.270_796_3 + 1.870_796_4 * t]))
        } else if swim_pos < 26.0 {
            let t = (swim_pos - 22.0) / 4.0;
            let x = PI / 2.0 - PI / 2.0 * t;
            Some(([x, PI, PI], [x, PI, PI]))
        } else {
            None
        };
        if let Some((left, right)) = targets {
            l[LEFT_ARM].rot = Vec3::from_array(left);
            l[RIGHT_ARM].rot = Vec3::from_array(right);
        }
        l[LEFT_LEG].rot.x = 0.3 * (pos * 0.333_333_34 + PI).cos();
        l[RIGHT_LEG].rot.x = 0.3 * (pos * 0.333_333_34).cos();
    }
    l
}

/// `AvatarRenderer.scale`: every player model is drawn at 15/16 size (Horizon's `species_scale`).
pub const PLAYER_SCALE: f32 = 0.9375;

/// The model-space → draw-frame transform (`LivingEntityRenderer.submit` / `setupRotations`
/// after the body-yaw turn Horizon's draw pose already has).
fn root_transform(i: &PoseInput) -> Mat4 {
    // Minecraft: rotY(180 - bodyYaw) · [tilt] · scale(-1,-1,1) · scale(0.9375 · entity)
    // · translate(0,-1.501,0). Horizon's pose is rotY(-bodyYaw), so the rest starts with rotY(180).
    // The scale comes before the -1.501 lift, so it shrinks the player about the feet.
    let mut m = Mat4::from_rotation_y(std::f32::consts::PI);
    if i.swimming || i.fall_flying {
        // AvatarRenderer.setupRotations, swim amount / fall-flying scale taken as 1. A swimmer out
        // of water (crawling) lies flat: rotX(isInWater ? -90 - xRot : -90).
        let tilt = if i.swimming && !i.fall_flying && !i.in_water { -90.0 } else { -90.0 - i.pitch };
        m *= Mat4::from_rotation_x(tilt.to_radians());
        if i.swimming {
            m *= Mat4::from_translation(Vec3::new(0.0, -1.0, 0.3));
        }
    }
    m * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(PLAYER_SCALE))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0))
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
}

/// `ModelRenderManager.transform(stack, pos)` in model pixels: `translateRotate(pos / 16, rot)`
/// then the clamped scale. Under the 1/16 of model space, `T(pos / 16)` is `T(pos)` here.
fn part_transform(t: &PartTransform) -> Mat4 {
    translate_rotate(Vec3::from_array(t.pos), deg(t.rot)) * Mat4::from_scale(Vec3::from_array(t.effective_scale()))
}

/// `setupTransform`'s render scale: the model's render transform, set up before every part (in
/// model space, after the entity's own transform). CPM applies it to the third-person player
/// (`AnimationMode.PLAYER`) and in its own GUI previews (`DirectModelRenderManager.doRender0`),
/// not to the first-person arm ([`pose_arm`]).
fn render_transform(model: &Model) -> Mat4 {
    model.render_transform.as_ref().map_or(Mat4::IDENTITY, part_transform)
}

/// `translateRotate`: translate (pixels), then rotate Z · Y · X (radians).
fn translate_rotate(pos: Vec3, rot: Vec3) -> Mat4 {
    Mat4::from_translation(pos)
        * Mat4::from_rotation_z(rot.z)
        * Mat4::from_rotation_y(rot.y)
        * Mat4::from_rotation_x(rot.x)
}

fn apply3(own: [f32; 3], delta: Option<[f32; 3]>, additive: bool) -> [f32; 3] {
    match delta {
        None => own,
        Some(d) if additive => [own[0] + d[0], own[1] + d[1], own[2] + d[2]],
        Some(d) => d,
    }
}

fn deg(v: [f32; 3]) -> Vec3 {
    Vec3::from_array(v.map(f32::to_radians))
}

/// An element's current render state (`RenderedCube` after `reset`, the animations and the copy
/// transforms): local position (pixels), rotation (radians), render scale, visibility, tint.
#[derive(Debug, Clone, Copy)]
struct ElemState {
    pos: Vec3,
    rot: Vec3,
    scale: [f32; 3],
    visible: bool,
    tint: u32,
}

/// Every element's state: bones 0..6 (`RootModelElement`, final position / rotation, as
/// `getTransformPosition` returns them), then the cubes by index (a non-player root's fake cube
/// holds that root's `RootModelElement` state).
struct States(Vec<ElemState>);

/// `bases[i]`: the pose (`setPosAndRot`'s px..rz) of the vanilla part `model.extra_roots[i]` is
/// redirected from, before its part position; then the same for each of `model.dup_roots`.
type Bases = [(Vec3, Vec3)];

impl States {
    /// Bone `b`'s pose (`RootModelElement.setPosAndRot` + animation) is `roots[b]`; non-player
    /// roots are posed from `bases`; cubes take their own values and the frame's deltas; then the
    /// model's copy transforms are applied.
    fn new(model: &Model, frame: &Frame, roots: [(Vec3, Vec3); BONE_COUNT], bases: &Bases) -> States {
        let none = PartDelta::default();
        let mut v = Vec::with_capacity(BONE_COUNT + model.cubes.len());
        for (b, (pos, rot)) in roots.into_iter().enumerate() {
            // RootModelElement: render scale always 1, displayed unless hidden (vanilla part off).
            v.push(ElemState { pos, rot, scale: [1.0; 3], visible: model.keep_vanilla[b], tint: 0xFF_FFFF });
        }
        for node in &model.cubes {
            let d = frame.parts.get(&node.id).unwrap_or(&none);
            let pos = Vec3::from_array(apply3(node.pos, d.pos, d.pos_additive));
            let rot = match d.rot {
                None => deg(node.rotation),
                Some(r) if d.rot_additive => deg(node.rotation) + deg(r),
                Some(r) => deg(r),
            };
            let mut scale = node.render_scale;
            if let Some(s) = d.scale {
                for k in 0..3 {
                    if d.scale_additive {
                        scale[k] *= s[k];
                    } else if s[k] != 0.0 {
                        scale[k] = s[k];
                    }
                }
            }
            let tint = match (node.color, d.color) {
                (Some(_), Some(c)) => {
                    let c = c.map(|v| (v as i32).clamp(0, 255) as u32);
                    c[0] << 16 | c[1] << 8 | c[2]
                }
                (Some(c), None) => c & 0xFF_FFFF,
                (None, _) => 0xFF_FFFF,
            };
            v.push(ElemState { pos, rot, scale, visible: d.visible.unwrap_or(!node.hidden), tint });
        }
        for (r, &(base_pos, base_rot)) in model.extra_roots.iter().zip(bases) {
            let node = &model.cubes[r.node];
            // RootModelElement.setPosAndRot: the part's pose plus posN / rotN (the fake cube's
            // position / rotation); with vanilla animation disabled, the default position.
            let (pos_n, rot_n) = (Vec3::from_array(node.pos), deg(node.rotation));
            let (def_pos, def_rot) = if r.disable_vanilla {
                (Vec3::from_array(r.kind.values().pos) + pos_n, rot_n)
            } else {
                (base_pos + pos_n, base_rot + rot_n)
            };
            let d = frame.parts.get(&node.id).unwrap_or(&none);
            v[BONE_COUNT + r.node] = ElemState {
                pos: delta_pos(def_pos, d),
                rot: delta_rot(def_rot, d),
                scale: [1.0; 3],
                // Only the hidden flag shows or hides the root's vanilla piece: an animation's
                // visibility does nothing on a root (`RootModelElement.setVisible` is empty).
                visible: !r.hidden,
                tint: 0xFF_FFFF,
            };
        }
        let dup_bases = bases.get(model.extra_roots.len()..).unwrap_or(&[]);
        for (r, &(base_pos, base_rot)) in model.dup_roots.iter().zip(dup_bases) {
            let node = &model.cubes[r.node];
            // RootModelElement.setPosAndRot, as for the part's main root, with the fake cube's
            // position / rotation as posN / rotN.
            let (pos_n, rot_n) = (Vec3::from_array(node.pos), deg(node.rotation));
            let (def_pos, def_rot) = if r.disable_vanilla {
                (Vec3::from_array(part_values(r.bone, model.skin_type).pos) + pos_n, rot_n)
            } else {
                (base_pos + pos_n, base_rot + rot_n)
            };
            let d = frame.parts.get(&node.id).unwrap_or(&none);
            v[BONE_COUNT + r.node] = ElemState {
                pos: delta_pos(def_pos, d),
                rot: delta_rot(def_rot, d),
                scale: [1.0; 3],
                // As above: not animatable.
                visible: !r.hidden,
                tint: 0xFF_FFFF,
            };
        }
        let mut s = States(v);
        // ModelRenderManager.RedirectHolder.setupTransform: AnimationRegistry.applyCopy after the
        // animations and the root poses, before rendering.
        for ct in &model.copies {
            s.copy(model, ct);
        }
        s
    }

    fn slot(model: &Model, id: u32) -> Option<usize> {
        if (id as usize) < BONE_COUNT {
            Some(id as usize)
        } else {
            model.node_index(id).map(|i| BONE_COUNT + i)
        }
    }

    /// `CopyTransform.apply`.
    fn copy(&mut self, model: &Model, ct: &TransformCopy) {
        let (Some(fi), Some(ti)) = (Self::slot(model, ct.from), Self::slot(model, ct.to)) else { return };
        let from = self.0[fi];
        let is_root = |i: usize| {
            i < BONE_COUNT || model.extra_root(i - BONE_COUNT).is_some() || model.dup_root(i - BONE_COUNT).is_some()
        };
        let (from_root, to_root) = (is_root(fi), is_root(ti));
        let add = ct.has(TransformCopy::ADDITIVE);
        let m = ct.multiply;
        let to = &mut self.0[ti];
        // setPosition / setRotation(additive, ...): additive adds the copied axes (the others add
        // 0); set replaces the copied axes and keeps the others (they are set to themselves).
        for k in 0..3 {
            if ct.pos(k) {
                let v = from.pos[k] * m;
                to.pos[k] = if add { to.pos[k] + v } else { v };
            }
            if ct.rot(k) {
                let v = from.rot[k] * m;
                to.rot[k] = if add { to.rot[k] + v } else { v };
            }
        }
        if to_root {
            // RootModelElement: render scale, visibility and colour are not settable.
            return;
        }
        // setRenderScale: additive multiplies, set replaces; a 0 component is skipped.
        for k in 0..3 {
            if ct.scale(k) {
                let v = from.scale[k] * m;
                if v != 0.0 {
                    to.scale[k] = if add { to.scale[k] * v } else { v };
                }
            }
        }
        if ct.has(TransformCopy::VISIBLE) {
            to.visible = from.visible;
        }
        if ct.has(TransformCopy::COLOR) {
            // to.setColor(from.getRGB()): the source's base colour (-1, white, for a textured cube
            // or a bone); only a recoloured / colour-only target takes it.
            if model.cubes[ti - BONE_COUNT].color.is_some() {
                to.tint = if from_root {
                    0xFF_FFFF
                } else {
                    model.cubes[fi - BONE_COUNT].color.unwrap_or(0xFF_FFFF) & 0xFF_FFFF
                };
            }
        }
    }

    fn cube(&self, idx: usize) -> &ElemState {
        &self.0[BONE_COUNT + idx]
    }
}

/// Poses `model` with the animation `frame` and the vanilla motion of `input`.
pub fn pose_model(model: &Model, frame: &Frame, input: &PoseInput) -> Vec<PosedBox> {
    pose_model_with_items(model, frame, input).boxes
}

/// What the walk of the model's parts collects: boxes (while `draw`) and the item parts met.
struct Sink {
    boxes: Vec<PosedBox>,
    /// Slot, cube id and [`ItemAnchor::transform`] of each item part reached, in walk order
    /// (`storeTransform`); `None` for one not drawn (it or a cube above it hidden).
    items: Vec<(ItemPart, u32, Option<Mat4>)>,
    /// Boxes are kept; false walks a hidden head for its item parts only (they still place).
    draw: bool,
    /// No cube above is hidden (`render`'s `doRenderElems`): a hidden cube's children are still
    /// walked, for their item parts, but draw nothing.
    shown: bool,
}

impl Sink {
    fn new() -> Sink {
        Sink { boxes: Vec::new(), items: Vec::new(), draw: true, shown: true }
    }
}

/// [`ItemAnchor::transform`] is in blocks: the pixel transforms here end in a 1/16 scale.
const PIXELS_PER_BLOCK: f32 = 16.0;

/// `PlayerModel.translateToHand`: a slim arm's hand is half a pixel further out.
const SLIM_HAND_SHIFT: f32 = 0.5;

/// Poses `model` as [`pose_model`] does, with the held and head item anchors.
///
/// Hide-skull (`registerHead`'s render predicate): with the model's `hideHeadIfSkull` and a skull
/// worn ([`crate::Equipment::skull`]), the head bone's boxes (its vanilla part, hat and cubes) are not
/// drawn; its item parts still place the head item.
pub fn pose_model_with_items(model: &Model, frame: &Frame, input: &PoseInput) -> Posed {
    let base = root_transform(input);
    let render = render_transform(model);
    let root = base * render;
    let limbs = vanilla_limbs(input);
    let none = PartDelta::default();
    let roots = std::array::from_fn(|b| {
        let bone = &model.bones[b];
        let values = part_values(b, model.skin_type);
        // RootModelElement.setPosAndRot: vanilla pose + part position (posN / rotN).
        let pos_n = Vec3::from_array(bone.pivot) - Vec3::from_array(values.pos);
        let rot_n = deg(bone.rotation);
        let (def_pos, def_rot) = if model.vanilla_anim_disabled[b] {
            (Vec3::from_array(bone.pivot), rot_n)
        } else {
            (limbs[b].pos + pos_n, limbs[b].rot + rot_n)
        };
        let d = frame.parts.get(&(b as u32)).unwrap_or(&none);
        (delta_pos(def_pos, d), delta_rot(def_rot, d))
    });
    let vanilla: [(Vec3, Vec3); BONE_COUNT] = limbs.map(|l| (l.pos, l.rot));
    let bases = extra_bases(model, &roots, &vanilla, input);
    let states = States::new(model, frame, roots, &bases);
    let mut out = Sink::new();
    let hide_head = model.hide_head_if_skull && input.equipment.skull;
    for b in 0..BONE_COUNT {
        let s = &states.0[b];
        let m = root * translate_rotate(s.pos, s.rot);
        out.draw = !(b == HEAD && hide_head);
        bone_boxes(model, &states, b, m, &mut out);
        dup_root_boxes(model, &states, b, root, &mut out);
    }
    out.draw = true;
    // The armour and elytra holders set up after the player model has drawn: each armour part
    // copies the player part's final pose (after the copy transforms, `copyPropertiesTo`), then
    // its `setupTransform` applies the copies again. Only a copy can make the two differ.
    let posted: [(Vec3, Vec3); BONE_COUNT] = std::array::from_fn(|b| (states.0[b].pos, states.0[b].rot));
    let worn_states = if model.copies.is_empty() || model.extra_roots.is_empty() {
        None
    } else {
        Some(States::new(model, frame, posted, &extra_bases(model, &posted, &vanilla, input)))
    };
    worn_boxes(model, &states, worn_states.as_ref().unwrap_or(&states), input, base, render, &mut out);
    // After the armour and elytra: their roots' item parts store too, while drawn.
    // An item part's anchor is under the render transform (CPM's own stack); the vanilla fallback
    // is not (`PlayerRenderManager.translateAndRotate`: the plain vanilla part).
    let items = item_anchors(model, &states, base, &out.items, input.left_handed);
    let shoulders = shoulder_anchors(base, &out.items, input.sneaking);
    Posed { boxes: out.boxes, items, shoulders }
}

/// `ParrotOnShoulderLayer.submitOnShoulder`: the parrot ±0.4 blocks to the side (+ the left),
/// 1.5 up (1.3 crouching; Y is down in model space) from the model root.
const SHOULDER_X: f32 = 0.4;
const SHOULDER_Y: f32 = -1.5;
const SHOULDER_Y_CROUCHING: f32 = -1.3;
/// `ParrotOnShoulderLayerMixin`: crouching, a shoulder part's parrot is lifted a further 0.2.
const SHOULDER_PART_CROUCH_Y: f32 = -0.2;

/// The shoulder parrot anchors (`ParrotOnShoulderLayerMixin`): for each shoulder, the layer's
/// translate on the model root (`base` in blocks, without the render transform: the vanilla
/// layer's pose stack), then the last drawn shoulder part's transform (`getTransform`, relative
/// to the model root, so the stored matrix is taken back out of `base`) and the crouch lift; none
/// when parts of that shoulder were reached but none drawn (`NO_RENDER`); the translate alone
/// when the model has no part for it (`getTransform` is null).
fn shoulder_anchors(base: Mat4, parts: &[(ItemPart, u32, Option<Mat4>)], crouching: bool) -> Vec<ShoulderAnchor> {
    let root = base * Mat4::from_scale(Vec3::splat(PIXELS_PER_BLOCK));
    let to_root = root.inverse();
    let y = if crouching { SHOULDER_Y_CROUCHING } else { SHOULDER_Y };
    let mut out = Vec::new();
    for (part, left) in [(ItemPart::LeftShoulder, true), (ItemPart::RightShoulder, false)] {
        let layer = root * Mat4::from_translation(Vec3::new(if left { SHOULDER_X } else { -SHOULDER_X }, y, 0.0));
        if parts.iter().any(|(p, _, _)| *p == part) {
            let drawn = parts.iter().rev().find_map(|&(p, id, t)| t.filter(|_| p == part).map(|t| (id, t)));
            if let Some((id, t)) = drawn {
                let lift = if crouching { Vec3::new(0.0, SHOULDER_PART_CROUCH_Y, 0.0) } else { Vec3::ZERO };
                let transform = layer * to_root * t * Mat4::from_translation(lift);
                out.push(ShoulderAnchor { left, part: Some(id), transform });
            }
            continue;
        }
        out.push(ShoulderAnchor { left, part: None, transform: layer });
    }
    out
}

/// The item anchors: for each of the right hand, left hand and head, the last drawn item part of
/// that slot reached this frame (`ModelDefinition.storeTransform` keeps the last matrix stored);
/// none when parts of the slot were reached but none drawn (`NO_RENDER`); else, no part of the
/// slot reached (none, or only under an armour root not worn or below another item part:
/// `getTransform` is null), the bone's transform (`translateToHand` / `translateToHead`) under
/// `base`, without the model's render transform (the vanilla part's). The right arm holds the
/// main hand unless `left_handed`.
fn item_anchors(
    model: &Model,
    states: &States,
    base: Mat4,
    parts: &[(ItemPart, u32, Option<Mat4>)],
    left_handed: bool,
) -> Vec<ItemAnchor> {
    let to_blocks = Mat4::from_scale(Vec3::splat(PIXELS_PER_BLOCK));
    let slim = model.skin_type == SkinType::Slim;
    let (right, left) = if left_handed {
        (ItemSlot::OffHand, ItemSlot::MainHand)
    } else {
        (ItemSlot::MainHand, ItemSlot::OffHand)
    };
    let slots = [
        (ItemPart::RightHand, RIGHT_ARM, false, right),
        (ItemPart::LeftHand, LEFT_ARM, true, left),
        (ItemPart::Head, HEAD, false, ItemSlot::Head),
    ];
    let mut out = Vec::new();
    for (part, bone, left, slot) in slots {
        if parts.iter().any(|(p, _, _)| *p == part) {
            let drawn = parts.iter().rev().find_map(|&(p, id, t)| t.filter(|_| p == part).map(|t| (id, t)));
            if let Some((id, transform)) = drawn {
                out.push(ItemAnchor { slot, left, part: Some(id), transform });
            }
            continue;
        }
        let s = &states.0[bone];
        let shift = match (bone, slim) {
            (RIGHT_ARM, true) => SLIM_HAND_SHIFT,
            (LEFT_ARM, true) => -SLIM_HAND_SHIFT,
            _ => 0.0,
        };
        let transform = base * translate_rotate(s.pos + Vec3::new(shift, 0.0, 0.0), s.rot) * to_blocks;
        out.push(ItemAnchor { slot, left, part: None, transform });
    }
    out
}

/// `RootModelElement.getPos`: the default position plus the frame's position (additive), or the
/// frame's alone.
fn delta_pos(def: Vec3, d: &PartDelta) -> Vec3 {
    match d.pos {
        None => def,
        Some(p) if d.pos_additive => def + Vec3::from_array(p),
        Some(p) => Vec3::from_array(p),
    }
}

/// `RootModelElement.getRot`, as [`delta_pos`].
fn delta_rot(def: Vec3, d: &PartDelta) -> Vec3 {
    match d.rot {
        None => def,
        Some(r) if d.rot_additive => def + deg(r),
        Some(r) => deg(r),
    }
}

// ---------------------------------------------------------------------------------------------
// Armour and elytra

/// The texture a root type's boxes sample.
fn root_texture(t: RootType) -> Option<BoxTexture> {
    use RootType::*;
    Some(match t {
        Cape => BoxTexture::Cape,
        ElytraLeft | ElytraRight => BoxTexture::Elytra,
        ArmorHelmet => BoxTexture::Armor(ArmorSlot::Helmet),
        ArmorBody | ArmorLeftArm | ArmorRightArm => BoxTexture::Armor(ArmorSlot::Chestplate),
        ArmorLeggingsBody | ArmorLeftLeg | ArmorRightLeg => BoxTexture::Armor(ArmorSlot::Leggings),
        ArmorLeftFoot | ArmorRightFoot => BoxTexture::Armor(ArmorSlot::Boots),
    })
}

/// Whether `t`'s layer is drawn (`HumanoidArmorLayer` per slot, `WingsLayer`, `CapeLayer`).
fn worn(t: RootType, i: &PoseInput) -> bool {
    let e = i.equipment;
    match root_texture(t) {
        None => false,
        // CapeLayer and CapeLayerMixin: not under an elytra's wings. (Vanilla's invisible check is
        // in `worn_boxes`: CapeLayerMixin draws a cape root before it.)
        Some(BoxTexture::Cape | BoxTexture::PlayerCape) => i.cape.is_some() && !e.chest_has_wings,
        Some(BoxTexture::Elytra) => e.elytra,
        Some(BoxTexture::Armor(ArmorSlot::Helmet)) => e.head,
        Some(BoxTexture::Armor(ArmorSlot::Chestplate)) => e.chest,
        Some(BoxTexture::Armor(ArmorSlot::Leggings)) => e.legs,
        Some(BoxTexture::Armor(ArmorSlot::Boots)) => e.feet,
        Some(BoxTexture::Skin) => false,
    }
}

/// The armour and elytra sheets' default size (`TextureSheetType.ARMOR1` / `ARMOR2` / `ELYTRA`).
const EQUIPMENT_SHEET: [u32; 2] = [64, 32];

/// `WingsLayer`'s `translate(0, 0, 0.125)` before the elytra model: 2 model pixels.
const ELYTRA_Z: f32 = 2.0;

/// `CapeLayer`'s `translate(0, -0.053125, 0.06875)` over a chestplate, in model pixels.
const CAPE_OVER_CHESTPLATE: Vec3 = Vec3::new(0.0, -0.85, 1.1);

/// `PlayerCapeModel`'s cape part at rest (`PartPose.offsetAndRotation(0, 0, 2, 0, PI, 0)`), on
/// the body.
const CAPE_POS: Vec3 = Vec3::new(0.0, 0.0, 2.0);

/// The cape's pose (position in pixels, `ZYX` Euler rotation in radians) on the player root:
/// `PlayerCapeModel.setupAnim` turns the cape part by `rotateBy` with the flap and leans (as
/// Horizon's `mobs::render::cape`), and `CapeTransformUtil.combine` folds the body's pose
/// `body` into it, as CPM's cape root takes it (`setPosAndRot`). The body is the cape model's
/// own, vanilla one (`HumanoidModel.setupAnim`: [`vanilla_limbs`]), never the CPM body bone:
/// `RedirectHolderCape` redirects only the cape.
fn cape_pose(body: (Vec3, Vec3), c: CapeState) -> (Vec3, Vec3) {
    use glam::{EulerRot, Quat};
    use std::f32::consts::PI;
    let by = Quat::from_rotation_y(-PI)
        * Quat::from_rotation_x((6.0 + c.lean / 2.0 + c.flap).to_radians())
        * Quat::from_rotation_z((c.lean2 / 2.0).to_radians())
        * Quat::from_rotation_y((180.0 - c.lean2 / 2.0).to_radians());
    let cape = Quat::from_rotation_y(PI) * by;
    let m = translate_rotate(body.0, body.1) * Mat4::from_rotation_translation(cape, CAPE_POS);
    let (_, rot, pos) = m.to_scale_rotation_translation();
    let (z, y, x) = rot.to_euler(EulerRot::ZYX);
    (pos, Vec3::new(x, y, z))
}

/// `ElytraModel.setupAnim` for wing `left` (else right): the left wing at (5, y, 0) turned by the
/// render state's elytra angles, the right one mirrored. Standing: 15° x, -15° z; crouching:
/// 40° x, -45° z, 5° y, 3 px down; fall flying (taken level, the angles' full lerp): 20° x, -90° z.
fn wing_pose(i: &PoseInput, left: bool) -> (Vec3, Vec3) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let (x, y, z) = if i.fall_flying {
        (PI / 9.0, 0.0, -FRAC_PI_2)
    } else if i.sneaking {
        (2.0 * PI / 9.0, 0.087_266_46, -PI / 4.0)
    } else {
        (PI / 12.0, 0.0, -PI / 12.0)
    };
    let py = if i.sneaking { 3.0 } else { 0.0 };
    if left {
        (Vec3::new(5.0, py, 0.0), Vec3::new(x, y, z))
    } else {
        (Vec3::new(-5.0, py, 0.0), Vec3::new(x, -y, -z))
    }
}

/// The pose of the vanilla part `t` stands in for, before any root's part position: an armour
/// part takes its player part's pose (`copyPropertiesTo` from the player model, `bones`), a wing
/// `ElytraModel`'s, the cape `PlayerCapeModel`'s on the vanilla body ([`cape_pose`]).
fn vanilla_root_pose(t: RootType, bones: &[(Vec3, Vec3); BONE_COUNT], i: &PoseInput) -> (Vec3, Vec3) {
    match t {
        RootType::Cape => {
            let body = &vanilla_limbs(i)[BODY];
            cape_pose((body.pos, body.rot), i.cape.unwrap_or_default())
        }
        RootType::ElytraLeft => wing_pose(i, true),
        RootType::ElytraRight => wing_pose(i, false),
        _ => t.copy_from().map_or((Vec3::ZERO, Vec3::ZERO), |b| bones[b]),
    }
}

/// [`Bases`] for every non-player root (`setRootPosAndRot` from the redirected part), then every
/// duplicated player root (its part's vanilla pose, `vanilla`, as the part's main root takes it).
fn extra_bases(
    model: &Model,
    bones: &[(Vec3, Vec3); BONE_COUNT],
    vanilla: &[(Vec3, Vec3); BONE_COUNT],
    i: &PoseInput,
) -> Vec<(Vec3, Vec3)> {
    let extra = model.extra_roots.iter().map(|r| vanilla_root_pose(r.kind, bones, i));
    extra.chain(model.dup_roots.iter().map(|r| vanilla[r.bone])).collect()
}

/// The UV space of a root's cubes in the model's own `sheet`: its declared size when it has a
/// usable image, else the sheet type's default 64x32 (the texture sampled then is the game's).
fn sheet_px(sheet: &Option<crate::format::RawTexture>) -> [u32; 2] {
    match sheet {
        Some(t) if !t.png.is_empty() && t.declared[0] > 0 && t.declared[1] > 0 => t.declared,
        _ => EQUIPMENT_SHEET,
    }
}

/// `t`'s vanilla box (`RootModelValues`, in the 64x32 equipment sheet).
fn root_vanilla_box(t: RootType, tex: BoxTexture, m: Mat4) -> PosedBox {
    let v = t.values();
    let faces = box_faces(v.offset, v.size, [1.0; 3], v.inflate, v.uv, if v.mirror { -1 } else { 1 });
    PosedBox {
        transform: m,
        faces,
        tint: 0xFF_FFFF,
        glow: false,
        textured: true,
        source: BoxSource::RootVanilla(t),
        texture: tex,
        texture_px: EQUIPMENT_SHEET,
    }
}

/// The worn armour and elytra (`HumanoidArmorLayer` / `WingsLayer` through CPM's
/// `RedirectRenderer.render`): for each root type of a worn slot, each of the model's roots of
/// that type draws the vanilla part unless hidden, then its cubes in that slot's texture; a type
/// the model has no root for draws the vanilla part at the redirected part's pose (an armour
/// part follows its player part's final pose).
///
/// `states` is the player model's (its bones' final poses); `roots` the armour holders' (the
/// non-player roots and their cubes, posed from those bones with the copies applied again).
///
/// `base` is the model space before the render transform `render`: the elytra and cape layers'
/// own translates come before it (the holder's `setupTransform` runs at each part). The vanilla
/// cape of a model with no cape root is vanilla's `CapeLayer`, outside CPM: no render transform,
/// and none on an invisible player.
#[allow(clippy::too_many_arguments)]
fn worn_boxes(
    model: &Model,
    states: &States,
    roots: &States,
    input: &PoseInput,
    base: Mat4,
    render: Mat4,
    out: &mut Sink,
) {
    let root = base * render;
    let bones: [(Vec3, Vec3); BONE_COUNT] = std::array::from_fn(|b| (states.0[b].pos, states.0[b].rot));
    let elytra_px = sheet_px(&model.elytra_texture);
    let cape_px = sheet_px(&model.cape_texture);
    for t in RootType::ALL {
        if !worn(t, input) {
            continue;
        }
        let Some(tex) = root_texture(t) else { continue };
        // A layer's own translate comes before the holder's setupTransform (the render transform).
        let (holder, cube_px) = match tex {
            BoxTexture::Elytra => (base * Mat4::from_translation(Vec3::new(0.0, 0.0, ELYTRA_Z)) * render, elytra_px),
            BoxTexture::Cape if input.equipment.chest_humanoid => {
                (base * Mat4::from_translation(CAPE_OVER_CHESTPLATE) * render, cape_px)
            }
            BoxTexture::Cape => (root, cape_px),
            _ => (root, EQUIPMENT_SHEET),
        };
        let mut typed = model.extra_roots.iter().filter(|r| r.kind == t).peekable();
        if typed.peek().is_none() {
            let (pos, rot) = vanilla_root_pose(t, &bones, input);
            if tex == BoxTexture::Cape {
                // No cape root: vanilla's CapeLayer (CapeLayerMixin lets it run), in the player's
                // own cape, on the vanilla body, unscaled, and not on an invisible player.
                if input.invisible {
                    continue;
                }
                let layer = if input.equipment.chest_humanoid {
                    base * Mat4::from_translation(CAPE_OVER_CHESTPLATE)
                } else {
                    base
                };
                out.boxes.push(root_vanilla_box(t, BoxTexture::PlayerCape, layer * translate_rotate(pos, rot)));
                continue;
            }
            out.boxes.push(root_vanilla_box(t, tex, holder * translate_rotate(pos, rot)));
            continue;
        }
        for r in typed {
            let s = roots.cube(r.node);
            let m = holder * translate_rotate(s.pos, s.rot);
            if s.visible {
                out.boxes.push(root_vanilla_box(t, tex, m));
            }
            extra_root_cubes(model, roots, r, m, tex, cube_px, out);
        }
    }
}

/// `doRender0` for a non-player root: its cubes under its transform `m`.
fn extra_root_cubes(
    model: &Model,
    states: &States,
    r: &ExtraRoot,
    m: Mat4,
    tex: BoxTexture,
    px: [u32; 2],
    out: &mut Sink,
) {
    for &c in &model.cubes[r.node].children {
        walk(model, states, c, m, Paint { texture: tex, texture_px: px }, out);
    }
}

/// The texture the boxes of a subtree sample.
#[derive(Debug, Clone, Copy)]
struct Paint {
    texture: BoxTexture,
    texture_px: [u32; 2],
}

/// Bone `b`'s boxes under its transform `m`: the kept vanilla part and its second layer, then
/// its cubes.
fn bone_boxes(model: &Model, states: &States, b: usize, m: Mat4, out: &mut Sink) {
    if model.keep_vanilla[b] && out.draw {
        vanilla_boxes(model, b, m, out);
    }
    let skin = Paint { texture: BoxTexture::Skin, texture_px: model.texture_size };
    for &c in &model.roots_by_bone[b] {
        walk(model, states, c, m, skin, out);
    }
}

/// The duplicated roots of bone `b` (`PartRoot.forEach` after the main root), each under `root`
/// and its own pose: the part's vanilla box and second layer unless hidden, then its cubes.
fn dup_root_boxes(model: &Model, states: &States, b: usize, root: Mat4, out: &mut Sink) {
    let skin = Paint { texture: BoxTexture::Skin, texture_px: model.texture_size };
    for r in model.dup_roots.iter().filter(|r| r.bone == b) {
        let s = states.cube(r.node);
        let m = root * translate_rotate(s.pos, s.rot);
        if s.visible && out.draw {
            vanilla_boxes(model, b, m, out);
        }
        for &c in &model.cubes[r.node].children {
            walk(model, states, c, m, skin, out);
        }
    }
}

/// Bone `b`'s vanilla part and its second layer (`PlayerPartValues`) under `m`.
fn vanilla_boxes(model: &Model, b: usize, m: Mat4, out: &mut Sink) {
    {
        let values = part_values(b, model.skin_type);
        let sheet = sheet_scale(model.texture_size);
        let layers = [
            (values.uv, 0.0, BoxSource::Vanilla(b)),
            (values.uv2, layer_inflate(b), BoxSource::VanillaLayer(b)),
        ];
        for (uv, inflate, source) in layers {
            out.boxes.push(PosedBox {
                transform: m,
                faces: scale_uvs(box_faces(values.offset, values.size, [1.0; 3], inflate, uv, 1), sheet),
                tint: 0xFF_FFFF,
                glow: false,
                textured: true,
                source,
                texture: BoxTexture::Skin,
                texture_px: model.texture_size,
            });
        }
    }
}

/// `AvatarRenderer.renderHand`'s z turn of the first-person arm: outwards, 0.1 radians.
const FIRST_PERSON_ARM_Z: f32 = 0.1;

/// The first-person arm (`left`, else right): the arm bone's kept vanilla part with its sleeve
/// and its cubes, posed as vanilla poses the arm it replaces (`renderHand`: the part's rest
/// position, here the bone's pivot with its part position, no rotation but z ±0.1). Static, as
/// CPM draws it: no walk, bob, gesture or animation; cubes hidden in the model are left out.
/// Copy transforms apply, reading the other bones at rest (pivot and part rotation). The arm's
/// first-person hand transform ([`Model::fp_hand`]) applies; the render transform does not.
///
/// Each [`PosedBox::transform`] maps box-local model pixels into the space Horizon's
/// `first_person_arm` pose `m` takes (`mobs::render_model`'s `base`): blocks, the arm's model
/// space (Y down, pivot not yet applied), so a box is drawn at `m * transform`.
pub fn pose_arm(model: &Model, left: bool) -> Vec<PosedBox> {
    let (bone, z) = if left { (LEFT_ARM, -FIRST_PERSON_ARM_Z) } else { (RIGHT_ARM, FIRST_PERSON_ARM_Z) };
    let roots = std::array::from_fn(|b| {
        let pivot = Vec3::from_array(model.bones[b].pivot);
        if b == bone { (pivot, Vec3::new(0.0, 0.0, z)) } else { (pivot, deg(model.bones[b].rotation)) }
    });
    // The vanilla parts at rest (the arm turned by z), as the duplicated roots take them.
    let vanilla = std::array::from_fn(|b| {
        let rot = if b == bone { Vec3::new(0.0, 0.0, z) } else { Vec3::ZERO };
        (Vec3::from_array(part_values(b, model.skin_type).pos), rot)
    });
    let bases = extra_bases(model, &roots, &vanilla, &PoseInput::default());
    let states = States::new(model, &Frame::default(), roots, &bases);
    let s = &states.0[bone];
    // setupTransform in AnimationMode.HAND: that hand's first-person transform, before the arm's
    // own translateRotate. The render transform is not applied (PLAYER mode only).
    let hand = model.fp_hand[if left { 0 } else { 1 }].as_ref().map_or(Mat4::IDENTITY, part_transform);
    // render_model: T(pivot / 16) · Rz · (v / 16), which is S(1/16) · T(pivot) · Rz · v.
    let base = Mat4::from_scale(Vec3::splat(1.0 / 16.0)) * hand;
    let m = base * translate_rotate(s.pos, s.rot);
    let mut out = Sink::new();
    bone_boxes(model, &states, bone, m, &mut out);
    dup_root_boxes(model, &states, bone, base, &mut out);
    out.boxes
}

/// `RedirectRenderer.render(RenderedCube, …)` for one cube and its children.
///
/// An item part (`cube.itemRenderer`) draws nothing and neither do its children: its transform
/// with its offset and mesh scale is stored for the item (`storeTransform`).
fn walk(model: &Model, states: &States, idx: usize, parent: Mat4, paint: Paint, out: &mut Sink) {
    let node = &model.cubes[idx];
    let s = states.cube(idx);
    let shown = out.shown && s.visible;
    let m = parent * translate_rotate(s.pos, s.rot) * Mat4::from_scale(Vec3::from_array(s.scale.map(|v| v.max(0.01))));
    if let Some(part) = node.item {
        let t = m
            * Mat4::from_translation(Vec3::from_array(node.offset))
            * Mat4::from_scale(Vec3::from_array(node.scale))
            * Mat4::from_scale(Vec3::splat(PIXELS_PER_BLOCK));
        out.items.push((part, node.id, shown.then_some(t)));
        return;
    }
    if !shown || !out.draw {
        // Hidden (or a skull-hidden head): walked for the item parts below only.
    } else if let Some((meshes, textured)) = node_meshes(node) {
        for faces in meshes {
            out.boxes.push(PosedBox {
                transform: m,
                faces,
                tint: s.tint,
                glow: node.glow,
                textured,
                source: BoxSource::Cube(node.id),
                texture: paint.texture,
                texture_px: paint.texture_px,
            });
        }
    }
    let above = out.shown;
    out.shown = shown;
    for &c in &node.children {
        walk(model, states, c, m, paint, out);
    }
    out.shown = above;
}

// ---------------------------------------------------------------------------------------------
// Box geometry (BoxRender)

/// Corners of each face (our face order) as `BoxRender` lists them, by index into
/// [`Corners`]: 0 (x,y,z) 1 (ex,y,z) 2 (ex,ey,z) 3 (x,ey,z) 4 (x,y,ez) 5 (ex,y,ez) 6 (ex,ey,ez) 7 (x,ey,ez).
const FACE_VERTS: [[usize; 4]; 6] = [
    [2, 3, 7, 6], // down  (POSITIVE_Y, CPM DOWN)
    [5, 4, 0, 1], // up    (NEGATIVE_Y, CPM UP)
    [1, 0, 3, 2], // north (NEGATIVE_Z)
    [4, 5, 6, 7], // south (POSITIVE_Z)
    [0, 4, 7, 3], // west  (NEGATIVE_X)
    [5, 1, 2, 6], // east  (POSITIVE_X)
];

/// Index into CPM's `Direction` order (UP, DOWN, NORTH, SOUTH, EAST, WEST) for each of our faces.
const CPM_DIRECTION: [usize; 6] = [1, 0, 2, 3, 5, 4];

struct Corners([[f32; 3]; 8]);

impl Corners {
    fn new(x: f32, y: f32, z: f32, ex: f32, ey: f32, ez: f32) -> Corners {
        Corners([
            [x, y, z],
            [ex, y, z],
            [ex, ey, z],
            [x, ey, z],
            [x, y, ez],
            [ex, y, ez],
            [ex, ey, ez],
            [x, ey, ez],
        ])
    }
    fn face(&self, f: usize) -> [[f32; 3]; 4] {
        FACE_VERTS[f].map(|i| self.0[i])
    }
}

/// `BoxRender.Quad(posIn, u1, v1, u2, v2, …, mirror)`: vertex 0..3 get (u2,v1), (u1,v1), (u1,v2),
/// (u2,v2); a mirrored quad reverses its vertices. Rotated into TL, BL, BR, TR order.
fn mc_quad(p: [[f32; 3]; 4], [u1, v1, u2, v2]: [f32; 4], mirror: bool) -> FaceQuad {
    if mirror {
        FaceQuad { corners: [p[0], p[3], p[2], p[1]], uv: [u2, v1, u1, v2] }
    } else {
        FaceQuad { corners: [p[1], p[2], p[3], p[0]], uv: [u1, v1, u2, v2] }
    }
}

/// `BoxRender.Quad(posIn, Face, …)`: vertex `i` takes UV slot `(i + rot) % 4` of
/// (ex,sy), (sx,sy), (sx,ey), (ex,ey).
fn per_face_quad(p: [[f32; 3]; 4], f: &FaceUv) -> FaceQuad {
    let r = f.rot as usize % 4;
    let at = |slot: usize| p[(slot + 4 - r) % 4];
    FaceQuad {
        corners: [at(1), at(2), at(3), at(0)],
        uv: [f.u0 as f32, f.v0 as f32, f.u1 as f32, f.v1 as f32],
    }
}

/// The Minecraft box layout from (u, v), sizes dx, dy, dz, in our face order.
fn box_rects(u: f32, v: f32, dx: f32, dy: f32, dz: f32) -> [[f32; 4]; 6] {
    let (f4, f5, f6, f7, f8, f9) = (u, u + dz, u + dz + dx, u + dz + dx + dx, u + dz + dx + dz, u + dz + dx + dz + dx);
    let (f10, f11, f12) = (v, v + dz, v + dz + dy);
    [
        [f6, f11, f7, f10],
        [f5, f10, f6, f11],
        [f5, f11, f6, f12],
        [f8, f11, f9, f12],
        [f4, f11, f5, f12],
        [f6, f11, f8, f12],
    ]
}

/// Inflated box bounds `(x, y, z, ex, ey, ez)`; x and ex swapped for a mirrored box.
fn bounds(offset: [f32; 3], size: [f32; 3], sc: [f32; 3], delta: f32, mirror: bool) -> [f32; 6] {
    let [x, y, z] = offset;
    let (mut x0, mut x1) = (x - delta, x + size[0] * sc[0] + delta);
    if mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    [x0, y - delta, z - delta, x1, y + size[1] * sc[1] + delta, z + size[2] * sc[2] + delta]
}

/// `BoxRender.createTextured(…, texU, texV, texSize, …)`.
fn box_faces(offset: [f32; 3], size: [f32; 3], sc: [f32; 3], delta: f32, uv: [u32; 2], tex: i32) -> [Option<FaceQuad>; 6] {
    let ts = tex.unsigned_abs() as f32;
    let mirror = tex < 0;
    let [x, y, z, ex, ey, ez] = bounds(offset, size, sc, delta, mirror);
    let c = Corners::new(x, y, z, ex, ey, ez);
    let [dx, dy, dz] = size.map(|s| (s * ts).ceil());
    let rects = box_rects(uv[0] as f32 * ts, uv[1] as f32 * ts, dx, dy, dz);
    std::array::from_fn(|f| Some(mc_quad(c.face(f), rects[f], mirror)))
}

/// `BoxRender.createTexturedSingle`: every face shows the same square from (u, v).
fn single_faces(offset: [f32; 3], size: [f32; 3], sc: [f32; 3], delta: f32, uv: [u32; 2], tex: i32) -> [Option<FaceQuad>; 6] {
    let ts = tex.unsigned_abs() as f32;
    let mirror = tex < 0;
    let [x, y, z, mut ex, mut ey, mut ez] = bounds(offset, size, sc, delta, mirror);
    let [dx, dy, dz] = size.map(|s| (s * ts).ceil());
    let (u, v) = (uv[0] as f32 * ts, uv[1] as f32 * ts);
    let mut faces = [None; 6];
    // A flat box gets only its two large faces, the second with u reversed.
    let flat = if ex == x {
        ex += 0.001;
        Some((FACE_WEST, FACE_EAST, dz, dy))
    } else if ey == y {
        ey += 0.001;
        Some((FACE_UP, FACE_DOWN, dx, dz))
    } else if ez == z {
        ez += 0.001;
        Some((FACE_NORTH, FACE_SOUTH, dx, dy))
    } else {
        None
    };
    let c = Corners::new(x, y, z, ex, ey, ez);
    if let Some((a, b, du, dv)) = flat {
        let (tu, tv) = (u + du, v + dv);
        faces[a] = Some(mc_quad(c.face(a), [u, v, tu, tv], mirror));
        faces[b] = Some(mc_quad(c.face(b), [tu, v, u, tv], mirror));
        return faces;
    }
    let s = dx.max(dy).max(dz);
    std::array::from_fn(|f| Some(mc_quad(c.face(f), [u, v, u + s, v + s], mirror)))
}

/// [`node_faces`], as one box's faces or, for an extruded cube (`createBox`: after `singleTex`,
/// before per-face UVs), its sliced quads in boxes of six (one transform, so drawn alike).
fn node_meshes(n: &Node) -> Option<(Vec<[Option<FaceQuad>; 6]>, bool)> {
    let tex = n.tex_scale as i8 as i32;
    if n.extrude && tex != 0 && !n.single_tex && !(n.size == [0.0; 3] && n.mc_scale == 0.0) {
        let quads = extruded_faces(n.offset, n.size, n.scale, n.mc_scale, n.uv, tex);
        let boxes = quads
            .chunks(6)
            .map(|c| std::array::from_fn(|i| c.get(i).copied()))
            .collect();
        return Some((boxes, true));
    }
    node_faces(n).map(|(faces, textured)| (vec![faces], textured))
}

/// A quad of `BoxRender`'s with per-vertex texture coordinates `t` (texture pixels) as a
/// [`FaceQuad`]: the vertex order turned (never reversed, so it faces the same way) until it runs
/// TL, BL, BR, TR over an axis-aligned UV rectangle.
fn uv_quad(p: [[f32; 3]; 4], t: [[f32; 2]; 4]) -> FaceQuad {
    for r in 0..4 {
        let at = |k: usize| (r + k) % 4;
        let (tl, bl, br, tr) = (t[at(0)], t[at(1)], t[at(2)], t[at(3)]);
        if bl[0] == tl[0] && bl[1] == br[1] && tr[0] == br[0] && tr[1] == tl[1] {
            return FaceQuad { corners: [p[at(0)], p[at(1)], p[at(2)], p[at(3)]], uv: [tl[0], tl[1], br[0], br[1]] };
        }
    }
    FaceQuad { corners: p, uv: [t[0][0], t[0][1], t[2][0], t[2][1]] }
}

/// `BoxRender.createTexturedExtruded` and its `ExtrudedMesh`: the cube's front texture
/// (`dx` by `dy` texels from the UV) on a front and a back face, and between them a slice per
/// texel column (each side) and per texel row (each side) sampling that one texel line, so the
/// texture's transparent texels leave holes and its opaque ones get sides: a 2D sprite made 3D.
fn extruded_faces(offset: [f32; 3], size: [f32; 3], sc: [f32; 3], delta: f32, uv: [u32; 2], tex: i32) -> Vec<FaceQuad> {
    let ts = tex.unsigned_abs() as f32;
    let dx = (size[0] * ts).ceil().max(0.0) as usize;
    let dy = (size[1] * ts).ceil().max(0.0) as usize;
    let [x, y, z] = offset.map(|v| v - delta);
    let (w, h, d) = (size[0] * sc[0] + delta * 2.0, size[1] * sc[1] + delta * 2.0, size[2] * sc[2] + delta * 2.0);
    let (min_u, min_v) = (uv[0] as f32 * ts, uv[1] as f32 * ts);
    let (max_u, max_v) = (min_u + dx as f32, min_v + dy as f32);
    // The unit mesh's placement: x and y run from the texture's far corner, z out of its back.
    let m = if tex < 0 {
        Mat4::from_translation(Vec3::new(x, y + h, z)) * Mat4::from_scale(Vec3::new(w, -h, -d))
    } else {
        Mat4::from_translation(Vec3::new(x + w, y + h, z + d)) * Mat4::from_scale(Vec3::new(-w, -h, d))
    };
    let pt = |a: f32, b: f32, c: f32| m.transform_point3(Vec3::new(a, b, c)).to_array();
    let mut q = Vec::with_capacity(2 + 2 * (dx + dy));
    let mut quad = |p: [[f32; 3]; 4], t: [[f32; 2]; 4]| q.push(uv_quad(p, t));
    quad(
        [pt(0.0, 0.0, 0.0), pt(1.0, 0.0, 0.0), pt(1.0, 1.0, 0.0), pt(0.0, 1.0, 0.0)],
        [[max_u, max_v], [min_u, max_v], [min_u, min_v], [max_u, min_v]],
    );
    quad(
        [pt(0.0, 1.0, -1.0), pt(1.0, 1.0, -1.0), pt(1.0, 0.0, -1.0), pt(0.0, 0.0, -1.0)],
        [[max_u, min_v], [min_u, min_v], [min_u, max_v], [max_u, max_v]],
    );
    // Half a texel in: each slice samples the middle of its texel line.
    let (f5, f6) = (0.5 * (max_u - min_u) / dx.max(1) as f32, 0.5 * (max_v - min_v) / dy.max(1) as f32);
    for k in 0..dx {
        let f7 = k as f32 / dx as f32;
        let f8 = max_u + (min_u - max_u) * f7 - f5;
        quad(
            [pt(f7, 0.0, -1.0), pt(f7, 0.0, 0.0), pt(f7, 1.0, 0.0), pt(f7, 1.0, -1.0)],
            [[f8, max_v], [f8, max_v], [f8, min_v], [f8, min_v]],
        );
    }
    for k in 0..dx {
        let f7 = k as f32 / dx as f32;
        let f8 = max_u + (min_u - max_u) * f7 - f5;
        let f9 = f7 + 1.0 / dx as f32;
        quad(
            [pt(f9, 1.0, -1.0), pt(f9, 1.0, 0.0), pt(f9, 0.0, 0.0), pt(f9, 0.0, -1.0)],
            [[f8, min_v], [f8, min_v], [f8, max_v], [f8, max_v]],
        );
    }
    for k in 0..dy {
        let f7 = k as f32 / dy as f32;
        let f8 = max_v + (min_v - max_v) * f7 - f6;
        let f9 = f7 + 1.0 / dy as f32;
        quad(
            [pt(0.0, f9, 0.0), pt(1.0, f9, 0.0), pt(1.0, f9, -1.0), pt(0.0, f9, -1.0)],
            [[max_u, f8], [min_u, f8], [min_u, f8], [max_u, f8]],
        );
    }
    for k in 0..dy {
        let f7 = k as f32 / dy as f32;
        let f8 = max_v + (min_v - max_v) * f7 - f6;
        quad(
            [pt(1.0, f7, 0.0), pt(0.0, f7, 0.0), pt(0.0, f7, -1.0), pt(1.0, f7, -1.0)],
            [[min_u, f8], [max_u, f8], [max_u, f8], [min_u, f8]],
        );
    }
    q
}

/// `ModelRenderManager.createBox`: `None` for an empty mesh, else the faces and whether the
/// box is textured.
fn node_faces(n: &Node) -> Option<([Option<FaceQuad>; 6], bool)> {
    if n.size == [0.0; 3] && n.mc_scale == 0.0 {
        return None;
    }
    let tex = n.tex_scale as i8 as i32;
    if tex == 0 {
        // createColored: the colour layout from (0, 0), sizes of the mesh-scaled box.
        let size = [0, 1, 2].map(|k| n.size[k] * n.scale[k]);
        let [x, y, z, ex, ey, ez] = bounds(n.offset, size, [1.0; 3], n.mc_scale, false);
        let c = Corners::new(x, y, z, ex, ey, ez);
        let [dx, dy, dz] = size.map(f32::ceil);
        let rects = box_rects(0.0, 0.0, dx, dy, dz);
        return Some((std::array::from_fn(|f| Some(mc_quad(c.face(f), rects[f], false))), false));
    }
    if n.single_tex {
        return Some((single_faces(n.offset, n.size, n.scale, n.mc_scale, n.uv, tex), true));
    }
    if let Some(per_face) = &n.face_uv {
        let [x, y, z, ex, ey, ez] = bounds(n.offset, n.size, n.scale, n.mc_scale, false);
        let c = Corners::new(x, y, z, ex, ey, ez);
        let faces = std::array::from_fn(|f| per_face[CPM_DIRECTION[f]].as_ref().map(|uv| per_face_quad(c.face(f), uv)));
        return Some((faces, true));
    }
    Some((box_faces(n.offset, n.size, n.scale, n.mc_scale, n.uv, tex), true))
}
