// Models shared in Horizon-hosted worlds (multiplayer phase 2): the `cpm:sync` messages, the host's
// relay, what a client sends of its chosen model, and the models received overriding the skin's.
// Each test runs on its own thread, so its own state.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cpm_core::skin::{self, Image, SkinDataOutputStream};
use cpm_core::write::{Writer, checksummed};
use horizon_mod_cpm as cpm;
use cpm::{ModelKind, NetShare, OtherPlayer, OtherSkin, OtherStatus, Reassembly, Relay, SharePlan, SyncMessage};

/// The tests run one at a time: the link downloads and loading jobs are the process's.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sync_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A solid `argb` PNG of `w`x`h`.
fn png(w: u32, h: u32, argb: u32) -> Vec<u8> {
    let mut img = Image::new(w, h);
    img.argb.fill(argb);
    img.encode_png()
}

/// A PNG of `w`x`h` noise: it does not compress, so the model is big.
fn noise_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = Image::new(w, h);
    let mut x = 0x1234_5678u32;
    for p in img.argb.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *p = x | 0xFF00_0000;
    }
    img.encode_png()
}

/// Model data: `cubes_n` textured head cubes and a texture of its own (`texture`, declared `w`x`h`).
fn model_data(cubes_n: u32, texture: Option<(u32, u32, Vec<u8>)>) -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut cubes = Writer::new();
    cubes.varint(cubes_n);
    for _ in 0..cubes_n {
        cubes.u8(0x03).varint(0);
        cubes.var_vec3([0.0; 3]).angle([0.0; 3]);
        cubes.var_vec3([4.0; 3]).var_vec3([-2.0; 3]).varint(0).varint(0);
    }
    let mut body = Writer::new();
    body.object_block(11, &skin_type);
    if let Some((w, h, png)) = texture {
        let mut tex = Writer::new();
        let mut block = Writer::new();
        block.bytes(&png);
        tex.i16(w as i16).i16(h as i16).block(&block);
        body.object_block(5, &tex);
    }
    body.object_block(21, &cubes).object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

/// A `.cpmmodel` file of a textured model (`argb`, 64x64, `cubes_n` cubes).
fn model_file(argb: u32, cubes_n: u32) -> Vec<u8> {
    skin::container(&model_data(cubes_n, Some((64, 64, png(64, 64, argb)))))
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

fn file_of(location: &str) -> PathBuf {
    PathBuf::from(location)
}

fn texture_of(png: &[u8]) -> Option<([u32; 2], Vec<u8>)> {
    Image::decode(png).ok().map(|i| ([i.w, i.h], i.rgba()))
}

const READER: cpm::SkinReader = cpm::SkinReader { file: file_of, texture: texture_of };

/// Ticks the others until none of `players` is pending (10 s at most).
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

/// The messages in `frames`, reassembled as a client would.
fn messages(frames: &[Vec<u8>]) -> Vec<SyncMessage> {
    let mut r = Reassembly::new(64);
    frames.iter().filter_map(|f| r.push(None, f).unwrap()).collect()
}

/// One relay tick: every frame sent, by recipient.
fn tick(relay: &mut Relay, now: Instant) -> Vec<(u128, Vec<u8>)> {
    let mut out = Vec::new();
    relay.tick(now, |to, frame| {
        out.push((to, frame.to_vec()));
        true
    });
    out
}

/// The messages `to` got out of `sent`.
fn got(sent: &[(u128, Vec<u8>)], to: u128) -> Vec<SyncMessage> {
    let frames: Vec<Vec<u8>> = sent.iter().filter(|(t, _)| *t == to).map(|(_, f)| f.clone()).collect();
    messages(&frames)
}

/// `from` sends `m` to the relay (its uuid is the sender's whatever it says).
fn send(relay: &mut Relay, from: u128, m: &SyncMessage, now: Instant) {
    for frame in cpm::encode_sync(m) {
        relay.receive(from, &frame, now);
    }
}

fn model(uuid: u128, bytes: &[u8]) -> SyncMessage {
    SyncMessage::Model { uuid, kind: ModelKind::Inline, data: bytes.to_vec() }
}

/// A client's sends this tick, online: frames, with `send` taking each (or refusing them).
fn client_tick(on: bool, ok: bool) -> Vec<Vec<u8>> {
    client_tick_with(on, true, |_| ok)
}

/// A client's tick, `online` or not, with `take(index)` answering each frame tried.
fn client_tick_with(on: bool, online: bool, mut take: impl FnMut(usize) -> bool) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    cpm::net_client_tick(on, online, |frame| {
        i += 1;
        if take(i - 1) {
            out.push(frame.to_vec());
            Ok(())
        } else {
            Err("past 32 messages this tick".into())
        }
    });
    out
}

#[test]
fn codec_round_trip_and_chunks() {
    // Small messages: one frame each, versioned.
    let small = [
        SyncMessage::Clear { uuid: 7 },
        SyncMessage::State { uuid: 7, data: vec![1, 2, 3] },
        SyncMessage::Model { uuid: u128::MAX, kind: ModelKind::Link, data: vec![9; 40] },
    ];
    for m in &small {
        let frames = cpm::encode_sync(m);
        assert_eq!(frames.len(), 1, "{m:?}");
        assert_eq!(frames[0][0], cpm::SYNC_VERSION);
        assert_eq!(messages(&frames), vec![m.clone()]);
    }
    // A 64 KiB model: chunks within Horizon's 30 000 bytes, reassembled whole.
    let data: Vec<u8> = (0..cpm::SYNC_MAX_INLINE).map(|i| (i * 31 % 251) as u8).collect();
    let big = SyncMessage::Model { uuid: 42, kind: ModelKind::Inline, data };
    let frames = cpm::encode_sync(&big);
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|f| f.len() <= cpm::SYNC_MAX_FRAME));
    assert_eq!(messages(&frames), vec![big.clone()]);
    // A later version is not read.
    let mut later = cpm::encode_sync(&small[0]);
    later[0][0] = cpm::SYNC_VERSION + 1;
    assert!(Reassembly::new(4).push(None, &later[0]).is_err());
    // Chunks out of order, a missing one, or a transfer past the chunk cap: nothing.
    let mut r = Reassembly::new(4);
    assert!(r.push(None, &frames[1]).is_err());
    assert_eq!(r.push(None, &frames[0]).unwrap(), None);
    assert!(r.push(None, &frames[2]).is_err());
    // (A new transfer starts over with its first chunk.)
    assert_eq!(r.push(None, &frames[0]).unwrap(), None);
    assert_eq!(r.push(None, &frames[1]).unwrap(), None);
    assert_eq!(r.push(None, &frames[2]).unwrap(), Some(big.clone()));
    let mut too_many = frames[0].clone();
    too_many[19] = 1 + cpm::SYNC_MAX_CHUNKS as u8;
    assert!(Reassembly::new(4).push(None, &too_many).is_err());
    // Truncated or garbage frames.
    assert!(Reassembly::new(4).push(None, &[]).is_err());
    assert!(Reassembly::new(4).push(None, &frames[0][..10]).is_err());
    // Partial transfers are bounded: past the cap the oldest goes.
    let mut r = Reassembly::new(2);
    for uuid in 1..=3u128 {
        let m = SyncMessage::Model { uuid, kind: ModelKind::Inline, data: vec![0; cpm::SYNC_MAX_INLINE] };
        r.push(None, &cpm::encode_sync(&m)[0]).unwrap();
    }
    assert_eq!(r.partials(), 2);
}

#[test]
fn relay_join_change_leave() {
    let now = Instant::now();
    let mut relay = Relay::default();
    // The host (1) is a player like any other; 2 joins.
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 1, &model(1, b"host model"), now);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    let sent = tick(&mut relay, now);
    assert_eq!(got(&sent, 2), vec![model(1, b"host model")]);
    assert_eq!(got(&sent, 1), vec![SyncMessage::Clear { uuid: 2 }]);
    // 2 picks a model: the host gets it.
    send(&mut relay, 2, &model(2, b"guest model"), now);
    let sent = tick(&mut relay, now);
    assert_eq!(got(&sent, 1), vec![model(2, b"guest model")]);
    assert!(got(&sent, 2).is_empty());
    // The same again changes nothing.
    send(&mut relay, 2, &model(2, b"guest model"), now);
    assert!(tick(&mut relay, now).is_empty());
    // A State is relayed too (its content is Task 2's).
    send(&mut relay, 2, &SyncMessage::State { uuid: 2, data: vec![5] }, now);
    assert_eq!(got(&tick(&mut relay, now), 1), vec![SyncMessage::State { uuid: 2, data: vec![5] }]);
    // 2 leaves: dropped, and the host is told to forget them (an Unset, not a vanilla choice).
    relay.left(2);
    assert_eq!(relay.model_of(2), None);
    assert_eq!(got(&tick(&mut relay, now), 1), vec![SyncMessage::Unset { uuid: 2 }]);
    // A message that comes after its player left (or from anyone not on the server) is ignored.
    send(&mut relay, 2, &model(2, b"late"), now);
    assert_eq!(relay.model_of(2), None);
    assert!(tick(&mut relay, now).is_empty());
    assert_eq!(relay.model_of(1), Some(&model(1, b"host model")));
}

#[test]
fn late_joiner_gets_current_models() {
    let now = Instant::now();
    let mut relay = Relay::default();
    let big: Vec<u8> = vec![7; cpm::SYNC_MAX_INLINE];
    for uuid in 1..=12u128 {
        relay.joined(uuid);
        send(&mut relay, uuid, &model(uuid, &big), now);
        send(&mut relay, uuid, &SyncMessage::State { uuid, data: vec![uuid as u8] }, now);
    }
    tick(&mut relay, now);
    // 13 joins late: once it says anything, it gets every model and state held, over several
    // ticks (Horizon's 32 messages a tick to a player).
    relay.joined(13);
    send(&mut relay, 13, &SyncMessage::Clear { uuid: 13 }, now);
    let mut all = Vec::new();
    for _ in 0..10 {
        let sent = tick(&mut relay, now);
        assert!(sent.iter().filter(|(to, _)| *to == 13).count() <= cpm::RELAY_SEND_PER_TICK);
        all.extend(sent);
    }
    let received = got(&all, 13);
    for uuid in 1..=12u128 {
        assert!(received.contains(&model(uuid, &big)), "model {uuid}");
        assert!(received.contains(&SyncMessage::State { uuid, data: vec![uuid as u8] }), "state {uuid}");
    }
    assert_eq!(received.len(), 24);
    // A player that sent nothing (no mod) is sent nothing.
    relay.joined(14);
    assert!(all.iter().all(|(to, _)| *to != 14));
    assert!(tick(&mut relay, now).iter().all(|(to, _)| *to != 14));
}

#[test]
fn no_echo_to_sender() {
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 1, &model(1, b"a"), now);
    send(&mut relay, 2, &model(2, b"b"), now);
    let sent = tick(&mut relay, now);
    assert_eq!(got(&sent, 1), vec![model(2, b"b")]);
    assert_eq!(got(&sent, 2), vec![model(1, b"a")]);
    // A player cannot speak for another: the sender's UUID is the one relayed.
    send(&mut relay, 2, &model(1, b"forged"), now);
    let sent = tick(&mut relay, now);
    assert_eq!(got(&sent, 1), vec![model(2, b"forged")]);
    assert!(got(&sent, 2).is_empty());
    assert_eq!(relay.model_of(1), Some(&model(1, b"a")));
}

#[test]
fn relay_caps_size_and_rate() {
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    // Past 64 KiB inline, or a link past 1 KiB, or a state past its cap: dropped.
    let over = SyncMessage::Model { uuid: 1, kind: ModelKind::Inline, data: vec![0; cpm::SYNC_MAX_INLINE + 1] };
    send(&mut relay, 1, &over, now);
    let link = SyncMessage::Model { uuid: 1, kind: ModelKind::Link, data: vec![0; cpm::SYNC_MAX_LINK + 1] };
    send(&mut relay, 1, &link, now);
    send(&mut relay, 1, &SyncMessage::State { uuid: 1, data: vec![0; cpm::SYNC_MAX_STATE + 1] }, now);
    assert_eq!(relay.model_of(1), None);
    assert!(got(&tick(&mut relay, now), 2).is_empty());
    // Within the caps: kept.
    let ok = SyncMessage::Model { uuid: 1, kind: ModelKind::Link, data: vec![1; cpm::SYNC_MAX_LINK] };
    send(&mut relay, 1, &ok, now);
    assert_eq!(got(&tick(&mut relay, now), 2), vec![ok]);
    // Garbage is ignored.
    relay.receive(1, &[], now);
    relay.receive(1, &[cpm::SYNC_VERSION, 0xEE, 1, 2], now);
    assert!(tick(&mut relay, now).is_empty());
    // A flood of changes: a few relayed now, then the latest once the window allows.
    let (window, limit) = (cpm::RELAY_MODEL_WINDOW, cpm::RELAY_MODEL_RELAYS);
    let mut sent = Vec::new();
    let mut relay2 = Relay::default();
    relay2.joined(1);
    relay2.joined(2);
    send(&mut relay2, 2, &SyncMessage::Clear { uuid: 2 }, now);
    tick(&mut relay2, now);
    for i in 0..20u8 {
        send(&mut relay2, 1, &model(1, &[i]), now);
        sent.extend(tick(&mut relay2, now));
    }
    let models = got(&sent, 2);
    assert_eq!(models.len(), limit, "{models:?}");
    // The rest wait; the latest goes out once the window has passed (not every one between).
    let later = got(&tick(&mut relay2, now + window), 2);
    assert_eq!(later, vec![model(1, &[19])]);
    assert!(tick(&mut relay2, now + window * 2).is_empty());
    // A player's send budget is shared with other mods: a refused frame is sent again next tick.
    send(&mut relay2, 1, &model(1, b"again"), now + window * 3);
    let mut refused = 0;
    relay2.tick(now + window * 3, |_, _| {
        refused += 1;
        false
    });
    assert_eq!(refused, 1);
    assert_eq!(got(&tick(&mut relay2, now + window * 3), 2), vec![model(1, b"again")]);
}

#[test]
fn client_sends_inline_up_to_64kib() {
    let _serial = serial();
    let dir = scratch("inline");
    // The rule: at most 64 KiB inline.
    let at = vec![0u8; cpm::SYNC_MAX_INLINE];
    assert_eq!(cpm::share_plan(Some(&at)), SharePlan::Model(ModelKind::Inline, at.clone()));
    let over = vec![0u8; cpm::SYNC_MAX_INLINE + 1];
    assert_eq!(cpm::share_plan(Some(&over)), SharePlan::HeldBack(cpm::SYNC_MAX_INLINE + 1));
    assert_eq!(cpm::share_plan(None), SharePlan::Clear);
    assert_eq!(cpm::SYNC_MAX_INLINE, cpm::GIST_LIMIT_KIB as usize * 1024);
    // Nothing chosen: a Clear, once.
    let frames = client_tick(true, true);
    assert_eq!(messages(&frames), vec![SyncMessage::Clear { uuid: 0 }]);
    assert!(client_tick(true, true).is_empty());
    // A model chosen: its file exactly as loaded.
    let a = dir.join("a.cpmmodel");
    std::fs::write(&a, model_file(0xFF11_2233, 1)).unwrap();
    cpm::select(Some(a.clone()));
    cpm::ensure_loaded();
    let sent = messages(&client_tick(true, true));
    assert_eq!(sent, vec![model(0, &std::fs::read(&a).unwrap())]);
    assert_eq!(cpm::net_share_status(), NetShare::Shared(ModelKind::Inline));
    assert!(client_tick(true, true).is_empty());
    // A refused send (the tick's allowance taken): sent whole once the server takes it.
    let b = dir.join("b.cpmmodel");
    std::fs::write(&b, model_file(0xFF44_5566, 2)).unwrap();
    cpm::select(Some(b.clone()));
    cpm::ensure_loaded();
    assert!(client_tick(true, false).is_empty());
    let mut later = Vec::new();
    for _ in 0..40 {
        later.extend(client_tick(true, true));
    }
    assert_eq!(messages(&later), vec![model(0, &std::fs::read(&b).unwrap())]);
    // Left and joined again: sent again.
    cpm::net_client_reset();
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &std::fs::read(&b).unwrap())]);
    // The mod turned off: a Clear.
    assert_eq!(messages(&client_tick(false, true)), vec![SyncMessage::Clear { uuid: 0 }]);
    cpm::select(None);
    cpm::ensure_loaded();
}

#[test]
fn client_sends_link_models() {
    let _serial = serial();
    let dir = scratch("link");
    cpm::set_data_dir(dir.clone());
    let link = "git:someone/0123456789abcdef";
    let target = model_data(1, Some((64, 64, png(64, 64, 0xFF77_8899))));
    let cache = cpm::cache_path(&dir, link).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, cpm_core::link::gist_text(&skin::container(&target)).unwrap()).unwrap();
    let file = dir.join("l.link.cpmmodel");
    let bytes = cpm_core::link::link_model("l", "", cpm_core::format::SkinType::Default, link);
    std::fs::write(&file, &bytes).unwrap();
    assert_eq!(cpm::share_plan(Some(&bytes)), SharePlan::Model(ModelKind::Link, bytes.clone()));
    cpm::select(Some(file));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    let sent = messages(&client_tick(true, true));
    assert_eq!(sent, vec![SyncMessage::Model { uuid: 0, kind: ModelKind::Link, data: bytes }]);
    assert_eq!(cpm::net_share_status(), NetShare::Shared(ModelKind::Link));
    cpm::select(None);
    cpm::ensure_loaded();
}

#[test]
fn oversized_non_link_held_back() {
    let _serial = serial();
    let dir = scratch("held");
    let file = dir.join("big.cpmmodel");
    let bytes = skin::container(&model_data(1, Some((256, 256, noise_png(256, 256)))));
    assert!(bytes.len() > cpm::SYNC_MAX_INLINE);
    std::fs::write(&file, &bytes).unwrap();
    cpm::select(Some(file));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    // Never sent: an Unset (the others see the skin's model, or the normal skin), and the status
    // says why.
    let sent = messages(&client_tick(true, true));
    assert_eq!(sent, vec![SyncMessage::Unset { uuid: 0 }]);
    assert_eq!(cpm::net_share_status(), NetShare::HeldBack(bytes.len()));
    assert!(client_tick(true, true).is_empty());
    // Once a small model is chosen, it goes.
    let small = dir.join("small.cpmmodel");
    std::fs::write(&small, model_file(0xFF00_00FF, 1)).unwrap();
    cpm::select(Some(small.clone()));
    cpm::ensure_loaded();
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &std::fs::read(&small).unwrap())]);
    assert_eq!(cpm::net_share_status(), NetShare::Shared(ModelKind::Inline));
    cpm::select(None);
    cpm::ensure_loaded();
}

#[test]
fn network_model_overrides_skin() {
    let _serial = serial();
    let dir = scratch("override");
    cpm::set_data_dir(dir.clone());
    // Player 5's skin holds a one-cube model.
    let file = dir.join("five.png");
    std::fs::write(&file, skin_png(0xFF33_6699, Some(&model_data(1, None)))).unwrap();
    let skin = OtherSkin { location: file.to_string_lossy().into_owned(), slim: false };
    let players = [OtherPlayer { uuid: 5, skin: Some(skin), distance: 0.0 }, OtherPlayer { uuid: 6, skin: None, distance: 1.0 }];
    settle(&players);
    assert_eq!(cpm::other_status(5), Some(OtherStatus::Loaded));
    cpm::texture_upload(Instant::now());
    assert_eq!(cpm::with_other_model(5, |m, _| m.model.cubes.len()), Some(1));
    // The server sends 5's chosen model (three cubes): it wins over the skin's.
    let feed = |m: &SyncMessage| {
        for f in cpm::encode_sync(m) {
            cpm::net_client_receive(&f);
        }
    };
    feed(&model(5, &model_file(0xFFAA_0000, 3)));
    // And 6, with no skin model at all, gets one from the network.
    feed(&model(6, &model_file(0xFF00_AA00, 2)));
    settle(&players);
    cpm::texture_upload(Instant::now());
    assert_eq!(cpm::with_other_model(5, |m, _| m.model.cubes.len()), Some(3));
    assert_eq!(cpm::with_other_model(6, |m, _| m.model.cubes.len()), Some(2));
    // Network models keep the phase 1 limits: past 256 cubes, refused.
    feed(&model(6, &model_file(0xFF00_AA00, 300)));
    settle(&players);
    assert!(matches!(cpm::other_status(6), Some(OtherStatus::Error(_))));
    // The same model again does not load again.
    feed(&model(5, &model_file(0xFFAA_0000, 3)));
    cpm::others_tick(&players, &READER);
    assert_eq!(cpm::other_status(5), Some(OtherStatus::Loaded));
    // A Clear (a vanilla choice): vanilla, though the skin holds a model.
    feed(&SyncMessage::Clear { uuid: 5 });
    settle(&players);
    assert_eq!(cpm::other_status(5), None);
    // An Unset (held back, not loaded, or left): the skin's model again.
    feed(&SyncMessage::Unset { uuid: 5 });
    settle(&players);
    cpm::texture_upload(Instant::now());
    assert_eq!(cpm::with_other_model(5, |m, _| m.model.cubes.len()), Some(1));
    feed(&SyncMessage::Clear { uuid: 5 });
    settle(&players);
    assert_eq!(cpm::other_status(5), None);
    // Leaving the server drops what it sent: the skin's model again.
    cpm::net_client_reset();
    settle(&players);
    cpm::texture_upload(Instant::now());
    assert_eq!(cpm::with_other_model(5, |m, _| m.model.cubes.len()), Some(1));
    assert_eq!(cpm::other_status(6), None);
}

#[test]
fn client_sends_whole_after_errors_then_only_have() {
    let _serial = serial();
    let dir = scratch("resend");
    let a = dir.join("a.cpmmodel");
    // Big enough for chunks.
    let bytes = skin::container(&model_data(1, Some((128, 128, noise_png(96, 96)))));
    assert!(bytes.len() > cpm::SYNC_MAX_FRAME && bytes.len() <= cpm::SYNC_MAX_INLINE, "{}", bytes.len());
    std::fs::write(&a, &bytes).unwrap();
    cpm::select(Some(a));
    cpm::ensure_loaded();
    // The second chunk refused: nothing is taken as sent, and the whole message goes again.
    let first = client_tick_with(true, true, |i| i == 0);
    assert_eq!(first.len(), 1);
    let mut again = Vec::new();
    for _ in 0..30 {
        again.extend(client_tick(true, true));
    }
    assert_eq!(messages(&again), vec![model(0, &bytes)]);
    assert_eq!(again.len(), cpm::encode_sync(&model(0, &bytes)).len());
    // Then not again: every few seconds only a few bytes naming it (the host asks for it when it
    // lost it).
    let mut later = Vec::new();
    for _ in 0..cpm::CLIENT_HAVE_TICKS + cpm::CLIENT_HAVE_JITTER + 1 {
        later.extend(client_tick(true, true));
    }
    let digest = cpm::model_digest(&model(0, &bytes));
    let later_messages = messages(&later);
    assert!(matches!(later_messages[..], [SyncMessage::Have { digest: d, .. }] if d == digest), "{later_messages:?}");
    assert!(later.iter().all(|f| f.len() < 64));
    cpm::select(None);
    cpm::ensure_loaded();
}

#[test]
fn client_resends_a_model_loaded_again() {
    let _serial = serial();
    let dir = scratch("reload");
    let a = dir.join("a.cpmmodel");
    std::fs::write(&a, model_file(0xFF11_1111, 1)).unwrap();
    cpm::select(Some(a.clone()));
    cpm::ensure_loaded();
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &std::fs::read(&a).unwrap())]);
    // The file converted again in place, and loaded again before the next tick.
    std::fs::write(&a, model_file(0xFF22_2222, 2)).unwrap();
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::select(Some(a.clone()));
    cpm::ensure_loaded();
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &std::fs::read(&a).unwrap())]);
    // A model that does not load: an Unset (the skin's model shows), not a vanilla choice.
    std::fs::write(&a, b"not a model").unwrap();
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::select(Some(a));
    cpm::ensure_loaded();
    assert_eq!(messages(&client_tick(true, true)), vec![SyncMessage::Unset { uuid: 0 }]);
    assert_eq!(cpm::net_share_status(), NetShare::NotLoaded);
    cpm::select(None);
    cpm::ensure_loaded();
}

#[test]
fn going_offline_drops_received_and_resends() {
    let _serial = serial();
    let dir = scratch("offline");
    cpm::set_data_dir(dir.clone());
    let players = [OtherPlayer { uuid: 9, skin: None, distance: 0.0 }];
    assert_eq!(messages(&client_tick(true, true)), vec![SyncMessage::Clear { uuid: 0 }]);
    for f in cpm::encode_sync(&model(9, &model_file(0xFF00_AA00, 2))) {
        cpm::net_client_receive(&f);
    }
    settle(&players);
    assert_eq!(cpm::other_status(9), Some(OtherStatus::Loaded));
    // Reconfigured (a proxy moving the player to another server): nothing is sent while offline,
    // and the old server's models go.
    assert!(client_tick_with(true, false, |_| panic!("sent while offline")).is_empty());
    settle(&players);
    assert_eq!(cpm::other_status(9), None);
    // Online again: the model (here a Clear) is sent to the new server.
    assert_eq!(messages(&client_tick(true, true)), vec![SyncMessage::Clear { uuid: 0 }]);
}

#[test]
fn relay_peers_on_any_frame_and_keeps_senders_apart() {
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    relay.joined(3);
    send(&mut relay, 1, &model(1, b"one"), now);
    tick(&mut relay, now);
    // 2's first frame is a chunk whose start Horizon dropped: still a player with the mod.
    let big = vec![3u8; cpm::SYNC_MAX_INLINE];
    let frames2 = cpm::encode_sync(&model(2, &big));
    relay.receive(2, &frames2[1], now);
    assert_eq!(got(&tick(&mut relay, now), 2), vec![model(1, b"one")]);
    // 2's and 3's chunks interleaved: each sender's transfer is its own.
    let frames3 = cpm::encode_sync(&model(3, &[4u8; 40_000]));
    for i in 0..frames2.len().max(frames3.len()) {
        if let Some(f) = frames2.get(i) {
            relay.receive(2, f, now);
        }
        if let Some(f) = frames3.get(i) {
            relay.receive(3, f, now);
        }
    }
    assert_eq!(relay.model_of(2), Some(&model(2, &big)));
    assert_eq!(relay.model_of(3), Some(&model(3, &[4u8; 40_000])));
    for _ in 0..5 {
        tick(&mut relay, now);
    }
    // An unchanged resend is not relayed again.
    send(&mut relay, 2, &model(2, &big), now);
    assert!(tick(&mut relay, now).is_empty());
    // States: at most their rate, the latest after.
    let mut sent = Vec::new();
    for i in 0..(cpm::RELAY_STATE_RELAYS + 5) as u8 {
        send(&mut relay, 1, &SyncMessage::State { uuid: 1, data: vec![i] }, now);
        sent.extend(tick(&mut relay, now));
    }
    assert_eq!(got(&sent, 2).len(), cpm::RELAY_STATE_RELAYS);
    let last = (cpm::RELAY_STATE_RELAYS + 4) as u8;
    assert_eq!(
        got(&tick(&mut relay, now + cpm::RELAY_STATE_WINDOW), 2),
        vec![SyncMessage::State { uuid: 1, data: vec![last] }]
    );
}

// ---- Gestures, poses and parameters; Have and Need (multiplayer phase 2, Task 2) ----

/// Animations for a test model: gestures `wave` (looping), `glow` (value layer), `hat` (layer: a
/// toggle) and custom poses `sit`, `lie`.
fn gesture_set() -> cpm_core::anim::AnimSet {
    use cpm_core::anim::{AnimSet, Clip, Gesture, GestureKind, Interp, Pose};
    let clip = || Clip {
        components: vec![],
        targets: vec![],
        frames: 1,
        duration_ms: 1000,
        priority: 0,
        additive: false,
        must_finish: false,
        interp: Interp::NoInterpolate,
        dynamic: false,
        channels: vec![],
        show: vec![],
        present: vec![],
    };
    let g = |name: &str, kind| Gesture {
        name: name.into(),
        kind,
        looping: true,
        must_finish: false,
        layer: !matches!(kind, GestureKind::Gesture),
        default_value: 0,
        clips: vec![clip()],
        ..Default::default()
    };
    AnimSet {
        poses: vec![(Pose::Custom("sit".into()), clip()), (Pose::Custom("lie".into()), clip())],
        gestures: vec![g("wave", GestureKind::Gesture), g("glow", GestureKind::Value), g("hat", GestureKind::Layer)],
        ..Default::default()
    }
}

/// The first toggle button of a [`gesture_set`] engine.
fn toggle_of(e: &cpm_core::Engine) -> usize {
    e.buttons().iter().position(|b| matches!(b.kind, cpm_core::anim::ButtonKind::Toggle)).expect("a toggle")
}

/// The states among `frames`, decoded.
fn states_in(frames: &[Vec<u8>]) -> Vec<cpm::PlayerState> {
    messages(frames)
        .into_iter()
        .filter_map(|m| match m {
            SyncMessage::State { data, .. } => Some(cpm::decode_state(&data).expect("a state that reads")),
            _ => None,
        })
        .collect()
}

/// Selects a fresh model file `name` in `dir` with [`gesture_set`]'s animations; returns its bytes.
fn load_local_with_gestures(dir: &Path, name: &str) -> Vec<u8> {
    let file = dir.join(name);
    let bytes = model_file(0xFF12_3456, 1);
    std::fs::write(&file, &bytes).unwrap();
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::net_client_reset();
    cpm::select(Some(file));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.engine = cpm_core::Engine::new(gesture_set())).unwrap();
    bytes
}

fn unload() {
    cpm::net_client_reset();
    cpm::select(None);
    cpm::ensure_loaded();
}

/// Feeds `m` to the client as the server would.
fn feed(m: &SyncMessage) {
    for f in cpm::encode_sync(m) {
        cpm::net_client_receive(&f);
    }
}

#[test]
fn state_sent_on_change() {
    let _serial = serial();
    let dir = scratch("state_change");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    // At rest (no gesture, no pose, default parameters): only the model goes.
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &bytes)]);
    // A gesture played: its state goes, naming the model it is for.
    cpm::with_model(|m| m.engine.play_gesture(0, 0)).unwrap();
    let sent = states_in(&client_tick(true, true));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].digest, cpm::model_digest(&model(0, &bytes)));
    assert_eq!(sent[0].gesture.map(|(g, _)| g), Some(0));
    assert_eq!(sent[0].pose, None);
    // Unchanged: nothing more.
    assert!(client_tick(true, true).is_empty());
    // A pose and a toggle, changed on consecutive ticks: each goes, never two closer than the
    // client's own spacing (so under the host's 20 a second).
    cpm::with_model(|m| m.engine.set_custom_pose(Some(1))).unwrap();
    let mut ticks = vec![client_tick(true, true)];
    cpm::with_model(|m| {
        let t = toggle_of(&m.engine);
        m.engine.press(t, 0);
    })
    .unwrap();
    for _ in 0..cpm::CLIENT_STATE_MIN_TICKS * 2 {
        ticks.push(client_tick(true, true));
    }
    let at: Vec<usize> = ticks.iter().enumerate().filter(|(_, f)| !f.is_empty()).map(|(i, _)| i).collect();
    assert_eq!(at.len(), 2, "{at:?}");
    assert!(at[1] - at[0] >= cpm::CLIENT_STATE_MIN_TICKS as usize, "{at:?}");
    assert!(20 / cpm::CLIENT_STATE_MIN_TICKS as usize <= cpm::RELAY_STATE_RELAYS);
    let last = states_in(&ticks.concat()).pop().expect("a state");
    let params = cpm::with_model(|m| m.engine.parameters()).unwrap();
    assert_eq!((last.gesture.map(|(g, _)| g), last.pose, last.params), (Some(0), Some(1), params));
    // Back at rest: that goes too (the others reset).
    cpm::with_model(|m| m.engine = cpm_core::Engine::new(gesture_set())).unwrap();
    let mut back = Vec::new();
    for _ in 0..cpm::CLIENT_STATE_MIN_TICKS + 1 {
        back.extend(client_tick(true, true));
    }
    let back = states_in(&back);
    assert_eq!(back.len(), 1);
    assert_eq!((back[0].gesture, back[0].pose), (None, None));
    unload();
}

#[test]
fn model_sent_once_then_only_have() {
    let _serial = serial();
    let dir = scratch("have");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    cpm::with_model(|m| m.engine.play_gesture(0, 0)).unwrap();
    let first = messages(&client_tick(true, true));
    assert_eq!(first.len(), 2, "the model, then its state");
    let digest = cpm::model_digest(&model(0, &bytes));
    // A minute on: never the model again, only Haves, every few seconds, each naming the model
    // and carrying the state (a lost State heals).
    let mut sent = Vec::new();
    for _ in 0..1200 {
        sent.extend(messages(&client_tick(true, true)));
    }
    assert!(sent.iter().all(|m| matches!(m, SyncMessage::Have { .. })), "{sent:?}");
    let most = 1200 / cpm::CLIENT_HAVE_TICKS as usize;
    let least = 1200 / (cpm::CLIENT_HAVE_TICKS + cpm::CLIENT_HAVE_JITTER) as usize;
    assert!(sent.len() >= least && sent.len() <= most, "{}", sent.len());
    let SyncMessage::Have { digest: d, state, .. } = &sent[0] else { unreachable!() };
    assert_eq!(*d, digest);
    assert_eq!(cpm::decode_state(state).unwrap().gesture.map(|(g, _)| g), Some(0));
    unload();
}

#[test]
fn need_triggers_exactly_one_resend() {
    let _serial = serial();
    let dir = scratch("need");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &bytes)]);
    assert!(client_tick(true, true).is_empty());
    // The host asks (it holds another, or none): the model once more, then nothing.
    feed(&SyncMessage::Need { uuid: 42 });
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &bytes)]);
    for _ in 0..cpm::CLIENT_HAVE_TICKS - 1 {
        assert!(client_tick(true, true).is_empty());
    }
    unload();
}

#[test]
fn digest_mismatch_triggers_need() {
    // The host holds model A of player 1; 1's Have names another (a re-converted file whose
    // change was lost): the host asks 1 for it, once in its window, and relays nothing.
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    send(&mut relay, 1, &model(1, b"A"), now);
    tick(&mut relay, now);
    let b = cpm::model_digest(&model(1, b"B"));
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest: b, state: vec![] }, now);
    let out = tick(&mut relay, now);
    assert_eq!(got(&out, 1), vec![SyncMessage::Need { uuid: 1 }]);
    assert!(got(&out, 2).is_empty());
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest: b, state: vec![] }, now);
    assert!(tick(&mut relay, now).is_empty(), "asked once in the window");
    let later = now + cpm::RELAY_ASK_WINDOW;
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest: b, state: vec![] }, later);
    assert_eq!(got(&tick(&mut relay, later), 1), vec![SyncMessage::Need { uuid: 1 }]);
    // A host holding nothing of a player (a Have first) asks too.
    relay.joined(3);
    send(&mut relay, 3, &SyncMessage::Have { uuid: 3, digest: b, state: vec![] }, now);
    assert!(got(&tick(&mut relay, now), 3).contains(&SyncMessage::Need { uuid: 3 }));
    // The client answers with its model: the re-converted file goes whole.
    send(&mut relay, 1, &model(1, b"B"), later);
    assert_eq!(got(&tick(&mut relay, later), 2), vec![model(1, b"B")]);
}

#[test]
fn peer_missing_model_recovers_via_need() {
    let _serial = serial();
    let dir = scratch("recover");
    cpm::set_data_dir(dir.clone());
    unload();
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    let one = model_file(0xFF00_5500, 2);
    send(&mut relay, 1, &model(1, &one), now);
    // 2's copy of 1's model is lost (Horizon dropped it): 2 is this client, and gets nothing.
    tick(&mut relay, now);
    // 1's heartbeat, relayed with the digest of the model held of 1.
    let digest = cpm::model_digest(&model(1, &one));
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest, state: vec![] }, now);
    let out = tick(&mut relay, now);
    assert_eq!(got(&out, 2), vec![SyncMessage::Have { uuid: 1, digest, state: vec![] }]);
    for (to, f) in &out {
        if *to == 2 {
            cpm::net_client_receive(f);
        }
    }
    // 2 asks for 1's model, once (another Have soon after does not ask again).
    let asked = messages(&client_tick(true, true));
    assert!(asked.contains(&SyncMessage::Need { uuid: 1 }), "{asked:?}");
    feed(&SyncMessage::Have { uuid: 1, digest, state: vec![] });
    assert!(!messages(&client_tick(true, true)).contains(&SyncMessage::Need { uuid: 1 }));
    // The host sends it to 2 only.
    send(&mut relay, 2, &SyncMessage::Need { uuid: 1 }, now);
    let out = tick(&mut relay, now);
    assert_eq!(got(&out, 2), vec![model(1, &one)]);
    assert!(got(&out, 1).is_empty());
    for (_, f) in &out {
        cpm::net_client_receive(f);
    }
    let players = [OtherPlayer { uuid: 1, skin: None, distance: 0.0 }];
    settle(&players);
    assert_eq!(cpm::other_status(1), Some(OtherStatus::Loaded));
    // Held now: a Have of it asks nothing.
    feed(&SyncMessage::Have { uuid: 1, digest, state: vec![] });
    for _ in 0..cpm::CLIENT_NEED_TICKS + 1 {
        assert!(!messages(&client_tick(true, true)).iter().any(|m| matches!(m, SyncMessage::Need { .. })));
    }
    unload();
}

#[test]
fn need_rate_limited() {
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 1, &model(1, &[7u8; 40_000]), now);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    tick(&mut relay, now);
    // 2 asks for 1's model over and over: only RELAY_NEED_RELAYS go in the window.
    let mut sent = Vec::new();
    for _ in 0..cpm::RELAY_NEED_RELAYS * 5 {
        send(&mut relay, 2, &SyncMessage::Need { uuid: 1 }, now);
        for _ in 0..3 {
            sent.extend(tick(&mut relay, now));
        }
    }
    assert_eq!(got(&sent, 2).len(), cpm::RELAY_NEED_RELAYS);
    // Asking for a player not held, or for itself, gets nothing.
    let later = now + cpm::RELAY_NEED_WINDOW;
    send(&mut relay, 2, &SyncMessage::Need { uuid: 99 }, later);
    send(&mut relay, 2, &SyncMessage::Need { uuid: 2 }, later);
    assert!(tick(&mut relay, later).is_empty());
    // After the window, again.
    send(&mut relay, 2, &SyncMessage::Need { uuid: 1 }, later);
    let mut again = Vec::new();
    for _ in 0..3 {
        again.extend(tick(&mut relay, later));
    }
    assert_eq!(got(&again, 2), vec![model(1, &[7u8; 40_000])]);
}

#[test]
fn state_applied_to_remote_engine() {
    let _serial = serial();
    let dir = scratch("state_apply");
    cpm::set_data_dir(dir.clone());
    // The local player has a model with the same gestures, which must stay as it is.
    let local = load_local_with_gestures(&dir, "mine.cpmmodel");
    let players = [OtherPlayer { uuid: 7, skin: None, distance: 0.0 }];
    let bytes = model_file(0xFF00_7700, 2);
    let params = {
        let mut e = cpm_core::Engine::new(gesture_set());
        let t = toggle_of(&e);
        e.press(t, 0);
        e.parameters()
    };
    let digest = cpm::model_digest(&model(7, &bytes));
    let state = cpm::PlayerState { digest, gesture: Some((0, 1)), pose: Some(1), params: params.clone() };
    let send_state = |s: &cpm::PlayerState| feed(&SyncMessage::State { uuid: 7, data: cpm::encode_state(s) });
    // The state may come before the model: kept, applied once the model is there.
    send_state(&state);
    feed(&model(7, &bytes));
    settle(&players);
    cpm::texture_upload(Instant::now());
    // (The test model has no animations: its engine is swapped for gesture_set's, unapplied.)
    cpm::with_other_model(7, |m, _| m.engine = cpm_core::Engine::new(gesture_set())).unwrap();
    let state = cpm::PlayerState { gesture: Some((0, 5)), ..state };
    send_state(&state);
    cpm::others_tick(&players, &READER);
    let seen = || {
        cpm::with_other_model(7, |m, _| (m.engine.playing_gesture(), m.engine.custom_pose().map(str::to_owned), m.engine.parameters()))
            .unwrap()
    };
    assert_eq!(seen(), (Some(0), Some("lie".to_owned()), params.clone()));
    // Never the local engine.
    let rest = cpm_core::Engine::new(gesture_set()).parameters();
    let mine = cpm::with_model(|m| (m.engine.playing_gesture(), m.engine.custom_pose().is_none(), m.engine.parameters())).unwrap();
    assert_eq!(mine, (None, true, rest.clone()));
    // A parameter change does not restart the gesture (it ended there).
    cpm::with_other_model(7, |m, _| m.engine.stop_gesture()).unwrap();
    send_state(&cpm::PlayerState { params: rest.clone(), ..state.clone() });
    cpm::others_tick(&players, &READER);
    assert_eq!(seen(), (None, Some("lie".to_owned()), rest.clone()));
    // The same gesture started again (a new start number): it plays again.
    send_state(&cpm::PlayerState { gesture: Some((0, 2)), params: rest.clone(), ..state.clone() });
    cpm::others_tick(&players, &READER);
    assert_eq!(seen().0, Some(0));
    // Reset Gesture and Reset Pose.
    send_state(&cpm::PlayerState { gesture: None, pose: None, params: rest.clone(), ..state.clone() });
    cpm::others_tick(&players, &READER);
    assert_eq!(seen(), (None, None, rest.clone()));
    // A state for another model (an old one) is not applied; a Have's state is applied as a State.
    send_state(&cpm::PlayerState { digest: cpm::model_digest(&model(7, &local)), ..state.clone() });
    cpm::others_tick(&players, &READER);
    assert_eq!(seen(), (None, None, rest.clone()));
    feed(&SyncMessage::Have { uuid: 7, digest, state: cpm::encode_state(&state) });
    cpm::others_tick(&players, &READER);
    assert_eq!(seen(), (Some(0), Some("lie".to_owned()), params));
    unload();
}

#[test]
fn state_resent_on_rejoin() {
    let _serial = serial();
    let dir = scratch("state_rejoin");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    cpm::with_model(|m| m.engine.set_custom_pose(Some(0))).unwrap();
    assert_eq!(messages(&client_tick(true, true)).len(), 2);
    assert!(client_tick(true, true).is_empty());
    // Disconnected and joined again: the model, then the state.
    cpm::net_client_reset();
    let again = client_tick(true, true);
    assert_eq!(messages(&again)[0], model(0, &bytes));
    assert_eq!(states_in(&again).len(), 1);
    assert_eq!(states_in(&again)[0].pose, Some(0));
    // Moved to another server (offline, then online): both again.
    client_tick_with(true, false, |_| panic!("sent while offline"));
    assert_eq!(messages(&client_tick(true, true)).len(), 2);
    // The mod turned off: a Clear, and no state.
    assert_eq!(messages(&client_tick(false, true)), vec![SyncMessage::Clear { uuid: 0 }]);
    for _ in 0..cpm::CLIENT_HAVE_TICKS + cpm::CLIENT_HAVE_JITTER + 1 {
        assert!(states_in(&client_tick(false, true)).is_empty());
    }
    unload();
}

#[test]
fn gist_prompt_for_oversized_in_multiplayer() {
    let _serial = serial();
    let dir = scratch("gist_prompt");
    let file = dir.join("big.cpmmodel");
    let bytes = skin::container(&model_data(1, Some((256, 256, noise_png(256, 256)))));
    std::fs::write(&file, &bytes).unwrap();
    unload();
    while cpm::take_gist_popup().is_some() {}
    cpm::select(Some(file.clone()));
    cpm::ensure_loaded();
    // Alone (as in single player, which also runs on a server): no popup.
    for _ in 0..5 {
        client_tick(true, true);
    }
    assert_eq!(cpm::pending_gist_popup(), None);
    // Another player with the mod, but offline: none.
    feed(&SyncMessage::Clear { uuid: 9 });
    client_tick_with(true, false, |_| true);
    assert_eq!(cpm::pending_gist_popup(), None);
    // Online with another player who has the mod: the gist popup, once, its gist text written.
    feed(&SyncMessage::Clear { uuid: 9 });
    client_tick(true, true);
    assert_eq!(cpm::pending_gist_popup(), Some((file.clone(), bytes.len())));
    assert_eq!(std::fs::read_to_string(cpm::gist_path(&file)).unwrap(), cpm_core::link::gist_text(&bytes).unwrap());
    assert_eq!(cpm::take_gist_popup(), Some((file.clone(), bytes.len())));
    for _ in 0..5 {
        client_tick(true, true);
    }
    assert_eq!(cpm::pending_gist_popup(), None);
    // Not again for that model: not on a rejoin, nor when it is chosen again.
    cpm::net_client_reset();
    feed(&SyncMessage::Clear { uuid: 9 });
    cpm::select(None);
    cpm::ensure_loaded();
    client_tick(true, true);
    cpm::select(Some(file));
    cpm::ensure_loaded();
    for _ in 0..5 {
        client_tick(true, true);
    }
    assert_eq!(cpm::pending_gist_popup(), None);
    unload();
}

#[test]
fn status_explains_held_back_model() {
    let _serial = serial();
    // The line for each status: why the others see the normal skin, when they do.
    let held = cpm::share_status_line(&NetShare::HeldBack(100 * 1024)).unwrap();
    assert!(held.contains("100 KiB") && held.contains("64 KiB") && held.contains("normal skin") && held.contains("gist"), "{held}");
    let failed = cpm::share_status_line(&NetShare::NotLoaded).unwrap();
    assert!(failed.contains("normal skin") && failed.contains("load"), "{failed}");
    assert!(cpm::share_status_line(&NetShare::Shared(ModelKind::Inline)).unwrap().contains("shared"));
    assert_eq!(cpm::share_status_line(&NetShare::Off), None);
    assert_eq!(cpm::share_status_line(&NetShare::Nothing), None);
    // On the settings page only while in a world that carries the mod's messages.
    unload();
    let dir = scratch("status");
    let file = dir.join("big.cpmmodel");
    let bytes = skin::container(&model_data(1, Some((256, 256, noise_png(256, 256)))));
    std::fs::write(&file, &bytes).unwrap();
    cpm::select(Some(file));
    cpm::ensure_loaded();
    client_tick_with(true, false, |_| true);
    assert_eq!(cpm::net_status_line(), None);
    client_tick(true, true);
    assert_eq!(cpm::net_status_line(), cpm::share_status_line(&NetShare::HeldBack(bytes.len())));
    while cpm::take_gist_popup().is_some() {}
    unload();
}

// ---- Review fixes (Task 2) ----

/// Player 7's network model (a [`model_file`]) loaded and drawn, with [`gesture_set`]'s engine;
/// returns its digest.
fn remote_with_gestures(players: &[OtherPlayer]) -> u64 {
    let bytes = model_file(0xFF00_7700, 2);
    feed(&model(7, &bytes));
    settle(players);
    cpm::texture_upload(Instant::now());
    cpm::with_other_model(7, |m, _| m.engine = cpm_core::Engine::new(gesture_set())).unwrap();
    cpm::model_digest(&model(7, &bytes))
}

#[test]
fn remote_pose_survives_its_own_main_pose_change() {
    let _serial = serial();
    let dir = scratch("pose_held");
    cpm::set_data_dir(dir);
    unload();
    let players = [OtherPlayer { uuid: 7, skin: None, distance: 0.0 }];
    let digest = remote_with_gestures(&players);
    let params = {
        let mut e = cpm_core::Engine::new(gesture_set());
        let t = toggle_of(&e);
        e.press(t, 0);
        e.parameters()
    };
    let state = cpm::PlayerState { digest, gesture: None, pose: Some(0), params: params.clone() };
    feed(&SyncMessage::State { uuid: 7, data: cpm::encode_state(&state) });
    cpm::others_tick(&players, &READER);
    assert_eq!(cpm::with_other_model(7, |m, _| m.engine.custom_pose().map(str::to_owned)).unwrap(), Some("sit".into()));
    // Their engine's own main pose changes here (a jitter of what the game knows of them): it
    // drops the pose, and the parameters were changed too.
    let stand = cpm_core::PoseInput { time_ms: 10, on_ground: true, ..Default::default() };
    let sleep = cpm_core::PoseInput { time_ms: 20, sleeping: true, ..stand };
    cpm::with_other_model(7, |m, _| {
        m.engine.sample(&stand);
        m.engine.sample(&sleep);
        let rest = m.engine.default_parameters().to_vec();
        m.engine.set_parameters(&rest);
    })
    .unwrap();
    assert_eq!(cpm::with_other_model(7, |m, _| m.engine.custom_pose().is_none()).unwrap(), true);
    // The next pass sets the sender's pose and parameters again (as CPM's remote players hold the
    // synced pose), with no new state from them.
    cpm::others_tick(&players, &READER);
    let seen = cpm::with_other_model(7, |m, _| (m.engine.custom_pose().map(str::to_owned), m.engine.parameters())).unwrap();
    assert_eq!(seen, (Some("sit".into()), params));
    unload();
}

#[test]
fn finished_one_shot_gesture_not_shared() {
    let _serial = serial();
    let dir = scratch("one_shot");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    let mut set = gesture_set();
    set.gestures[0].looping = false;
    set.gestures[0].clips[0].duration_ms = 1;
    cpm::with_model(|m| m.engine = cpm_core::Engine::new(set)).unwrap();
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &bytes)]);
    let now = cpm::clock_ms(Instant::now());
    cpm::with_model(|m| m.engine.play_gesture(0, now)).unwrap();
    assert_eq!(states_in(&client_tick(true, true))[0].gesture.map(|(g, _)| g), Some(0));
    // CPM's auto-reset: one tick (50 ms) for the clip, plus 5 ticks.
    std::thread::sleep(Duration::from_millis(6 * 50 + 30));
    let mut after = Vec::new();
    for _ in 0..cpm::CLIENT_STATE_MIN_TICKS + 1 {
        after.extend(client_tick(true, true));
    }
    let after = states_in(&after);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].gesture, None, "a finished one-shot is not in the shared state");
    assert_eq!(cpm::with_model(|m| m.engine.playing_gesture()).unwrap(), None);
    // So its Haves (what a late joiner or a fresh entry gets) carry none either.
    let mut haves = Vec::new();
    for _ in 0..cpm::CLIENT_HAVE_TICKS + cpm::CLIENT_HAVE_JITTER + 1 {
        haves.extend(messages(&client_tick(true, true)));
    }
    let SyncMessage::Have { state, .. } = &haves[0] else { panic!("{haves:?}") };
    assert_eq!(cpm::decode_state(state).unwrap().gesture, None);
    unload();
}

#[test]
fn relayed_have_is_tiny_and_skips_rate_held_models() {
    let now = Instant::now();
    let mut relay = Relay::default();
    relay.joined(1);
    relay.joined(2);
    send(&mut relay, 2, &SyncMessage::Clear { uuid: 2 }, now);
    send(&mut relay, 1, &model(1, b"m0"), now);
    let state = vec![9u8; 500];
    send(&mut relay, 1, &SyncMessage::State { uuid: 1, data: state.clone() }, now);
    tick(&mut relay, now);
    // 1's Have, with a large state: the others get the digest alone (they have the state).
    let digest = cpm::model_digest(&model(1, b"m0"));
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest, state: state.clone() }, now);
    let out = tick(&mut relay, now);
    assert_eq!(got(&out, 2), vec![SyncMessage::Have { uuid: 1, digest, state: vec![] }]);
    assert!(out.iter().all(|(_, f)| f.len() < 64));
    // At most once in its window.
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest, state: state.clone() }, now);
    assert!(tick(&mut relay, now).is_empty());
    // A model change the rate holds back: its Have is not relayed (the others would all ask for a
    // model that is on its way).
    let later = now + cpm::RELAY_HAVE_WINDOW;
    for i in 1..=cpm::RELAY_MODEL_RELAYS as u8 {
        send(&mut relay, 1, &model(1, &[b'm', i]), later);
        tick(&mut relay, later);
    }
    let held = model(1, b"held");
    send(&mut relay, 1, &held, later);
    tick(&mut relay, later);
    send(&mut relay, 1, &SyncMessage::Have { uuid: 1, digest: cpm::model_digest(&held), state: vec![] }, later);
    assert!(got(&tick(&mut relay, later), 2).is_empty());
    // A Need for it then is not answered early either; the held change goes in its time.
    send(&mut relay, 2, &SyncMessage::Need { uuid: 1 }, later);
    assert!(got(&tick(&mut relay, later), 2).is_empty());
    assert_eq!(got(&tick(&mut relay, later + cpm::RELAY_MODEL_WINDOW), 2), vec![held]);
}

#[test]
fn unset_drops_received_state() {
    let _serial = serial();
    unload();
    feed(&SyncMessage::State { uuid: 5, data: vec![1, 2, 3] });
    assert!(cpm::net_state(5).is_some());
    // They left (or are held back): an old pose must not come back with their next model.
    feed(&SyncMessage::Unset { uuid: 5 });
    assert_eq!(cpm::net_state(5), None);
    unload();
}

#[test]
fn shared_status_only_with_others_here() {
    let _serial = serial();
    let dir = scratch("status_shared");
    load_local_with_gestures(&dir, "g.cpmmodel");
    client_tick(true, true);
    // Alone (single player): nothing to say of sharing.
    assert_eq!(cpm::net_status_line(), None);
    feed(&SyncMessage::Clear { uuid: 9 });
    assert_eq!(cpm::net_status_line(), cpm::share_status_line(&NetShare::Shared(ModelKind::Inline)));
    unload();
}

#[test]
fn need_answered_with_state_right_after() {
    let _serial = serial();
    let dir = scratch("need_state");
    let bytes = load_local_with_gestures(&dir, "g.cpmmodel");
    cpm::with_model(|m| m.engine.set_custom_pose(Some(1))).unwrap();
    assert_eq!(messages(&client_tick(true, true)).len(), 2);
    // The host lost the model (so it dropped the state too): both again, at once.
    feed(&SyncMessage::Need { uuid: 0 });
    let again = client_tick(true, true);
    assert_eq!(messages(&again)[0], model(0, &bytes));
    assert_eq!(states_in(&again).len(), 1);
    unload();
}

// ---- What is shared is the model, not its icon; the live status line; the gist button ----

/// A `.cpmmodel` of `data` with `icon` as its picker icon (CPM's layout: name, description, data,
/// an empty overflow, the icon).
fn file_with_icon(data: &[u8], icon: &[u8]) -> Vec<u8> {
    let mut d = Writer::new();
    d.bytes(data);
    let mut i = Writer::new();
    i.bytes(icon);
    let mut w = Writer::new();
    w.utf("iconic").utf("").block(&d).varint(0).block(&i);
    checksummed(0x53, &w.into_bytes())
}

#[test]
fn icon_alone_over_the_limit_still_shared_inline() {
    let _serial = serial();
    unload();
    let dir = scratch("icon_inline");
    // A small model whose file is over 64 KiB only because of its icon (as ProtogenPublic's ~11 KB
    // icon tips it over).
    let file = file_with_icon(&model_data(1, Some((64, 64, png(64, 64, 0xFF12_3456)))), &noise_png(160, 160));
    assert!(file.len() > cpm::SYNC_MAX_INLINE, "{}", file.len());
    let stripped = cpm_core::format::strip_icon(&file).unwrap();
    assert!(stripped.len() <= cpm::SYNC_MAX_INLINE);
    assert_eq!(cpm::share_plan(Some(&file)), SharePlan::Model(ModelKind::Inline, stripped.clone()));
    // A file that does not read is judged (and sent) as it is.
    let broken = vec![0u8; 10];
    assert_eq!(cpm::share_plan(Some(&broken)), SharePlan::Model(ModelKind::Inline, broken.clone()));
    let path = dir.join("iconic.cpmmodel");
    std::fs::write(&path, &file).unwrap();
    cpm::select(Some(path));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert_eq!(messages(&client_tick(true, true)), vec![model(0, &stripped)]);
    assert_eq!(cpm::net_share_status(), NetShare::Shared(ModelKind::Inline));
    assert_eq!(cpm::gist_button(), None);
    unload();
}

#[test]
fn status_line_follows_the_loaded_model_without_a_tick() {
    let _serial = serial();
    unload();
    let dir = scratch("status_live");
    let big = dir.join("big.cpmmodel");
    let bytes = skin::container(&model_data(1, Some((256, 256, noise_png(256, 256)))));
    std::fs::write(&big, &bytes).unwrap();
    let small = dir.join("small.cpmmodel");
    std::fs::write(&small, model_file(0xFF00_00FF, 1)).unwrap();
    cpm::select(Some(small.clone()));
    cpm::ensure_loaded();
    client_tick(true, true);
    feed(&SyncMessage::Clear { uuid: 9 });
    assert_eq!(cpm::net_status_line(), cpm::share_status_line(&NetShare::Shared(ModelKind::Inline)));
    // Switched on the settings page while the game is paused: no tick runs, the line follows.
    cpm::select(Some(big.clone()));
    cpm::ensure_loaded();
    assert_eq!(cpm::net_status_line(), cpm::share_status_line(&NetShare::HeldBack(bytes.len())));
    cpm::select(Some(small));
    cpm::ensure_loaded();
    assert_eq!(cpm::net_status_line(), cpm::share_status_line(&NetShare::Shared(ModelKind::Inline)));
    // Still nothing offline.
    client_tick_with(true, false, |_| true);
    cpm::select(Some(big));
    cpm::ensure_loaded();
    assert_eq!(cpm::net_status_line(), None);
    while cpm::take_gist_popup().is_some() {}
    unload();
}

#[test]
fn gist_button_makes_the_link_and_uses_it() {
    let _serial = serial();
    unload();
    let dir = scratch("gist_button");
    cpm::set_data_dir(dir.clone());
    let big = dir.join("big.cpmmodel");
    let bytes = skin::container(&model_data(1, Some((256, 256, noise_png(256, 256)))));
    std::fs::write(&big, &bytes).unwrap();
    // Nothing loaded: no button.
    assert_eq!(cpm::gist_button(), None);
    // Held back: the button, with no one else here and no tick.
    cpm::select(Some(big.clone()));
    cpm::ensure_loaded();
    assert_eq!(cpm::gist_button().as_deref(), Some("Make a gist link for big…"));
    // Right after the safety level on the page, before the information lines.
    let idle = cpm::ConvertView { busy: false, last: None, old_conversion: false };
    let view = cpm::OthersView { on: true, max: 8, ..Default::default() };
    let gist = cpm::gist_button();
    let labels = cpm::settings_page(Some(&big), Some("cpm: x"), &idle, &view, gist.as_deref());
    assert_eq!(labels[5], "Safety level: Medium");
    assert_eq!(labels[6], "Make a gist link for big…");
    assert_eq!(labels[7], "Other players' models loaded: 0");
    assert_eq!(cpm::click_page(6, &[], None, true), cpm::Click::MakeGist);
    assert_eq!(cpm::click_page(6, &[], None, false), cpm::Click::Nothing);
    assert_eq!(cpm::click_page(5, &[], None, true), cpm::Click::Safety);
    assert_eq!(cpm::click_page(4, &[], None, true), cpm::Click::MaxOthers);
    // A click: the gist text written, the popup due at once, every click (not once a session).
    assert!(cpm::make_gist());
    assert!(cpm::gist_path(&big).exists());
    assert_eq!(cpm::pending_gist_popup(), Some((big.clone(), bytes.len())));
    cpm::take_gist_popup();
    assert!(cpm::make_gist());
    let (model, size) = cpm::take_gist_popup().unwrap();
    // Save link: the link model is written, selected, and kept as the `model` setting.
    let mut p = cpm::GistPopup::new(model, size);
    p.paste("https://gist.github.com/someone/0123456789abcdef");
    cpm::show_gist_popup(p);
    let link = cpm::save_gist_link().unwrap().unwrap();
    assert_eq!(link, dir.join("big.link.cpmmodel"));
    assert!(link.exists());
    assert_eq!(cpm::selected(), Some(link.clone()));
    assert_eq!(cpm::take_model_setting_due(), Some(cpm::model_setting(Some(&link))));
    assert_eq!(cpm::take_model_setting_due(), None);
    cpm::hide_gist_popup();
    // A bad link: nothing selected or kept.
    cpm::select(Some(big.clone()));
    let mut p = cpm::GistPopup::new(big.clone(), size);
    p.paste("https://example.com/x");
    cpm::show_gist_popup(p);
    assert!(cpm::save_gist_link().unwrap().is_err());
    assert_eq!(cpm::selected(), Some(big));
    assert_eq!(cpm::take_model_setting_due(), None);
    cpm::hide_gist_popup();
    // A small model: no button, and a click does nothing.
    let small = dir.join("small.cpmmodel");
    std::fs::write(&small, model_file(0xFF00_00FF, 1)).unwrap();
    cpm::select(Some(small));
    cpm::ensure_loaded();
    assert_eq!(cpm::gist_button(), None);
    assert!(!cpm::make_gist());
    assert_eq!(cpm::pending_gist_popup(), None);
    unload();
}
