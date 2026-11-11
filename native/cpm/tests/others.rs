// Other players' models, from the model stored in their skins (multiplayer phase 1): a registry by
// UUID, apart from the local player's model, loaded off the frame thread, with CPM's caps and a
// region of the mod image each. Each test runs on its own thread, so its own state.
mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cpm_core::skin::{self, Image, SkinDataOutputStream};
use cpm_core::write::{Writer, checksummed};
use horizon_mod_cpm as cpm;
use cpm::{OtherPlayer, OtherSkin, OtherStatus};

/// The tests run one at a time: the link downloads (`take_fetch_requests`, `finish_fetch`) are
/// the process's, not a test thread's, so one test would take another's.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("others_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One textured head cube, then `parts` (each a type and its block) before the end.
fn model_data(cubes_n: u32, parts: &[(u8, Writer)]) -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut cubes = Writer::new();
    cubes.varint(cubes_n);
    for _ in 0..cubes_n {
        cubes.u8(0x03).varint(0); // HAS_MESH | HAS_TEXTURE, parent: the head
        cubes.var_vec3([0.0; 3]).angle([0.0; 3]);
        cubes.var_vec3([4.0; 3]).var_vec3([-2.0; 3]).varint(0).varint(0);
    }
    let mut body = Writer::new();
    body.object_block(11, &skin_type);
    for (ty, block) in parts {
        body.object_block(*ty, block);
    }
    body.object_block(21, &cubes).object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

/// `count` cubes of side `size`, each extruded (`RenderEffects.EXTRUDE`), with a 64x64 texture.
fn extruded_model(count: u32, size: f32) -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut tex = Writer::new();
    let mut block = Writer::new();
    block.bytes(&png(64, 64, 0xFF00_FF00));
    tex.i16(64).i16(64).block(&block);
    let mut cubes = Writer::new();
    cubes.varint(count);
    for _ in 0..count {
        cubes.u8(0x03).varint(0);
        cubes.var_vec3([0.0; 3]).angle([0.0; 3]);
        cubes.var_vec3([size; 3]).var_vec3([-2.0; 3]).varint(0).varint(0);
    }
    let mut body = Writer::new();
    body.object_block(11, &skin_type).object_block(5, &tex).object_block(21, &cubes);
    for i in 0..count {
        let mut e = Writer::new();
        e.u8(10).varint(10 + i);
        body.object_block(8, &e);
    }
    body.object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A PNG of only a signature and an IHDR claiming `w`x`h` RGBA (then an empty IDAT and IEND).
fn header_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |ty: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend(data);
        out.extend(&c);
        out.extend(crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[]);
    chunk(b"IEND", &[]);
    out
}

/// A model with no texture of its own: it draws with the skin.
fn cube_model() -> Vec<u8> {
    model_data(1, &[])
}

/// A solid `argb` PNG of `w`x`h`.
fn png(w: u32, h: u32, argb: u32) -> Vec<u8> {
    let mut img = Image::new(w, h);
    img.argb.fill(argb);
    img.encode_png()
}

/// A model with a `w`x`h` texture of its own (declared that size), `cubes_n` cubes.
fn textured_model(w: u32, h: u32, argb: u32, cubes_n: u32) -> Vec<u8> {
    let mut tex = Writer::new();
    let mut block = Writer::new();
    block.bytes(&png(w, h, argb));
    tex.i16(w as i16).i16(h as i16).block(&block);
    model_data(cubes_n, &[(5, tex)])
}

/// Skin data holding only a link (CPM's usual export of a big model: the package in a gist).
fn link_data(link: &str) -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut payload = Writer::new();
    payload.u8(link.len() as u8).bytes(link.as_bytes());
    let mut body = Writer::new();
    body.object_block(11, &skin_type).object_block(20, &payload).object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

/// Writes `data` (model data) as `link`'s cached gist text in `dir`.
fn cache_link(dir: &Path, link: &str, data: &[u8]) {
    let cache = cpm::cache_path(dir, link).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, cpm_core::link::gist_text(&skin::container(data)).unwrap()).unwrap();
}

/// A wide 64x64 skin PNG of colour `argb` holding `data` (none: a plain skin).
fn skin_png(argb: u32, data: Option<&[u8]>) -> Vec<u8> {
    let mut img = Image::new(64, 64);
    img.argb.fill(argb);
    if let Some(data) = data {
        let mut out = SkinDataOutputStream::new(&mut img, skin::template().unwrap(), 1);
        for &b in data {
            out.write(b).unwrap();
        }
    }
    img.encode_png()
}

/// Writes a skin file `name` in `dir`: its location is the file's path ([`file_of`]).
fn skin_file(dir: &Path, name: &str, argb: u32, data: Option<&[u8]>) -> OtherSkin {
    let file = dir.join(name);
    std::fs::write(&file, skin_png(argb, data)).unwrap();
    OtherSkin { location: file.to_string_lossy().into_owned(), slim: false }
}

fn file_of(location: &str) -> PathBuf {
    PathBuf::from(location)
}

/// As the game draws a skin: here just its pixels.
fn texture_of(png: &[u8]) -> Option<([u32; 2], Vec<u8>)> {
    Image::decode(png).ok().map(|i| ([i.w, i.h], i.rgba()))
}

const READER: cpm::SkinReader = cpm::SkinReader { file: file_of, texture: texture_of };

fn player(uuid: u128, skin: &OtherSkin) -> OtherPlayer {
    OtherPlayer { uuid, skin: Some(skin.clone()), distance: 0.0 }
}

/// [`player`] `distance` blocks away.
fn player_at(uuid: u128, skin: &OtherSkin, distance: f32) -> OtherPlayer {
    OtherPlayer { distance, ..player(uuid, skin) }
}

/// [`settle`] with the clock at `now`.
fn settle_at(players: &[OtherPlayer], now: Instant) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cpm::others_tick_at(players, &READER, now);
        if players.iter().all(|p| cpm::other_status(p.uuid) != Some(OtherStatus::Pending)) || Instant::now() > end {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Ticks until none of `players` is pending (10 s at most).
fn settle(players: &[OtherPlayer]) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cpm::others_tick(players, &READER);
        if players.iter().all(|p| cpm::other_status(p.uuid) != Some(OtherStatus::Pending)) || Instant::now() > end {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Ticks until downloads are asked for (10 s at most), and takes them.
fn requested(players: &[OtherPlayer]) -> Vec<(String, Vec<String>)> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cpm::others_tick(players, &READER);
        // Both players' loads end before the requests are looked at.
        std::thread::sleep(Duration::from_millis(20));
        cpm::others_tick(players, &READER);
        let r = cpm::take_fetch_requests();
        if !r.is_empty() || Instant::now() > end {
            return r;
        }
    }
}

/// The image's pixel at `at` (RGBA).
fn pixel(image: &(u32, u32, Vec<u8>), at: [u32; 2]) -> [u8; 4] {
    let i = ((at[1] * image.0 + at[0]) * 4) as usize;
    image.2[i..i + 4].try_into().unwrap()
}

/// The head's front in an image at `at` (skin data lives in the corners).
fn head(at: [u32; 2]) -> [u32; 2] {
    [at[0] + 8, at[1] + 8]
}

fn overlap(a: [u32; 4], b: [u32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn others_load_in_background_and_become_ready() {
    let _serial = serial();
    let dir = scratch("ready");
    cpm::set_data_dir(dir.clone());
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let plain = skin_file(&dir, "plain.png", 0xFF11_2233, None);
    let players = [player(1, &a), player(2, &plain), OtherPlayer { uuid: 3, skin: None, distance: 0.0 }];
    cpm::others_tick(&players, &READER);
    // Loading off the frame thread: pending at first, nothing to draw yet.
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Pending));
    assert_eq!(cpm::with_other_model(1, |_, _| ()), None);
    settle(&players);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    // A skin without a model, or no downloaded skin: vanilla, no error.
    assert_eq!(cpm::other_status(2), Some(OtherStatus::None));
    assert_eq!(cpm::other_status(3), None);
    // Drawn only once its texture is in the uploaded image.
    assert_eq!(cpm::with_other_model(1, |_, _| ()), None);
    let image = cpm::texture_upload(Instant::now()).expect("an upload");
    let (cubes, png, at) = cpm::with_other_model(1, |m, at| (m.model.cubes.len(), m.png_px, at)).expect("drawn");
    assert_eq!((cubes, png), (1, [64, 64]));
    assert_eq!(cpm::other_region(1), Some(at));
    // Its own copy of the skin's pixels (the head's front, clear of the data), where its region is.
    assert_eq!(pixel(&image, head(at)), [0x33, 0x66, 0x99, 0xFF]);
    assert_eq!(cpm::others_loaded(), 1);
    // The local player is untouched.
    assert!(!cpm::has_model());
    assert_eq!(cpm::last_error(), None);
    // A local skin model and the other's are separate models.
    cpm::set_skin(Some(cpm::SkinInput {
        location: "skins/local".into(),
        slim: false,
        png: skin_png(0xFF00_FF00, Some(&cube_model())),
        texture: texture_of(&skin_png(0xFF00_FF00, None)),
    }));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    let image = cpm::texture_upload(Instant::now()).expect("an upload");
    assert_eq!(pixel(&image, head([0, 0])), [0x00, 0xFF, 0x00, 0xFF]);
    let at = cpm::other_region(1).unwrap();
    assert_eq!(pixel(&image, head(at)), [0x33, 0x66, 0x99, 0xFF]);
    assert!(!overlap([0, 0, 68, 64], [at[0], at[1], 68, 64]));
    // Editing the local model's pixels never touches the other's (own textures each).
    cpm::with_model(|m| m.texture_rgba.fill(0));
    assert_eq!(cpm::with_other_model(1, |m, _| m.texture_rgba[(8 * 64 + 8) * 4]), Some(0x33));
}

#[test]
fn others_links_download_under_their_own_keys() {
    let _serial = serial();
    let dir = scratch("links");
    cpm::set_data_dir(dir.clone());
    let link = "git:otherdl/f00d";
    let linked = skin_file(&dir, "linked.png", 0xFF33_6699, Some(&link_data(link)));
    let players = [player(7, &linked), player(8, &linked)];
    let requests = requested(&players);
    // Not cached: both wait for one download, which the frame hook starts.
    assert_eq!(cpm::other_status(7), Some(OtherStatus::Pending));
    assert_eq!(cpm::other_status(8), Some(OtherStatus::Pending));
    assert_eq!(requests.len(), 1, "{requests:?}");
    let key = requests[0].0.clone();
    assert_ne!(key, link);
    assert!(key.ends_with(link));
    // Not the local player's download.
    assert!(!cpm::fetch_in_flight(link));
    let text = cpm_core::link::gist_text(&skin::container(&textured_model(64, 64, 0xFFAB_CDEF, 2))).unwrap();
    cpm::finish_fetch(&key, Ok(text.clone().into_bytes()));
    // (A load that asked for the link after the first took the text asks again: served too.)
    let end = Instant::now() + Duration::from_secs(10);
    while players.iter().any(|p| cpm::other_status(p.uuid) == Some(OtherStatus::Pending)) && Instant::now() < end {
        cpm::others_tick(&players, &READER);
        for (key, _) in cpm::take_fetch_requests() {
            cpm::finish_fetch(&key, Ok(text.clone().into_bytes()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(cpm::other_status(7), Some(OtherStatus::Loaded));
    assert_eq!(cpm::other_status(8), Some(OtherStatus::Loaded));
    assert_eq!(cpm::with_other_model(7, |m, _| m.model.cubes.len()), None);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::with_other_model(7, |m, _| m.model.cubes.len()), Some(2));
    // Cached for next time.
    assert_eq!(std::fs::read(cpm::cache_path(&dir, link).unwrap()).unwrap(), text.into_bytes());
    // A downloaded text past the cap (the fetcher stops there; refused here too).
    let over = "git:otherdl/huge2";
    let huge = skin_file(&dir, "huge.png", 0xFF33_6699, Some(&link_data(over)));
    let key = requested(&[player(10, &huge)]).pop().unwrap().0;
    cpm::finish_fetch(&key, Ok(vec![b'A'; cpm::OTHER_MAX_LINK_BYTES + 1]));
    settle(&[player(10, &huge)]);
    match cpm::other_status(10) {
        Some(OtherStatus::Error(e)) => assert!(e.contains("100 KiB"), "{e}"),
        s => panic!("{s:?}"),
    }
    // A failed download: an error line, vanilla.
    let other = "git:otherdl/bad1";
    let bad = skin_file(&dir, "bad.png", 0xFF33_6699, Some(&link_data(other)));
    let key = requested(&[player(9, &bad)]).pop().unwrap().0;
    cpm::finish_fetch(&key, Err("404".into()));
    settle(&[player(9, &bad)]);
    match cpm::other_status(9) {
        Some(OtherStatus::Error(e)) => assert!(e.contains(other) && e.contains("404"), "{e}"),
        s => panic!("{s:?}"),
    }
}

#[test]
fn others_dropped_when_absent() {
    let _serial = serial();
    let dir = scratch("absent");
    cpm::set_data_dir(dir.clone());
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let b = skin_file(&dir, "b.png", 0xFF99_6633, Some(&cube_model()));
    let both = [player(1, &a), player(2, &b)];
    settle(&both);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::others_loaded(), 2);
    // Player 1 leaves.
    cpm::others_tick(&both[1..], &READER);
    assert_eq!(cpm::other_status(1), None);
    assert_eq!(cpm::with_other_model(1, |_, _| ()), None);
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::others_loaded(), 1);
    // A load still running for a player who left is dropped; one who comes back loads afresh.
    cpm::others_tick(&[], &READER);
    assert_eq!(cpm::other_status(2), None);
    cpm::others_tick(&both[..1], &READER);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Pending));
    cpm::others_tick(&[], &READER);
    assert_eq!(cpm::other_status(1), None);
    settle(&both[..1]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    // Everyone gone (out of the world): nothing left, nothing for others in the image.
    cpm::others_tick(&[], &READER);
    assert_eq!(cpm::texture_upload(Instant::now()), None);
    assert_eq!(cpm::others_loaded(), 0);
}

#[test]
fn others_reload_on_skin_change() {
    let _serial = serial();
    let dir = scratch("change");
    cpm::set_data_dir(dir.clone());
    let one = skin_file(&dir, "one.png", 0xFF33_6699, Some(&cube_model()));
    settle(&[player(1, &one)]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    cpm::texture_upload(Instant::now()).unwrap();
    // A new skin without a model: the old model goes at once, then vanilla.
    let plain = skin_file(&dir, "plain.png", 0xFF11_2233, None);
    cpm::others_tick(&[player(1, &plain)], &READER);
    assert_eq!(cpm::with_other_model(1, |_, _| ()), None);
    settle(&[player(1, &plain)]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::None));
    // Another with a model: loaded from it.
    let two = skin_file(&dir, "two.png", 0xFF99_0000, Some(&cube_model()));
    settle(&[player(1, &two)]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    let image = cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(pixel(&image, head(cpm::other_region(1).unwrap())), [0x99, 0x00, 0x00, 0xFF]);
    // The same file read as slim is a new skin too.
    let slim = OtherSkin { slim: true, ..two.clone() };
    cpm::others_tick(&[player(1, &slim)], &READER);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Pending));
}

#[test]
fn others_capped_by_setting() {
    let _serial = serial();
    let dir = scratch("cap");
    cpm::set_data_dir(dir.clone());
    assert_eq!(cpm::max_others(None), cpm::DEFAULT_MAX_OTHERS);
    assert_eq!(cpm::DEFAULT_MAX_OTHERS, 8);
    assert_eq!(cpm::max_others(Some("3".into())), 3);
    assert_eq!(cpm::max_others(Some("junk".into())), 8);
    assert_eq!(cpm::max_others(Some("0".into())), 1);
    assert_eq!(cpm::max_others(Some("1000".into())), 32);
    assert_eq!(cpm::next_max_others(8), 16);
    assert_eq!(cpm::next_max_others(32), 1);
    assert_eq!(cpm::next_max_others(5), 8);
    cpm::set_max_others(2);
    let skins: Vec<OtherSkin> =
        (0..4).map(|i| skin_file(&dir, &format!("{i}.png"), 0xFF00_0000 | i * 40, Some(&cube_model()))).collect();
    let players: Vec<OtherPlayer> = skins.iter().enumerate().map(|(i, s)| player(10 + i as u128, s)).collect();
    settle(&players);
    let loaded = |ids: &[u128]| ids.iter().filter(|u| cpm::other_status(**u) == Some(OtherStatus::Loaded)).count();
    // The first two in order; the rest vanilla, not even read.
    assert_eq!(cpm::other_status(10), Some(OtherStatus::Loaded));
    assert_eq!(cpm::other_status(11), Some(OtherStatus::Loaded));
    assert_eq!(cpm::other_status(12), None);
    assert_eq!(cpm::other_status(13), None);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::others_loaded(), 2);
    // More allowed: the rest load.
    cpm::set_max_others(4);
    settle(&players);
    assert_eq!(loaded(&[10, 11, 12, 13]), 4);
    // Fewer: the last ones go.
    cpm::set_max_others(1);
    cpm::others_tick(&players, &READER);
    assert_eq!(loaded(&[10, 11, 12, 13]), 1);
    assert_eq!(cpm::other_status(10), Some(OtherStatus::Loaded));
    // A skin without a model takes no place once read.
    cpm::set_max_others(2);
    let plain = skin_file(&dir, "plain.png", 0xFF11_2233, None);
    let mixed = [player(20, &plain), player(21, &skins[0]), player(22, &skins[1])];
    settle(&mixed);
    settle(&mixed);
    assert_eq!(cpm::other_status(20), Some(OtherStatus::None));
    assert_eq!(loaded(&[21, 22]), 2);
}

#[test]
fn others_off_setting_draws_none() {
    let _serial = serial();
    let dir = scratch("off");
    cpm::set_data_dir(dir.clone());
    assert!(cpm::others_on(None));
    assert!(cpm::others_on(Some("on".into())));
    assert!(!cpm::others_on(Some("off".into())));
    assert_eq!(cpm::others_setting(false), "off");
    assert_eq!(cpm::others_setting(true), "on");
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    settle(&[player(1, &a)]);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::others_loaded(), 1);
    cpm::set_others_on(false);
    cpm::others_tick(&[player(1, &a)], &READER);
    assert_eq!(cpm::other_status(1), None);
    assert_eq!(cpm::with_other_model(1, |_, _| ()), None);
    assert_eq!(cpm::texture_upload(Instant::now()), None);
    assert_eq!(cpm::others_loaded(), 0);
    // The settings page: the two buttons after Gestures…, then the count.
    let idle = cpm::ConvertView { busy: false, last: None, limit_kib: 8, old_conversion: false };
    let view = cpm::OthersView { on: false, max: 8, loaded: 0, no_room: 0 };
    let labels = cpm::settings_page(None, None, &idle, &view);
    assert_eq!(labels[..5], cpm::settings_labels(None, None, &idle)[..]);
    assert_eq!(labels[5], "Other players' models: Off");
    assert_eq!(labels[6], "Max other models: 8");
    assert_eq!(labels.len(), 7);
    let view = cpm::OthersView { on: true, max: 4, loaded: 3, no_room: 1 };
    let labels = cpm::settings_page(None, Some("cpm: x: bad"), &idle, &view);
    assert_eq!(labels[5], "Other players' models: On");
    assert_eq!(labels[6], "Max other models: 4");
    assert_eq!(labels[7], "Other players' models loaded: 3 (1 without room)");
    assert_eq!(labels.last().unwrap(), "Error: cpm: x: bad");
    assert_eq!(cpm::click(5, &[], None), cpm::Click::OthersToggle);
    assert_eq!(cpm::click(6, &[], None), cpm::Click::MaxOthers);
    assert_eq!(cpm::click(7, &[], None), cpm::Click::Nothing);
    // On again: loaded again.
    cpm::set_others_on(true);
    settle(&[player(1, &a)]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    assert_eq!(cpm::others_view().on, true);
}

#[test]
fn oversized_model_refused() {
    let _serial = serial();
    let dir = scratch("oversized");
    cpm::set_data_dir(dir.clone());
    // A texture past CPM's limit for other players (MAX_TEX_SHEET_SIZE, 256).
    assert_eq!(cpm::OTHER_MAX_TEXTURE_PX, 256);
    cache_link(&dir, "git:big/tex1", &textured_model(512, 512, 0xFFFF_0000, 1));
    let big = skin_file(&dir, "big.png", 0xFF33_6699, Some(&link_data("git:big/tex1")));
    // More cubes than CPM's limit (MAX_CUBE_COUNT, 256).
    cache_link(&dir, "git:big/cubes1", &textured_model(64, 64, 0xFF00_FF00, 300));
    let cubes = skin_file(&dir, "cubes.png", 0xFF33_6699, Some(&link_data("git:big/cubes1")));
    // A link text past CPM's MAX_LINK_SIZE for other players (100 KB).
    let cache = cpm::cache_path(&dir, "git:big/data1").unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, vec![b'A'; cpm::OTHER_MAX_LINK_BYTES + 1]).unwrap();
    let data = skin_file(&dir, "data.png", 0xFF33_6699, Some(&link_data("git:big/data1")));
    // Within the limits: 256 a side, from the cache.
    cache_link(&dir, "git:ok/tex2", &textured_model(256, 256, 0xFF00_00FF, 1));
    let ok = skin_file(&dir, "ok.png", 0xFF33_6699, Some(&link_data("git:ok/tex2")));
    // A texture whose PNG header claims a huge image: refused from the header, nothing allocated
    // (an attempt would abort the game).
    let mut huge = Writer::new();
    let mut block = Writer::new();
    block.bytes(&header_png(16_000, 2_000_000_000));
    huge.i16(64).i16(64).block(&block);
    cache_link(&dir, "git:big/huge1", &model_data(1, &[(5, huge)]));
    let huge = skin_file(&dir, "huge.png", 0xFF33_6699, Some(&link_data("git:big/huge1")));
    // Extruded cubes count as CPM counts them (BoxRender.getExtrudeSize): 2 cubes of 16, 386.
    cache_link(&dir, "git:big/extrude1", &extruded_model(2, 16.0));
    let extruded = skin_file(&dir, "extruded.png", 0xFF33_6699, Some(&link_data("git:big/extrude1")));
    let players = [player(1, &big), player(2, &cubes), player(3, &data), player(4, &ok), player(5, &huge), player(6, &extruded)];
    settle(&players);
    for (uuid, what) in [(1, "512x512 is too big"), (2, "cubes"), (3, "100 KiB"), (5, "16000x2000000000"), (6, "386 cubes")] {
        match cpm::other_status(uuid) {
            Some(OtherStatus::Error(e)) => assert!(e.contains(what), "{uuid}: {e}"),
            s => panic!("{uuid}: {s:?}"),
        }
    }
    assert_eq!(cpm::other_status(4), Some(OtherStatus::Loaded));
    let image = cpm::texture_upload(Instant::now()).unwrap();
    for uuid in [1, 2, 3, 5, 6] {
        assert_eq!(cpm::with_other_model(uuid, |_, _| ()), None);
    }
    let at = cpm::other_region(4).unwrap();
    assert_eq!(pixel(&image, at), [0x00, 0x00, 0xFF, 0xFF]);
    assert_eq!(cpm::others_loaded(), 1);
    // A refused model is never retried while the skin stays, and costs no place; nor when the
    // player comes back with it (no new load, no new log line).
    cpm::others_tick(&players, &READER);
    assert!(matches!(cpm::other_status(1), Some(OtherStatus::Error(_))));
    cpm::others_tick(&players[1..], &READER);
    assert_eq!(cpm::other_status(1), None);
    cpm::others_tick(&players, &READER);
    assert!(matches!(cpm::other_status(1), Some(OtherStatus::Error(_))));
    // The texts other players' links download are capped while downloading; the local player's
    // keep the gist fetcher's 16 MiB.
    assert_eq!(cpm::OTHER_MAX_LINK_BYTES, 100 * 1024);
    assert_eq!(cpm::fetch_limit("others|git:a/b"), cpm::OTHER_MAX_LINK_BYTES as u64);
    assert_eq!(cpm::fetch_limit("git:a/b"), 16 * 1024 * 1024);
    // The local player has no error of theirs.
    assert_eq!(cpm::last_error(), None);
}

#[test]
fn texture_budget_separate_regions() {
    let _serial = serial();
    let dir = scratch("budget");
    cpm::set_data_dir(dir.clone());
    cpm::set_max_others(32);
    // Two models with textures of their own, and two drawn with their skins.
    cache_link(&dir, "git:two/red", &textured_model(128, 64, 0xFFFF_0000, 1));
    cache_link(&dir, "git:two/blue", &textured_model(64, 128, 0xFF00_00FF, 1));
    let red = skin_file(&dir, "red.png", 0xFF11_1111, Some(&link_data("git:two/red")));
    let blue = skin_file(&dir, "blue.png", 0xFF22_2222, Some(&link_data("git:two/blue")));
    let green = skin_file(&dir, "green.png", 0xFF00_FF00, Some(&cube_model()));
    let grey = skin_file(&dir, "grey.png", 0xFF80_8080, Some(&cube_model()));
    let four = [player(1, &red), player(2, &blue), player(3, &green), player(4, &grey)];
    settle(&four);
    let image = cpm::texture_upload(Instant::now()).unwrap();
    let rects: Vec<[u32; 4]> = [(1, [128 + 4, 64]), (2, [64 + 4, 128]), (3, [68, 64]), (4, [68, 64])]
        .iter()
        .map(|(u, [w, h])| {
            let at = cpm::other_region(*u).unwrap_or_else(|| panic!("{u} placed"));
            [at[0], at[1], *w, *h]
        })
        .collect();
    for (i, a) in rects.iter().enumerate() {
        assert!(a[0] + a[2] <= cpm::SPRITE_PX && a[1] + a[3] <= cpm::SPRITE_PX, "{a:?}");
        for b in &rects[i + 1..] {
            assert!(!overlap(*a, *b), "{a:?} {b:?}");
        }
    }
    // Each region holds its own model's pixels, the white patch to its right.
    for (r, colour) in rects.iter().zip([[0xFF, 0, 0, 0xFF], [0, 0, 0xFF, 0xFF], [0, 0xFF, 0, 0xFF], [0x80, 0x80, 0x80, 0xFF]]) {
        assert_eq!(pixel(&image, head([r[0], r[1]])), colour, "{r:?}");
        assert_eq!(pixel(&image, [r[0] + r[2] - 1, r[1]]), [0xFF; 4], "{r:?}");
    }
    // Quads of each sample their own region only.
    for (uuid, r) in [1u128, 2, 3, 4].iter().zip(&rects) {
        let quads = cpm::with_other_model(*uuid, |m, at| {
            let posed = m.pose_with_items(&cpm_core::PoseInput::default());
            cpm::box_quads_at(m, &posed.boxes, &cpm::Worn::default(), at)
        })
        .unwrap();
        assert!(!quads.quads.is_empty());
        let px = cpm::SPRITE_PX as f32 / 16.0;
        for q in &quads.quads {
            let [u0, v0, u1, v1] = q.uv.map(|t| t * px);
            for (u, v) in [(u0, v0), (u1, v1)] {
                assert!(u >= r[0] as f32 - 0.01 && u <= (r[0] + r[2]) as f32 + 0.01, "{uuid}: u {u} {r:?}");
                assert!(v >= r[1] as f32 - 0.01 && v <= (r[1] + r[3]) as f32 + 0.01, "{uuid}: v {v} {r:?}");
            }
        }
    }
    // A full budget: 256x256 models take 260x256 each, 12 fit in 1024x1024 (3 a row, 4 rows);
    // the rest are loaded but drawn vanilla, and the count says so.
    cache_link(&dir, "git:full/big", &textured_model(256, 256, 0xFF12_3456, 1));
    let big: Vec<OtherPlayer> =
        (0..14).map(|i| player(100 + i, &skin_file(&dir, &format!("big{i}.png"), 0xFF33_6699, Some(&link_data("git:full/big"))))).collect();
    settle(&big);
    cpm::texture_upload(Instant::now()).unwrap();
    let placed: Vec<u128> = big.iter().map(|p| p.uuid).filter(|u| cpm::other_region(*u).is_some()).collect();
    assert_eq!(placed.len(), 12);
    assert!(big.iter().all(|p| cpm::other_status(p.uuid) == Some(OtherStatus::Loaded)));
    assert_eq!(cpm::others_loaded(), 12);
    assert_eq!(cpm::others_view().no_room, 2);
    let vanilla = big.iter().find(|p| !placed.contains(&p.uuid)).unwrap().uuid;
    assert_eq!(cpm::with_other_model(vanilla, |_, _| ()), None);
    // One leaves: a waiting one takes its place, and the others keep theirs.
    let kept: Vec<(u128, [u32; 2])> = placed[1..].iter().map(|u| (*u, cpm::other_region(*u).unwrap())).collect();
    let rest: Vec<OtherPlayer> = big.iter().filter(|p| p.uuid != placed[0]).cloned().collect();
    cpm::others_tick(&rest, &READER);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::others_loaded(), 12);
    for (u, at) in kept {
        assert_eq!(cpm::other_region(u), Some(at));
    }
}

#[test]
fn others_packed_to_keep_the_upload_small() {
    let _serial = serial();
    let dir = scratch("small");
    cpm::set_data_dir(dir.clone());
    // The local skin model (68x64 at the top left) and two others drawn with their skins.
    cpm::set_skin(Some(cpm::SkinInput {
        location: "skins/local".into(),
        slim: false,
        png: skin_png(0xFF00_FF00, Some(&cube_model())),
        texture: texture_of(&skin_png(0xFF00_FF00, None)),
    }));
    cpm::ensure_loaded();
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let b = skin_file(&dir, "b.png", 0xFF99_6633, Some(&cube_model()));
    let two = [player(1, &a), player(2, &b)];
    settle(&two);
    let (w, h, _) = cpm::texture_upload(Instant::now()).unwrap();
    // Beside the local image, not down the sprite: 3 x 68 wide, 64 tall.
    assert_eq!((w, h), (3 * 68, 64));
    assert_eq!(cpm::others_loaded(), 2);
    // A bigger one goes where the image (sent whole) grows least: under them, 260x320, not beside
    // them (464x256).
    cache_link(&dir, "git:small/big", &textured_model(256, 256, 0xFF12_3456, 1));
    let big = skin_file(&dir, "big.png", 0xFF33_6699, Some(&link_data("git:small/big")));
    let three = [player(1, &a), player(2, &b), player(3, &big)];
    settle(&three);
    let (w, h, _) = cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!((w, h), (260, 64 + 256));
}

#[test]
fn remote_pose_input_mapping() {
    let _serial = serial();
    use cpm_core::{CapeState, Hand, UseKind};
    let base = cpm::RemoteState { time_ms: 1234, air: 300, ..Default::default() };
    let i = cpm::remote_input(&base);
    // The unknowns read as off: no hunger frame, no creative flight, never falling; no health known.
    assert_eq!((i.time_ms, i.hunger, i.health, i.light), (1234, None, None, None));
    assert!(!i.creative_flying && !i.falling && !i.hurt && !i.dying && !i.sneaking);
    assert_eq!(i.air, Some(1.0));
    assert_eq!(i.using, None);
    assert_eq!(i.attack, None);
    assert_eq!(i.equipment, cpm_core::Equipment::default());
    // Everything the game knows of them.
    let cape = CapeState { flap: 0.5, lean: 0.25, lean2: 0.125 };
    let s = cpm::RemoteState {
        time_ms: 99,
        walk_position: 3.5,
        walk_speed: 0.75,
        head_yaw: 20.0,
        pitch: -10.0,
        age: 77.5,
        on_ground: true,
        hurt_time: 8,
        dead: false,
        health: Some(0.25),
        crouching: true,
        sprinting: true,
        visually_swimming: true,
        fall_flying: true,
        sleeping: true,
        riding: true,
        on_fire: true,
        spin_attack: true,
        in_water: false,
        ticks_frozen: 70,
        air: 150,
        on_climbable: true,
        dy: 0.1,
        left_handed: true,
        swing: Some((Hand::Off, 0.4)),
        swinging: true,
        using: Some(cpm::Use { hand: Hand::Main, kind: Some(UseKind::Bow), ticks: 10, pull: cpm::Pull::Bow }),
        holding: [true, true],
        charged: [false, true],
        light: Some([3, 12]),
        cape: Some(cape),
        invisible: true,
        parrots: [true, false],
    };
    let i = cpm::remote_input(&s);
    assert_eq!(i.time_ms, 99);
    assert_eq!((i.walk_position, i.walk_speed, i.head_yaw, i.pitch, i.age), (3.5, 0.75, 20.0, -10.0, 77.5));
    assert!(i.on_ground && i.sneaking && i.sprinting && i.swimming && i.fall_flying && i.sleeping && i.riding && i.on_fire);
    assert!(i.hurt && !i.dying);
    // Health is synced: its value plays.
    assert_eq!(i.health, Some(0.25));
    assert!(i.trident_spin && i.on_ladder && i.climbing);
    // Swimming out of water is crawling.
    assert!(i.crawling && !i.in_water);
    assert_eq!(i.freezing, 0.5);
    assert_eq!(i.air, Some(0.5));
    assert!(i.left_handed);
    assert_eq!(i.attack, Some((Hand::Off, 0.4)));
    assert_eq!(i.using, Some((Hand::Main, UseKind::Bow, 0.5)));
    assert_eq!(i.holding, [true, true]);
    // A charged crossbow is not held up while swinging.
    assert_eq!(i.crossbow_charged, [false, false]);
    assert_eq!(i.light, Some(12.0 / 15.0));
    assert_eq!(i.cape, Some(cape));
    assert!(i.invisible && i.parrot_left && !i.parrot_right);
    // Still unknown: hunger.
    assert_eq!(i.hunger, None);
    // In water: swimming, not crawling; dying once dead.
    let i = cpm::remote_input(&cpm::RemoteState { in_water: true, dead: true, swinging: false, ..s });
    assert!(i.swimming && !i.crawling && i.in_water && i.dying);
    assert_eq!(i.crossbow_charged, [false, true]);
}

#[test]
fn two_models_one_frame_separate_textures() {
    let _serial = serial();
    let dir = scratch("two_frame");
    cpm::set_data_dir(dir.clone());
    cache_link(&dir, "git:frame/red", &textured_model(64, 64, 0xFFFF_0000, 1));
    cache_link(&dir, "git:frame/blue", &textured_model(64, 64, 0xFF00_00FF, 2));
    let red = skin_file(&dir, "red.png", 0xFF11_1111, Some(&link_data("git:frame/red")));
    let blue = skin_file(&dir, "blue.png", 0xFF22_2222, Some(&link_data("git:frame/blue")));
    let players = [player(1, &red), player(2, &blue)];
    settle(&players);
    // Nothing draws before the image holding them reached the game.
    let input = cpm::remote_input(&cpm::RemoteState { time_ms: 50, air: 300, ..Default::default() });
    assert_eq!(cpm::other_quads(1, &input, &cpm::Worn::default()), None);
    let image = cpm::texture_upload(Instant::now()).unwrap();
    // One frame: both drawn, each from its own model and its own region of the image.
    let a = cpm::other_quads(1, &input, &cpm::Worn::default()).expect("1 drawn");
    let b = cpm::other_quads(2, &input, &cpm::Worn::default()).expect("2 drawn");
    // (The skin's own parts, then each model's cubes: one, and two.)
    assert_eq!(b.quads.len(), a.quads.len() + 6);
    let (ra, rb) = (cpm::other_region(1).unwrap(), cpm::other_region(2).unwrap());
    assert!(!overlap([ra[0], ra[1], 68, 64], [rb[0], rb[1], 68, 64]));
    let px = cpm::SPRITE_PX as f32 / 16.0;
    for (quads, r, colour) in [(&a, ra, [0xFF, 0, 0, 0xFF]), (&b, rb, [0, 0, 0xFF, 0xFF])] {
        for q in &quads.quads {
            assert_eq!(q.sprite, 0);
            let [u0, v0, u1, v1] = q.uv.map(|t| t * px);
            for (u, v) in [(u0, v0), (u1, v1)] {
                assert!(u >= r[0] as f32 - 0.01 && u <= (r[0] + 68) as f32 + 0.01, "u {u} {r:?}");
                assert!(v >= r[1] as f32 - 0.01 && v <= (r[1] + 64) as f32 + 0.01, "v {v} {r:?}");
            }
            // The texel inside the face's corner is that model's colour.
            let at = [(u0.min(u1) + 0.5) as u32, (v0.min(v1) + 0.5) as u32];
            assert_eq!(pixel(&image, at), colour, "{at:?}");
        }
    }
    // The local player has no model: no quads, whatever the others draw.
    assert_eq!(cpm::quads(&input, &cpm::Worn::default()), None);
    // Drawing one never changes the other's.
    let b2 = cpm::other_quads(2, &input, &cpm::Worn::default()).unwrap();
    assert_eq!(b2, b);
}

#[test]
fn local_path_unchanged() {
    let _serial = serial();
    let dir = scratch("local_same");
    cpm::set_data_dir(dir.clone());
    cpm::set_skin(Some(cpm::SkinInput {
        location: "skins/local".into(),
        slim: false,
        png: skin_png(0xFF00_FF00, Some(&cube_model())),
        texture: texture_of(&skin_png(0xFF00_FF00, None)),
    }));
    cpm::ensure_loaded();
    cpm::texture_upload(Instant::now()).unwrap();
    let input = cpm_core::PoseInput { time_ms: 10, ..Default::default() };
    let before = cpm::quads(&input, &cpm::Worn::default()).expect("local drawn");
    // Others come, load and draw: the local player's quads are the same, at the top left.
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let b = skin_file(&dir, "b.png", 0xFF99_6633, Some(&cube_model()));
    let players = [player(1, &a), player(2, &b)];
    settle(&players);
    cpm::texture_upload(Instant::now()).unwrap();
    assert!(cpm::other_quads(1, &input, &cpm::Worn::default()).is_some());
    assert!(cpm::other_quads(2, &input, &cpm::Worn::default()).is_some());
    let after = cpm::quads(&input, &cpm::Worn::default()).expect("local drawn");
    assert_eq!(before, after);
    let px = cpm::SPRITE_PX as f32 / 16.0;
    for q in &after.quads {
        assert!(q.uv.iter().all(|t| t * px <= 68.0 + 0.01), "{:?}", q.uv);
    }
    // And when they go.
    cpm::others_tick(&[], &READER);
    cpm::texture_upload(Instant::now());
    assert_eq!(cpm::quads(&input, &cpm::Worn::default()), Some(before));
}

#[test]
fn others_cap_keeps_the_nearest() {
    let _serial = serial();
    let dir = scratch("nearest");
    cpm::set_data_dir(dir.clone());
    cpm::set_max_others(1);
    let far = skin_file(&dir, "far.png", 0xFF33_6699, Some(&cube_model()));
    let near = skin_file(&dir, "near.png", 0xFF99_6633, Some(&cube_model()));
    let start = Instant::now();
    settle_at(&[player_at(1, &far, 20.0)], start);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    let players = [player_at(2, &near, 2.0), player_at(1, &far, 20.0)];
    // Not while the far one's model has just loaded (it keeps its place a while).
    cpm::others_tick_at(&players, &READER, start + Duration::from_secs(1));
    assert_eq!(cpm::other_status(2), None);
    // Then the nearer player loads as a trial; the far one draws on meanwhile.
    let later = start + cpm::OTHER_MIN_HOLD + Duration::from_secs(1);
    cpm::others_tick_at(&players, &READER, later);
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Pending));
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    assert!(cpm::other_shown(1));
    // Once it has a model, it takes the one place.
    settle_at(&players, later);
    cpm::others_tick_at(&players, &READER, later);
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
    assert_eq!(cpm::other_status(1), None);
    // Not nearer by the margin: no trial.
    let close = [player_at(1, &far, 2.0 + cpm::OTHER_SWAP_MARGIN - 0.5), player_at(2, &near, 2.0 + cpm::OTHER_SWAP_MARGIN)];
    cpm::others_tick_at(&close, &READER, later + cpm::OTHER_MIN_HOLD * 2);
    assert_eq!(cpm::other_status(1), None);
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
}

#[test]
fn others_cap_does_not_thrash_at_the_edge() {
    let _serial = serial();
    let dir = scratch("edge");
    cpm::set_data_dir(dir.clone());
    cpm::set_max_others(1);
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let b = skin_file(&dir, "b.png", 0xFF99_6633, Some(&cube_model()));
    let start = Instant::now();
    settle_at(&[player_at(1, &a, 10.0), player_at(2, &b, 10.5)], start);
    cpm::texture_upload(Instant::now()).unwrap();
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    assert_eq!(cpm::other_status(2), None);
    // Walking together, their order flipping each frame, long past the hold: the one drawn stays,
    // the other never loads.
    for i in 0..200u64 {
        let now = start + Duration::from_secs(i);
        let (d1, d2) = if i % 2 == 0 { (10.5, 10.0) } else { (10.0, 10.5) };
        let mut players = vec![player_at(1, &a, d1), player_at(2, &b, d2)];
        players.sort_by(|x, y| x.distance.total_cmp(&y.distance));
        cpm::others_tick_at(&players, &READER, now);
        assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded), "{i}");
        assert_eq!(cpm::other_status(2), None, "{i}");
    }
    // Nothing came or went: no new image to send.
    assert_eq!(cpm::texture_upload(Instant::now()), None);
    assert!(cpm::other_shown(1));
}

#[test]
fn nearer_vanilla_player_keeps_the_loaded_models() {
    let _serial = serial();
    let dir = scratch("vanilla_near");
    cpm::set_data_dir(dir.clone());
    cpm::set_max_others(2);
    let a = skin_file(&dir, "a.png", 0xFF33_6699, Some(&cube_model()));
    let b = skin_file(&dir, "b.png", 0xFF99_6633, Some(&cube_model()));
    let plain = skin_file(&dir, "plain.png", 0xFF11_2233, None);
    let start = Instant::now();
    let two = [player_at(1, &a, 10.0), player_at(2, &b, 30.0)];
    settle_at(&two, start);
    cpm::texture_upload(Instant::now()).unwrap();
    let (r1, r2) = (cpm::other_region(1).unwrap(), cpm::other_region(2).unwrap());
    // A player with a plain skin walks up: read (as a trial), found to hold no model, and neither
    // loaded model loses its place (nor ever reloads).
    let later = start + cpm::OTHER_MIN_HOLD * 2;
    let three = [player_at(3, &plain, 1.0), player_at(1, &a, 10.0), player_at(2, &b, 30.0)];
    let end = Instant::now() + Duration::from_secs(10);
    while cpm::other_status(3) != Some(OtherStatus::None) {
        assert!(Instant::now() < end, "read");
        cpm::others_tick_at(&three, &READER, later);
        assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
        assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..20 {
        cpm::others_tick_at(&three, &READER, later);
    }
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
    assert_eq!(cpm::texture_upload(Instant::now()), None);
    assert_eq!((cpm::other_region(1), cpm::other_region(2)), (Some(r1), Some(r2)));
    // Players with no downloaded skin take no place either.
    let none = OtherPlayer { uuid: 4, skin: None, distance: 0.5 };
    cpm::others_tick_at(&[none, three[0].clone(), three[1].clone(), three[2].clone()], &READER, later);
    assert_eq!(cpm::other_status(4), None);
    assert_eq!(cpm::other_status(2), Some(OtherStatus::Loaded));
}

#[test]
fn texture_declared_size_is_not_capped() {
    let _serial = serial();
    // CPM (`TextureProvider`) caps the image's own size, not the size the model declares.
    let dir = scratch("declared");
    cpm::set_data_dir(dir.clone());
    let mut tex = Writer::new();
    let mut block = Writer::new();
    block.bytes(&png(64, 64, 0xFF00_FF00));
    tex.i16(512).i16(512).block(&block);
    cache_link(&dir, "git:declared/a", &model_data(1, &[(5, tex)]));
    let s = skin_file(&dir, "d.png", 0xFF33_6699, Some(&link_data("git:declared/a")));
    settle(&[player(1, &s)]);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
}

/// Ticks at `now` until a download is asked for (10 s at most) and fails it with `reason`, then
/// until the load has failed.
fn fail_next_download(players: &[OtherPlayer], now: Instant, reason: &str) {
    let end = Instant::now() + Duration::from_secs(10);
    let key = loop {
        cpm::others_tick_at(players, &READER, now);
        if let Some((k, _)) = cpm::take_fetch_requests().pop() {
            break k;
        }
        assert!(Instant::now() < end, "a download is asked for");
        std::thread::sleep(Duration::from_millis(5));
    };
    cpm::finish_fetch(&key, Err(reason.into()));
    while !matches!(cpm::other_status(players[0].uuid), Some(OtherStatus::Error(_))) {
        assert!(Instant::now() < end, "failed");
        cpm::others_tick_at(players, &READER, now);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn failed_downloads_retried_later() {
    let _serial = serial();
    let dir = scratch("retry");
    cpm::set_data_dir(dir.clone());
    let s = skin_file(&dir, "r.png", 0xFF33_6699, Some(&link_data("git:retry/net1")));
    let players = [player(1, &s)];
    let start = Instant::now();
    fail_next_download(&players, start, "Network Error: timed out reading response");
    // Not before the delay, nor when they come back meanwhile.
    cpm::others_tick_at(&players, &READER, start + Duration::from_secs(1));
    assert!(matches!(cpm::other_status(1), Some(OtherStatus::Error(_))));
    cpm::others_tick_at(&[], &READER, start + Duration::from_secs(2));
    cpm::others_tick_at(&players, &READER, start + Duration::from_secs(3));
    assert!(matches!(cpm::other_status(1), Some(OtherStatus::Error(_))));
    std::thread::sleep(Duration::from_millis(50));
    cpm::others_tick_at(&players, &READER, start + Duration::from_secs(3));
    assert!(cpm::take_fetch_requests().is_empty());
    // After it: loaded again (the download asked for again), OTHER_TRIES times in all.
    let mut later = start;
    for _ in 1..cpm::OTHER_TRIES {
        later += cpm::OTHER_RETRY_DELAY + Duration::from_secs(1);
        cpm::others_tick_at(&players, &READER, later);
        assert_eq!(cpm::other_status(1), Some(OtherStatus::Pending));
        fail_next_download(&players, later, "Network Error: no route");
    }
    // Then failed for the session.
    later += cpm::OTHER_RETRY_DELAY * 10;
    cpm::others_tick_at(&players, &READER, later);
    assert!(matches!(cpm::other_status(1), Some(OtherStatus::Error(_))));
    std::thread::sleep(Duration::from_millis(50));
    cpm::others_tick_at(&players, &READER, later);
    assert!(cpm::take_fetch_requests().is_empty());
    // A download that comes back once the network is back: loaded.
    let ok = skin_file(&dir, "ok.png", 0xFF33_6699, Some(&link_data("git:retry/net2")));
    let players = [player(3, &ok)];
    fail_next_download(&players, start, "Network Error: timed out");
    let later = start + cpm::OTHER_RETRY_DELAY + Duration::from_secs(1);
    let text = cpm_core::link::gist_text(&skin::container(&textured_model(64, 64, 0xFFAB_CDEF, 1))).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while cpm::other_status(3) != Some(OtherStatus::Loaded) {
        assert!(Instant::now() < end, "loaded on retry");
        cpm::others_tick_at(&players, &READER, later);
        for (key, _) in cpm::take_fetch_requests() {
            cpm::finish_fetch(&key, Ok(text.clone().into_bytes()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // A model's own fault (past a limit) is never retried.
    cache_link(&dir, "git:retry/bad", &textured_model(512, 512, 0xFFFF_0000, 1));
    let bad = skin_file(&dir, "bad.png", 0xFF33_6699, Some(&link_data("git:retry/bad")));
    let players = [player(2, &bad)];
    settle(&players);
    assert!(matches!(cpm::other_status(2), Some(OtherStatus::Error(_))));
    cpm::others_tick_at(&players, &READER, Instant::now() + cpm::OTHER_RETRY_DELAY * 2);
    assert!(matches!(cpm::other_status(2), Some(OtherStatus::Error(_))));
}
