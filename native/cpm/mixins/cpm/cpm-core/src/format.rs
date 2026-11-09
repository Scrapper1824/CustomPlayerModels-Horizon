//! `.cpmmodel` container and legacy (pre-V2) model definition decoding.

use crate::io::{CpmError, Reader, Result};

/// First byte of both the container and the model data.
pub const HEADER: u8 = 0x53;

mod part {
    pub const END: u8 = 0;
    pub const DEFINITION: u8 = 3;
    pub const DEFINITION_LINK: u8 = 4;
    pub const SKIN: u8 = 5;
    pub const SKIN_LINK: u8 = 6;
    pub const PLAYER_PARTPOS: u8 = 7;
    pub const RENDER_EFFECT: u8 = 8;
    pub const ANIMATION_DATA: u8 = 10;
    pub const SKIN_TYPE: u8 = 11;
    pub const PLAYER: u8 = 1;
    pub const SCALE: u8 = 16;
    pub const TEXTURE: u8 = 17;
    pub const CUBES: u8 = 21;
    pub const PACKAGE_LINK: u8 = 20;
    pub const MODEL_ROOT: u8 = 12;
    pub const DUP_ROOT: u8 = 14;
    pub const ROOT_INFO: u8 = 22;
    pub const ANIMATION_NEW: u8 = 23;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinType {
    Default,
    Slim,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCube {
    pub id: u32,
    pub parent: u32,
    pub size: [f32; 3],
    pub pos: [f32; 3],
    pub offset: [f32; 3],
    /// Degrees.
    pub rotation: [f32; 3],
    pub tex_size: u8,
    pub u: u32,
    pub v: u32,
    pub rgb: u32,
    pub mesh_scale: [f32; 3],
    pub scale: [f32; 3],
    pub mc_scale: f32,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawTexture {
    pub declared: [u32; 2],
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartPos {
    pub part: u32,
    pub pos: [f32; 3],
    /// Degrees.
    pub rot: [f32; 3],
}

/// A `ROOT_INFO` that creates a non-player root (`ROOT_MODEL | ROOT_CREATE`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawRoot {
    /// `RootModelType` ordinal.
    pub root: u8,
    /// Id of the fake cube (a `CUSTOM_PART` child) the root is made from.
    pub create_from: u32,
    pub hidden: bool,
    pub disable_vanilla: bool,
}

/// A duplicated player root: `ROOT_INFO` with `ROOT_CREATE` but not `ROOT_MODEL`, or the legacy
/// `DUP_ROOT` part (`ModelPartDupRoot`). Both `def.addRoot(createFrom, PlayerModelParts)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawDupRoot {
    /// `PlayerModelParts` ordinal (the bone).
    pub part: u8,
    /// Id of the fake cube (a `CUSTOM_PART` child) the root is made from.
    pub create_from: u32,
    /// `ROOT_HIDDEN` (`ROOT_INFO`).
    pub hidden: bool,
    /// `ROOT_DISABLE_VANILLA` (`ROOT_INFO`).
    pub disable_vanilla: bool,
    /// Legacy `DUP_ROOT`: hidden as its fake cube is once the effects ran (`setHidden(rc.isHidden())`).
    pub hidden_from_cube: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawEffect {
    pub kind: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawModel {
    pub skin_type: SkinType,
    pub cubes: Vec<RawCube>,
    /// Vanilla parts still drawn; all true unless a PLAYER part or ROOT_INFO says otherwise.
    pub keep_vanilla: [bool; 6],
    /// Vanilla part animation disabled (ROOT_INFO `ROOT_DISABLE_VANILLA`).
    pub vanilla_anim_disabled: [bool; 6],
    pub texture: Option<RawTexture>,
    /// `TEXTURE` of sheet type `ELYTRA`.
    pub elytra_texture: Option<RawTexture>,
    /// `TEXTURE` of sheet type `CAPE`.
    pub cape_texture: Option<RawTexture>,
    /// Non-player roots (`ROOT_INFO` with `ROOT_MODEL | ROOT_CREATE`), in file order.
    pub extra_roots: Vec<RawRoot>,
    /// Duplicated player roots, in file order.
    pub dup_roots: Vec<RawDupRoot>,
    pub part_pos: Vec<PartPos>,
    pub effects: Vec<RawEffect>,
    pub legacy_anim: Option<Vec<u8>>,
    pub new_anim: Option<Vec<u8>>,
    pub scale: Option<f32>,
    /// The first `DEFINITION_LINK` / `SKIN_LINK` / `PACKAGE_LINK` payload (`loader:path`) left
    /// unresolved (no local copy, no resolver): part of the model's data lives there.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelFile {
    pub name: String,
    pub desc: String,
    pub data: RawModel,
    pub icon_png: Vec<u8>,
}

/// Wrapping u16 sum of bytes, as `ChecksumInputStream` computes it.
fn sum(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |s, &b| s.wrapping_add(b as u16))
}

/// Reads the big-endian 2-byte checksum at `r` and compares with the sum of `covered`.
fn check(r: &mut Reader, covered: &[u8]) -> Result<()> {
    let b = r.bytes(2)?;
    let expected = u16::from_be_bytes([b[0], b[1]]);
    let actual = sum(covered);
    if expected != actual {
        return Err(CpmError::Checksum { expected, actual });
    }
    Ok(())
}

/// Decodes a whole `.cpmmodel` file. A link part it cannot resolve from the file itself is left
/// in [`RawModel::link`].
pub fn read_cpmmodel(bytes: &[u8]) -> Result<ModelFile> {
    read_cpmmodel_with(bytes, None)
}

/// What a remote link resolves to: `resolve(link)` gives the link's target bytes (`0x53`, the
/// linked part's payload, its checksum: the downloaded text, base64-decoded).
pub type Resolve<'a> = &'a dyn Fn(&str) -> Result<Vec<u8>>;

/// [`read_cpmmodel`], resolving every link part that the file itself does not hold through
/// `resolve` (`ModelPartLink.resolve`), in place.
pub fn read_cpmmodel_resolving(bytes: &[u8], resolve: Resolve) -> Result<ModelFile> {
    read_cpmmodel_with(bytes, Some(resolve))
}

fn read_cpmmodel_with(bytes: &[u8], resolve: Option<Resolve>) -> Result<ModelFile> {
    let mut r = Reader::new(bytes);
    if r.u8()? != HEADER {
        return Err(CpmError::BadHeader);
    }
    let name = r.utf()?;
    let desc = r.utf()?;
    let data_block = r.block()?;
    // `ModelFile.load`: a model too big for its data block keeps its data in this overflow
    // block, followed by the link (`Link.write`: 1-byte length, then `loader:path`) that the
    // data block's link part points at; `registerLocalCache` resolves that link to it.
    let overflow = block_bytes(r.block()?);
    let local = if overflow.is_empty() {
        None
    } else {
        let n = r.u8()? as usize;
        let link = String::from_utf8(r.bytes(n)?.to_vec())
            .map_err(|e| CpmError::Invalid(format!("link utf8: {e}")))?;
        Some(Local { link, data: overflow })
    };
    let icon_png = block_bytes(r.block()?).to_vec();
    let covered = &bytes[1..r.pos()];
    check(&mut r, covered)?;
    let ctx = Ctx { local: local.as_ref(), resolve, in_package: false };
    let data = read_definition_with(block_bytes(data_block), ctx)?;
    Ok(ModelFile { name, desc, data, icon_png })
}

fn block_bytes(mut b: Reader<'_>) -> &[u8] {
    let n = b.remaining();
    b.bytes(n).unwrap_or(&[])
}

/// Decodes model data starting at the `0x53` header, verifying its trailing checksum.
pub fn read_definition(data: &[u8]) -> Result<RawModel> {
    read_definition_with(data, Ctx { local: None, resolve: None, in_package: false })
}

/// A link resolved from the file itself (`ModelDefinitionLoader.putLocalResource`).
struct Local<'a> {
    link: String,
    data: &'a [u8],
}

/// How link parts are resolved while reading.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    /// A link part naming this link reads from its data.
    local: Option<&'a Local<'a>>,
    /// Other links resolve through this, else stay in [`RawModel::link`].
    resolve: Option<Resolve<'a>>,
    /// Inside a linked package: link parts are refused (`Pack`).
    in_package: bool,
}

/// [`read_definition`], with link parts resolved per `ctx`.
fn read_definition_with(data: &[u8], ctx: Ctx) -> Result<RawModel> {
    let mut r = Reader::new(data);
    if r.u8()? != HEADER {
        return Err(CpmError::BadHeader);
    }
    let mut m = RawModel {
        skin_type: SkinType::Unknown,
        cubes: Vec::new(),
        keep_vanilla: [true; 6],
        vanilla_anim_disabled: [false; 6],
        texture: None,
        elytra_texture: None,
        cape_texture: None,
        extra_roots: Vec::new(),
        dup_roots: Vec::new(),
        part_pos: Vec::new(),
        effects: Vec::new(),
        legacy_anim: None,
        new_anim: None,
        scale: None,
        link: None,
    };
    read_parts(&mut r, &mut m, 0, ctx)?;
    let covered = &data[1..r.pos()];
    check(&mut r, covered)?;
    Ok(m)
}

/// `ModelPartDefinition`: the cube count, the cubes, then its own parts until `END`.
fn read_definition_body(b: &mut Reader, m: &mut RawModel, depth: u32, ctx: Ctx) -> Result<()> {
    let count = b.varint()?;
    let first = m.cubes.len() as u32;
    for i in 0..count {
        let mut c = read_cube(b)?;
        c.id = 10 + first + i;
        m.cubes.push(c);
    }
    read_parts(b, m, depth + 1, ctx)
}

/// `TextureProvider`: the declared size, then the PNG block.
fn read_texture(b: &mut Reader) -> Result<RawTexture> {
    let w = b.i16()? as u16 as u32;
    let h = b.i16()? as u16 as u32;
    let png = block_bytes(b.block()?).to_vec();
    Ok(RawTexture { declared: [w, h], png })
}

/// Reads part blocks until `END`.
fn read_parts(r: &mut Reader, m: &mut RawModel, depth: u32, ctx: Ctx) -> Result<()> {
    loop {
        let ty = r.u8()?;
        let mut b = r.block()?;
        match ty {
            part::END => return Ok(()),
            part::DEFINITION => {
                // (`Pack` refuses a definition in a linked package; this reader accepts one, as
                // the legacy packages its own tests and `gist_text` of a legacy model hold.)
                if depth > 0 {
                    return Err(CpmError::Invalid("nested definition".into()));
                }
                read_definition_body(&mut b, m, depth, ctx)?;
            }
            part::SKIN_TYPE => {
                m.skin_type = match b.u8()? {
                    0 => SkinType::Slim,
                    1 => SkinType::Default,
                    _ => SkinType::Unknown,
                };
            }
            part::PLAYER => {
                let k = b.u8()?;
                for (i, v) in m.keep_vanilla.iter_mut().enumerate() {
                    *v = k & (1 << i) != 0;
                }
            }
            part::SKIN => m.texture = Some(read_texture(&mut b)?),
            part::PLAYER_PARTPOS => {
                m.part_pos.push(PartPos { part: b.varint()?, pos: b.vec6b()?, rot: b.angle()? });
            }
            part::RENDER_EFFECT => {
                let kind = b.u8()?;
                m.effects.push(RawEffect { kind, payload: block_bytes(b).to_vec() });
            }
            part::CUBES => {
                // ModelPartCubes: ids are `i + 10` within this part.
                let count = b.varint()?;
                for i in 0..count {
                    let mut c = read_cube_v2(&mut b)?;
                    c.id = 10 + i;
                    m.cubes.push(c);
                }
            }
            part::TEXTURE => {
                // TextureSheetType ordinal 0 = SKIN, 2 = CAPE, 3 = ELYTRA; other sheets are not
                // rendered.
                let sheet = b.u8()?;
                if sheet == 0 || sheet == SHEET_CAPE || sheet == SHEET_ELYTRA {
                    let w = b.i16()? as u16 as u32;
                    let h = b.i16()? as u16 as u32;
                    let png = block_bytes(b.block()?).to_vec();
                    let tex = Some(RawTexture { declared: [w, h], png });
                    match sheet {
                        0 => m.texture = tex,
                        SHEET_CAPE => m.cape_texture = tex,
                        _ => m.elytra_texture = tex,
                    }
                }
            }
            part::ROOT_INFO => {
                let flags = b.u8()?;
                let root = b.u8()?;
                let (mut pos, mut rot) = ([0.0; 3], [0.0; 3]);
                if flags & ROOT_TRANSFORM != 0 {
                    pos = b.var_vec3()?;
                    rot = b.angle()?;
                }
                // ModelPartRootInfo: a ROOT_MODEL root (RootModelType) is created from a fake
                // cube (ROOT_CREATE, the exporter's only form for it); ROOT_CREATE alone makes a
                // duplicated root of the player part `root`.
                if flags & ROOT_MODEL == 0 && flags & ROOT_CREATE != 0 {
                    let create_from = b.varint()?;
                    m.dup_roots.push(RawDupRoot {
                        part: root,
                        create_from,
                        hidden: flags & ROOT_HIDDEN != 0,
                        disable_vanilla: flags & ROOT_DISABLE_VANILLA != 0,
                        hidden_from_cube: false,
                    });
                } else if flags & ROOT_MODEL != 0 {
                    if flags & ROOT_CREATE != 0 {
                        let create_from = b.varint()?;
                        m.extra_roots.push(RawRoot {
                            root,
                            create_from,
                            hidden: flags & ROOT_HIDDEN != 0,
                            disable_vanilla: flags & ROOT_DISABLE_VANILLA != 0,
                        });
                    }
                } else if flags & ROOT_CREATE == 0 {
                    m.part_pos.push(PartPos { part: root as u32, pos, rot });
                    if let Some(i) = Some(root as usize).filter(|&i| i < 6) {
                        if flags & ROOT_HIDDEN != 0 {
                            m.keep_vanilla[i] = false;
                        }
                        if flags & ROOT_DISABLE_VANILLA != 0 {
                            m.vanilla_anim_disabled[i] = true;
                        }
                    }
                }
            }
            part::MODEL_ROOT => {
                // Legacy `ModelPartRoot`: the `CUSTOM_PART` child cube `id` becomes a root of
                // `RootModelType` `root`, hidden when that cube is (`setHidden(rc.isHidden())`).
                let id = b.varint()?;
                let root = b.u8()?;
                let hidden = m.cubes.iter().find(|c| c.id == id).is_some_and(|c| c.hidden);
                m.extra_roots.push(RawRoot { root, create_from: id, hidden, disable_vanilla: false });
            }
            part::DUP_ROOT => {
                // Legacy `ModelPartDupRoot`: the `CUSTOM_PART` child cube `id` becomes a second
                // root of player part `part`, hidden when that cube is.
                let create_from = b.varint()?;
                let part = b.u8()?;
                m.dup_roots.push(RawDupRoot {
                    part,
                    create_from,
                    hidden: false,
                    disable_vanilla: false,
                    hidden_from_cube: true,
                });
            }
            part::ANIMATION_DATA => m.legacy_anim = Some(block_bytes(b).to_vec()),
            part::ANIMATION_NEW => m.new_anim = Some(block_bytes(b).to_vec()),
            part::DEFINITION_LINK | part::SKIN_LINK | part::PACKAGE_LINK => {
                // `Link`: one unsigned length byte, then "loader:path".
                let len = b.u8()? as usize;
                let raw = b.bytes(len)?;
                let s = String::from_utf8(raw.to_vec())
                    .map_err(|e| CpmError::Invalid(format!("link utf8: {e}")))?;
                if !s.contains(':') {
                    return Err(CpmError::Invalid("link without loader".into()));
                }
                if depth > 0 && ty != part::PACKAGE_LINK {
                    // `ModelPartDefinition`: no definition or skin link among its own parts,
                    // refused before anything is fetched.
                    return Err(CpmError::Invalid("invalid tag in definition".into()));
                }
                if ctx.in_package {
                    // `Pack`: a linked package holds no further links.
                    return Err(CpmError::Invalid("invalid tag in package".into()));
                }
                let fetched;
                let target: &[u8] = match (ctx.local.filter(|l| l.link == s), ctx.resolve) {
                    (Some(l), _) => l.data,
                    (None, Some(resolve)) => {
                        fetched = resolve(&s)?;
                        &fetched
                    }
                    (None, None) => {
                        m.link.get_or_insert(s);
                        continue;
                    }
                };
                // `ModelPartLink.resolve`: the target is `0x53`, the linked part's own payload,
                // then a checksum.
                let mut t = Reader::new(target);
                if t.u8()? != HEADER {
                    return Err(CpmError::BadHeader);
                }
                match ty {
                    part::DEFINITION_LINK => read_definition_body(&mut t, m, depth, ctx)?,
                    // `ModelPartSkin`: the skin texture.
                    part::SKIN_LINK => m.texture = Some(read_texture(&mut t)?),
                    _ => {
                        // A package: every part but the skin type (the outer one wins), and no
                        // further links.
                        let skin_type = m.skin_type;
                        read_parts(&mut t, m, depth, Ctx { in_package: true, ..ctx })?;
                        m.skin_type = skin_type;
                    }
                }
                let covered = &target[1..t.pos()];
                check(&mut t, covered)?;
            }
            part::SCALE => m.scale = Some(b.i8()? as f32 / 10.0),
            _ => {} // skipped by block length
        }
    }
}

/// `TextureSheetType.CAPE.ordinal()`.
pub const SHEET_CAPE: u8 = 2;

/// `TextureSheetType.ELYTRA.ordinal()`.
pub const SHEET_ELYTRA: u8 = 3;

const ROOT_HIDDEN: u8 = 1 << 0;
const ROOT_DISABLE_VANILLA: u8 = 1 << 4;
const ROOT_CREATE: u8 = 1 << 1;
const ROOT_MODEL: u8 = 1 << 2;
const ROOT_TRANSFORM: u8 = 1 << 3;

const CUBE_HAS_MESH: u8 = 1 << 0;
const CUBE_HAS_TEXTURE: u8 = 1 << 1;
const CUBE_HIDDEN: u8 = 1 << 2;
const CUBE_MESH_SCALED: u8 = 1 << 3;
const CUBE_UV_SCALED: u8 = 1 << 4;
const CUBE_MC_SCALED: u8 = 1 << 5;
const CUBE_SCALED: u8 = 1 << 6;

/// `Cube.loadDefinitionCubeV2`.
fn read_cube_v2(b: &mut Reader) -> Result<RawCube> {
    let flags = b.u8()?;
    let parent = b.varint()?;
    let pos = b.var_vec3()?;
    let rotation = b.angle()?;
    let (mut size, mut offset) = ([0.0; 3], [0.0; 3]);
    let (mut tex_size, mut u, mut v, mut rgb, mut mc_scale) = (0u8, 0, 0, 0u32, 0.0);
    if flags & CUBE_HAS_MESH != 0 {
        size = b.var_vec3()?;
        offset = b.var_vec3()?;
        if flags & CUBE_HAS_TEXTURE != 0 {
            tex_size = if flags & CUBE_UV_SCALED != 0 { b.u8()? } else { 1 };
            u = b.varint()?;
            v = b.varint()?;
        } else {
            let c = b.bytes(3)?;
            rgb = 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        }
        if flags & CUBE_MC_SCALED != 0 {
            mc_scale = b.float2()?;
        }
    }
    let mesh_scale = if flags & CUBE_MESH_SCALED != 0 { b.var_vec3()? } else { [1.0; 3] };
    let scale = if flags & CUBE_SCALED != 0 { b.var_vec3()? } else { [1.0; 3] };
    Ok(RawCube {
        id: 0,
        parent,
        size,
        pos,
        offset,
        rotation,
        tex_size,
        u,
        v,
        rgb,
        mesh_scale,
        scale,
        mc_scale,
        hidden: flags & CUBE_HIDDEN != 0,
    })
}

/// `Cube.loadDefinitionCube` (legacy layout).
fn read_cube(b: &mut Reader) -> Result<RawCube> {
    let size = b.vec3ub()?;
    let pos = b.vec6b()?;
    let offset = b.vec6b()?;
    let rotation = b.angle()?;
    let parent = b.varint()?;
    let tex = b.i8()? as u8;
    let (mut u, mut v, mut rgb) = (0, 0, 0);
    if tex == 0 {
        let c = b.bytes(3)?;
        rgb = 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
    } else {
        u = b.u8()? as u32;
        v = b.u8()? as u32;
    }
    Ok(RawCube {
        id: 0,
        parent,
        size,
        pos,
        offset,
        rotation,
        tex_size: tex,
        u,
        v,
        rgb,
        mesh_scale: [1.0; 3],
        scale: [1.0; 3],
        mc_scale: 0.0,
        hidden: false,
    })
}
