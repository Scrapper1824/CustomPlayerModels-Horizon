mod common;

use cpm_core::effects::{decode_effects, Effect};
use cpm_core::format::{read_cpmmodel, RawCube, RawModel, SkinType};
use cpm_core::model::Model;

fn reggie_raw() -> RawModel {
    let b = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/ReggieTheRat.cpmmodel")).unwrap();
    read_cpmmodel(&b).unwrap().data
}
fn reggie() -> Model {
    Model::build(reggie_raw()).unwrap()
}
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.002
}
fn parent_of(m: &Model, i: usize) -> Option<usize> {
    m.cubes.iter().position(|c| c.children.contains(&i))
}

#[test]
fn decodes_reggie_effects() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let e = decode_effects(&reggie_raw().effects);
    match &e[0] {
        Effect::Scale { cube, mc_scale, scale } => {
            assert_eq!(*cube, 18);
            assert!(near(*mc_scale, -0.0997), "{mc_scale}");
            assert!(scale.iter().all(|s| near(*s, 1.0)), "{scale:?}");
        }
        o => panic!("{o:?}"),
    }
    // HIDE_SKULL, written only to turn hideHeadIfSkull off.
    assert!(e.contains(&Effect::HideSkull { hide: false }));
    assert!(e.contains(&Effect::Other(9)));
}

#[test]
fn head_pivot_offset() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    assert!(near(reggie().bones[0].pivot[1], -1.5));
}

#[test]
fn left_arm_pivot() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let p = reggie().bones[2].pivot;
    assert!(near(p[0], 4.5) && near(p[1], 2.0) && near(p[2], 0.0), "{p:?}");
}

#[test]
fn tree_parents() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let m = reggie();
    let n23 = m.node_index(23).unwrap();
    let n22 = parent_of(&m, n23).unwrap();
    assert_eq!(m.cubes[n22].id, 22);
    let n21 = parent_of(&m, n22).unwrap();
    assert_eq!(m.cubes[n21].id, 21);
    assert!(parent_of(&m, n21).is_none());
    assert!(m.roots_by_bone[1].contains(&n21));
}

#[test]
fn scale_effect_applied() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let m = reggie();
    let n = &m.cubes[m.node_index(18).unwrap()];
    assert!(n.scale.iter().all(|s| near(*s, 1.0)));
    assert!(near(n.mc_scale, -0.0997));
}

#[test]
fn color_cube() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let m = reggie();
    assert_eq!(m.cubes[m.node_index(21).unwrap()].color, Some(0x000000));
}

fn cube(id: u32, parent: u32) -> RawCube {
    RawCube {
        id, parent, size: [1.0; 3], pos: [0.0; 3], offset: [0.0; 3], rotation: [0.0; 3],
        tex_size: 1, u: 0, v: 0, rgb: 0, mesh_scale: [1.0; 3], scale: [1.0; 3], mc_scale: 0.0, hidden: false,
    }
}

#[test]
fn cycle_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let mut raw = reggie_raw();
    raw.skin_type = SkinType::Default;
    raw.cubes = vec![cube(10, 11), cube(11, 10)];
    raw.effects.clear();
    assert!(Model::build(raw).is_err());
}

#[test]
fn missing_parent_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let mut raw = reggie_raw();
    raw.cubes = vec![cube(10, 99)];
    assert!(Model::build(raw).is_err());
}

/// A chain of `n` cubes, the first on the body, each the next one's parent.
fn chain(n: u32) -> RawModel {
    let mut raw = reggie_raw();
    raw.cubes = (0..n).map(|i| cube(10 + i, if i == 0 { 1 } else { 9 + i })).collect();
    raw.effects.clear();
    raw
}

#[test]
fn tree_depth_is_capped() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    use cpm_core::io::CpmError;
    use cpm_core::model::MAX_DEPTH;
    assert_eq!(MAX_DEPTH, 256);
    // 256 nested cubes are fine; 257 are refused as invalid, with no recursion or stack use.
    let m = Model::build(chain(256)).unwrap();
    assert_eq!(m.cubes.len(), 256);
    match Model::build(chain(257)) {
        Err(CpmError::Invalid(why)) => assert!(why.contains("deeper than 256"), "{why}"),
        other => panic!("{:?}", other.map(|m| m.cubes.len())),
    }
    // Also when the deep chain is listed leaf first (depths found from memoised parents).
    let mut raw = chain(257);
    raw.cubes.reverse();
    assert!(matches!(Model::build(raw), Err(CpmError::Invalid(_))));
    // A long cycle is still reported as a cycle.
    let mut raw = chain(300);
    raw.cubes[0].parent = 10 + 299;
    match Model::build(raw) {
        Err(CpmError::Invalid(why)) => assert!(why.contains("cycle") || why.contains("deeper"), "{why}"),
        other => panic!("{:?}", other.map(|m| m.cubes.len())),
    }
}

#[test]
fn wide_trees_build_in_linear_time() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    // 20 000 cubes in chains of 200: the old O(n^2) walk would take many seconds.
    let mut raw = reggie_raw();
    raw.effects.clear();
    raw.cubes = (0..20_000u32).map(|i| cube(10 + i, if i % 200 == 0 { 1 } else { 9 + i })).collect();
    let started = std::time::Instant::now();
    assert_eq!(Model::build(raw).unwrap().cubes.len(), 20_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
