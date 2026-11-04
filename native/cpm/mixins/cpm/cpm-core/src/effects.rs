//! Render effects (`RENDER_EFFECT` blocks), ported from `CPM/effects/Effect*.load`.

use crate::format::RawEffect;
use crate::io::{Reader, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceUv {
    pub u0: i32,
    pub v0: i32,
    pub u1: i32,
    pub v1: i32,
    /// 0..=3, quarter turns.
    pub rot: u8,
    pub auto_uv: bool,
}

/// `EffectCopyTransform` / `model/CopyTransform`: after the animations, element `to` takes (or
/// adds) the chosen parts of element `from`'s current transform. Ids are element ids (bone
/// 0..6 or cube id).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformCopy {
    pub from: u32,
    pub to: u32,
    /// `CopyTransform` bits: 0..2 position x/y/z, 3..5 rotation x/y/z, 6..8 scale x/y/z,
    /// 9 visibility, 10 colour, 11 additive, 14 a multiplier follows.
    pub copy: u16,
    pub multiply: f32,
}

impl TransformCopy {
    pub const VISIBLE: u16 = 1 << 9;
    pub const COLOR: u16 = 1 << 10;
    pub const ADDITIVE: u16 = 1 << 11;
    pub const MULTIPLY: u16 = 1 << 14;

    /// Whether bit `bit` is set.
    pub fn has(&self, bit: u16) -> bool {
        self.copy & bit != 0
    }
    /// Position axis `k` (0..3) is copied.
    pub fn pos(&self, k: usize) -> bool {
        self.copy & (1 << k) != 0
    }
    /// Rotation axis `k` is copied.
    pub fn rot(&self, k: usize) -> bool {
        self.copy & (1 << (3 + k)) != 0
    }
    /// Scale axis `k` is copied.
    pub fn scale(&self, k: usize) -> bool {
        self.copy & (1 << (6 + k)) != 0
    }
}

/// A `PartPosition` render transform (`ScaleData`'s render scale, a first-person hand): position
/// in model pixels, rotation in degrees (applied Z · Y · X, as `Rotation.asQ`), per-axis scale as
/// stored (see [`PartTransform::effective_scale`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartTransform {
    pub pos: [f32; 3],
    pub rot: [f32; 3],
    pub scale: [f32; 3],
}

impl PartTransform {
    /// `ModelRenderManager.transform`: a scale component below 0.1 or above 10 is 1 (as is a
    /// non-finite one, which Java would let through).
    pub fn effective_scale(&self) -> [f32; 3] {
        self.scale.map(|s| if (0.1..=10.0).contains(&s) { s } else { 1.0 })
    }

    /// `ScalingElement.hasTransform`: a position or rotation, or a scale other than 0 / 1.
    pub fn has_transform(&self) -> bool {
        self.pos.iter().chain(&self.rot).any(|&v| v != 0.0) || self.scale.iter().any(|&s| s != 0.0 && s != 1.0)
    }

    /// `PartPosition.isChanged`: any value non-zero.
    pub fn is_changed(&self) -> bool {
        self.pos.iter().chain(&self.rot).chain(&self.scale).any(|&v| v != 0.0)
    }

    /// `PartPosition`'s default: all zero (the identity once the scale is clamped).
    pub const ZERO: PartTransform = PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [0.0; 3] };
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Glow { cube: u32 },
    Scale { cube: u32, mc_scale: f32, scale: [f32; 3] },
    Hide { cube: u32 },
    Color { cube: u32, rgb: u32 },
    SingleTex { cube: u32 },
    /// Faces in `Direction` order: up, down, north, south, east, west.
    PerFaceUv { cube: u32, faces: [Option<FaceUv>; 6] },
    UvOverflow { cube: u32 },
    /// `EffectModelScale` (`MODEL_SCALE`): the render transform of the model's `ScaleData`.
    ModelScale(PartTransform),
    /// `EffectScaling` (`SCALING`): a new `ScaleData` (its gameplay values are not read).
    Scaling,
    /// `EffectFirstPersonHandPos` (`FIRST_PERSON_HAND`): the left, then the right hand.
    FirstPersonHand([PartTransform; 2]),
    Extrude { cube: u32 },
    CopyTransform(TransformCopy),
    /// `EffectRenderItem` (`ITEM`): cube `cube` is an item part for `ItemSlot` ordinal `slot`
    /// (0 left hand, 1 right hand, 2 head, …), `slot_id` its index (0 in projects).
    Item { cube: u32, slot: u8, slot_id: u8 },
    /// `EffectHideSkull` (`HIDE_SKULL`): the model's `hideHeadIfSkull` (on unless this says off).
    HideSkull { hide: bool },
    Other(u8),
}

fn decode_one(e: &RawEffect) -> Result<Effect> {
    let mut r = Reader::new(&e.payload);
    Ok(match e.kind {
        0 => Effect::Glow { cube: r.varint()? },
        1 => {
            let cube = r.varint()?;
            let mc_scale = r.float2()?;
            let scale = r.vec6b()?;
            Effect::Scale { cube, mc_scale, scale }
        }
        2 => Effect::Hide { cube: r.varint()? },
        3 => {
            let cube = r.varint()?;
            let c = r.bytes(3)?;
            Effect::Color { cube, rgb: (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32 }
        }
        4 => Effect::SingleTex { cube: r.varint()? },
        5 => {
            let cube = r.varint()?;
            let mask = r.u8()?;
            let mut faces = [None; 6];
            for (i, f) in faces.iter_mut().enumerate() {
                if mask & (1 << i) != 0 {
                    let u0 = r.varint()? as i32;
                    let v0 = r.varint()? as i32;
                    let u1 = r.varint()? as i32;
                    let v1 = r.varint()? as i32;
                    let rot = r.u8()?;
                    *f = Some(FaceUv { u0, v0, u1, v1, rot: rot & 3, auto_uv: false });
                }
            }
            Effect::PerFaceUv { cube, faces }
        }
        6 => Effect::UvOverflow { cube: r.varint()? },
        7 => {
            // EffectRenderItem.load: id, readEnum (a byte), slot id (a byte).
            let cube = r.varint()?;
            let slot = r.u8()?;
            let slot_id = r.u8()?;
            Effect::Item { cube, slot, slot_id }
        }
        8 => Effect::HideSkull { hide: r.bool()? },
        10 => Effect::Extrude { cube: r.varint()? },
        12 => Effect::ModelScale(part_transform(&mut r)?),
        13 => Effect::Scaling,
        14 => {
            // EffectCopyTransform.load: copy is read as a varint and narrowed to a short.
            let from = r.varint()?;
            let to = r.varint()?;
            let copy = r.varint()? as u16;
            let multiply = if copy & TransformCopy::MULTIPLY != 0 {
                let b = r.bytes(4)?;
                f32::from_be_bytes([b[0], b[1], b[2], b[3]])
            } else {
                1.0
            };
            Effect::CopyTransform(TransformCopy { from, to, copy, multiply })
        }
        15 => {
            let left = part_transform(&mut r)?;
            Effect::FirstPersonHand([left, part_transform(&mut r)?])
        }
        k => Effect::Other(k),
    })
}

/// `readVec6b` position, `readAngle` rotation, `readVec6b` scale.
fn part_transform(r: &mut Reader) -> Result<PartTransform> {
    Ok(PartTransform { pos: r.vec6b()?, rot: r.angle()?, scale: r.vec6b()? })
}

/// Decodes effects; one that fails to parse becomes `Other(kind)` rather than failing the model.
pub fn decode_effects(raw: &[RawEffect]) -> Vec<Effect> {
    raw.iter().map(|e| decode_one(e).unwrap_or(Effect::Other(e.kind))).collect()
}
