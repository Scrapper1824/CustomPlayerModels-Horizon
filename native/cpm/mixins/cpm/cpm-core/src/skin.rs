//! A model stored in the free space of a skin (CPM's "export to skin"): `SkinDataInputStream`,
//! `SkinDataOutputStream` (CPM's encoder, kept for tests and tools) and the read of
//! `ModelDefinitionLoader.loadModel(Image skin, Player)`.
//!
//! Which pixels hold data comes from CPM's `free_space_template.png` (MIT, bundled): a pixel whose
//! template channel (red for slim, green for wide, alpha for unknown) is `0xff` holds data, three
//! bytes (blue, green, red) where the template's blue is `0xff`, else four (alpha too).

use crate::format::{HEADER, SkinType};
use crate::io::{CpmError, MAX_BLOCK_SIZE, Result};
use crate::mesh::LoadedModel;
use crate::write::{Writer, checksummed};

/// CPM's `assets/cpm/textures/template/free_space_template.png`.
pub const TEMPLATE_PNG: &[u8] = include_bytes!("../data/free_space_template.png");

const END: u8 = 0;
const SKIN_TYPE: u8 = 11;

/// `com.tom.cpl.util.Image`: pixels as `getRGB` gives them (`0xAARRGGBB`), row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub argb: Vec<u32>,
}

impl Image {
    /// A transparent black image.
    pub fn new(w: u32, h: u32) -> Self {
        Image { w, h, argb: vec![0; (w * h) as usize] }
    }

    /// `Image.loadFrom`: a PNG, any colour type, as non-premultiplied ARGB (16-bit channels keep
    /// their high byte).
    pub fn decode(png: &[u8]) -> Result<Self> {
        let ([w, h], rgba) = crate::mesh::decode_png(png)?;
        let argb = rgba.chunks_exact(4).map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
        Ok(Image { w, h, argb })
    }

    /// The image as an 8-bit RGBA PNG.
    pub fn encode_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, self.w, self.h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let rgba: Vec<u8> = self.rgba();
        let mut w = e.write_header().expect("png header");
        w.write_image_data(&rgba).expect("png data");
        w.finish().expect("png end");
        out
    }

    /// The pixels as RGBA8.
    pub fn rgba(&self) -> Vec<u8> {
        self.argb
            .iter()
            .flat_map(|p| {
                let [a, r, g, b] = p.to_be_bytes();
                [r, g, b, a]
            })
            .collect()
    }

    /// `getRGB(x, y)`: `data[y * w + x]`, so an `x` past the width reads the next row; an error
    /// past the end (Java's `ArrayIndexOutOfBoundsException`).
    pub fn get_rgb(&self, x: u32, y: u32) -> Result<u32> {
        let i = y as usize * self.w as usize + x as usize;
        self.argb.get(i).copied().ok_or_else(|| CpmError::Invalid(format!("pixel {x},{y} outside the image")))
    }

    /// `setRGB(x, y, v)`, with [`get_rgb`](Self::get_rgb)'s indexing.
    pub fn set_rgb(&mut self, x: u32, y: u32, v: u32) -> Result<()> {
        let i = y as usize * self.w as usize + x as usize;
        let p = self.argb.get_mut(i).ok_or_else(|| CpmError::Invalid(format!("pixel {x},{y} outside the image")))?;
        *p = v;
        Ok(())
    }
}

/// The free space template, decoded once.
pub fn template() -> Result<&'static Image> {
    static T: std::sync::OnceLock<std::result::Result<Image, CpmError>> = std::sync::OnceLock::new();
    T.get_or_init(|| Image::decode(TEMPLATE_PNG)).as_ref().map_err(Clone::clone)
}

/// `SkinType.getChannel`: 0 slim, 1 wide (`DEFAULT`), -1 unknown.
pub fn channel(t: SkinType) -> i32 {
    match t {
        SkinType::Slim => 0,
        SkinType::Default => 1,
        SkinType::Unknown => -1,
    }
}

/// Whether template pixel `t` holds data on `channel` (`shift = 8 * (2 - channel)`).
fn free(t: u32, channel: i32) -> bool {
    let shift = 8 * (2 - channel);
    (t >> shift) & 0xFF == 0xFF
}

/// `SkinDataInputStream`: the bytes kept in `img`'s free pixels on `channel`.
pub struct SkinDataInputStream<'a> {
    img: &'a Image,
    template: &'a Image,
    block: i32,
    channel: i32,
    x: i64,
    y: u32,
    finished: bool,
}

impl<'a> SkinDataInputStream<'a> {
    pub fn new(img: &'a Image, template: &'a Image, channel: i32) -> Self {
        SkinDataInputStream { img, template, block: -1, channel, x: -1, y: 0, finished: false }
    }

    /// `read()`: the next byte, `None` at the end of the free space (`-1`).
    pub fn read(&mut self) -> Result<Option<u8>> {
        if self.finished {
            return Ok(None);
        }
        if self.block == -1 || self.block > 3 {
            self.find_next_block()?;
            if self.finished {
                return Ok(None);
            }
            self.block = 0;
        }
        let (x, y) = (self.x as u32, self.y);
        let dt = self.img.get_rgb(x, y)?;
        let shift = self.block * 8;
        self.block += 1;
        if self.template.get_rgb(x, y)? & 0xFF == 0xFF && self.block > 2 {
            self.block += 1;
        }
        Ok(Some((dt >> shift) as u8))
    }

    fn find_next_block(&mut self) -> Result<()> {
        for y in self.y..self.img.h {
            let mut x = self.x + 1;
            while x < self.img.w as i64 {
                if free(self.template.get_rgb(x as u32, y)?, self.channel) {
                    self.x = x;
                    self.y = y;
                    return Ok(());
                }
                x += 1;
            }
            self.x = -1;
        }
        self.finished = true;
        Ok(())
    }

    pub fn set_channel(&mut self, channel: i32) {
        self.channel = channel;
    }

    pub fn channel(&self) -> i32 {
        self.channel
    }
}

/// `SkinDataOutputStream`: writes bytes into `img`'s free pixels on `channel` (CPM's encoder; the
/// mod only reads). Making it clears every free pixel of the channel (opaque black where the
/// template's blue is `0xff`, else transparent).
pub struct SkinDataOutputStream<'a> {
    img: &'a mut Image,
    template: &'a Image,
    block: i32,
    channel: i32,
    x: i64,
    y: u32,
}

impl<'a> SkinDataOutputStream<'a> {
    pub fn new(img: &'a mut Image, template: &'a Image, channel: i32) -> Self {
        for y in 0..img.h {
            for x in 0..img.w {
                let Ok(t) = template.get_rgb(x, y) else { continue };
                if free(t, channel) {
                    let v = if t & 0xFF == 0xFF { 0xFF00_0000 } else { 0 };
                    let _ = img.set_rgb(x, y, v);
                }
            }
        }
        SkinDataOutputStream { img, template, block: -1, channel, x: -1, y: 0 }
    }

    /// `write(b)`; [`CpmError::Eof`] when the free space is full.
    pub fn write(&mut self, b: u8) -> Result<()> {
        if self.block == -1 || self.block > 3 {
            if !self.find_next_block()? {
                return Err(CpmError::Eof);
            }
            self.block = 0;
        }
        let (x, y) = (self.x as u32, self.y);
        let mut dt = self.img.get_rgb(x, y)?;
        let shift = self.block * 8;
        self.block += 1;
        dt &= !(0xFF << shift);
        dt |= (b as u32) << shift;
        if self.template.get_rgb(x, y)? & 0xFF == 0xFF && self.block > 2 {
            self.block += 1;
            dt |= 0xFF00_0000;
        }
        self.img.set_rgb(x, y, dt)
    }

    fn find_next_block(&mut self) -> Result<bool> {
        for y in self.y..self.img.h {
            let mut x = self.x + 1;
            while x < self.img.w as i64 {
                if free(self.template.get_rgb(x as u32, y)?, self.channel) {
                    self.x = x;
                    self.y = y;
                    return Ok(true);
                }
                x += 1;
            }
            self.x = -1;
        }
        Ok(false)
    }
}

/// The next byte of `s`, the end of the free space being [`CpmError::Eof`].
fn byte(s: &mut SkinDataInputStream) -> Result<u8> {
    s.read()?.ok_or(CpmError::Eof)
}

/// `loadModel(Image skin, Player)` for a player of `skin_type`: the model data in `skin` (`0x53`,
/// its parts, its checksum, as a `.cpmmodel`'s data block holds them), or `None` when the first
/// free byte is not the header (no model: the vanilla player, no error).
///
/// The parts are read as `IOHelper.readObjectBlock` frames them (a type byte, a varint length,
/// the payload) until `END` and the 2-byte checksum. A `SKIN_TYPE` part that names a known skin
/// type of another channel turns the reader to that channel (CPM's mismatch handling: the
/// template's first pixels are free on both). The parts themselves are decoded afterwards
/// ([`crate::format::read_definition`]), which also checks the checksum.
pub fn read_skin_data(skin: &Image, skin_type: SkinType) -> Result<Option<Vec<u8>>> {
    let template = template()?;
    let mut s = SkinDataInputStream::new(skin, template, channel(skin_type));
    if s.read()? != Some(HEADER) {
        return Ok(None);
    }
    let mut out = vec![HEADER];
    loop {
        let ty = byte(&mut s)?;
        out.push(ty);
        // `readVarInt`: at most 5 bytes.
        let mut len: u32 = 0;
        let mut j = 0;
        loop {
            let b = byte(&mut s)?;
            out.push(b);
            len |= ((b & 0x7F) as u32).wrapping_shl(7 * j);
            j += 1;
            if b & 0x80 == 0 {
                break;
            }
            if j >= 5 {
                return Err(CpmError::Invalid("VarInt too big".into()));
            }
        }
        if len > MAX_BLOCK_SIZE {
            return Err(CpmError::BlockTooBig(len));
        }
        let start = out.len();
        for _ in 0..len {
            out.push(byte(&mut s)?);
        }
        if ty == SKIN_TYPE {
            // `ModelPartSkinType`: `SkinType.VALUES[t]`, unknown past the end.
            let t = match out.get(start) {
                Some(0) => SkinType::Slim,
                Some(1) => SkinType::Default,
                Some(_) => SkinType::Unknown,
                None => return Err(CpmError::Eof),
            };
            if t != SkinType::Unknown && channel(t) != s.channel() {
                s.set_channel(channel(t));
            }
        }
        if ty == END {
            out.push(byte(&mut s)?);
            out.push(byte(&mut s)?);
            return Ok(Some(out));
        }
    }
}

/// The model in the skin PNG `skin_png` (the raw downloaded file) of a `skin_type` player, as a
/// `.cpmmodel` (name `skin`, no icon) for the usual loading path (links resolve as any model's);
/// `None` when the skin holds no model. An error when the PNG does not decode or the data is cut
/// short; a bad checksum or bad parts fail when the file is loaded.
pub fn skin_model_file(skin_png: &[u8], skin_type: SkinType) -> Result<Option<Vec<u8>>> {
    let img = Image::decode(skin_png)?;
    Ok(read_skin_data(&img, skin_type)?.map(|data| container(&data)))
}

/// Model data ([`read_skin_data`]) as a `.cpmmodel`: name `skin`, no description, no overflow,
/// no icon.
pub fn container(data: &[u8]) -> Vec<u8> {
    let mut d = Writer::new();
    d.bytes(data);
    let mut w = Writer::new();
    w.utf("skin").utf("").block(&d).varint(0).block(&Writer::new());
    checksummed(HEADER, &w.into_bytes())
}

/// `ModelDefinition` without a skin part: the model draws with the player's skin (`size`, RGBA8),
/// declared 64x64. True when it was given; a model with a texture of its own keeps it (false).
pub fn use_skin_texture(model: &mut LoadedModel, size: [u32; 2], rgba: Vec<u8>) -> bool {
    if !model.texture_rgba.is_empty() || size[0] == 0 || size[1] == 0 || rgba.len() != (size[0] * size[1] * 4) as usize {
        return false;
    }
    model.texture_rgba = rgba;
    model.png_px = size;
    model.texture_px = [64, 64];
    model.model.texture_size = [64, 64];
    true
}
