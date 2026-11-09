//! Reader for CPM editor projects (`.cpmproject`): a port of `ProjectIO.loadProject` and the V1 part
//! loaders (`ElementsLoaderV1`, `TexturesLoaderV1`, `DescriptionLoaderV1`, `AnimationsLoaderV1`,
//! `PropertiesLoaderV1`).
//!
//! Every value the exporter needs is read with Java's default for a missing key. Where Java has no
//! default (it would throw on a missing key), the value the editor's constructors start with is used.
//! Features the converter does not export are skipped and noted in [`Project::warnings`].

mod anims;
mod elements;

use std::io::{Cursor, Read};

use serde_json::{Map, Value};

use crate::anim::texture::{AnimTex, SHEET_SKIN};
use crate::effects::PartTransform;
use crate::format::{SkinType, SHEET_CAPE, SHEET_ELYTRA};
use crate::io::{CpmError, Result};

pub use anims::{AnimKind, FrameComponent, ProjectAnim, ProjectFrame};
pub(crate) use anims::VANILLA_POSES;
pub use elements::{CopyTransform, Face, FaceUv, ProjectElement, ProjectRoot, RootPart};

/// `ETextures.MAX_TEX_SIZE`.
const MAX_TEX_SIZE: u32 = 8192;

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// `description.json` `name` ("" when the file is missing).
    pub name: String,
    /// `description.json` `desc` ("" when the file is missing).
    pub desc: String,
    /// `description.json` `cam.copyProt` (`"normal"` by default; `cloneable` / `uuid_lock` exist).
    pub copy_protection: String,
    /// `desc_icon.png`, only when it is a 256x256 PNG (`DescriptionLoaderV1.loadIcon`).
    pub icon_png: Option<Vec<u8>>,
    pub skin_type: SkinType,
    /// `skin.png` as stored in the project.
    pub skin_png: Vec<u8>,
    /// Declared texture grid size: `skinSize`, else `TextureSheetType.SKIN` default 64x64.
    pub skin_size: [u32; 2],
    /// Actual pixel size of `skin_png`.
    pub skin_png_size: [u32; 2],
    /// `elytra.png` as stored in the project (the model's own elytra sheet), if any.
    pub elytra_png: Option<Vec<u8>>,
    /// Declared elytra grid size: `elytraSize`, else `TextureSheetType.ELYTRA` default 64x32.
    pub elytra_size: [u32; 2],
    /// `cape.png` as stored in the project (the model's own cape sheet), if any.
    pub cape_png: Option<Vec<u8>>,
    /// Declared cape grid size: `capeSize`, else `TextureSheetType.CAPE` default 64x32.
    pub cape_size: [u32; 2],
    /// The animated textures (`textures.<sheet>.anim`) of the skin, cape and elytra sheets the
    /// project has an image for, by sheet (skin, cape, elytra), each in file order.
    pub anim_textures: Vec<AnimTex>,
    pub hide_head_if_skull: bool,
    /// `removeArmorOffset`, default: no root is duplicated.
    pub remove_armor_offset: bool,
    pub remove_bed_offset: bool,
    /// `enableInvisGlow`.
    pub invis_glow: bool,
    /// The render transform (`scalingEx` `render_position` / `render_rotation` / `render_scale`),
    /// when scaling is on (`scaling` not 0) and it is not the identity (`hasTransform`); the
    /// exporter writes it as `MODEL_SCALE`.
    pub render_transform: Option<PartTransform>,
    /// `firstPersonHand` `[left, right]`, when either is changed (`isChanged`); written as
    /// `FIRST_PERSON_HAND`.
    pub fp_hand: Option<[PartTransform; 2]>,
    /// `modelId` (written in the animation `CONTROL_INFO`).
    pub model_id: Option<String>,
    /// The six player roots in `PlayerModelParts` order, then duplicated and non-player
    /// (cape, elytra, armour) roots in file order.
    pub roots: Vec<ProjectRoot>,
    /// Sorted by file name (Java's order is a `HashMap` key set, i.e. unspecified).
    pub anims: Vec<ProjectAnim>,
    /// `anim_enc.json`, raw.
    pub anim_enc: Option<Value>,
    /// Things read but not converted (CPM's "log note").
    pub warnings: Vec<String>,
}

/// Parses a `.cpmproject` zip.
pub fn read_project(zip_bytes: &[u8]) -> Result<Project> {
    let files = ZipFiles::new(zip_bytes)?;
    let mut warnings = Vec::new();

    let config = files
        .json("config.json")?
        .ok_or_else(|| CpmError::Invalid("project has no config.json".into()))?;
    let config = config
        .as_object()
        .ok_or_else(|| CpmError::Invalid("config.json is not an object".into()))?;
    let version = config.get("version").and_then(Value::as_i64);
    if version != Some(1) {
        return Err(CpmError::Invalid(format!("unsupported project version {version:?}")));
    }

    // ElementsLoaderV1 (load order -1).
    let elements::Roots { roots, any_duplicated } = elements::load_roots(config, &mut warnings);

    // TexturesLoaderV1.
    let skin_png = files
        .get("skin.png")?
        .ok_or_else(|| CpmError::Invalid("project has no skin.png".into()))?;
    let skin_png_size = png_size(&skin_png)?;
    if skin_png_size[0] > MAX_TEX_SIZE || skin_png_size[1] > MAX_TEX_SIZE {
        return Err(CpmError::Invalid(format!("skin.png is too big: {skin_png_size:?}")));
    }
    let skin_size = vec2i(config.get("skinSize"), [64, 64]);
    let skin_size = [skin_size[0].max(0) as u32, skin_size[1].max(0) as u32];
    // TexturesLoaderV1 for the CAPE and ELYTRA sheets: `<name>.png` and `<name>Size`.
    let mut sheet = |name: &str| -> Result<(Option<Vec<u8>>, [u32; 2])> {
        let mut png = files.get(&format!("{name}.png"))?;
        if let Some(p) = &png {
            match png_size(p) {
                Ok([w, h]) if w <= MAX_TEX_SIZE && h <= MAX_TEX_SIZE => {}
                // "Illegal image size for texture", the sheet is not loaded.
                _ => {
                    warnings.push(format!("{name}.png is unreadable or too big: dropped"));
                    png = None;
                }
            }
        }
        let size = vec2i(config.get(&format!("{name}Size")), [64, 32]);
        Ok((png, [size[0].max(0) as u32, size[1].max(0) as u32]))
    };
    let (elytra_png, elytra_size) = sheet("elytra")?;
    let (cape_png, cape_size) = sheet("cape")?;
    // TexturesLoaderV1.loadAnimatedTexture: `textures.<name>.anim` of each editable sheet whose
    // image loaded. The list icon's are not converted (its sheet is not).
    let mut anim_textures = Vec::new();
    let tex = obj(config.get("textures"));
    for (name, sheet, has) in [
        ("skin", SHEET_SKIN, true),
        ("cape", SHEET_CAPE, cape_png.is_some()),
        ("elytra", SHEET_ELYTRA, elytra_png.is_some()),
    ] {
        if has {
            anim_textures.extend(animated_textures(tex, name).map(|m| anim_tex(sheet, m)));
        }
    }
    if files.has("list_icon.png") && animated_textures(tex, "list_icon").next().is_some() {
        warnings.push("animated list_icon textures are not converted".into());
    }
    if files.has("list_icon.png") {
        warnings.push("list_icon.png texture is not converted".into());
    }

    // DescriptionLoaderV1.
    let (mut name, mut desc, mut copy_protection) = (String::new(), String::new(), "normal".to_string());
    if let Some(d) = files.json("description.json")? {
        let d = d.as_object();
        name = string(d, "name").unwrap_or_default();
        desc = string(d, "desc").unwrap_or_default();
        let cam = d.and_then(|d| obj(d.get("cam")));
        copy_protection = string(cam, "copyProt").unwrap_or(copy_protection);
    }
    let icon_png = match files.get("desc_icon.png")? {
        Some(png) => match png_size(&png) {
            Ok([256, 256]) => Some(png),
            Ok(s) => {
                warnings.push(format!("model icon must be 256x256, is {s:?}: dropped"));
                None
            }
            Err(e) => {
                warnings.push(format!("model icon unreadable ({e}): dropped"));
                None
            }
        },
        None => None,
    };

    // PropertiesLoaderV1.
    let skin_type = match config.get("skinType") {
        None | Some(Value::Null) => SkinType::Default,
        Some(v) => match v.as_str() {
            Some("slim") => SkinType::Slim,
            Some("default") => SkinType::Default,
            _ => SkinType::Unknown,
        },
    };
    let c = Some(config);
    let hide_head_if_skull = boolean(c, "hideHeadIfSkull", true);
    let remove_armor_offset = boolean(c, "removeArmorOffset", !any_duplicated);
    let remove_bed_offset = boolean(c, "removeBedOffset", false);
    let invis_glow = boolean(c, "enableInvisGlow", false);
    let model_id = string(c, "modelId");
    // `scaling` (the entity scale) turns scaling on; its gameplay values (entity size, scalingEx
    // attributes) need CPM's server side and are dropped. The render transform is kept.
    let scaling = float(c, "scaling", 0.0);
    if scaling != 0.0 && scaling != 1.0 {
        warnings.push("model scaling (entity size) is not converted".into());
    }
    let mut render_transform = None;
    if let Some(ex) = obj(config.get("scalingEx")) {
        let changed = ex.iter().any(|(k, v)| !k.starts_with("render_") && v.as_f64().is_some_and(|f| f != 0.0 && f != 1.0));
        // Exported only while scaling is on (ExporterImpl): otherwise nothing is skipped.
        if changed && scaling != 0.0 {
            warnings.push("scaling and attribute settings (scalingEx) are not converted".into());
        }
        let t = PartTransform {
            pos: vec3(ex.get("render_position"), [0.0; 3]),
            rot: vec3(ex.get("render_rotation"), [0.0; 3]),
            scale: vec3(ex.get("render_scale"), [0.0; 3]),
        };
        if scaling != 0.0 && t.has_transform() {
            render_transform = Some(t);
        }
    }
    // PropertiesLoaderV1.loadPartPos: a missing hand or value is 0.
    let fp_hand = obj(config.get("firstPersonHand")).and_then(|fp| {
        let hand = |name: &str| match obj(fp.get(name)) {
            Some(h) => PartTransform {
                pos: vec3(h.get("position"), [0.0; 3]),
                rot: vec3(h.get("rotation"), [0.0; 3]),
                scale: vec3(h.get("scale"), [0.0; 3]),
            },
            None => PartTransform::ZERO,
        };
        let hands = [hand("left"), hand("right")];
        hands.iter().any(PartTransform::is_changed).then_some(hands)
    });
    if config.get("templates").and_then(Value::as_array).is_some_and(|t| !t.is_empty()) {
        warnings.push("templates are not converted".into());
    }
    // TagsLoaderV1: tags/<kind>.json (an unreadable one counts as none; it is not converted anyway).
    for kind in ["items", "blocks", "entities", "biomes"] {
        let tags = files.json(&format!("tags/{kind}.json")).ok().flatten();
        if tags.as_ref().and_then(|t| t.get("tags")).and_then(Value::as_array).is_some_and(|t| !t.is_empty()) {
            warnings.push(format!("{kind} tags are not converted"));
        }
    }

    // AnimationsLoaderV1.
    let mut store_ids = std::collections::HashSet::new();
    elements::collect_store_ids(&roots, &mut store_ids);
    let mut anims = Vec::new();
    for file in files.list("animations") {
        let path = format!("animations/{file}");
        let data = files.json(&path)?.unwrap_or(Value::Null);
        if let Some(a) = anims::load_animation(&file, data.as_object(), &store_ids) {
            anims.push(a);
        } else {
            warnings.push(format!("animation {file} has an unknown type: skipped"));
        }
    }
    let anim_enc = files.json("anim_enc.json")?;

    Ok(Project {
        name,
        desc,
        copy_protection,
        icon_png,
        skin_type,
        skin_png,
        skin_size,
        skin_png_size,
        elytra_png,
        elytra_size,
        cape_png,
        cape_size,
        anim_textures,
        hide_head_if_skull,
        remove_armor_offset,
        remove_bed_offset,
        invis_glow,
        render_transform,
        fp_hand,
        model_id,
        roots,
        anims,
        anim_enc,
        warnings,
    })
}

/// The zip's file entries, by path (`ProjectFile`).
struct ZipFiles {
    archive: std::cell::RefCell<zip::ZipArchive<Cursor<Vec<u8>>>>,
    names: Vec<String>,
}

impl ZipFiles {
    fn new(bytes: &[u8]) -> Result<ZipFiles> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
            .map_err(|e| CpmError::Invalid(format!("not a project zip: {e}")))?;
        let names = archive
            .file_names()
            .map(|n| n.map(|n| n.into_owned()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| CpmError::Invalid(format!("bad project zip entry name: {e}")))?;
        Ok(ZipFiles { archive: std::cell::RefCell::new(archive), names })
    }

    fn has(&self, path: &str) -> bool {
        self.names.iter().any(|n| n == path)
    }

    fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if !self.has(path) {
            return Ok(None);
        }
        let mut archive = self.archive.borrow_mut();
        let mut f = archive
            .by_name(path)
            .map_err(|e| CpmError::Invalid(format!("{path}: {e}")))?;
        if f.is_dir() {
            return Ok(None);
        }
        let mut out = Vec::new();
        f.read_to_end(&mut out).map_err(|e| CpmError::Invalid(format!("{path}: {e}")))?;
        Ok(Some(out))
    }

    fn json(&self, path: &str) -> Result<Option<Value>> {
        match self.get(path)? {
            Some(b) => serde_json::from_slice(&b)
                .map(Some)
                .map_err(|e| CpmError::Invalid(format!("{path}: {e}"))),
            None => Ok(None),
        }
    }

    /// File names directly inside `dir`, sorted.
    fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        let mut out: Vec<String> = self
            .names
            .iter()
            .filter_map(|n| n.strip_prefix(&prefix))
            .filter(|n| !n.is_empty() && !n.contains('/'))
            .map(str::to_string)
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

fn png_size(png: &[u8]) -> Result<[u32; 2]> {
    let dec = png::Decoder::new(Cursor::new(png));
    let r = dec.read_info().map_err(|e| CpmError::Png(e.to_string()))?;
    let i = r.info();
    Ok([i.width, i.height])
}

// JSON accessors mirroring `JsonMap`'s getters. Numbers may be written as ints or floats.

pub(crate) type JMap = Map<String, Value>;

/// The `anim` list of `textures.<name>`: its objects.
fn animated_textures<'a>(tex: Option<&'a JMap>, name: &str) -> impl Iterator<Item = &'a JMap> {
    obj(tex.and_then(|t| t.get(name)))
        .and_then(|d| d.get("anim"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

/// `AnimatedTex(Editor, TextureSheetType, JsonMap)`.
fn anim_tex(sheet: u8, m: &JMap) -> AnimTex {
    let m = Some(m);
    AnimTex {
        sheet,
        uv_start: vec2i(get(m, "start"), [0, 0]),
        uv_size: vec2i(get(m, "size"), [0, 0]),
        anim_start: vec2i(get(m, "from"), [0, 0]),
        frame_count: int(m, "frameCount", 0),
        frame_time: int(m, "frameTime", 0),
        an_x: boolean(m, "anX", false),
        interpolate: boolean(m, "interpolate", false),
    }
}

pub(crate) fn obj(v: Option<&Value>) -> Option<&JMap> {
    v.and_then(Value::as_object)
}

fn get<'a>(m: Option<&'a JMap>, key: &str) -> Option<&'a Value> {
    m.and_then(|m| m.get(key)).filter(|v| !v.is_null())
}

pub(crate) fn boolean(m: Option<&JMap>, key: &str, def: bool) -> bool {
    get(m, key).and_then(Value::as_bool).unwrap_or(def)
}

pub(crate) fn float(m: Option<&JMap>, key: &str, def: f32) -> f32 {
    get(m, key).and_then(Value::as_f64).map(|f| f as f32).unwrap_or(def)
}

/// `Number.intValue()`: floats truncate, longs wrap.
pub(crate) fn int(m: Option<&JMap>, key: &str, def: i32) -> i32 {
    get(m, key).and_then(num_long).map(|l| l as i32).unwrap_or(def)
}

/// `Number.longValue()`, exact for integers beyond f64 precision.
pub(crate) fn long(m: Option<&JMap>, key: &str, def: i64) -> i64 {
    get(m, key).and_then(num_long).unwrap_or(def)
}

fn num_long(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| u as i64))
        .or_else(|| v.as_f64().map(|f| f as i64))
}

pub(crate) fn string(m: Option<&JMap>, key: &str) -> Option<String> {
    get(m, key).and_then(Value::as_str).map(str::to_string)
}

/// `new Vec3f(map, def)`; a component missing from a present map keeps the default.
pub(crate) fn vec3(v: Option<&Value>, def: [f32; 3]) -> [f32; 3] {
    match obj(v) {
        Some(m) => {
            let m = Some(m);
            [float(m, "x", def[0]), float(m, "y", def[1]), float(m, "z", def[2])]
        }
        None => def,
    }
}

/// `new Vec2i(map, def)`.
pub(crate) fn vec2i(v: Option<&Value>, def: [i32; 2]) -> [i32; 2] {
    match obj(v) {
        Some(m) => {
            let m = Some(m);
            [int(m, "x", def[0]), int(m, "y", def[1])]
        }
        None => def,
    }
}

/// `Integer.parseUnsignedInt(s, 16)`; unparsable or missing gives 0.
pub(crate) fn hex_color(m: Option<&JMap>, key: &str) -> u32 {
    string(m, key).and_then(|s| u32::from_str_radix(&s, 16).ok()).unwrap_or(0)
}
