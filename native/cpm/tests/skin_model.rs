// The model stored in the local player's skin (`cpm::set_skin`): used when nothing is selected, an
// entry of the Model button's cycle and of the picker only while the skin holds one. Each test
// runs on its own thread, so its own state.
mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cpm_core::skin::{self, Image, SkinDataOutputStream};
use cpm_core::write::{Writer, checksummed};
use horizon_mod_cpm as cpm;

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Model data with one textured cube on the head and no texture of its own (it draws with the
/// skin), skin type wide.
fn cube_model() -> Vec<u8> {
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut cubes = Writer::new();
    cubes.varint(1);
    cubes.u8(0x03).varint(0); // HAS_MESH | HAS_TEXTURE, parent: the head
    cubes.var_vec3([0.0; 3]).angle([0.0; 3]);
    cubes.var_vec3([4.0; 3]).var_vec3([-2.0; 3]).varint(0).varint(0);
    let mut body = Writer::new();
    body.object_block(11, &skin_type).object_block(21, &cubes).object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

/// A wide 64x64 skin PNG holding `data` (none: a plain skin).
fn skin_png(data: Option<&[u8]>) -> Vec<u8> {
    let mut img = Image::new(64, 64);
    img.argb.fill(0xFF33_6699);
    if let Some(data) = data {
        let mut out = SkinDataOutputStream::new(&mut img, skin::template().unwrap(), 1);
        for &b in data {
            out.write(b).unwrap();
        }
    }
    img.encode_png()
}

fn skin(location: &str, data: Option<&[u8]>) -> cpm::SkinInput {
    let png = skin_png(data);
    let texture = Image::decode(&png).map(|i| ([i.w, i.h], i.rgba())).ok();
    cpm::SkinInput { location: location.into(), slim: false, png, texture }
}

fn idle() -> cpm::ConvertView<'static> {
    cpm::ConvertView { busy: false, last: None, limit_kib: 8, old_conversion: false }
}

#[test]
fn skin_model_used_when_nothing_selected() {
    cpm::set_data_dir(scratch("skin_used"));
    cpm::select(None);
    cpm::set_skin(Some(skin("skins/aa01", Some(&cube_model()))));
    assert!(cpm::skin_known("skins/aa01", false));
    assert!(!cpm::skin_known("skins/aa01", true));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    assert!(cpm::has_model());
    assert_eq!(cpm::active_selection(), Some(cpm::skin_selection()));
    assert!(cpm::skin_model_in_use());
    // Drawn with the skin itself, declared 64x64.
    assert_eq!(cpm::with_model(|m| (m.png_px, m.texture_px, m.model.cubes.len())), Some(([64, 64], [64, 64], 1)));
    // The settings page says so.
    let labels = cpm::settings_labels(cpm::active_selection().as_deref(), None, &idle());
    assert_eq!(labels[0], "Model: Your skin's model");
    assert!(labels.iter().any(|l| l == cpm::SKIN_IN_USE), "{labels:?}");
    // Kept as its own setting value.
    assert_eq!(cpm::model_setting(Some(&cpm::skin_selection())), cpm::SKIN_SETTING);
    assert_eq!(cpm::selection_from_setting(Path::new("nowhere"), Some(cpm::SKIN_SETTING.into())), Some(cpm::skin_selection()));
    // A selected file wins over it.
    let dir = scratch("skin_used_file");
    let file = dir.join("other.cpmmodel");
    std::fs::write(&file, b"not a model").unwrap();
    cpm::select(Some(file.clone()));
    cpm::ensure_loaded();
    assert!(!cpm::skin_model_in_use());
    assert!(cpm::last_error().unwrap().contains("other.cpmmodel"));
    // Nothing selected again: the skin's model.
    cpm::select(None);
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert_eq!(cpm::last_error(), None);
    // A new skin without a model: the vanilla player, no error, even with the skin's model chosen.
    cpm::select(Some(cpm::skin_selection()));
    cpm::set_skin(Some(skin("skins/bb02", None)));
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
    assert_eq!(cpm::last_error(), None);
    assert_eq!(cpm::active_selection(), None);
    assert!(!cpm::skin_model_in_use());
    assert_eq!(cpm::settings_labels(cpm::active_selection().as_deref(), None, &idle())[0], "Model: None");
    // Corrupt data (a bad checksum): an error line, as for any model, and no model.
    let mut bad = cube_model();
    let n = bad.len();
    bad[n - 1] ^= 0xFF;
    cpm::set_skin(Some(skin("skins/cc03", Some(&bad))));
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
    let error = cpm::last_error().expect("an error");
    assert!(error.starts_with("cpm: skin model: "), "{error}");
    // The skin changes back to one with a model: loaded again.
    cpm::set_skin(Some(skin("skins/aa01", Some(&cube_model()))));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    assert_eq!(cpm::last_error(), None);
    // No skin known (a default skin): vanilla.
    cpm::set_skin(None);
    cpm::ensure_loaded();
    assert!(!cpm::has_model());
    assert_eq!(cpm::last_error(), None);
}

#[test]
fn skin_model_links_resolve() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    // A skin holding a link model's data (CPM's usual skin export: the package in a gist): it
    // loads through the link cache as a link model file does.
    let link = "git:skintest/5k1n";
    let mut skin_type = Writer::new();
    skin_type.u8(1);
    let mut payload = Writer::new();
    payload.u8(link.len() as u8).bytes(link.as_bytes());
    let mut body = Writer::new();
    body.object_block(11, &skin_type).object_block(20, &payload).object_block(0, &Writer::new());
    let data = checksummed(0x53, &body.into_bytes());
    let dir = scratch("skin_link");
    cpm::set_data_dir(dir.clone());
    let cache = cpm::cache_path(&dir, link).unwrap();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let reggie = Path::new(env!("CARGO_MANIFEST_DIR")).join("mixins/cpm/cpm-core/tests/samples/ReggieTheRat.cpmmodel");
    std::fs::write(&cache, cpm_core::link::gist_text(&std::fs::read(reggie).unwrap()).unwrap()).unwrap();
    cpm::select(None);
    cpm::set_skin(Some(skin("skins/ab07", Some(&data))));
    cpm::ensure_loaded();
    assert_eq!(cpm::last_error(), None);
    // Reggie's own texture, not the skin.
    assert_eq!(cpm::with_model(|m| (m.model.cubes.len(), m.png_px == [64, 64] && m.texture_px == [64, 64])), Some((26, false)));
}

/// Polls the picker's preview until it is no longer loading (10 s at most).
fn wait_for_preview() -> cpm::PreviewStatus {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        cpm::poll_picker();
        let s = cpm::picker_preview_status();
        if s != cpm::PreviewStatus::Loading || Instant::now() > end {
            return s;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_tiles() {
    let end = Instant::now() + Duration::from_secs(10);
    while cpm::picker_tiles().iter().any(|t| t.scanning) && Instant::now() < end {
        cpm::poll_picker();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn skin_model_entry_only_when_present() {
    let dir = scratch("skin_entry");
    let a = dir.join("a.cpmmodel");
    std::fs::write(&a, b"not a model").unwrap();
    // No skin, or one without a model: no entry.
    for s in [None, Some(skin("skins/dd04", None))] {
        cpm::set_skin(s);
        assert_eq!(cpm::model_cycle(vec![a.clone()]), vec![a.clone()]);
        cpm::open_picker(&dir);
        wait_for_tiles();
        assert_eq!(cpm::picker_tiles().iter().map(|t| t.name.clone()).collect::<Vec<_>>(), ["a"]);
        cpm::close_picker();
    }
    // A skin with a model: first in the cycle and in the picker.
    cpm::set_skin(Some(skin("skins/ee05", Some(&cube_model()))));
    let cycle = cpm::model_cycle(vec![a.clone()]);
    assert_eq!(cycle, vec![cpm::skin_selection(), a.clone()]);
    // The Model button: nothing chosen shows the skin's model, so the next is the first file; after
    // the last file comes "nothing" (the skin's model again).
    cpm::select(None);
    assert_eq!(cpm::active_selection(), Some(cpm::skin_selection()));
    assert_eq!(cpm::click(0, &cycle, cpm::active_selection().as_deref()), cpm::Click::Select(Some(a.clone())));
    assert_eq!(cpm::click(0, &cycle, Some(&a)), cpm::Click::Select(None));
    cpm::open_picker(&dir);
    wait_for_tiles();
    let tiles = cpm::picker_tiles();
    assert_eq!(tiles.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), [cpm::SKIN_MODEL, "a"]);
    assert_eq!(tiles[0].error, None);
    assert_eq!(tiles[1].error.as_deref(), Some(cpm::CANT_READ));
    cpm::preview_select(0);
    assert_eq!(wait_for_preview(), cpm::PreviewStatus::Ready);
    assert_eq!(cpm::use_previewed().as_deref(), Some(cpm::SKIN_SETTING));
    assert_eq!(cpm::selected(), Some(cpm::skin_selection()));
    cpm::ensure_loaded();
    assert!(cpm::has_model());
    // The skin loses its model: the entry goes.
    cpm::set_skin(Some(skin("skins/ff06", None)));
    assert_eq!(cpm::model_cycle(vec![a.clone()]), vec![a.clone()]);
}
