mod common;

use cpm_core::{BoxSource, PoseInput};

/// A model with its own head yaw / pitch poses (GreenRaptor: a neck of cubes 12, 13 on the head
/// bone, the head 14 on it) turns only what those poses move: the neck's base stays where it meets
/// the body while the player looks down, and the head still turns.
#[test]
fn look_poses_keep_the_neck_on_the_body() {
    if common::missing(&["GreenRaptor.cpmmodel"]) {
        return;
    }
    let b = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/GreenRaptor.cpmmodel")).unwrap();
    let mut m = cpm_core::load(&b).unwrap();
    assert!(m.engine.drives_head_look());
    let pose = |m: &mut cpm_core::LoadedModel, pitch: f32, id: u32| {
        let i = PoseInput { time_ms: 500, on_ground: true, pitch, ..Default::default() };
        let p = m.pose_with_items(&i);
        p.boxes.iter().find(|b| b.source == BoxSource::Cube(id)).unwrap().transform
    };
    let (ahead, down) = (pose(&mut m, 0.0, 12), pose(&mut m, 45.0, 12));
    // The neck's pivot (its base) stays on the body: only the pose's own small shift (2 px), not
    // the head bone's swing (7 px at 45 degrees).
    let base = |t: cpm_core::glam::Mat4| t.transform_point3(cpm_core::glam::Vec3::ZERO);
    assert!(base(ahead).distance(base(down)) < 3.0 / 16.0, "{:?} {:?}", base(ahead), base(down));
    // The head turns down.
    let snout = |t: cpm_core::glam::Mat4| t.transform_vector3(cpm_core::glam::Vec3::Y).normalize();
    let (a, d) = (snout(pose(&mut m, 0.0, 65)), snout(pose(&mut m, 45.0, 65)));
    assert!(d.y < a.y - 0.2, "{a:?} {d:?}");
}
