//! Gesture buttons and parameters (CPM's `parts/anim/menu/*` + parameter bytes).

mod common;

use std::sync::OnceLock;

use cpm_core::anim::engine::{Engine, Frame, PoseInput};
use cpm_core::anim::{AnimSet, Button, ButtonKind, ButtonMeta, ButtonState, Clip, Gesture, GestureKind, Interp, Target, CHANNELS};
use cpm_core::export::{element_ids, export_model};
use cpm_core::project::{read_project, Project, ProjectElement};
use cpm_core::LoadedModel;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn rainwing_project() -> &'static Project {
    static P: OnceLock<Project> = OnceLock::new();
    P.get_or_init(|| read_project(&sample("rainwing-old.cpmproject")).unwrap())
}

fn rainwing() -> LoadedModel {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    cpm_core::load(BYTES.get_or_init(|| export_model(rainwing_project()).unwrap())).unwrap()
}

fn idx(e: &Engine, name: &str) -> usize {
    e.buttons().iter().position(|b| b.name == name).unwrap_or_else(|| panic!("no button {name:?}: {:?}", e.buttons()))
}

fn at(t: u64) -> PoseInput {
    PoseInput { time_ms: t, on_ground: true, ..Default::default() }
}

/// Element ids of the project elements called `name`.
fn ids_named(p: &Project, name: &str) -> Vec<u32> {
    fn walk(e: &ProjectElement, name: &str, out: &mut Vec<i64>) {
        if e.name == name {
            out.push(e.store_id);
        }
        for c in &e.children {
            walk(c, name, out);
        }
    }
    let mut sids = Vec::new();
    for r in &p.roots {
        for c in &r.children {
            walk(c, name, &mut sids);
        }
    }
    let ids = element_ids(p);
    sids.iter().flat_map(|s| ids.get(s).cloned().unwrap_or_default()).collect()
}

/// A fresh rainwing engine, set up by `f`, sampled at a fixed time.
fn frame_with(f: impl FnOnce(&mut Engine)) -> Frame {
    let mut m = rainwing();
    f(&mut m.engine);
    m.engine.sample(&at(1000))
}

#[test]
fn rainwing_buttons() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let m = rainwing();
    let e = &m.engine;
    let visible: Vec<&Button> = e.buttons().iter().filter(|b| !b.hidden).collect();
    let kind = |n: &str| &visible.iter().find(|b| b.name == n).unwrap_or_else(|| panic!("{n}: {visible:?}")).kind;
    assert!(matches!(kind("Head tilt"), ButtonKind::Slider { .. }));
    assert!(matches!(kind("Armor toggle"), ButtonKind::Toggle));
    assert!(matches!(kind("Eyes"), ButtonKind::Dropdown { options } if options.len() >= 2), "{:?}", kind("Eyes"));
    assert!(matches!(kind("Sit"), ButtonKind::CustomPose));
    for g in ["No", "Yes", "Growl", "Venom"] {
        assert!(matches!(kind(g), ButtonKind::Gesture), "{g}");
    }
    for s in ["Jaw", "Frill flare", "Wings"] {
        assert!(matches!(kind(s), ButtonKind::Slider { .. }), "{s}");
    }
    let ht = idx(e, "Head tilt");
    let ButtonKind::Slider { max } = e.buttons()[ht].kind else { unreachable!() };
    let ButtonState::Value(v) = e.button_state(ht) else { panic!("{:?}", e.button_state(ht)) };
    assert!((v as f32 / max as f32 - 0.5).abs() <= 0.02, "{v}/{max}");
}

#[test]
fn head_tilt_moves_head() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let head = ids_named(rainwing_project(), "Head");
    assert!(!head.is_empty());
    let rot = |v: Option<u8>| {
        let f = frame_with(|e| {
            let ht = idx(e, "Head tilt");
            let ButtonKind::Slider { max } = e.buttons()[ht].kind else { unreachable!() };
            e.set_value(ht, v.unwrap_or(max));
        });
        head.iter().map(|id| f.parts.get(id).and_then(|p| p.rot)).collect::<Vec<_>>()
    };
    let low = rot(Some(0));
    let high = rot(None);
    assert_ne!(low, high, "{low:?}");
}

/// Rainwing's `Armor toggle` only hides elements under the armor roots (`armor_helmet`, ...).
/// This checks the toggle and its parameters; its effect on the worn armour cubes is checked in
/// `tests/armor.rs` (`armor_toggle_hides_the_armor_cubes`).
#[test]
fn armor_toggle_flips() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let e = &mut m.engine;
    let i = idx(e, "Armor toggle");
    let p = e.parameters();
    assert_eq!(e.button_state(i), ButtonState::Active(false));
    e.press(i, 0);
    assert_eq!(e.button_state(i), ButtonState::Active(true));
    assert_ne!(e.parameters(), p);
    e.press(i, 0);
    assert_eq!(e.button_state(i), ButtonState::Active(false));
    assert_eq!(e.parameters(), p);
}

#[test]
fn toggle_changes_visibility() {
    let mut clip = const_clip(CUBE_A, 0.0);
    clip.show = vec![vec![false]];
    clip.present = vec![[false, false, false, false, true]];
    let set = AnimSet { gestures: vec![layer("hat", None, 0, clip)], ..Default::default() };
    let mut e = Engine::new(set);
    let vis = |e: &mut Engine| e.sample(&at(0)).parts.get(&CUBE_A).and_then(|p| p.visible);
    assert_eq!(vis(&mut e), None);
    e.press(0, 0);
    assert_eq!(e.button_state(0), ButtonState::Active(true));
    assert_eq!(vis(&mut e), Some(false));
    e.press(0, 0);
    assert_eq!(vis(&mut e), None);
}

#[test]
fn eyes_dropdown_changes_elements() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let with = |opt: usize| {
        frame_with(|e| {
            let i = idx(e, "Eyes");
            e.select(i, opt);
            assert_eq!(e.button_state(i), ButtonState::Selected(opt));
        })
    };
    assert_ne!(with(0).parts, with(1).parts);
}

#[test]
fn custom_pose_press_and_reset() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let e = &mut m.engine;
    let sit = idx(e, "Sit");
    assert_eq!(e.button_state(sit), ButtonState::Active(false));
    e.press(sit, 0);
    assert_eq!(e.button_state(sit), ButtonState::Active(true));
    e.reset_pose();
    assert_eq!(e.button_state(sit), ButtonState::Active(false));
}

#[test]
fn gesture_press_and_reset() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let e = &mut m.engine;
    let no = idx(e, "No");
    e.press(no, 0);
    assert_eq!(e.button_state(no), ButtonState::Active(true));
    e.reset_gesture();
    assert_eq!(e.button_state(no), ButtonState::Active(false));
}

#[test]
fn params_roundtrip() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let mut m = rainwing();
    let e = &mut m.engine;
    let p = e.parameters();
    assert!(e.set_parameters(&p));
    assert_eq!(e.parameters(), p);

    let ht = idx(e, "Head tilt");
    let armor = idx(e, "Armor toggle");
    let eyes = idx(e, "Eyes");
    e.set_value(ht, 10);
    e.press(armor, 0);
    e.select(eyes, 2);
    let changed = e.parameters();
    assert_ne!(changed, p);

    let mut long = changed.clone();
    long.push(0);
    assert!(!e.set_parameters(&long));
    assert!(!e.set_parameters(&changed[1..]));
    assert_eq!(e.parameters(), changed);

    let mut fresh = rainwing();
    assert!(fresh.engine.set_parameters(&changed));
    for i in [ht, armor, eyes] {
        assert_eq!(fresh.engine.button_state(i), e.button_state(i), "{}", e.buttons()[i].name);
    }
}

fn legacy(name: &str) -> LoadedModel {
    cpm_core::load(&sample(name)).unwrap()
}

#[test]
fn legacy_reggie_buttons() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    for file in ["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel"] {
        let m = legacy(file);
        let e = &m.engine;
        let mut want = 0;
        for g in e.gestures() {
            let kind = match g.kind {
                GestureKind::Gesture => ButtonKind::Gesture,
                GestureKind::Layer => ButtonKind::Toggle,
                GestureKind::Value => ButtonKind::Slider { max: 255 },
                _ => continue,
            };
            want += 1;
            let b = e.buttons().iter().find(|b| b.name == g.name).unwrap_or_else(|| panic!("{file}: {}", g.name));
            assert_eq!(std::mem::discriminant(&b.kind), std::mem::discriminant(&kind), "{file}: {}", g.name);
        }
        for p in e.custom_poses() {
            want += 1;
            assert!(e.buttons().iter().any(|b| b.name == p && b.kind == ButtonKind::CustomPose), "{file}: {p}");
        }
        // Neither sample has gesture buttons (Reggie: poses only; SkullDog: one staged `c:`
        // setup animation, which `LegacyAnimationParser` never makes a button).
        assert_eq!(e.buttons().len(), want, "{file}: {:?}", e.buttons());
        // Legacy layout: pose byte, gesture byte, then one byte per layer / value layer.
        let layers = e.gestures().iter().filter(|g| matches!(g.kind, GestureKind::Layer | GestureKind::Value)).count();
        assert_eq!(e.parameters().len(), layers + 2, "{file}");
    }
}

const CUBE_A: u32 = 10;
const CUBE_B: u32 = 11;

fn const_clip(id: u32, v: f32) -> Clip {
    let mut ch: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0.0]);
    ch[3] = vec![v];
    Clip {
        components: vec![id],
        targets: vec![Target::Cube(0)],
        frames: 1,
        duration_ms: 1000,
        priority: 0,
        additive: false,
        must_finish: false,
        interp: Interp::NoInterpolate,
        dynamic: false,
        channels: vec![ch],
        show: vec![vec![true]],
        present: vec![[false, true, false, false, false]; 1],
    }
}

fn layer(name: &str, group: Option<&str>, order: i32, clip: Clip) -> Gesture {
    Gesture {
        name: name.into(),
        kind: GestureKind::Layer,
        looping: true,
        layer: true,
        clips: vec![clip],
        meta: ButtonMeta { order, group: group.map(Into::into), ..Default::default() },
        ..Default::default()
    }
}

#[test]
fn legacy_grouped_layers_become_dropdown() {
    let set = AnimSet {
        gestures: vec![
            layer("wide", Some("Eyes"), 0, const_clip(CUBE_A, 30.0)),
            layer("hat", None, 1, const_clip(CUBE_B, 40.0)),
            layer("closed", Some("Eyes"), 2, const_clip(CUBE_B, 50.0)),
        ],
        ..Default::default()
    };
    let mut e = Engine::new(set);
    let names: Vec<&str> = e.buttons().iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Eyes", "hat"]);
    assert_eq!(e.buttons()[0].kind, ButtonKind::Dropdown { options: vec![String::new(), "wide".into(), "closed".into()] });
    assert_eq!(e.buttons()[1].kind, ButtonKind::Toggle);
    assert_eq!(e.buttons().iter().filter(|b| matches!(b.kind, ButtonKind::Dropdown { .. })).count(), 1);
    assert_eq!(e.parameters().len(), 5);

    let rot = |e: &mut Engine, id| e.sample(&at(0)).parts.get(&id).and_then(|p| p.rot).map(|r| r[0]);
    assert_eq!(e.button_state(0), ButtonState::Selected(0));
    assert_eq!(rot(&mut e, CUBE_A), None);
    e.select(0, 1);
    assert_eq!(e.button_state(0), ButtonState::Selected(1));
    assert_eq!(rot(&mut e, CUBE_A), Some(30.0));
    e.select(0, 2);
    assert_eq!(e.button_state(0), ButtonState::Selected(2));
    assert_eq!(rot(&mut e, CUBE_A), None);
    assert_eq!(rot(&mut e, CUBE_B), Some(50.0));
    e.select(0, 0);
    assert_eq!(rot(&mut e, CUBE_B), None);
    // Out of range: ignored.
    e.select(0, 7);
    assert_eq!(e.button_state(0), ButtonState::Selected(0));
    // The ungrouped layer is its own toggle.
    e.press(1, 0);
    assert_eq!(e.button_state(1), ButtonState::Active(true));
    assert_eq!(rot(&mut e, CUBE_B), Some(40.0));
}

/// Legacy `ANIMATION_DATA` button metadata: ORDER, GROUP, BUTTON_HIDDEN, SLIDER_OPT,
/// LAYER_DEFAULT, custom pose ORDER.
#[test]
fn legacy_metadata_builds_buttons() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    use cpm_core::write::Writer;
    let raw = cpm_core::format::read_cpmmodel(&sample("ReggieTheRat.cpmmodel")).unwrap().data;
    let model = cpm_core::model::Model::build(raw).unwrap();
    let mut w = Writer::new();
    let gesture = |w: &mut Writer, id: u8, name: &str| {
        w.object_block(2, Writer::new().u8(id).utf(name).u8(2));
    };
    gesture(&mut w, 0, "$value$tilt");
    w.object_block(21, Writer::new().u8(0).bool(true).u8(50)); // SLIDER_OPT
    w.object_block(14, Writer::new().u8(0).i8(3)); // ORDER
    w.object_block(13, Writer::new().u8(0).u8(200)); // LAYER_DEFAULT
    gesture(&mut w, 1, "$layer$hat");
    w.object_block(22, Writer::new().u8(1)); // BUTTON_HIDDEN
    w.object_block(13, Writer::new().u8(1).u8(1));
    gesture(&mut w, 2, "wave");
    w.object_block(14, Writer::new().u8(2).i8(-1));
    w.object_block(1, Writer::new().u8(0).u8(3).u8(0).utf("sit")); // custom POSE
    w.object_block(14, Writer::new().u8(3).i8(1));
    gesture(&mut w, 4, "$layer$a");
    w.object_block(16, Writer::new().u8(4).utf("G")); // GROUP
    w.u8(0);
    let set = cpm_core::anim::decode_legacy(&w.into_bytes(), &model).unwrap();
    let e = Engine::new(set);
    let got: Vec<(&str, &ButtonKind, bool, i32)> =
        e.buttons().iter().map(|b| (b.name.as_str(), &b.kind, b.hidden, b.order)).collect();
    assert_eq!(
        got,
        [
            ("wave", &ButtonKind::Gesture, false, -1),
            ("G", &ButtonKind::Dropdown { options: vec![String::new(), "a".into()] }, false, 0),
            ("hat", &ButtonKind::Toggle, true, 0),
            ("sit", &ButtonKind::CustomPose, false, 1),
            ("tilt", &ButtonKind::Slider { max: 50 }, false, 3),
        ]
    );
    // Layers / value layers get parameters 2.. in decode order, defaulting to LAYER_DEFAULT.
    assert_eq!(e.parameters(), [0, 0, 200, 1, 0]);
    assert_eq!(e.button_state(2), ButtonState::Active(true));
    // round(200 * 50 / 255) = 39.
    assert_eq!(e.button_state(4), ButtonState::Value(39));
}
