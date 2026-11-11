// The mod's runtime (game::cpm_mod) without the game: selection, loading, texture upload and the
// quads the player_submit helper draws. Each test runs on its own thread, so its own state.
mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cpm_core::PoseInput;
use horizon_mod_cpm as cpm;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("mixins/cpm/cpm-core/tests/samples").join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn loads_selected_model() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    assert_eq!(cpm::selected(), None);
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    assert_eq!(cpm::selected(), Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert!(cpm::with_model(|m| m.png_px[0] > 0).unwrap());
    // Selecting nothing drops it.
    cpm::select(None);
    cpm::ensure_loaded();
    assert!(cpm::with_model(|_| ()).is_none());
}

#[test]
fn bad_file_sets_error_and_no_model() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = scratch("bad_file");
    let bad = dir.join("broken.cpmmodel");
    std::fs::write(&bad, b"not a model").unwrap();
    cpm::select(Some(bad));
    cpm::ensure_loaded();
    let error = cpm::last_error().expect("an error");
    assert!(error.contains("broken.cpmmodel"), "{error}");
    assert!(cpm::with_model(|_| ()).is_none());
    // A missing file too.
    cpm::select(Some(dir.join("missing.cpmmodel")));
    cpm::ensure_loaded();
    assert!(cpm::last_error().unwrap().contains("missing.cpmmodel"));
    // A good one clears the error.
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert!(cpm::with_model(|_| ()).is_some());
}

#[test]
fn upload_once_then_periodic() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let [pw, ph] = cpm::with_model(|m| m.png_px).unwrap();
    let t0 = Instant::now();
    let (w, h, rgba) = cpm::texture_upload(t0).expect("first upload");
    assert_eq!((w, h), (pw + 4, ph.max(4)));
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    // The PNG at the top left...
    let png = cpm::with_model(|m| m.texture_rgba[..4].to_vec()).unwrap();
    assert_eq!(&rgba[..4], &png[..]);
    // ...and a white patch to its right.
    for y in 0..4 {
        for x in pw..pw + 4 {
            let i = ((y * w + x) * 4) as usize;
            assert_eq!(&rgba[i..i + 4], &[255, 255, 255, 255], "white at {x},{y}");
        }
    }
    assert!(cpm::texture_upload(t0 + Duration::from_secs(1)).is_none());
}

#[test]
fn reupload_is_periodic() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let t0 = Instant::now();
    assert!(cpm::texture_upload(t0).is_some());
    assert!(cpm::texture_upload(t0 + Duration::from_secs(1)).is_none());
    assert!(cpm::texture_upload(t0 + Duration::from_millis(4999)).is_none());
    assert!(cpm::texture_upload(t0 + Duration::from_secs(5)).is_some());
    assert!(cpm::texture_upload(t0 + Duration::from_secs(6)).is_none());
    // A new model uploads at once.
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    assert!(cpm::texture_upload(t0 + Duration::from_secs(7)).is_some());
    // Nothing selected: nothing to upload.
    cpm::select(None);
    cpm::ensure_loaded();
    assert!(cpm::texture_upload(t0 + Duration::from_secs(20)).is_none());
}

#[test]
fn oversize_texture_refused() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| {
        m.png_px = [2048, 16];
        m.texture_rgba = vec![0; 2048 * 16 * 4];
    });
    assert!(cpm::texture_upload(Instant::now()).is_none());
    assert!(cpm::last_error().unwrap().contains("1024"));
    // Refused: no model drawn.
    assert!(cpm::quads(&PoseInput::default(), &cpm::Worn::default()).is_none());
}

#[test]
fn texture_just_too_wide_for_the_white_patch_refused() {
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| {
        m.png_px = [1021, 8];
        m.texture_rgba = vec![0; 1021 * 8 * 4];
    });
    assert!(cpm::texture_upload(Instant::now()).is_none());
    assert!(cpm::last_error().is_some());
}

#[test]
fn uv_to_sprite_scales() {
    assert_eq!(cpm::uv_to_sprite([0.0, 0.0, 1024.0, 512.0]), [0.0, 0.0, 16.0, 8.0]);
    assert_eq!(cpm::uv_to_sprite([64.0, 32.0, 128.0, 64.0]), [1.0, 0.5, 2.0, 1.0]);
}

#[test]
fn face_uv_scales_declared_to_png_pixels() {
    // Declared 64x64, PNG 128x128: UVs double before going to sprite units.
    assert_eq!(cpm::slot_uv([8.0, 8.0, 16.0, 16.0], [64, 64], [128, 128], [0, 0]), cpm::uv_to_sprite([16.0, 16.0, 32.0, 32.0]));
    // Same size: unchanged.
    assert_eq!(cpm::slot_uv([8.0, 8.0, 16.0, 16.0], [64, 64], [64, 64], [0, 0]), cpm::uv_to_sprite([8.0, 8.0, 16.0, 16.0]));
    // No declared size: taken as the PNG's.
    assert_eq!(cpm::slot_uv([8.0, 8.0, 16.0, 16.0], [0, 0], [64, 64], [0, 0]), cpm::uv_to_sprite([8.0, 8.0, 16.0, 16.0]));
}

#[test]
fn white_uv_is_the_patch_centre() {
    let c = 66.0 * 16.0 / 1024.0;
    let r = 2.0 * 16.0 / 1024.0;
    assert_eq!(cpm::white_uv(64), [c, r, c, r]);
}

#[test]
fn quads_cover_every_face_in_blocks() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    assert!(cpm::quads(&PoseInput::default(), &cpm::Worn::default()).is_none());
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let input = PoseInput { time_ms: 1000, on_ground: true, ..PoseInput::default() };
    let faces: usize = cpm::with_model(|m| {
        m.pose(&input).iter().map(|b| b.faces.iter().flatten().count()).sum()
    })
    .unwrap();
    let png_w = cpm::with_model(|m| m.png_px[0]).unwrap();
    // Not before its texture went to the game.
    assert!(cpm::quads(&input, &cpm::Worn::default()).is_none());
    assert!(cpm::texture_upload(Instant::now()).is_some());
    let quads = cpm::quads(&input, &cpm::Worn::default()).expect("quads").quads;
    assert_eq!(quads.len(), faces);
    assert!(faces > 0);
    for q in &quads {
        for c in q.corners {
            assert!(c.iter().all(|v| v.is_finite()));
            // A player-sized model: within a few blocks of the feet.
            assert!(c.iter().all(|v| v.abs() < 4.0), "{c:?}");
        }
        // Inside the image this mod uploads.
        let max = (png_w + 4) as f32 * 16.0 / 1024.0;
        assert!(q.uv.iter().all(|v| (0.0..=max).contains(v)), "{:?}", q.uv);
    }
    // Something reaches above the waist: the model stands up, feet at 0.
    assert!(quads.iter().any(|q| q.corners.iter().any(|c| c[1] > 1.0)));
}

#[test]
fn untextured_boxes_use_the_white_patch() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let input = PoseInput::default();
    let png_w = cpm::with_model(|m| m.png_px[0]).unwrap();
    let flags: Vec<(bool, u32, usize)> = cpm::with_model(|m| {
        m.pose(&input).iter().map(|b| (b.textured, b.tint, b.faces.iter().flatten().count())).collect()
    })
    .unwrap();
    cpm::texture_upload(Instant::now()).unwrap();
    let quads = cpm::quads(&input, &cpm::Worn::default()).unwrap().quads;
    let mut i = 0;
    for (textured, tint, n) in flags {
        for q in &quads[i..i + n] {
            assert_eq!(q.tint, tint);
            if !textured {
                assert_eq!(q.uv, cpm::white_uv(png_w));
            }
        }
        i += n;
    }
}

#[test]
fn arm_quads_are_the_posed_arm_in_blocks() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    assert!(cpm::arm_quads(false).is_none());
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    // Not before its texture went to the game.
    assert!(cpm::arm_quads(false).is_none());
    cpm::texture_upload(Instant::now()).unwrap();
    let png = cpm::with_model(|m| m.png_px).unwrap();
    for left in [false, true] {
        let boxes = cpm::with_model(|m| m.pose_arm(left)).unwrap();
        let quads = cpm::arm_quads(left).expect("arm quads");
        // The same corner transform and UV mapping as the whole player's quads.
        let mut want = None;
        cpm::with_model(|m| want = Some(cpm::box_quads(m, &boxes, &cpm::Worn::default()))).unwrap();
        assert_eq!(Some(&quads), want.as_ref());
        assert_eq!(quads.sprites, vec![cpm::SPRITE.to_owned()]);
        let quads = quads.quads;
        assert_eq!(quads.len(), boxes.iter().map(|b| b.faces.iter().flatten().count()).sum::<usize>());
        for q in &quads {
            // An arm: within a block of its shoulder, on its own side (+x left).
            assert!(q.corners.iter().flatten().all(|v| v.abs() < 1.0), "{:?}", q.corners);
            assert!(q.corners.iter().all(|c| (c[0] > 0.0) == left), "{left}: {:?}", q.corners);
            let max = (png[0] + 4) as f32 * 16.0 / 1024.0;
            assert!(q.uv.iter().all(|v| (0.0..=max).contains(v)), "{:?}", q.uv);
        }
    }
}

#[test]
fn clock_counts_from_the_first_call() {
    let t0 = Instant::now();
    assert_eq!(cpm::clock_ms(t0), 0);
    assert_eq!(cpm::clock_ms(t0 + Duration::from_millis(1500)), 1500);
}

#[test]
fn animated_texture_frame_uploads_again() {
    if common::missing(&["SkullDog.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    // A 1x1 region at (0, 0) showing frames along x from (1, 0), 100 ms each.
    let first = cpm::with_model(|m| {
        m.anim_textures = vec![cpm_core::anim::texture::AnimatedTexture::new(cpm_core::anim::texture::AnimTex {
            sheet: 0,
            uv_start: [0, 0],
            uv_size: [1, 1],
            anim_start: [1, 0],
            frame_count: 2,
            frame_time: 100,
            an_x: true,
            interpolate: false,
        })];
        let w = m.png_px[0] as usize;
        m.texture_rgba[..4].copy_from_slice(&[1, 2, 3, 4]);
        m.texture_rgba[4..8].copy_from_slice(&[10, 20, 30, 40]);
        m.texture_rgba[8..12].copy_from_slice(&[50, 60, 70, 80]);
        w
    })
    .unwrap();
    assert!(first > 2);
    let t0 = Instant::now();
    assert_eq!(cpm::clock_ms(t0), 0);
    assert!(cpm::texture_upload(t0).is_some());
    // Frame 0 still: nothing changed, nothing sent.
    cpm::tick_textures(t0 + Duration::from_millis(50));
    assert!(cpm::texture_upload(t0 + Duration::from_millis(50)).is_none());
    // Frame 1: its pixel is copied over the region and the image is sent at once.
    cpm::tick_textures(t0 + Duration::from_millis(120));
    let (_, _, rgba) = cpm::texture_upload(t0 + Duration::from_millis(120)).expect("upload on a new frame");
    assert_eq!(&rgba[..4], &[50, 60, 70, 80]);
    cpm::tick_textures(t0 + Duration::from_millis(150));
    assert!(cpm::texture_upload(t0 + Duration::from_millis(150)).is_none());
    // Back to frame 0.
    cpm::tick_textures(t0 + Duration::from_millis(210));
    let (_, _, rgba) = cpm::texture_upload(t0 + Duration::from_millis(210)).expect("upload on frame 0");
    assert_eq!(&rgba[..4], &[10, 20, 30, 40]);
}

#[test]
fn animated_texture_after_switching_to_a_bigger_model() {
    if common::missing(&["SkullDog.cpmmodel", "ReggieTheRat.cpmmodel"]) {
        return;
    }
    let t0 = Instant::now();
    assert_eq!(cpm::clock_ms(t0), 0);
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let (sw, sh, _) = cpm::texture_upload(t0).unwrap();
    assert!(sw < 1000 && sh < 1000, "{sw}x{sh}");
    // Switch (as the settings page does) to a model with a bigger, animated texture.
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| {
        m.png_px = [1000, 1000];
        m.texture_rgba = vec![0; 1000 * 1000 * 4];
        m.texture_rgba[4..8].copy_from_slice(&[10, 20, 30, 40]);
        m.texture_rgba[8..12].copy_from_slice(&[50, 60, 70, 80]);
        m.anim_textures = vec![cpm_core::anim::texture::AnimatedTexture::new(cpm_core::anim::texture::AnimTex {
            sheet: 0,
            uv_start: [0, 0],
            uv_size: [1, 1],
            anim_start: [1, 0],
            frame_count: 2,
            frame_time: 100,
            an_x: true,
            interpolate: false,
        })];
    })
    .unwrap();
    // Frame 1 on its first tick: the old model's image must not be written into.
    let t = t0 + Duration::from_millis(120);
    cpm::tick_textures(t);
    let (w, h, rgba) = cpm::texture_upload(t).expect("the new model uploads");
    assert_eq!((w, h), (1004, 1000));
    assert_eq!(&rgba[..4], &[50, 60, 70, 80]);
}

#[test]
fn refused_upload_drops_the_model() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    cpm::texture_upload(Instant::now()).unwrap();
    cpm::refuse("cpm: SkullDog.cpmmodel: the game refused the texture".into());
    assert!(cpm::last_error().unwrap().contains("refused"));
    assert!(cpm::quads(&PoseInput::default(), &cpm::Worn::default()).is_none());
    assert!(cpm::texture_upload(Instant::now() + Duration::from_secs(10)).is_none());
    // Not retried until the selection changes.
    cpm::ensure_loaded();
    assert!(cpm::with_model(|_| ()).is_none());
}

// --- Task 10: settings page, model picker, gesture menu key ---

#[test]
fn models_dir_is_game_root() {
    let root = scratch("models_dir_root").join("horizon");
    let data = root.join("horizon-mods").join(".horizon-mod-data").join("cpm");
    let dir = cpm::models_dir(&data);
    assert_eq!(dir, root.join("player_models"));
    assert!(dir.is_dir(), "created when missing");
    // Again: already there.
    assert_eq!(cpm::models_dir(&data), root.join("player_models"));
}

#[test]
fn list_models_sorted_cpmmodel_only() {
    let dir = scratch("list_models");
    assert!(cpm::list_models(&dir).is_empty());
    std::fs::write(dir.join("notes.txt"), b"").unwrap();
    std::fs::write(dir.join("b.cpmmodel"), b"").unwrap();
    std::fs::write(dir.join("A.CPMMODEL"), b"").unwrap();
    std::fs::create_dir(dir.join("c.cpmmodel")).unwrap();
    assert_eq!(cpm::list_models(&dir), vec![dir.join("A.CPMMODEL"), dir.join("b.cpmmodel")]);
    assert!(cpm::list_models(&dir.join("missing")).is_empty());
}

/// The settings page's convert state with nothing running or done, the default gist limit.
fn idle() -> cpm::ConvertView<'static> {
    cpm::ConvertView { busy: false, last: None, limit_kib: 8, old_conversion: false }
}

#[test]
fn labels_none_selected() {
    assert_eq!(
        cpm::settings_labels(None, None, &idle()),
        ["Model: None", "Open models folder", "Convert project…", "Gist limit: 8 KiB", "Gestures…"]
    );
}

#[test]
fn labels_selected_and_error() {
    assert_eq!(
        cpm::settings_labels(Some(Path::new("m/b.cpmmodel")), Some("cpm: b.cpmmodel: bad"), &idle()),
        ["Model: b", "Open models folder", "Convert project…", "Gist limit: 8 KiB", "Gestures…", "Error: cpm: b.cpmmodel: bad"]
    );
}

#[test]
fn settings_has_gestures_button() {
    let labels = cpm::settings_labels(None, None, &idle());
    assert!(!labels.iter().any(|l| l.starts_with("Gesture ") || l.starts_with("Pose ")));
    let at = labels.iter().position(|l| l == "Gestures…").expect("button");
    assert_eq!(cpm::click(at, &[], None), cpm::Click::Gestures);
    assert!(!cpm::take_menu_request());
    cpm::request_menu();
    assert!(cpm::take_menu_request());
    assert!(!cpm::take_menu_request(), "taken once");
}

#[test]
fn click_cycles_models() {
    let a = PathBuf::from("m/a.cpmmodel");
    let b = PathBuf::from("m/b.cpmmodel");
    let models = vec![a.clone(), b.clone()];
    use cpm::Click;
    assert_eq!(cpm::click(0, &models, None), Click::Select(Some(a.clone())));
    assert_eq!(cpm::click(0, &models, Some(&a)), Click::Select(Some(b.clone())));
    assert_eq!(cpm::click(0, &models, Some(&b)), Click::Select(None));
    // A selection no longer on disk starts over at the first.
    assert_eq!(cpm::click(0, &models, Some(Path::new("m/gone.cpmmodel"))), Click::Select(Some(a.clone())));
    assert_eq!(cpm::click(0, &[], None), Click::Select(None));
    assert_eq!(cpm::click(1, &models, None), Click::OpenFolder);
    assert_eq!(cpm::click(2, &models, None), Click::Convert);
    assert_eq!(cpm::click(3, &models, None), Click::GistLimit);
    assert_eq!(cpm::click(4, &models, None), Click::Gestures);
    assert_eq!(cpm::click(5, &models, None), Click::OthersToggle);
    assert_eq!(cpm::click(6, &models, None), Click::MaxOthers);
    assert_eq!(cpm::click(7, &models, None), Click::Nothing);
    // Stored as the file name ("" for none).
    assert_eq!(cpm::model_setting(Some(&b)), "b.cpmmodel");
    assert_eq!(cpm::model_setting(None), "");
}

#[test]
fn selection_falls_back_when_missing() {
    let dir = scratch("selection_setting");
    std::fs::write(dir.join("a.cpmmodel"), b"").unwrap();
    assert_eq!(cpm::selection_from_setting(&dir, Some("a.cpmmodel".into())), Some(dir.join("a.cpmmodel")));
    assert_eq!(cpm::selection_from_setting(&dir, Some("gone.cpmmodel".into())), None);
    assert_eq!(cpm::selection_from_setting(&dir, None), None);
    assert_eq!(cpm::selection_from_setting(&dir, Some(String::new())), None);
    // Only a file directly in the folder.
    std::fs::write(dir.parent().unwrap().join("outside.cpmmodel"), b"").unwrap();
    assert_eq!(cpm::selection_from_setting(&dir, Some("../outside.cpmmodel".into())), None);
}

#[test]
fn keys_only_menu() {
    assert_eq!(cpm::keys(), [["gesture_menu".to_string(), "Gesture menu".into(), "key.keyboard.g".into()]]);
    assert_eq!(cpm::MENU_KEY, "gesture_menu");
    assert_eq!(cpm::DEFAULT_MENU_KEY, "key.keyboard.g");
}

#[test]
fn params_encoding() {
    assert_eq!(cpm::params_setting_key("a.cpmmodel"), "params.a.cpmmodel");
    assert_eq!(cpm::encode_params(&[0, 15, 16, 255]), "000f10ff");
    assert_eq!(cpm::decode_params("000f10ff"), Some(vec![0, 15, 16, 255]));
    assert_eq!(cpm::decode_params("000F10FF"), Some(vec![0, 15, 16, 255]));
    assert_eq!(cpm::decode_params(""), Some(vec![]));
    assert_eq!(cpm::decode_params("abc"), None);
    assert_eq!(cpm::decode_params("zz"), None);
    assert_eq!(cpm::decode_params("+1"), None);
    assert_eq!(cpm::decode_params("é"), None);
}

/// Loads a sample, then swaps its animations for: gestures `wave` (looping), `glow` (value
/// layer: not playable), `hat` (layer) and custom poses `sit`, `lie`.
fn load_with_gestures() {
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
    let set = AnimSet {
        poses: vec![(Pose::Custom("sit".into()), clip()), (Pose::Custom("lie".into()), clip())],
        gestures: vec![g("wave", GestureKind::Gesture), g("glow", GestureKind::Value), g("hat", GestureKind::Layer)],
        ..Default::default()
    };
    // Unload first, so a second call loads again (and restores parameters again).
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.engine = cpm_core::Engine::new(set)).unwrap();
}

/// The index of the first toggle button of the loaded `load_with_gestures` model.
fn toggle_button() -> usize {
    cpm::with_model(|m| m.engine.buttons().iter().position(|b| matches!(b.kind, cpm_core::anim::ButtonKind::Toggle))).unwrap().expect("a toggle")
}

#[test]
fn stale_params_ignored() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    load_with_gestures();
    let defaults = cpm::with_model(|m| m.engine.parameters()).unwrap();
    // A length that is not the model's: ignored.
    let stale = cpm::encode_params(&vec![9; defaults.len() + 1]);
    cpm::restore_params(|k| (k == "params.SkullDog.cpmmodel").then(|| stale.clone()));
    assert_eq!(cpm::with_model(|m| m.engine.parameters()).unwrap(), defaults);
    assert_eq!(cpm::take_params_to_save(), None);
    // Bad hex: ignored.
    load_with_gestures();
    cpm::restore_params(|_| Some("nothex".into()));
    assert_eq!(cpm::with_model(|m| m.engine.parameters()).unwrap(), defaults);
}

#[test]
fn params_restored_and_saved_after_change() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    load_with_gestures();
    let key = "params.SkullDog.cpmmodel";
    // Nothing kept: nothing to save yet.
    cpm::restore_params(|_| None);
    assert_eq!(cpm::take_params_to_save(), None);
    // Restoring happens once per load.
    cpm::restore_params(|_| panic!("already restored"));
    let t = toggle_button();
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    let changed = cpm::with_model(|m| m.engine.parameters()).unwrap();
    assert_eq!(cpm::take_params_to_save(), Some((key.to_string(), cpm::encode_params(&changed))));
    // Unchanged since: no second save.
    assert_eq!(cpm::take_params_to_save(), None);
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    assert!(cpm::take_params_to_save().is_some(), "changed back is a change");

    // A new load restores what was kept, and that is not saved again.
    load_with_gestures();
    let kept = cpm::encode_params(&changed);
    cpm::restore_params(|k| (k == key).then(|| kept.clone()));
    assert_eq!(cpm::with_model(|m| m.engine.parameters()).unwrap(), changed);
    assert_eq!(cpm::take_params_to_save(), None);
}

#[test]
fn no_save_while_a_slider_is_dragged() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    load_with_gestures();
    cpm::restore_params(|_| None);
    let t = toggle_button();
    cpm::show_menu();
    cpm::with_menu(|m| m.dragging = Some(t)).unwrap();
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    assert_eq!(cpm::take_params_to_save(), None, "no save during a drag");
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    assert_eq!(cpm::take_params_to_save(), None);
    // Release: exactly one save, with the final value.
    cpm::with_menu(|m| m.dragging = None).unwrap();
    let fin = cpm::with_model(|m| m.engine.parameters()).unwrap();
    let key = "params.SkullDog.cpmmodel".to_string();
    assert_eq!(cpm::take_params_to_save(), Some((key, cpm::encode_params(&fin))));
    assert_eq!(cpm::take_params_to_save(), None);
    // Closing the menu mid-drag also lets the next poll save.
    cpm::with_menu(|m| m.dragging = Some(t)).unwrap();
    cpm::with_model(|m| m.engine.press(t, 0)).unwrap();
    assert_eq!(cpm::take_params_to_save(), None);
    cpm::hide_menu();
    assert!(cpm::take_params_to_save().is_some());
}

#[test]
fn model_without_texture_is_refused() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let mut model = cpm::with_model(|m| std::mem::replace(m, cpm_core::load(&std::fs::read(sample("SkullDog.cpmmodel")).unwrap()).unwrap())).unwrap();
    assert_eq!(cpm::check_texture("SkullDog.cpmmodel", &model), Ok(()));
    model.texture_rgba.clear();
    model.png_px = [0, 0];
    assert_eq!(cpm::check_texture("SkullDog.cpmmodel", &model), Err("cpm: SkullDog.cpmmodel: model has no texture".into()));
}

#[test]
fn has_model_follows_the_load() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    assert!(!cpm::has_model());
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    cpm::select(None);
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
}

#[test]
fn selected_name_is_the_file_name() {
    assert_eq!(cpm::selected_name(), "");
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    assert_eq!(cpm::selected_name(), "SkullDog.cpmmodel");
}

#[test]
fn stored_selection_is_restored_once() {
    let dir = scratch("restore_once");
    std::fs::write(dir.join("a.cpmmodel"), b"").unwrap();
    std::fs::write(dir.join("b.cpmmodel"), b"").unwrap();
    cpm::restore_once(|| cpm::selection_from_setting(&dir, Some("a.cpmmodel".into())));
    assert_eq!(cpm::selected(), Some(dir.join("a.cpmmodel")));
    // Later calls leave the selection (a click may have changed it) and do not read the setting.
    cpm::select(Some(dir.join("b.cpmmodel")));
    cpm::restore_once(|| panic!("read again"));
    assert_eq!(cpm::selected(), Some(dir.join("b.cpmmodel")));
}

/// The bundle's `[api]` package: what puts CPM in `mod_api::mods()`, without which the game never
/// reserves the `horizon/modimage` sprite (the launcher's `native_mods::api` rules; `native.py check`
/// compiles it into the generated game).
#[test]
fn bundle_declares_the_trait_mod_package() {
    let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mixins/cpm");
    // Line endings as checked out (CRLF on Windows) don't matter.
    let read = |path: &str| std::fs::read_to_string(bundle.join(path)).unwrap().replace("\r\n", "\n");
    let manifest = read("horizon-mixin.toml");
    // `[api]` accepts only `path`, a folder in the bundle with a Cargo.toml.
    let api = manifest.split("\n[api]\n").nth(1).expect("an [api] table");
    let table = api.split("\n[").next().unwrap();
    let keys: Vec<&str> = table.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).collect();
    assert_eq!(keys, ["path = \"api\""]);
    let cargo = read("api/Cargo.toml");
    assert!(cargo.lines().any(|l| l.starts_with("horizon-client = ")), "{cargo}");
    // Horizon's crates only: horizon-client is its one dependency.
    let deps = cargo.split("[dependencies]").nth(1).unwrap();
    assert_eq!(deps.lines().filter(|l| l.contains('=')).count(), 1, "{deps}");
    let lib = read("api/src/lib.rs");
    assert!(lib.contains("pub fn hooks() -> Box<dyn Hooks>"), "{lib}");
    assert!(lib.contains("impl Hooks for Cpm {}"), "overrides nothing: {lib}");
}

// ---- Converting a .cpmproject (the settings page's Convert project… button) ----

/// A fresh directory under the system temp dir, unique to this test and process.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cpm-runtime-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn expie() -> PathBuf {
    sample("Expie_by_Clouddy.cpmproject")
}

fn description(model: &Path) -> String {
    cpm_core::format::read_cpmmodel(&std::fs::read(model).unwrap()).unwrap().desc
}

#[test]
fn output_path_rules() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("output_path");
    let project = Path::new("somewhere/Foo.cpmproject");
    // Free: <stem>.cpmmodel.
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo.cpmmodel"));
    // Taken by a file that is not ours (not even a model): the next free _N.
    std::fs::write(dir.join("Foo.cpmmodel"), b"someone else's").unwrap();
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo_2.cpmmodel"));
    std::fs::copy(sample("SkullDog.cpmmodel"), dir.join("Foo_2.cpmmodel")).unwrap();
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo_3.cpmmodel"));
    // Our own earlier conversion is overwritten.
    let cpm::ConvertResult::Saved { model, .. } = cpm::convert_project(&expie(), &dir, 1024) else { panic!() };
    assert_eq!(model, dir.join("Expie_by_Clouddy.cpmmodel"));
    assert_eq!(cpm::output_path(&dir, &expie()), model);
    // A real model that is not one of ours keeps its name.
    std::fs::copy(sample("ReggieTheRat.cpmmodel"), dir.join("Reggie.cpmmodel")).unwrap();
    assert_eq!(cpm::converted_version(&description(&dir.join("Reggie.cpmmodel"))), None);
    assert_eq!(cpm::output_path(&dir, Path::new("Reggie.cpmproject")), dir.join("Reggie_2.cpmmodel"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn convert_expie_writes_model() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("convert_expie");
    let result = cpm::convert_project(&expie(), &dir, 1024);
    let model = dir.join("Expie_by_Clouddy.cpmmodel");
    let size = std::fs::metadata(&model).unwrap().len() as usize;
    // Every feature the reader skipped is counted (and logged): nothing in Expie now that its
    // cape root and cape texture are carried.
    let skipped = cpm_core::project::read_project(&std::fs::read(expie()).unwrap()).unwrap().warnings.len();
    assert_eq!(skipped, 0);
    assert_eq!(result, cpm::ConvertResult::Saved { model: model.clone(), too_big: false, size, skipped });
    // It loads, and carries the converter marker after the project's own description.
    let bytes = std::fs::read(&model).unwrap();
    assert!(cpm_core::load(&bytes).is_ok());
    let project = cpm_core::project::read_project(&std::fs::read(expie()).unwrap()).unwrap();
    assert_eq!(description(&model), format!("{} (converted by CPM for Horizon, v4)", project.desc));
    assert!(!dir.join("Expie_by_Clouddy.gist.txt").exists());
    // Converting again overwrites it rather than making Expie_by_Clouddy_2.
    assert_eq!(cpm::convert_project(&expie(), &dir, 1024), result);
    assert_eq!(cpm::list_models(&dir), vec![model]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn too_big_writes_full_model_and_flags_gist() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("too_big");
    let cpm::ConvertResult::Saved { model, too_big, size, .. } = cpm::convert_project(&expie(), &dir, 1) else {
        panic!("not saved")
    };
    assert!(too_big);
    assert!(size > 1024);
    let bytes = std::fs::read(&model).unwrap();
    assert_eq!(bytes.len(), size);
    let gist = dir.join("Expie_by_Clouddy.gist.txt");
    assert_eq!(cpm::gist_path(&model), gist);
    let text = std::fs::read_to_string(&gist).unwrap();
    assert_eq!(text, cpm_core::link::gist_text(&bytes).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bad_project_writes_nothing() {
    let dir = temp("bad_project");
    let models = dir.join("player_models");
    let bad = dir.join("broken.cpmproject");
    std::fs::write(&bad, b"not a zip").unwrap();
    let cpm::ConvertResult::Failed(error) = cpm::convert_project(&bad, &models, 8) else { panic!() };
    assert!(error.contains("broken.cpmproject"), "{error}");
    let cpm::ConvertResult::Failed(error) = cpm::convert_project(&dir.join("gone.cpmproject"), &models, 8) else {
        panic!()
    };
    assert!(error.contains("gone.cpmproject"), "{error}");
    assert!(!models.exists() || std::fs::read_dir(&models).unwrap().next().is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn gist_limit_parse() {
    assert_eq!(cpm::gist_limit(None), 8);
    assert_eq!(cpm::gist_limit(Some("16".into())), 16);
    assert_eq!(cpm::gist_limit(Some(" 32 ".into())), 32);
    assert_eq!(cpm::gist_limit(Some("".into())), 8);
    assert_eq!(cpm::gist_limit(Some("lots".into())), 8);
    assert_eq!(cpm::gist_limit(Some("0".into())), 1);
    assert_eq!(cpm::gist_limit(Some("5000".into())), 1024);
    // The button cycles 4, 8, 16, 32, 64; anything else starts over at 4.
    let cycle: Vec<u32> = [4, 8, 16, 32, 64, 1, 100].into_iter().map(cpm::next_gist_limit).collect();
    assert_eq!(cycle, [8, 16, 32, 64, 4, 4, 4]);
    assert_eq!(cpm::GIST_LIMIT_SETTING, "gist_limit_kib");
}

/// The only test touching the job handoff (a process-wide static, shared by the test threads).
#[test]
fn picker_cancel_is_noop() {
    assert!(!cpm::convert_busy());
    assert!(cpm::start_convert_job());
    assert!(cpm::convert_busy());
    // A second start while one runs is refused.
    assert!(!cpm::start_convert_job());
    cpm::finish_convert_job(None);
    assert!(!cpm::convert_busy());
    assert_eq!(cpm::take_convert_result(), None);
    // A result is handed over once.
    assert!(cpm::start_convert_job());
    cpm::finish_convert_job(Some(cpm::ConvertResult::Failed("boom".into())));
    assert!(!cpm::convert_busy());
    assert_eq!(cpm::take_convert_result(), Some(cpm::ConvertResult::Failed("boom".into())));
    assert_eq!(cpm::take_convert_result(), None);
    // poll_convert: true once per ended job (result or cancelled picker), so the frame hook
    // rebuilds the settings page and a click on a stale `Converting…` only shows the outcome.
    let _ = cpm::poll_convert();
    assert!(!cpm::poll_convert(), "nothing ended since");
    assert!(cpm::start_convert_job());
    assert!(!cpm::poll_convert(), "still running");
    cpm::finish_convert_job(Some(cpm::ConvertResult::Failed("late".into())));
    assert!(cpm::poll_convert());
    assert_eq!(cpm::last_convert(), Some(cpm::ConvertResult::Failed("late".into())));
    assert!(!cpm::poll_convert());
    assert!(cpm::start_convert_job());
    cpm::finish_convert_job(None);
    assert!(cpm::poll_convert(), "a cancelled picker ends the job too");
    assert!(!cpm::poll_convert());
}

#[test]
fn noted_result_is_shown_and_too_big_pends_popup() {
    assert_eq!(cpm::last_convert(), None);
    let small = cpm::ConvertResult::Saved { model: "m/a.cpmmodel".into(), too_big: false, size: 2048, skipped: 0 };
    cpm::note_convert_result(small.clone());
    assert_eq!(cpm::last_convert(), Some(small));
    assert_eq!(cpm::pending_gist_popup(), None);
    let big = cpm::ConvertResult::Saved { model: "m/b.cpmmodel".into(), too_big: true, size: 40000, skipped: 0 };
    cpm::note_convert_result(big.clone());
    assert_eq!(cpm::last_convert(), Some(big));
    assert_eq!(cpm::pending_gist_popup(), Some((PathBuf::from("m/b.cpmmodel"), 40000)));
    assert_eq!(cpm::take_gist_popup(), Some((PathBuf::from("m/b.cpmmodel"), 40000)));
    assert_eq!(cpm::pending_gist_popup(), None);
    // An error replaces the line but leaves no popup.
    cpm::note_convert_result(cpm::ConvertResult::Failed("x".into()));
    assert_eq!(cpm::pending_gist_popup(), None);
}

#[test]
fn labels_show_convert_state() {
    let view = |busy, last, limit_kib| cpm::ConvertView { busy, last, limit_kib, old_conversion: false };
    let labels = |v: &cpm::ConvertView<'_>| cpm::settings_labels(None, None, v)[2..].to_vec();
    assert_eq!(labels(&view(false, None, 8)), ["Convert project…", "Gist limit: 8 KiB", "Gestures…"]);
    assert_eq!(labels(&view(true, None, 64)), ["Converting…", "Gist limit: 64 KiB", "Gestures…"]);
    let small = cpm::ConvertResult::Saved { model: "m/a.cpmmodel".into(), too_big: false, size: 2049, skipped: 0 };
    assert_eq!(
        labels(&view(false, Some(&small), 8)),
        ["Convert project…", "Gist limit: 8 KiB", "Gestures…", "Converted: a.cpmmodel (3 KiB)"]
    );
    let skipped = cpm::ConvertResult::Saved { model: "m/a.cpmmodel".into(), too_big: false, size: 2049, skipped: 2 };
    assert_eq!(cpm::convert_label(&skipped), "Converted: a.cpmmodel (3 KiB) (2 skipped, see log)");
    let big = cpm::ConvertResult::Saved { model: "m/b.cpmmodel".into(), too_big: true, size: 40000, skipped: 0 };
    assert_eq!(
        labels(&view(false, Some(&big), 8)),
        [
            "Convert project…",
            "Gist limit: 8 KiB",
            "Gestures…",
            "Converted: b.cpmmodel — too big: the gist popup opens when you return to the game"
        ]
    );
    let big_skipped = cpm::ConvertResult::Saved { model: "m/b.cpmmodel".into(), too_big: true, size: 40000, skipped: 1 };
    assert_eq!(
        cpm::convert_label(&big_skipped),
        "Converted: b.cpmmodel — too big: the gist popup opens when you return to the game (1 skipped, see log)"
    );
    let failed = cpm::ConvertResult::Failed("broken.cpmproject: bad zip".into());
    // The result line comes before the model's error line.
    assert_eq!(
        cpm::settings_labels(None, Some("cpm: x: bad"), &view(true, Some(&failed), 4))[2..],
        ["Converting…", "Gist limit: 4 KiB", "Gestures…", "Convert error: broken.cpmproject: bad zip", "Error: cpm: x: bad"]
    );
}

#[test]
fn powershell_picker_takes_the_folder_from_the_environment() {
    let script = cpm::powershell_picker_script();
    // The folder is never put in the script: no quoting to get wrong.
    assert_eq!(cpm::PICKER_DIR_ENV, "CPM_DIR");
    assert!(script.contains("$d.InitialDirectory=$env:CPM_DIR;"), "{script}");
    assert!(script.contains("Add-Type -AssemblyName System.Windows.Forms;"), "{script}");
    assert!(script.contains("$d.Filter='CPM project (*.cpmproject)|*.cpmproject';"), "{script}");
    // Shown over a topmost owner, so it does not open behind the game.
    assert!(script.contains("$f=New-Object System.Windows.Forms.Form -Property @{TopMost=$true};"), "{script}");
    assert!(script.contains("if($d.ShowDialog($f) -eq 'OK'){ $d.FileName }; $f.Dispose()"), "{script}");
}

#[test]
fn picker_output_rules() {
    use cpm::picker_outcome as outcome;
    // A pick (trimmed).
    assert_eq!(outcome(b"C:\\m\\a.cpmproject\r\n", true, b"", true), Ok(Some(PathBuf::from("C:\\m\\a.cpmproject"))));
    // Cancelled: nothing printed (zenity, kdialog, osascript exit non-zero on cancel).
    assert_eq!(outcome(b"", true, b"", true), Ok(None));
    assert_eq!(outcome(b"\n", false, b"", false), Ok(None));
    assert_eq!(outcome(b"", false, b"some noise\n", false), Ok(None));
    // On Windows a cancel exits 0: a failure with a message is an error, its first line.
    assert_eq!(outcome(b"", false, b"\r\nAdd-Type: boom\r\nmore\r\n", true), Err("Add-Type: boom".into()));
    assert_eq!(outcome(b"", false, b"", true), Ok(None));
}

#[test]
fn failed_gist_write_leaves_earlier_conversion() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("failed_gist");
    let cpm::ConvertResult::Saved { model, .. } = cpm::convert_project(&expie(), &dir, 1024) else { panic!() };
    let before = std::fs::read(&model).unwrap();
    // The gist text cannot go where it belongs.
    std::fs::create_dir(cpm::gist_path(&model)).unwrap();
    let cpm::ConvertResult::Failed(error) = cpm::convert_project(&expie(), &dir, 1) else { panic!() };
    assert!(error.contains("Expie_by_Clouddy"), "{error}");
    assert_eq!(std::fs::read(&model).unwrap(), before);
    // No temporary files left behind.
    let mut names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["Expie_by_Clouddy.cpmmodel", "Expie_by_Clouddy.gist.txt"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- The gist popup (Task 7) ----

#[test]
fn field_editing() {
    let mut p = cpm::GistPopup::new("m/Foo.cpmmodel".into(), 20000);
    assert_eq!((p.url.as_str(), p.cursor), ("", 0));
    for c in "https://x".chars() {
        p.type_char(c);
    }
    assert_eq!((p.url.as_str(), p.cursor), ("https://x", 9));
    p.backspace();
    assert_eq!((p.url.as_str(), p.cursor), ("https://", 8));
    // Pasted text loses its surrounding whitespace and line breaks; the cursor stays at the end.
    p.paste("  gist.github.com/alice/abc123\r\n");
    assert_eq!(p.url, "https://gist.github.com/alice/abc123");
    assert_eq!(p.cursor, p.url.len());
    // Control characters are not typed; backspace on a non-ASCII char removes the whole char.
    p.type_char('\u{8}');
    p.type_char('é');
    assert_eq!(p.cursor, p.url.len());
    p.backspace();
    assert_eq!(p.url, "https://gist.github.com/alice/abc123");
    let mut empty = cpm::GistPopup::new("m/Foo.cpmmodel".into(), 1);
    empty.backspace();
    assert_eq!((empty.url.as_str(), empty.cursor), ("", 0));
}

#[test]
fn submit_writes_link_model() {
    let dir = temp("gist_submit");
    let model = dir.join("Foo.cpmmodel");
    let mut p = cpm::GistPopup::new(model.clone(), 20000);
    p.paste("https://gist.github.com/alice/abc123");
    let link = p.submit("Foo", "a fox (converted by CPM for Horizon)", cpm_core::format::SkinType::Slim).unwrap();
    assert_eq!(link, dir.join("Foo.link.cpmmodel"));
    let bytes = std::fs::read(&link).unwrap();
    let file = cpm_core::format::read_cpmmodel(&bytes).unwrap();
    assert_eq!((file.name.as_str(), file.desc.as_str()), ("Foo", "a fox (converted by CPM for Horizon)"));
    assert_eq!(file.data.skin_type, cpm_core::format::SkinType::Slim);
    match cpm_core::load(&bytes) {
        Err(cpm_core::CpmError::NeedsLink(l)) => assert_eq!(l, "git:alice/abc123"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_link_reads_the_converted_model() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("gist_save");
    let cpm::ConvertResult::Saved { model, size, .. } = cpm::convert_project(&expie(), &dir, 1) else { panic!() };
    let full = cpm_core::format::read_cpmmodel(&std::fs::read(&model).unwrap()).unwrap();
    let mut p = cpm::GistPopup::new(model.clone(), size);
    p.paste("https://gist.github.com/alice/abc123#file-x");
    let link = p.save_link().unwrap();
    assert_eq!(link, dir.join("Expie_by_Clouddy.link.cpmmodel"));
    let file = cpm_core::format::read_cpmmodel(&std::fs::read(&link).unwrap()).unwrap();
    assert_eq!((file.name, file.desc), (full.name, full.desc));
    assert_eq!(file.data.skin_type, full.data.skin_type);
    assert_eq!(p.message.as_deref(), Some("Saved Expie_by_Clouddy.link.cpmmodel"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn submit_bad_url_message() {
    let dir = temp("gist_bad_url");
    let mut p = cpm::GistPopup::new(dir.join("Foo.cpmmodel"), 20000);
    p.paste("https://example.com/alice/abc123");
    let bad = "Not a gist link: use https://gist.github.com/<user>/<id>";
    assert_eq!(p.submit("Foo", "", cpm_core::format::SkinType::Default), Err(bad.to_owned()));
    assert!(!dir.join("Foo.link.cpmmodel").exists());
    // save_link shows it as the message (the bad URL is checked before the model is read).
    assert!(p.save_link().is_err());
    assert_eq!(p.message.as_deref(), Some(bad));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn popup_lines_copy() {
    let p = cpm::GistPopup::new("m/Expie_by_Clouddy.cpmmodel".into(), 40 * 1024 + 1);
    assert_eq!(
        cpm::popup_lines(&p),
        vec![
            "Expie_by_Clouddy.cpmmodel is 41 KiB, too big to send directly.".to_owned(),
            "1. Press Copy text (also saved as Expie_by_Clouddy.gist.txt).".to_owned(),
            "2. Create a public gist with it at gist.github.com.".to_owned(),
            "3. Paste the gist link here and press Save link.".to_owned(),
        ]
    );
    assert_eq!(cpm::GIST_TITLE, "Upload to a GitHub Gist");
    assert_eq!(cpm::copy_failed_message(&p), "Couldn't copy — open Expie_by_Clouddy.gist.txt instead");
}

#[test]
fn gist_buttons_and_keys() {
    let layout = cpm::gist_layout([0.0, 0.0, 480.0, 270.0]);
    let labels: Vec<&str> = cpm::GIST_BUTTONS.iter().map(|(_, l)| *l).collect();
    assert_eq!(labels, ["Copy text", "Open gist.github.com", "Save link", "Close"]);
    for (i, (button, _)) in cpm::GIST_BUTTONS.iter().enumerate() {
        let [x, y, w, h] = layout.buttons[i];
        assert_eq!(cpm::gist_button_at(&layout, [x + w / 2.0, y + h / 2.0]), Some(*button));
    }
    assert_eq!(cpm::gist_button_at(&layout, [0.0, 0.0]), None);
    // The panel fits the window.
    let [x, y, w, h] = layout.panel;
    assert!(x >= 0.0 && y >= 0.0 && x + w <= 480.0 && y + h <= 270.0);
    // Keys: backspace, Enter saves, Ctrl+V (or Cmd+V) pastes, Escape is the game's, others swallowed.
    assert_eq!(cpm::gist_key("key.keyboard.backspace", 0), cpm::GistKey::Backspace);
    assert_eq!(cpm::gist_key("key.keyboard.enter", 0), cpm::GistKey::Save);
    assert_eq!(cpm::gist_key("key.keyboard.keypad.enter", 0), cpm::GistKey::Save);
    assert_eq!(cpm::gist_key("key.keyboard.v", 2), cpm::GistKey::Paste);
    assert_eq!(cpm::gist_key("key.keyboard.v", 8), cpm::GistKey::Paste);
    assert_eq!(cpm::gist_key("key.keyboard.v", 0), cpm::GistKey::Swallow);
    assert_eq!(cpm::gist_key("key.keyboard.e", 0), cpm::GistKey::Swallow);
    assert_eq!(cpm::gist_key("key.keyboard.escape", 0), cpm::GistKey::Pass);
}

#[test]
fn gist_popup_state() {
    assert!(!cpm::gist_popup_shown());
    cpm::show_gist_popup(cpm::GistPopup::new("m/Foo.cpmmodel".into(), 9000));
    assert!(cpm::gist_popup_shown());
    cpm::with_gist_popup(|p| p.type_char('h'));
    assert_eq!(cpm::with_gist_popup(|p| p.url.clone()), Some("h".to_owned()));
    cpm::request_gist(cpm::GistRequest::Copy);
    cpm::request_gist(cpm::GistRequest::Paste);
    assert_eq!(cpm::take_gist_requests(), vec![cpm::GistRequest::Copy, cpm::GistRequest::Paste]);
    assert_eq!(cpm::take_gist_requests(), vec![]);
    cpm::request_gist(cpm::GistRequest::Close);
    cpm::hide_gist_popup();
    assert!(!cpm::gist_popup_shown());
    assert_eq!(cpm::take_gist_requests(), vec![]);
}

// ---- Link models (Task 8): a gist link resolved through the cache or a background fetch ----
// The fetch handoff is a process-wide static: each test uses a link of its own.

fn reggie_gist() -> Vec<u8> {
    cpm_core::link::gist_text(&std::fs::read(sample("ReggieTheRat.cpmmodel")).unwrap()).unwrap().into_bytes()
}

/// A data folder (the mod's) holding `Reggie.link.cpmmodel`, a link model to `link`; it is selected.
fn select_link_model(test: &str, link: &str) -> PathBuf {
    let data = scratch(test);
    let model = data.join("Reggie.link.cpmmodel");
    std::fs::write(&model, cpm_core::link::link_model("Reggie", "", cpm_core::format::SkinType::Default, link)).unwrap();
    cpm::set_data_dir(data.clone());
    cpm::select(Some(model));
    data
}

#[test]
fn cache_path_shape() {
    let data = Path::new("d");
    assert_eq!(cpm::cache_path(data, "git:alice/abc123"), Some(data.join("gist-cache").join("alice_abc123")));
    assert_eq!(cpm::cache_path(data, "git:a-b_c/X9"), Some(data.join("gist-cache").join("a-b_c_X9")));
    for bad in ["alice/abc", "git:alice", "git:/abc", "git:alice/", "git:../x/abc", "git:al\\ice/abc", "git:a/b/c", "http:alice/abc"] {
        assert_eq!(cpm::cache_path(data, bad), None, "{bad}");
    }
}

#[test]
fn link_model_loads_from_cache() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let link = "git:cachetest/c0ffee1";
    let data = select_link_model("link_cache", link);
    let cache = cpm::cache_path(&data, link).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, reggie_gist()).unwrap();
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert_eq!(cpm::with_model(|m| m.model.cubes.len()), Some(26));
    assert!(!cpm::fetch_in_flight(link));
}

#[test]
fn link_model_miss_starts_fetch_then_loads() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let link = "git:misstest/beef01";
    let data = select_link_model("link_miss", link);
    cpm::ensure_loaded();
    // The vanilla player meanwhile: no model, no error.
    assert!(!cpm::has_model());
    assert_eq!(cpm::last_error(), None);
    assert!(cpm::fetch_in_flight(link));
    // Another frame does not start a second fetch.
    cpm::ensure_loaded();
    assert!(!cpm::start_fetch(link));
    cpm::finish_fetch(link, Ok(reggie_gist()));
    assert!(!cpm::fetch_in_flight(link));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert_eq!(cpm::with_model(|m| m.model.cubes.len()), Some(26));
    // Cached for next time.
    assert_eq!(std::fs::read(cpm::cache_path(&data, link).unwrap()).unwrap(), reggie_gist());
}

#[test]
fn bad_gist_content_is_error() {
    let link = "git:badtest/dead02";
    let data = select_link_model("link_bad", link);
    cpm::ensure_loaded();
    assert!(cpm::fetch_in_flight(link));
    cpm::finish_fetch(link, Ok(b"<!DOCTYPE html><html>Not Found</html>".to_vec()));
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
    let error = cpm::last_error().expect("an error");
    assert!(error.starts_with("cpm: Reggie.link.cpmmodel: gist git:badtest/dead02: "), "{error}");
    // Nothing bad is cached.
    assert!(!cpm::cache_path(&data, link).unwrap().exists());
}

#[test]
fn failed_fetch_is_error_and_retried_on_reselect() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let link = "git:failtest/f00d03";
    let data = select_link_model("link_fail", link);
    let model = cpm::selected();
    cpm::ensure_loaded();
    cpm::finish_fetch(link, Err("timed out".into()));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error().as_deref(), Some("cpm: Reggie.link.cpmmodel: gist git:failtest/f00d03: timed out"));
    // Later frames do not retry.
    cpm::ensure_loaded();
    assert!(!cpm::fetch_in_flight(link));
    // Selecting it again does.
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::select(model);
    cpm::ensure_loaded();
    assert!(cpm::fetch_in_flight(link));
    assert_eq!(cpm::last_error(), None);
    cpm::finish_fetch(link, Ok(reggie_gist()));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert!(cpm::cache_path(&data, link).unwrap().exists());
}

#[test]
fn bad_cache_is_error_and_dropped() {
    let link = "git:badcache/abc04";
    let data = select_link_model("link_bad_cache", link);
    let cache = cpm::cache_path(&data, link).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, b"garbage!").unwrap();
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
    assert!(cpm::last_error().unwrap().contains("gist git:badcache/abc04"));
    // Dropped, so selecting the model again downloads it afresh.
    assert!(!cache.exists());
}

#[test]
fn cache_path_other_loaders() {
    let data = Path::new("d");
    let lc = data.join("link-cache");
    assert_eq!(cpm::cache_path(data, "pb:Xy12"), Some(lc.join("pb").join("Xy12")));
    assert_eq!(cpm::cache_path(data, "p:abc-1_2"), Some(lc.join("p").join("abc-1_2")));
    assert_eq!(cpm::cache_path(data, "gh:a/r/main/m.txt"), Some(lc.join("gh").join("a~2Fr~2Fmain~2Fm~2Etxt")));
    assert_eq!(cpm::cache_path(data, "ms:../x"), Some(lc.join("ms").join("~2E~2E~2Fx")));
    for bad in ["local:x", "zz:abc", "pb:", "pb:a b"] {
        assert_eq!(cpm::cache_path(data, bad), None, "{bad}");
    }
    assert_eq!(cpm::cache_path(data, &format!("pb:{}", "a".repeat(161))), None);
    // Not a link CPM can download: nothing starts.
    assert!(!cpm::start_fetch("local:x"));
    assert!(!cpm::start_fetch("zz:abc"));
}

#[test]
fn unsupported_link_is_error() {
    let _ = select_link_model("link_unsupported", "zz:abc");
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error().as_deref(), Some("cpm: Reggie.link.cpmmodel: link zz:abc: invalid data: unsupported link"));
    assert!(!cpm::fetch_in_flight("zz:abc"));
}

/// Reggie's skin as a `SKIN_LINK` serves it: base64 of `0x53`, the texture, checksum.
fn reggie_skin_text() -> Vec<u8> {
    use cpm_core::write::{checksummed, Writer};
    let t = cpm_core::format::read_cpmmodel(&std::fs::read(sample("ReggieTheRat.cpmmodel")).unwrap()).unwrap().data.texture.unwrap();
    let mut img = Writer::new();
    img.bytes(&t.png);
    let mut w = Writer::new();
    w.i16(t.declared[0] as i16).i16(t.declared[1] as i16).block(&img);
    cpm_core::link::encode_link_text(&checksummed(0x53, &w.into_bytes())).into_bytes()
}

/// A model whose data block is SKIN_TYPE, a PACKAGE_LINK to `package`, a SKIN_LINK to `skin`.
fn two_link_model(package: &str, skin: &str) -> Vec<u8> {
    use cpm_core::write::{checksummed, Writer};
    let mut st = Writer::new();
    st.u8(1);
    let mut body = Writer::new();
    body.object_block(11, &st);
    for (ty, link) in [(20u8, package), (6, skin)] {
        let mut lp = Writer::new();
        lp.u8(link.len() as u8).bytes(link.as_bytes());
        body.object_block(ty, &lp);
    }
    body.object_block(0, &Writer::new());
    let mut d = Writer::new();
    d.bytes(&checksummed(0x53, &body.into_bytes()));
    let mut w = Writer::new();
    w.utf("Two").utf("").block(&d).varint(0).block(&Writer::new());
    checksummed(0x53, &w.into_bytes())
}

#[test]
fn links_of_other_loaders_download_in_turn_then_cache() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let (pkg, skin) = ("pb:twolinkpkg", "ms:twolink/skin");
    let data = scratch("link_two");
    let model = data.join("Two.cpmmodel");
    std::fs::write(&model, two_link_model(pkg, skin)).unwrap();
    cpm::set_data_dir(data.clone());
    cpm::select(Some(model.clone()));
    cpm::ensure_loaded();
    assert!(cpm::fetch_in_flight(pkg));
    assert!(!cpm::fetch_in_flight(skin));
    cpm::finish_fetch(pkg, Ok(reggie_gist()));
    cpm::ensure_loaded();
    // The package is in; the skin link is next.
    assert!(!cpm::has_model());
    assert!(cpm::fetch_in_flight(skin));
    cpm::finish_fetch(skin, Ok(reggie_skin_text()));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert_eq!(cpm::with_model(|m| (m.model.cubes.len(), m.texture_px)), Some((26, [75, 64])));
    // Both cached, keyed by loader and path; next time nothing downloads.
    assert_eq!(std::fs::read(cpm::cache_path(&data, pkg).unwrap()).unwrap(), reggie_gist());
    assert!(cpm::cache_path(&data, skin).unwrap().is_file());
    cpm::select(None);
    cpm::ensure_loaded();
    cpm::select(Some(model));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert!(!cpm::fetch_in_flight(pkg) && !cpm::fetch_in_flight(skin));
}

#[test]
fn failed_link_of_other_loader_is_error() {
    let link = "p:failpaste01";
    let _ = select_link_model("link_paste_fail", link);
    cpm::ensure_loaded();
    assert!(cpm::fetch_in_flight(link));
    cpm::finish_fetch(link, Err("timed out".into()));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error().as_deref(), Some("cpm: Reggie.link.cpmmodel: link p:failpaste01: timed out"));
}

/// A project whose export the game's loader rejects (a skin.png whose header reads but whose image
/// data is corrupt): Failed, nothing written.
#[test]
fn export_that_does_not_load_writes_nothing() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    use std::io::{Read, Write};
    let dir = temp("export_not_loading");
    let models = dir.join("player_models");
    // Expie with its skin.png's image data corrupted (the header, which the reader checks, kept).
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(expie()).unwrap())).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().unwrap().into_owned();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == "skin.png" {
            let idat = data.windows(4).position(|w| w == b"IDAT").unwrap();
            for b in &mut data[idat + 4..idat + 24] {
                *b ^= 0xA5;
            }
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    let bad = dir.join("Corrupt.cpmproject");
    std::fs::write(&bad, out.finish().unwrap().into_inner()).unwrap();
    let cpm::ConvertResult::Failed(error) = cpm::convert_project(&bad, &models, 8) else { panic!("saved") };
    assert!(error.starts_with("Corrupt.cpmproject: exported model does not load: "), "{error}");
    assert!(!models.exists() || std::fs::read_dir(&models).unwrap().next().is_none());
    // The guard itself.
    assert!(cpm::check_export(b"not a model").unwrap_err().starts_with("exported model does not load: "));
    assert_eq!(cpm::check_export(&std::fs::read(sample("SkullDog.cpmmodel")).unwrap()), Ok(()));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Converting over the selected model loads it again, with a fresh texture upload.
#[test]
fn reconverting_the_selected_model_reloads_it() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = temp("reconvert_selected");
    let cpm::ConvertResult::Saved { model, .. } = cpm::convert_project(&expie(), &dir, 1024) else { panic!() };
    cpm::select(Some(model.clone()));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    let now = Instant::now();
    assert!(cpm::texture_upload(now).is_some());
    assert!(cpm::texture_upload(now).is_none());
    // Another model's conversion does not reload this one.
    let other = cpm::ConvertResult::Saved { model: dir.join("Other.cpmmodel"), too_big: false, size: 1, skipped: 0 };
    cpm::note_convert_result(other);
    cpm::ensure_loaded();
    assert!(cpm::texture_upload(now).is_none());
    let again = cpm::convert_project(&expie(), &dir, 1024);
    cpm::note_convert_result(again);
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert!(cpm::texture_upload(now).is_some(), "the new texture uploads");
    cpm::select(None);
    cpm::ensure_loaded();
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- The gesture menu (Task 3): its layout, hit testing, sliders and state ----

/// Rainwing's buttons, as the game shows them once the project is converted.
fn rainwing_buttons() -> Vec<cpm_core::anim::Button> {
    let bytes = std::fs::read(sample("rainwing-old.cpmproject")).unwrap();
    let project = cpm_core::project::read_project(&bytes).unwrap();
    let model = cpm_core::load(&cpm_core::export::export_model(&project).unwrap()).unwrap();
    model.engine.buttons().to_vec()
}

fn menu_button(name: &str, kind: cpm_core::anim::ButtonKind, hidden: bool) -> cpm_core::anim::Button {
    cpm_core::anim::Button { name: name.into(), kind, hidden, order: 0 }
}

fn gestures(n: usize) -> Vec<cpm_core::anim::Button> {
    (0..n).map(|i| menu_button(&format!("g{i}"), cpm_core::anim::ButtonKind::Gesture, false)).collect()
}

fn centre(r: [f32; 4]) -> [f32; 2] {
    [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0]
}

fn on_screen(r: [f32; 4], w: f32, h: f32) -> bool {
    r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= w && r[1] + r[3] <= h
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn layout_pages_at_scale_4() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let buttons = rainwing_buttons();
    let visible: Vec<usize> = (0..buttons.len()).filter(|&i| !buttons[i].hidden).collect();
    let (w, h) = (480.0, 270.0);
    let layout = cpm::layout_menu(&buttons, w, h);
    assert!(layout.pages.len() > 1, "{} visible buttons in {} page(s)", visible.len(), layout.pages.len());
    // Every visible button once, in model order, page after page.
    let shown: Vec<usize> = layout.pages.iter().flatten().map(|wd| wd.button).collect();
    assert_eq!(shown, visible);
    assert!(layout.pages.iter().all(|p| !p.is_empty()));
    // CPM's cells: 80x30, 90 apart across (4 columns) and 40 down.
    let first = &layout.pages[0];
    assert!(first.len() >= 5);
    for c in 0..4 {
        assert_eq!(first[c].rect[2..], [80.0, 30.0]);
        assert_eq!(first[c].rect[1], first[0].rect[1]);
        assert_eq!(first[c].rect[0], first[0].rect[0] + 90.0 * c as f32);
    }
    assert_eq!(first[4].rect[0], first[0].rect[0]);
    assert_eq!(first[4].rect[1], first[0].rect[1] + 40.0);
    // All inside the screen, nothing overlapping.
    let mut fixed = vec![layout.reset_gesture, layout.reset_pose];
    fixed.extend(layout.prev);
    fixed.extend(layout.next);
    assert_eq!(fixed.len(), 4, "page buttons with more than one page");
    for page in &layout.pages {
        let mut rects: Vec<[f32; 4]> = page.iter().map(|wd| wd.rect).collect();
        rects.extend(&fixed);
        for (i, a) in rects.iter().enumerate() {
            assert!(on_screen(*a, w, h), "{a:?}");
            for b in &rects[i + 1..] {
                assert!(!overlap(*a, *b), "{a:?} {b:?}");
            }
        }
    }
    // The top row: Reset Gesture, then Reset Pose (CPM: 100 wide, 110 apart), above the grid.
    assert_eq!(layout.reset_gesture[2..], [100.0, 20.0]);
    assert_eq!(layout.reset_pose[0], layout.reset_gesture[0] + 110.0);
    assert!(layout.reset_gesture[1] + 20.0 <= first[0].rect[1]);
    assert!(layout.title_y < layout.reset_gesture[1]);
    // A taller GUI fits them on one page: no page buttons then.
    let tall = cpm::layout_menu(&buttons, 480.0, 1000.0);
    assert_eq!(tall.pages.len(), 1);
    assert_eq!((tall.prev, tall.next), (None, None));
    // A narrow GUI takes fewer columns, still on screen.
    let narrow = cpm::layout_menu(&buttons, 200.0, 270.0);
    for wd in narrow.pages.iter().flatten() {
        assert!(on_screen(wd.rect, 200.0, 270.0), "{:?}", wd.rect);
    }
    assert_eq!(narrow.pages.iter().flatten().count(), visible.len());
}

#[test]
fn layout_skips_hidden() {
    use cpm_core::anim::ButtonKind;
    let buttons = vec![
        menu_button("a", ButtonKind::Gesture, false),
        menu_button("b", ButtonKind::Toggle, true),
        menu_button("c", ButtonKind::Slider { max: 10 }, false),
    ];
    let layout = cpm::layout_menu(&buttons, 480.0, 270.0);
    assert_eq!(layout.pages.len(), 1);
    let shown: Vec<usize> = layout.pages[0].iter().map(|wd| wd.button).collect();
    assert_eq!(shown, [0, 2], "engine indices, the hidden one left out");
    // Side by side: the hidden one leaves no gap.
    assert_eq!(layout.pages[0][1].rect[0], layout.pages[0][0].rect[0] + 90.0);
    assert_eq!((layout.prev, layout.next), (None, None));
    // All hidden (or none at all): no pages, the top row still there.
    let hidden = vec![menu_button("b", ButtonKind::Toggle, true)];
    for buttons in [hidden, Vec::new()] {
        let layout = cpm::layout_menu(&buttons, 480.0, 270.0);
        assert!(layout.pages.is_empty());
        assert!(on_screen(layout.reset_gesture, 480.0, 270.0));
    }
    assert_eq!(cpm::MENU_TITLE, "Gestures");
    assert_eq!(cpm::NO_GESTURES, "This model has no gestures");
    assert_eq!((cpm::RESET_GESTURE, cpm::RESET_POSE), ("Reset Gesture", "Reset Pose"));
}

#[test]
fn hit_testing() {
    let buttons = gestures(40);
    let layout = cpm::layout_menu(&buttons, 480.0, 270.0);
    assert!(layout.pages.len() > 1);
    let at = |page: usize, p: [f32; 2]| cpm::hit(&layout, page, p[0], p[1]);
    for (page, widgets) in layout.pages.iter().enumerate() {
        for wd in widgets {
            assert_eq!(at(page, centre(wd.rect)), Some(cpm::Hit::Button(wd.button)));
        }
    }
    // The same spot on another page is that page's button.
    let spot = centre(layout.pages[0][0].rect);
    assert_eq!(at(1, spot), Some(cpm::Hit::Button(layout.pages[1][0].button)));
    // A page past the last: no grid button.
    assert_eq!(at(layout.pages.len(), spot), None);
    assert_eq!(at(0, centre(layout.reset_gesture)), Some(cpm::Hit::ResetGesture));
    assert_eq!(at(3, centre(layout.reset_pose)), Some(cpm::Hit::ResetPose));
    assert_eq!(at(0, centre(layout.prev.unwrap())), Some(cpm::Hit::Prev));
    assert_eq!(at(0, centre(layout.next.unwrap())), Some(cpm::Hit::Next));
    // The gaps between cells and the corners are nothing.
    let [x, y, w, _] = layout.pages[0][0].rect;
    assert_eq!(at(0, [x + w + 5.0, y + 5.0]), None);
    assert_eq!(at(0, [0.0, 0.0]), None);

    // A dropdown's list: one row per option under its box, each hit by its centre.
    let cell = layout.pages[0][0].rect;
    let list = cpm::dropdown_list(cell, 3, 270.0);
    assert_eq!(list.len(), 3);
    assert!(list[0][1] >= cell[1] + cell[3]);
    for (i, r) in list.iter().enumerate() {
        assert_eq!(cpm::option_at(&list, centre(*r)), Some(i));
        assert!(on_screen(*r, 480.0, 270.0));
    }
    assert_eq!(cpm::option_at(&list, [0.0, 0.0]), None);
    // Near the bottom it opens upwards, still on screen.
    let low = [x, 250.0, 80.0, 30.0];
    let list = cpm::dropdown_list(low, 4, 280.0);
    assert!(list.iter().all(|r| on_screen(*r, 480.0, 280.0)), "{list:?}");
    assert!(list[3][1] + list[3][3] <= 260.0);
    // Option 0 ("") is shown as None.
    assert_eq!(cpm::option_label(""), "None");
    assert_eq!(cpm::option_label("Red"), "Red");
}

#[test]
fn slider_drag_clamps() {
    let wd = cpm::Widget { button: 0, rect: [100.0, 50.0, 80.0, 30.0] };
    assert_eq!(cpm::slider_value(&wd, -50.0, 10), 0);
    assert_eq!(cpm::slider_value(&wd, 100.0, 10), 0);
    assert_eq!(cpm::slider_value(&wd, 180.0, 10), 10);
    assert_eq!(cpm::slider_value(&wd, 1000.0, 255), 255);
    assert_eq!(cpm::slider_value(&wd, 140.0, 10), 5);
    assert_eq!(cpm::slider_value(&wd, 140.0, 255), 128);
    assert_eq!(cpm::slider_value(&wd, 140.0, 0), 0);
    // Monotonic across the track.
    let mut last = 0;
    for x in 90..200 {
        let v = cpm::slider_value(&wd, x as f32, 100);
        assert!(v >= last && v <= 100);
        last = v;
    }
    // The marker sits where that value is set from.
    for v in [0u8, 3, 7, 10] {
        let mx = cpm::slider_marker_x(&wd, v, 10);
        assert_eq!(cpm::slider_value(&wd, mx + cpm::SLIDER_MARKER_W / 2.0, 10), v);
    }
}

#[test]
fn menu_key_only_while_playing() {
    assert!(cpm::menu_key_allowed(true, false));
    assert!(!cpm::menu_key_allowed(false, false));
    assert!(!cpm::menu_key_allowed(true, true));
    assert!(!cpm::menu_key_allowed(false, true));
}

#[test]
fn one_cpm_screen_at_a_time() {
    assert_eq!(cpm::shown_screen(), None);
    cpm::show_menu();
    assert_eq!(cpm::shown_screen(), Some(cpm::ModScreen::Menu));
    cpm::with_menu(|m| m.page = 2);
    assert_eq!(cpm::with_menu(|m| m.page), Some(2));
    cpm::hide_menu();
    assert_eq!(cpm::shown_screen(), None);
    assert_eq!(cpm::with_menu(|m| m.page), None);
    cpm::show_gist_popup(cpm::GistPopup::new("m/Foo.cpmmodel".into(), 9000));
    assert_eq!(cpm::shown_screen(), Some(cpm::ModScreen::Gist));
    cpm::hide_gist_popup();
    // A menu request waits until the screen can open.
    cpm::request_menu();
    assert!(cpm::menu_requested());
    assert!(cpm::menu_requested(), "looking does not clear it");
    assert!(cpm::take_menu_request());
    assert!(!cpm::menu_requested());
}

#[test]
fn converted_version_cases() {
    assert_eq!(cpm::converted_version("a fox (converted by CPM for Horizon)"), Some(1));
    assert_eq!(cpm::converted_version("a fox (converted by CPM for Horizon, v2)"), Some(2));
    assert_eq!(cpm::converted_version(" (converted by CPM for Horizon, v13)"), Some(13));
    assert_eq!(cpm::converted_version("a fox"), None);
    assert_eq!(cpm::converted_version(""), None);
    assert_eq!(cpm::converted_version("a fox (converted by CPM for Horizon, v)"), None);
    assert_eq!(cpm::converted_version("a fox (converted by CPM for Horizon, vx)"), None);
    assert_eq!(cpm::converted_version("a fox (converted by CPM for Horizon, v2) more"), None);
    assert_eq!(cpm::converted_version("a fox (converted by someone, v2)"), None);
}

/// An exported Expie model whose description is `desc`, written to `dir/file`.
fn with_desc(dir: &Path, file: &str, desc: &str) -> PathBuf {
    let mut project = cpm_core::project::read_project(&std::fs::read(expie()).unwrap()).unwrap();
    project.desc = desc.to_owned();
    let path = dir.join(file);
    std::fs::write(&path, cpm_core::export::export_model(&project).unwrap()).unwrap();
    path
}

#[test]
fn output_path_overwrites_old_and_new_marker_files_only() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = scratch("output_path_versions");
    let project = Path::new("Foo.cpmproject");
    with_desc(&dir, "Foo.cpmmodel", "dog (converted by CPM for Horizon)");
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo.cpmmodel"));
    with_desc(&dir, "Foo.cpmmodel", "dog (converted by CPM for Horizon, v2)");
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo.cpmmodel"));
    with_desc(&dir, "Foo.cpmmodel", "dog by someone");
    assert_eq!(cpm::output_path(&dir, project), dir.join("Foo_2.cpmmodel"));
}

#[test]
fn old_conversion_line_only_for_stale_models() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = scratch("old_conversion");
    let labels = || {
        let view = cpm::ConvertView { busy: false, last: None, limit_kib: 8, old_conversion: cpm::old_conversion() };
        cpm::settings_labels(cpm::selected().as_deref(), cpm::last_error().as_deref(), &view)
    };
    let stale = |l: &Vec<String>| l.iter().any(|x| x.starts_with("Old conversion"));
    cpm::select(Some(with_desc(&dir, "Old.cpmmodel", "dog (converted by CPM for Horizon)")));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert!(cpm::has_model());
    assert!(labels().contains(&"Old conversion: convert Old.cpmproject again".to_owned()), "{:?}", labels());
    // v2 lacked the render scale and the first-person hand.
    cpm::select(Some(with_desc(&dir, "V2.cpmmodel", "dog (converted by CPM for Horizon, v2)")));
    cpm::ensure_loaded();
    assert!(labels().contains(&"Old conversion: convert V2.cpmproject again".to_owned()), "{:?}", labels());
    // v3 lacked animated textures.
    cpm::select(Some(with_desc(&dir, "V3.cpmmodel", "dog (converted by CPM for Horizon, v3)")));
    cpm::ensure_loaded();
    assert!(labels().contains(&"Old conversion: convert V3.cpmproject again".to_owned()), "{:?}", labels());
    cpm::select(Some(with_desc(&dir, "Fresh.cpmmodel", "dog (converted by CPM for Horizon, v4)")));
    cpm::ensure_loaded();
    assert!(!stale(&labels()));
    cpm::select(Some(with_desc(&dir, "Other.cpmmodel", "dog")));
    cpm::ensure_loaded();
    assert!(!stale(&labels()));
    // A new conversion is current.
    let cpm::ConvertResult::Saved { model, .. } = cpm::convert_project(&expie(), &dir, 1024) else { panic!() };
    cpm::select(Some(model));
    cpm::ensure_loaded();
    assert!(cpm::has_model() && !stale(&labels()));
}

// ---- The gesture menu's 3D preview ----

fn reggie_boxes() -> Vec<cpm_core::PosedBox> {
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let input = PoseInput { time_ms: 1000, on_ground: true, ..PoseInput::default() };
    cpm::with_model(|m| m.pose(&input)).unwrap()
}

const PREVIEW_RECT: [f32; 4] = [300.0, 20.0, 160.0, 220.0];

fn corners_inside(faces: &[cpm::PreviewFace], rect: [f32; 4]) -> bool {
    let [x, y, w, h] = rect;
    faces.iter().flat_map(|f| f.corners).all(|c| c[0] >= x && c[0] <= x + w && c[1] >= y && c[1] <= y + h)
}

fn y_span(faces: &[cpm::PreviewFace]) -> f32 {
    let ys: Vec<f32> = faces.iter().flat_map(|f| f.corners).map(|c| c[1]).collect();
    ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
}

#[test]
fn preview_fits_rect() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let boxes = reggie_boxes();
    let view = cpm::PreviewView::default();
    assert_eq!((view.yaw, view.pitch, view.zoom), (30.0, 0.0, 1.0));
    let faces = cpm::preview_faces(&boxes, &view, PREVIEW_RECT);
    assert!(!faces.is_empty());
    assert!(corners_inside(&faces, PREVIEW_RECT));
    // Turned and tilted it may reach past the rect (the draw cuts it there), but stays drawn.
    for (yaw, pitch) in [(0.0, 60.0), (90.0, -60.0), (200.0, 45.0)] {
        let view = cpm::PreviewView { yaw, pitch, zoom: 1.0 };
        assert!(!cpm::preview_faces(&boxes, &view, PREVIEW_RECT).is_empty(), "{yaw} {pitch}");
    }
    // It fills a good part of the rect (not a speck).
    let span = y_span(&faces);
    assert!(span > PREVIEW_RECT[3] * 0.3, "{span}");
    for f in &faces {
        assert!(f.shade > 0.0 && f.shade <= 1.0);
        assert!(f.uv_src.box_index < boxes.len());
        assert!(boxes[f.uv_src.box_index].faces[f.uv_src.face].is_some());
    }
}

#[test]
fn preview_rotates() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let boxes = reggie_boxes();
    let at = |yaw: f32| {
        let faces = cpm::preview_faces(&boxes, &cpm::PreviewView { yaw, pitch: 0.0, zoom: 1.0 }, PREVIEW_RECT);
        faces.iter().flat_map(|f| f.corners).collect::<Vec<_>>()
    };
    assert_ne!(at(30.0), at(120.0));
    let mut view = cpm::PreviewView::default();
    view.drag(40.0, 0.0);
    assert_ne!(view.yaw, 30.0);
    assert_eq!(view.pitch, 0.0);
}

#[test]
fn preview_zoom_clamps() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut view = cpm::PreviewView::default();
    view.scroll(1.0);
    assert!(view.zoom > 1.0);
    view.scroll(1000.0);
    assert_eq!(view.zoom, 3.0);
    view.scroll(-1000.0);
    assert_eq!(view.zoom, 0.5);
    view.drag(0.0, 10_000.0);
    assert_eq!(view.pitch, 60.0);
    view.drag(0.0, -10_000.0);
    assert_eq!(view.pitch, -60.0);
    // Zoomed in, the model is bigger than at zoom 1.
    let boxes = reggie_boxes();
    let height =
        |zoom: f32| y_span(&cpm::preview_faces(&boxes, &cpm::PreviewView { zoom, ..Default::default() }, PREVIEW_RECT));
    assert!(height(2.0) > height(1.0) * 1.9);
}

#[test]
fn preview_falls_back_to_standing() {
    // Nothing drawn yet: standing on the ground at the time asked, the cape hanging still.
    let cape = Some(cpm_core::CapeState::default());
    let standing = cpm::preview_input(5000);
    assert_eq!(standing, PoseInput { time_ms: 5000, on_ground: true, cape, ..PoseInput::default() });
    // The world draw's input, with the preview's own time.
    let walking = PoseInput { time_ms: 4000, on_ground: true, walk_speed: 0.5, sneaking: true, ..PoseInput::default() };
    cpm::record_input(&walking);
    assert_eq!(cpm::preview_input(4100), PoseInput { time_ms: 4100, ..walking });
    // Long after the last world draw (first person: nothing else samples it): standing again.
    assert_eq!(cpm::preview_input(60_000), PoseInput { time_ms: 60_000, on_ground: true, cape, ..PoseInput::default() });
}

/// A face of a box at the identity transform (blocks), facing +z (toward a viewer at yaw 0) when
/// `front`, else -z, at depth `z`.
fn flat_face(z: f32, front: bool) -> cpm_core::FaceQuad {
    let (l, r) = if front { (-0.5, 0.5) } else { (0.5, -0.5) };
    cpm_core::FaceQuad { corners: [[l, 0.5, z], [l, -0.5, z], [r, -0.5, z], [r, 0.5, z]], uv: [0.0, 0.0, 8.0, 8.0] }
}

fn flat_box(faces: [Option<cpm_core::FaceQuad>; 6]) -> cpm_core::PosedBox {
    cpm_core::PosedBox {
        transform: Default::default(),
        faces,
        tint: 0xFF_FFFF,
        glow: false,
        textured: true,
        source: cpm_core::BoxSource::Cube(0),
        texture: cpm_core::BoxTexture::Skin,
        texture_px: [64, 64],
    }
}

#[test]
fn preview_depth_toward_viewer() {
    // Two boxes' front faces, one in front of the other, and a face looking away.
    let boxes = vec![
        flat_box([None, None, None, Some(flat_face(0.5, true)), None, None]),
        flat_box([None, None, None, Some(flat_face(-0.5, true)), None, None]),
        flat_box([None, None, Some(flat_face(-0.6, false)), None, None, None]),
    ];
    let view = cpm::PreviewView { yaw: 0.0, pitch: 0.0, zoom: 1.0 };
    let faces = cpm::preview_faces(&boxes, &view, PREVIEW_RECT);
    let z = |b: usize| {
        let f = faces.iter().find(|f| f.uv_src.box_index == b).expect("face");
        f.corners.iter().map(|c| c[2]).sum::<f32>() / 4.0
    };
    assert!(z(0) > z(1), "{} {}", z(0), z(1));
    // The face looking away is not drawn.
    assert!(faces.iter().all(|f| f.uv_src.box_index != 2));
    // Facing the viewer, lit from the front: not dark.
    assert!(faces.iter().all(|f| f.shade > 0.5));
    // Turned half round, only the face that looked away shows.
    let back = cpm::preview_faces(&boxes, &cpm::PreviewView { yaw: 180.0, ..view }, PREVIEW_RECT);
    assert!(!back.is_empty() && back.iter().all(|f| f.uv_src.box_index == 2));
}

#[test]
fn preview_quads_map_like_the_world_draw() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let view = cpm::PreviewView::default();
    let boxes = reggie_boxes();
    // Not before its texture went to the game.
    assert!(cpm::preview_quads(&PoseInput::default(), &cpm::Worn::default(), &view, PREVIEW_RECT).is_none());
    cpm::texture_upload(Instant::now()).unwrap();
    let mut both = None;
    cpm::with_model(|m| {
        let world = cpm::box_quads(m, &boxes, &cpm::Worn::default());
        let fit = cpm::rest_fit(m).unwrap();
        let preview = cpm::preview_box_quads(m, &boxes, &cpm::Worn::default(), &fit, &view, PREVIEW_RECT);
        both = Some((world, preview));
    });
    let (world, preview) = both.unwrap();
    assert_eq!(preview.sprites, world.sprites);
    assert!(!preview.quads.is_empty());
    for q in &preview.quads {
        // Its texels are one of the world quads' UV rects (the same mapping).
        let (us, vs) = (q.uv.map(|t| t[0]), q.uv.map(|t| t[1]));
        let same = world.quads.iter().any(|w| {
            let [u0, v0, u1, v1] = w.uv;
            us.iter().all(|u| *u == u0 || *u == u1) && vs.iter().all(|v| *v == v0 || *v == v1)
        });
        assert!(same, "{:?}", q.uv);
        // Opaque, packed as the GUI batch takes it.
        assert_eq!(q.color >> 24, 0xFF);
        // Wound as the game's GUI previews are: front faces turn negative in z.
        let [p0, p1, p2, _] = q.corners;
        let (a, b) = ([p1[0] - p0[0], p1[1] - p0[1]], [p2[0] - p0[0], p2[1] - p0[1]]);
        assert!(a[0] * b[1] - a[1] * b[0] <= 0.0, "{:?}", q.corners);
    }
    // Far to near.
    let depth = |q: &cpm::PreviewQuad| q.corners.iter().map(|c| c[2]).sum::<f32>();
    assert!(preview.quads.windows(2).all(|w| depth(&w[0]) <= depth(&w[1])));
    assert!(cpm::preview_quads(&PoseInput::default(), &cpm::Worn::default(), &view, PREVIEW_RECT).is_some());
}

#[test]
fn preview_fit_is_kept_from_the_first_frame() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let view = cpm::PreviewView::default();
    let boxes = reggie_boxes();
    let rest = cpm::with_model(|m| cpm::rest_fit(m)).unwrap().expect("a fit");
    // The rest pose fits the rect whichever way it is turned.
    let at_rest = cpm::with_model(|m| {
        cpm_core::mesh::pose_model(&m.model, &cpm_core::Frame::default(), &PoseInput { on_ground: true, ..Default::default() })
    })
    .unwrap();
    for (yaw, pitch) in [(30.0, 0.0), (0.0, 60.0), (90.0, -60.0)] {
        let v = cpm::PreviewView { yaw, pitch, zoom: 1.0 };
        assert!(corners_inside(&cpm::preview_faces_in(&at_rest, &rest, &v, PREVIEW_RECT), PREVIEW_RECT));
    }
    // A limb moved far out (one box's faces three times as big) neither rescales nor moves the rest.
    let mut moved = boxes.clone();
    let k = moved.iter().position(|b| b.faces.iter().any(Option::is_some)).unwrap();
    for f in moved[k].faces.iter_mut().flatten() {
        for c in &mut f.corners {
            *c = c.map(|v| v * 3.0 + 20.0);
        }
    }
    assert_ne!(cpm::preview_fit(&moved), cpm::preview_fit(&boxes), "the bounds did change");
    let still = cpm::preview_faces_in(&boxes, &rest, &view, PREVIEW_RECT);
    let after = cpm::preview_faces_in(&moved, &rest, &view, PREVIEW_RECT);
    let others = |faces: &[cpm::PreviewFace]| {
        faces.iter().filter(|f| f.uv_src.box_index != k).map(|f| f.corners).collect::<Vec<_>>()
    };
    assert_eq!(others(&still), others(&after));
    // The live preview keeps the rest fit, worked out once per load.
    assert_eq!(cpm::kept_preview_fit(), None);
    cpm::texture_upload(Instant::now()).unwrap();
    let input = PoseInput { time_ms: 2000, on_ground: true, ..Default::default() };
    cpm::preview_quads(&input, &cpm::Worn::default(), &view, PREVIEW_RECT).unwrap();
    // Kept from the first frame drawn (animated, as seen), and kept on later frames.
    let first = cpm::kept_preview_fit().expect("kept");
    let live = cpm::with_model(|m| cpm::preview_fit(&m.pose(&input))).unwrap();
    assert_eq!(Some(first), live);
    let later = PoseInput { time_ms: 2600, ..input };
    cpm::preview_quads(&later, &cpm::Worn::default(), &view, PREVIEW_RECT).unwrap();
    assert_eq!(cpm::kept_preview_fit(), Some(first));
    // Another model: worked out again.
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    assert_eq!(cpm::kept_preview_fit(), None);
}

#[test]
fn preview_culls_reggies_head_back() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let boxes = reggie_boxes();
    let z = |b: &cpm_core::PosedBox, f: usize| {
        let face = b.faces[f].as_ref().unwrap();
        face.corners.iter().map(|c| b.transform.transform_point3((*c).into()).z).sum::<f32>()
    };
    let top = |b: &cpm_core::PosedBox| {
        b.faces.iter().flatten().flat_map(|f| f.corners).map(|c| b.transform.transform_point3(c.into()).y).fold(f32::MIN, f32::max)
    };
    // The head: the highest box with a north and a south face.
    let (n, s) = (cpm_core::mesh::FACE_NORTH, cpm_core::mesh::FACE_SOUTH);
    let head = (0..boxes.len())
        .filter(|&i| boxes[i].faces[n].is_some() && boxes[i].faces[s].is_some())
        .max_by(|&a, &b| top(&boxes[a]).total_cmp(&top(&boxes[b])))
        .expect("a head");
    // Its front: the face further along +Z (the model faces +Z).
    let (front, back) = if z(&boxes[head], s) > z(&boxes[head], n) { (s, n) } else { (n, s) };
    let shown = |yaw: f32, face: usize| {
        let v = cpm::PreviewView { yaw, pitch: 0.0, zoom: 1.0 };
        cpm::preview_faces(&boxes, &v, PREVIEW_RECT).iter().any(|f| f.uv_src == cpm::UvSource { box_index: head, face })
    };
    assert!(shown(0.0, front) && !shown(0.0, back));
    assert!(shown(180.0, back) && !shown(180.0, front));
}

#[test]
fn menu_splits_controls_and_preview() {
    let (controls, preview) = cpm::menu_split(480.0, 270.0);
    assert!(controls > 480.0 * 0.5 && controls < 480.0 * 0.7, "{controls}");
    let [x, y, w, h] = preview;
    assert!(x >= controls && x + w <= 480.0 && y >= 0.0 && y + h <= 270.0);
    assert!(w > 480.0 * 0.3, "{w}");
    // The controls' layout stays left of the preview.
    let layout = cpm::layout_menu(&gestures(12), controls, 270.0);
    for wgt in layout.pages.iter().flatten() {
        assert!(wgt.rect[0] + wgt.rect[2] <= x, "{:?}", wgt.rect);
    }
    assert!(layout.reset_pose[0] + layout.reset_pose[2] <= x);
    // A new menu shows the model turned 30 degrees, not being turned.
    let menu = cpm::Menu::default();
    assert_eq!(menu.preview, cpm::PreviewView::default());
    assert!(!menu.rotating);
}

// ---- The Models picker ----

/// `[x, y, w, h]` rects overlap.
fn overlaps(a: [u32; 4], b: [u32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn slot_layout_fits() {
    // A 64x64 skin (with its white patch: 68x64) and the same for the preview, three icons.
    let l = cpm::slot_layout([68, 64], Some([68, 64]), 3);
    // The preview goes under the active model's region, which stays at the top left.
    assert_eq!(l.preview_at, Some([0, 64]));
    assert_eq!(l.icon_at.len(), 3);
    let active = [0, 0, 68, 64];
    let preview = [0, 64, 68, 64];
    let icons: Vec<[u32; 4]> =
        l.icon_at.iter().map(|a| a.expect("fits")).map(|[x, y]| [x, y, cpm::ICON_PX, cpm::ICON_PX]).collect();
    for (i, r) in icons.iter().enumerate() {
        assert!(r[0] + r[2] <= cpm::SPRITE_PX && r[1] + r[3] <= cpm::SPRITE_PX, "{r:?}");
        assert!(!overlaps(*r, active) && !overlaps(*r, preview), "{r:?}");
        for o in &icons[i + 1..] {
            assert!(!overlaps(*r, *o), "{r:?} {o:?}");
        }
    }
    // No preview: the icons still fit, clear of the active model.
    let l = cpm::slot_layout([68, 64], None, 2);
    assert_eq!(l.preview_at, None);
    assert!(l.icon_at.iter().all(|a| a.is_some_and(|[x, y]| !overlaps([x, y, 32, 32], active))));
    // More icons than the sprite holds: the first fit, the rest fall back (None).
    let l = cpm::slot_layout([68, 64], Some([68, 64]), 5000);
    assert!(l.icon_at[0].is_some());
    assert_eq!(l.icon_at[4999], None);
    let placed = l.icon_at.iter().filter(|a| a.is_some()).count();
    assert!(placed > 800 && placed < 1024, "{placed}");
    // The placed ones come first.
    assert!(l.icon_at[..placed].iter().all(Option::is_some));
    // No model at all (the vanilla player): the preview takes the top left.
    assert_eq!(cpm::slot_layout([0, 0], Some([132, 64]), 0).preview_at, Some([0, 0]));
}

#[test]
fn slot_layout_preview_too_large() {
    // Under a 600 high active region a 512 high preview runs off the sprite.
    let l = cpm::slot_layout([132, 600], Some([132, 512]), 2);
    assert_eq!(l.preview_at, None);
    // Its icons still find room.
    assert!(l.icon_at.iter().all(Option::is_some));
    // Too wide.
    assert_eq!(cpm::slot_layout([68, 64], Some([cpm::SPRITE_PX + 1, 16]), 0).preview_at, None);
    // Exactly the room there is: it fits.
    assert_eq!(cpm::slot_layout([68, 64], Some([cpm::SPRITE_PX, cpm::SPRITE_PX - 64]), 0).preview_at, Some([0, 64]));
}

#[test]
fn picker_pages() {
    let rect = [10.0, 20.0, 230.0, 160.0];
    assert!(cpm::picker_grid(0, rect).pages.is_empty());
    let g = cpm::picker_grid(1, rect);
    assert_eq!(g.pages.len(), 1);
    let per = g.per_page;
    assert!(per >= 4, "{per}");
    for n in [per, per + 1, 3 * per + 2] {
        let g = cpm::picker_grid(n, rect);
        assert_eq!(g.pages.len(), n.div_ceil(per), "{n}");
        assert_eq!(g.pages.iter().map(Vec::len).sum::<usize>(), n);
        assert!(g.pages[..g.pages.len() - 1].iter().all(|p| p.len() == per));
        for page in &g.pages {
            for (i, a) in page.iter().enumerate() {
                assert!(a[0] >= rect[0] && a[1] >= rect[1], "{a:?}");
                assert!(a[0] + a[2] <= rect[0] + rect[2] && a[1] + a[3] <= rect[1] + rect[3], "{a:?}");
                assert!(a[2] >= cpm::ICON_PX as f32 && a[3] >= cpm::ICON_PX as f32);
                for b in &page[i + 1..] {
                    let apart = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
                    assert!(apart, "{a:?} {b:?}");
                }
            }
        }
    }
    // A rect too small for one tile still shows one a page.
    let g = cpm::picker_grid(3, [0.0, 0.0, 5.0, 5.0]);
    assert_eq!(g.per_page, 1);
    assert_eq!(g.pages.len(), 3);
}

#[test]
fn picker_hit() {
    let rect = [10.0, 20.0, 230.0, 160.0];
    let g = cpm::picker_grid(20, rect);
    let per = g.per_page;
    let mid = |r: [f32; 4]| [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
    assert_eq!(g.hit(0, mid(g.pages[0][0])), Some(0));
    assert_eq!(g.hit(0, mid(g.pages[0][2])), Some(2));
    // The second page's tiles are files `per` on.
    assert_eq!(g.hit(1, mid(g.pages[1][1])), Some(per + 1));
    // Outside the tiles, or a page there is not.
    assert_eq!(g.hit(0, [0.0, 0.0]), None);
    assert_eq!(g.hit(9, mid(g.pages[0][0])), None);
    // Between two tiles.
    let [a, b] = [g.pages[0][0], g.pages[0][1]];
    assert!(b[0] > a[0] + a[2]);
    assert_eq!(g.hit(0, [(a[0] + a[2] + b[0]) / 2.0, a[1] + 1.0]), None);
}

/// Polls the picker until the preview is no longer loading (10 s at most).
fn wait_for_preview() -> cpm::PreviewStatus {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cpm::poll_picker();
        let status = cpm::picker_preview_status();
        if status != cpm::PreviewStatus::Loading || Instant::now() > end {
            return status;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Polls the picker until every tile was read (10 s at most).
fn wait_for_tiles() {
    let end = Instant::now() + Duration::from_secs(10);
    while cpm::picker_tiles().iter().any(|t| t.scanning) && Instant::now() < end {
        cpm::poll_picker();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn preview_load_bad_file_message() {
    let dir = scratch("preview_bad_file");
    std::fs::write(dir.join("broken.cpmmodel"), b"not a model").unwrap();
    let link = cpm_core::link::link_model("L", "", cpm_core::format::SkinType::Default, "git:someone/abc123");
    std::fs::write(dir.join("link.cpmmodel"), link).unwrap();
    assert_eq!(cpm::load_preview(&dir.join("broken.cpmmodel"), None).err().as_deref(), Some(cpm::CANT_READ));
    assert_eq!(cpm::load_preview(&dir.join("missing.cpmmodel"), None).err().as_deref(), Some(cpm::CANT_READ));
    assert_eq!(cpm::load_preview(&dir.join("link.cpmmodel"), Some(&dir)).err().as_deref(), Some(cpm::NOT_CACHED));
    assert_eq!(cpm::CANT_READ, "Can't read this model");
    assert_eq!(cpm::NOT_CACHED, "Select it once to download it");
    assert_eq!(cpm::TOO_LARGE, "Too large to preview");
    // Through the picker: the tile says so, and nothing is previewed.
    cpm::open_picker(&dir);
    assert!(cpm::picker_shown());
    assert_eq!(cpm::picker_preview_status(), cpm::PreviewStatus::Nothing);
    cpm::preview_select(0);
    assert_eq!(wait_for_preview(), cpm::PreviewStatus::Message(cpm::CANT_READ.to_owned()));
    let rect = [0.0, 0.0, 100.0, 100.0];
    let _ = cpm::texture_upload(Instant::now());
    assert!(cpm::picker_preview_quads(&cpm::Worn::default(), &cpm::PreviewView::default(), rect, 0).is_none());
    wait_for_tiles();
    let tiles = cpm::picker_tiles();
    assert_eq!(tiles[0].name, "broken");
    assert_eq!(tiles[0].error.as_deref(), Some(cpm::CANT_READ));
    // The link model reads (it has no icon: a name tile).
    assert_eq!(tiles[1].error, None);
    assert_eq!(tiles[1].icon_at, None);
    cpm::preview_select(1);
    assert_eq!(wait_for_preview(), cpm::PreviewStatus::Message(cpm::NOT_CACHED.to_owned()));
    cpm::close_picker();
    assert!(!cpm::picker_shown());
}

#[test]
fn use_model_stores_setting() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = scratch("use_model");
    for name in ["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"] {
        std::fs::copy(sample(name), dir.join(name)).unwrap();
    }
    cpm::open_picker(&dir);
    // Nothing chosen yet: nothing to use, and (no model, no icons read yet) no image to send.
    assert_eq!(cpm::use_previewed(), None);
    assert!(cpm::texture_upload(Instant::now()).is_none());
    cpm::preview_select(1);
    assert_eq!(wait_for_preview(), cpm::PreviewStatus::Ready);
    // Its image goes into the sprite (no model selected: the preview takes the top left); only
    // then does it draw.
    let rect = [0.0, 0.0, 100.0, 100.0];
    assert!(cpm::picker_preview_quads(&cpm::Worn::default(), &cpm::PreviewView::default(), rect, 0).is_none());
    let (w, h, rgba) = cpm::texture_upload(Instant::now()).expect("the preview's image");
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    let quads = cpm::picker_preview_quads(&cpm::Worn::default(), &cpm::PreviewView::default(), rect, 0).expect("drawn");
    assert!(!quads.quads.is_empty());
    // Once the icons are read they get places in the sprite (a new upload).
    wait_for_tiles();
    assert!(cpm::texture_upload(Instant::now()).is_some());
    // Nothing changed since: no new upload until REUPLOAD.
    assert!(cpm::texture_upload(Instant::now()).is_none());
    let tiles = cpm::picker_tiles();
    assert_eq!(tiles[0].name, "ReggieTheRat");
    assert!(tiles[0].icon_at.is_some());
    // Use it: the setting value is the settings page's for that file; the selection follows it.
    let name = cpm::use_previewed().expect("a file to keep");
    assert_eq!(name, "SkullDog.cpmmodel");
    assert_eq!(name, cpm::model_setting(Some(&dir.join("SkullDog.cpmmodel"))));
    assert_eq!(cpm::selected(), cpm::selection_from_setting(&dir, Some(name)));
    // Back on the gestures page.
    assert!(!cpm::picker_shown());
    cpm::ensure_loaded();
    assert!(cpm::has_model());
}

#[test]
fn picker_keeps_the_active_model_in_place() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let dir = scratch("picker_active");
    std::fs::copy(sample("ReggieTheRat.cpmmodel"), dir.join("ReggieTheRat.cpmmodel")).unwrap();
    cpm::select(Some(sample("SkullDog.cpmmodel")));
    cpm::ensure_loaded();
    let (aw, ah, active) = cpm::texture_upload(Instant::now()).unwrap();
    cpm::open_picker(&dir);
    cpm::preview_select(0);
    assert_eq!(wait_for_preview(), cpm::PreviewStatus::Ready);
    let (w, h, rgba) = cpm::texture_upload(Instant::now()).expect("grown");
    assert!(w >= aw && h > ah);
    // The active model's pixels are where they were.
    for y in 0..ah as usize {
        let row = aw as usize * 4;
        assert_eq!(&rgba[y * w as usize * 4..][..row], &active[y * row..][..row]);
    }
    // The world draw keeps going all along.
    assert!(cpm::quads(&PoseInput { on_ground: true, ..Default::default() }, &cpm::Worn::default()).is_some());
    // Closed: back to the active model's image alone.
    cpm::close_picker();
    let (w, h, rgba) = cpm::texture_upload(Instant::now()).expect("shrunk");
    assert_eq!((w, h), (aw, ah));
    assert_eq!(rgba, active);
}

fn x_span(faces: &[cpm::PreviewFace]) -> f32 {
    let xs: Vec<f32> = faces.iter().flat_map(|f| f.corners).map(|c| c[0]).collect();
    xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min)
}

#[test]
fn long_models_fit_whichever_way_they_turn() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // The fit is the animated pose's reach round the vertical axis: a long dragon turned side-on
    // still fits the rect (the game's preview is 248 by 321 GUI pixels), and fills it when it is.
    let bytes = std::fs::read(sample("rainwing-old.cpmproject")).unwrap();
    let project = cpm_core::project::read_project(&bytes).unwrap();
    let mut dragon = cpm_core::load(&cpm_core::export::export_model(&project).unwrap()).unwrap();
    let boxes = dragon.pose(&cpm_core::PoseInput { on_ground: true, ..Default::default() });
    let rect = [384.0, 8.0, 248.0, 321.0];
    let mut widest = 0.0f32;
    for yaw in (0..360).step_by(15) {
        let view = cpm::PreviewView { yaw: yaw as f32, pitch: 0.0, zoom: 1.0 };
        let faces = cpm::preview_faces(&boxes, &view, rect);
        assert!(corners_inside(&faces, rect), "yaw {yaw}");
        widest = widest.max(x_span(&faces) / rect[2]);
    }
    assert!(widest > 0.8, "side-on it fills {widest}");
    // A tall model is not made a speck either.
    let reggie = cpm::preview_faces(&reggie_boxes(), &cpm::PreviewView::default(), rect);
    assert!(y_span(&reggie) / rect[3] > 0.8);
}

#[test]
fn rainwing_is_fitted_animated_not_spread() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // Rainwing's base pose spreads its wings ~5 blocks each side; its standing animation folds
    // them. The preview fits what is seen: the animated pose.
    let bytes = std::fs::read(sample("rainwing-old.cpmproject")).unwrap();
    let project = cpm_core::project::read_project(&bytes).unwrap();
    let mut dragon = cpm_core::load(&cpm_core::export::export_model(&project).unwrap()).unwrap();
    let rest = cpm::rest_fit(&dragon).unwrap();
    let live = cpm::preview_fit(&dragon.pose(&cpm_core::PoseInput { on_ground: true, ..Default::default() })).unwrap();
    assert!(rest.half[0] > live.half[0] * 1.4, "rest {rest:?} live {live:?}");
    assert!(live.half[0] < 3.5, "{live:?}");
}

#[test]
fn picker_pages_start_at_the_default_view() {
    cpm::show_menu();
    cpm::with_menu(|m| m.preview.scroll(5.0)).unwrap();
    let dir = std::env::temp_dir().join(format!("cpm-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    cpm::open_picker(&dir);
    assert_eq!(cpm::with_menu(|m| m.preview.zoom).unwrap(), 1.0, "opening the picker resets the zoom");
    cpm::with_menu(|m| m.preview.scroll(5.0)).unwrap();
    cpm::close_picker();
    assert_eq!(cpm::with_menu(|m| m.preview.zoom).unwrap(), 1.0, "Back resets it too");
    cpm::hide_menu();
    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn preview_holds_nothing() {
    // The preview draws no items, so it poses empty hands (no HOLDING / use / crossbow poses).
    let busy = PoseInput {
        time_ms: 1000,
        on_ground: true,
        hurt: true,
        holding: [true, true],
        crossbow_charged: [false, true],
        using: Some((cpm_core::Hand::Main, cpm_core::UseKind::Eat, 0.0)),
        ..PoseInput::default()
    };
    cpm::record_input(&busy);
    let empty = PoseInput { holding: [false; 2], crossbow_charged: [false; 2], using: None, time_ms: 1100, ..busy };
    assert_eq!(cpm::preview_input(1100), empty);
}

#[test]
fn converter_version_is_4() {
    // v4 carries animated textures: earlier conversions are re-converted.
    assert_eq!(cpm::CONVERTER_VERSION, 4);
    assert_eq!(cpm::converted_version(&format!("x (converted by CPM for Horizon, v{})", cpm::CONVERTER_VERSION)), Some(4));
}

#[test]
fn preview_fit_includes_render_scale() {
    if common::missing(&["Expie_by_Clouddy.cpmproject", "ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    // CPM's own previews draw the render scale too (DirectModelRenderManager.doRender0), and the
    // fit frames the scaled model.
    let mut project = cpm_core::project::read_project(&std::fs::read(expie()).unwrap()).unwrap();
    let plain = cpm_core::load(&cpm_core::export::export_model(&project).unwrap()).unwrap();
    project.render_transform =
        Some(cpm_core::effects::PartTransform { pos: [0.0; 3], rot: [0.0; 3], scale: [2.0; 3] });
    let scaled = cpm_core::load(&cpm_core::export::export_model(&project).unwrap()).unwrap();
    let (a, b) = (cpm::rest_fit(&plain).unwrap(), cpm::rest_fit(&scaled).unwrap());
    for k in 0..2 {
        assert!((b.half[k] / a.half[k] - 2.0).abs() < 0.05, "plain {a:?} scaled {b:?}");
    }
}

fn quad_at(z: f32, tint: u32) -> cpm::Quad {
    cpm::Quad {
        corners: [[-1.0, 1.0, z], [-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z]],
        uv: [0.0; 4],
        tint,
        glow: false,
        emissive: false,
        glint: None,
        sprite: 0,
    }
}

/// CPM draws models as `entityTranslucent`: Minecraft sorts their quads farthest first by the
/// camera distance of their centres (stable for ties), and a later quad wins a depth tie.
#[test]
fn quads_sort_back_to_front_from_the_camera() {
    let mut q = vec![quad_at(1.0, 1), quad_at(5.0, 2), quad_at(3.0, 3), quad_at(5.0, 4)];
    // The draw pose puts the model 10 blocks in front of the camera (camera at the origin).
    cpm::back_to_front(&mut q, cpm_core::glam::Mat4::from_translation(cpm_core::glam::Vec3::new(0.0, 0.0, 10.0)));
    assert_eq!(q.iter().map(|q| q.tint).collect::<Vec<_>>(), vec![2, 4, 3, 1]);
    // From the other side the order turns round; ties keep their order.
    cpm::back_to_front(&mut q, cpm_core::glam::Mat4::from_translation(cpm_core::glam::Vec3::new(0.0, 0.0, -10.0)));
    assert_eq!(q.iter().map(|q| q.tint).collect::<Vec<_>>(), vec![1, 3, 2, 4]);
}

/// CPM's glow layer (`CustomRenderTypes.EYES`: additive, culled, no depth writes): glowing faces
/// turned away from the camera are left out; untinted ones go to the additive pass, tinted ones
/// (which that pass cannot tint) stay with the solid faces; the rest are solid.
#[test]
fn glow_faces_split_for_the_world_draw() {
    use cpm_core::glam::{Mat4, Vec3};
    // A quad at z facing +z (corners TL, BL, BR, TR seen from +z), and one facing -z.
    let facing = |z: f32, tint: u32, glow: bool| cpm::Quad { glow, ..quad_at(z, tint) };
    let away = |z: f32, tint: u32| {
        let mut q = facing(z, tint, true);
        q.corners.swap(1, 3);
        q
    };
    let quads = vec![facing(0.0, 0xFF_FFFF, false), facing(0.0, 0xFF_FFFF, true), away(0.0, 0xFF_FFFF), facing(0.0, 0x00_FF00, true)];
    // The camera 10 blocks out on +z.
    let pose = Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0));
    let (solid, additive) = cpm::split_glow(&quads, pose);
    assert_eq!(solid.iter().map(|q| (q.glow, q.tint)).collect::<Vec<_>>(), vec![(false, 0xFF_FFFF), (true, 0x00_FF00)]);
    assert_eq!(additive.len(), 1);
    assert!(additive[0].glow && additive[0].tint == 0xFF_FFFF);
}

#[test]
fn swirl_uv_is_the_place_in_the_sprite() {
    assert_eq!(cpm::swirl_uv([0.25, 0.75], [0.0, 0.5, 0.5, 1.0]), [0.5, 0.5]);
}
