// Which player heads are the local player's (`cpm::profile_matches`): only those show the model's
// head until multiplayer.
mod common;

use std::time::Instant;

use horizon_mod_cpm as cpm;
use cpm::{HeadProfile, Identity};

const ME: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;
const OTHER: u128 = 0xfedc_ba98_7654_3210_fedc_ba98_7654_3210;

fn me() -> Vec<Identity> {
    vec![Identity { id: Some(ME), name: "Steve_42".into() }, Identity { id: None, name: "Player".into() }]
}

fn head(id: Option<u128>, name: Option<&str>) -> HeadProfile {
    HeadProfile { id, name: name.map(str::to_owned) }
}

#[test]
fn only_own_profile_matches() {
    let own = me();
    // By UUID, whatever the name says.
    assert!(cpm::profile_matches(&head(Some(ME), None), &own));
    assert!(cpm::profile_matches(&head(Some(ME), Some("Someone")), &own));
    // Another UUID is not ours, even with our name.
    assert!(!cpm::profile_matches(&head(Some(OTHER), Some("Steve_42")), &own));
    // No UUID: by name, without case.
    assert!(cpm::profile_matches(&head(None, Some("Steve_42")), &own));
    assert!(cpm::profile_matches(&head(None, Some("steve_42")), &own));
    assert!(cpm::profile_matches(&head(None, Some("player")), &own));
    assert!(!cpm::profile_matches(&head(None, Some("Alex")), &own));
    // A head naming nothing, or an empty name, is nobody's.
    assert!(!cpm::profile_matches(&head(None, None), &own));
    assert!(!cpm::profile_matches(&head(None, Some("")), &own));
    // Outside a world there is no one to match.
    assert!(!cpm::profile_matches(&head(Some(ME), Some("Steve_42")), &[]));
    // An identity without a UUID is never matched by a head's UUID.
    assert!(!cpm::profile_matches(&head(Some(OTHER), None), &[Identity { id: None, name: "Player".into() }]));
}

#[test]
fn own_heads_follow_the_identities_set() {
    let mine = head(Some(ME), None);
    assert!(!cpm::is_own_head(&mine));
    cpm::set_own_identities(me());
    assert!(cpm::is_own_head(&mine));
    assert!(!cpm::is_own_head(&head(Some(OTHER), None)));
    cpm::set_own_identities(Vec::new());
    assert!(!cpm::is_own_head(&mine));
    // No model loaded: nothing to draw a head with.
    assert!(cpm::skull_quads(0).is_none());
}

#[test]
fn skull_quads_are_the_posed_head_in_blocks() {
    if common::missing(&["ReggieTheRat.cpmmodel", "SkullDog.cpmmodel", "rainwing-old.cpmproject"]) {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mixins/cpm/cpm-core/tests/samples/SkullDog.cpmmodel");
    cpm::select(Some(path));
    cpm::ensure_loaded();
    // Not before its texture went to the game.
    assert!(cpm::skull_quads(0).is_none());
    cpm::texture_upload(Instant::now()).unwrap();
    let boxes = cpm::with_model(|m| m.pose_skull(0)).unwrap();
    assert!(!boxes.is_empty());
    let quads = cpm::skull_quads(0).expect("skull quads");
    let mut want = None;
    cpm::with_model(|m| want = Some(cpm::box_quads(m, &boxes, &cpm::Worn::default()))).unwrap();
    assert_eq!(Some(&quads), want.as_ref());
    assert_eq!(quads.sprites, vec![cpm::SPRITE.to_owned()]);
    assert!(quads.items.is_empty() && quads.shoulders.is_empty());
    // A head: within a block of the skull part's origin.
    assert!(quads.quads.iter().all(|q| q.corners.iter().flatten().all(|v| v.abs() < 1.5)));
}
