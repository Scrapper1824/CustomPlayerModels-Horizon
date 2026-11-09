//! Animated textures: a port of `ModelPartAnimatedTexture` (the `ANIMATED_TEX` part) and
//! `AnimatedTexture` (`update`, `copyTexture`, `copyTextureInt`).
//!
//! A region of a texture sheet (`uv_start`, `uv_size`) shows one frame at a time, copied from a
//! strip of frames starting at `anim_start` (laid out along x when `an_x`, else along y). Frames
//! are copied within the same image, in place, as Java copies them.

/// `TextureSheetType.SKIN.ordinal()`.
pub const SHEET_SKIN: u8 = 0;

/// One `ANIMATED_TEX` part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimTex {
    /// `TextureSheetType` ordinal (`SKIN` 0, `CAPE` 2, `ELYTRA` 3 are drawn; others are not).
    pub sheet: u8,
    /// The region the frames are shown in, in image pixels.
    pub uv_start: [i32; 2],
    pub uv_size: [i32; 2],
    /// The first frame's top left.
    pub anim_start: [i32; 2],
    pub frame_count: i32,
    /// Milliseconds per frame.
    pub frame_time: i32,
    /// Frames follow each other along x (else along y).
    pub an_x: bool,
    /// Blend each frame into the next.
    pub interpolate: bool,
}

/// Throttle of the interpolated copy, in milliseconds.
pub const INTERPOLATE_EVERY_MS: u64 = 50;

/// An [`AnimTex`] with its update state (`AnimatedTexture.lastFrame` / `lastUpdate`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedTexture {
    pub tex: AnimTex,
    /// The frame last copied; 0 to begin with, as in Java (frame 0 is not copied until another
    /// frame was).
    last_frame: i32,
    /// When the interpolated copy last ran.
    last_update: u64,
}

impl AnimatedTexture {
    pub fn new(tex: AnimTex) -> Self {
        AnimatedTexture { tex, last_frame: 0, last_update: 0 }
    }

    /// The frame shown at `time` ms: `time / frameTime % frameCount` (0 when it has no frames).
    pub fn frame(&self, time: u64) -> i32 {
        let (count, ft) = (self.tex.frame_count, self.tex.frame_time);
        if count <= 0 || ft <= 0 {
            return 0;
        }
        (time / ft as u64 % count as u64) as i32
    }

    /// `AnimatedTexture.update` at `time` ms on the sheet's image `rgba` (`size` pixels, RGBA8).
    /// Without interpolation the frame is copied when it changed; with it, the blend of this
    /// frame and the next is copied once [`INTERPOLATE_EVERY_MS`] passed since the last copy.
    /// True when a copy changed a pixel.
    ///
    /// (Java never sets `lastUpdate`, so its interpolated copy runs every render past the first
    /// 50 ms; here it runs every 50 ms, so the texture is not sent to the game each frame. A
    /// blend steps at 20 Hz instead of the frame rate.)
    pub fn update(&mut self, time: u64, rgba: &mut [u8], size: [u32; 2]) -> bool {
        let t = self.tex;
        if t.frame_count <= 0 || t.frame_time <= 0 || rgba.len() < (size[0] as usize * size[1] as usize * 4) {
            return false;
        }
        let frm = self.frame(time);
        let mut img = Img { rgba, w: size[0] as i64, h: size[1] as i64 };
        if t.interpolate {
            if time.saturating_sub(self.last_update) >= INTERPOLATE_EVERY_MS {
                self.last_update = time;
                let at = frm as f32 + (time % t.frame_time as u64) as f32 / t.frame_time as f32;
                return copy_texture_int(&mut img, &t, at);
            }
            false
        } else if frm != self.last_frame {
            self.last_frame = frm;
            copy_texture(&mut img, &t, frm)
        } else {
            false
        }
    }
}

/// An image being written in place.
struct Img<'a> {
    rgba: &'a mut [u8],
    w: i64,
    h: i64,
}

impl Img<'_> {
    fn inside(&self, x: i64, y: i64) -> bool {
        x >= 0 && x < self.w && y >= 0 && y < self.h
    }

    fn at(&self, x: i64, y: i64) -> usize {
        ((y * self.w + x) * 4) as usize
    }

    fn get(&self, x: i64, y: i64) -> [u8; 4] {
        let i = self.at(x, y);
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Writes a pixel; true when it differed.
    fn set(&mut self, x: i64, y: i64, p: [u8; 4]) -> bool {
        let i = self.at(x, y);
        let changed = self.rgba[i..i + 4] != p;
        self.rgba[i..i + 4].copy_from_slice(&p);
        changed
    }
}

/// `AnimatedTexture.copyTexture`: frame `frame` over the region; pixels outside the image skipped.
fn copy_texture(img: &mut Img, t: &AnimTex, frame: i32) -> bool {
    let [uvx, uvy] = t.uv_start.map(i64::from);
    let [sx, sy] = t.uv_size.map(i64::from);
    let [mut ax, mut ay] = t.anim_start.map(i64::from);
    if t.an_x {
        ax += sx * frame as i64;
    } else {
        ay += sy * frame as i64;
    }
    let mut changed = false;
    for x in 0..sx {
        for y in 0..sy {
            let (x1, y1, x2, y2) = (x + ax, y + ay, x + uvx, y + uvy);
            if img.inside(x1, y1) && img.inside(x2, y2) {
                let p = img.get(x1, y1);
                changed |= img.set(x2, y2, p);
            }
        }
    }
    changed
}

/// `AnimatedTexture.copyTextureInt`: frame `(int) time` blended into the next (wrapping), by
/// `subfrm = 1 - (time - frame)` of the first; the alpha is the first frame's.
fn copy_texture_int(img: &mut Img, t: &AnimTex, time: f32) -> bool {
    let frame = time as i32;
    let subfrm = 1.0 - (time - frame as f32);
    let [uvx, uvy] = t.uv_start.map(i64::from);
    let [sx, sy] = t.uv_size.map(i64::from);
    let [ax, ay] = t.anim_start.map(i64::from);
    let next = ((frame + 1) % t.frame_count) as i64;
    let (axf, ayf, axs, ays) = if t.an_x {
        (ax + sx * frame as i64, ay, ax + sx * next, ay)
    } else {
        (ax, ay + sy * frame as i64, ax, ay + sy * next)
    };
    let mix = |a: u8, b: u8| (subfrm * a as f32 + (1.0 - subfrm) * b as f32) as i32 as u8;
    let mut changed = false;
    for x in 0..sx {
        for y in 0..sy {
            let (xf, yf, xs, ys, x2, y2) = (x + axf, y + ayf, x + axs, y + ays, x + uvx, y + uvy);
            if img.inside(xf, yf) && img.inside(xs, ys) && img.inside(x2, y2) {
                let (a, b) = (img.get(xf, yf), img.get(xs, ys));
                changed |= img.set(x2, y2, [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2]), a[3]]);
            }
        }
    }
    changed
}
