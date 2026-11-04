//! `AnimationsLoaderV1`: `animations/*.json`.

use std::collections::HashSet;

use serde_json::Value;

use super::{boolean, float, hex_color, int, long, string, vec3, JMap};
use crate::anim::Interp;

const LAYER_PREFIX: &str = "$layer$";
const VALUE_LAYER_PREFIX: &str = "$value$";
const SETUP_PREFIX: &str = "$pre$";
const FINISH_PREFIX: &str = "$post$";

/// `VanillaPose` names, by ordinal.
pub(crate) const VANILLA_POSES: [&str; 72] = [
    "custom",
    "standing",
    "walking",
    "running",
    "sneaking",
    "swimming",
    "falling",
    "sleeping",
    "riding",
    "flying",
    "dying",
    "skull_render",
    "global",
    "creative_flying",
    "eating_left",
    "eating_right",
    "retro_swimming",
    "jumping",
    "sneak_walk",
    "punch_left",
    "punch_right",
    "armor_head",
    "armor_body",
    "armor_legs",
    "armor_boots",
    "wearing_elytra",
    "bow_left",
    "bow_right",
    "crossbow_left",
    "crossbow_right",
    "crossbow_ch_left",
    "crossbow_ch_right",
    "trident_left",
    "trident_right",
    "trident_spin",
    "spyglass_left",
    "spyglass_right",
    "holding_left",
    "holding_right",
    "wearing_skull",
    "blocking_left",
    "blocking_right",
    "parrot_left",
    "parrot_right",
    "hurt",
    "on_fire",
    "freezing",
    "on_ladder",
    "climbing_on_ladder",
    "speaking",
    "toot_horn_left",
    "toot_horn_right",
    "in_gui",
    "first_person_mod",
    "voice_muted",
    "vr_first_person",
    "vr_third_person_sitting",
    "vr_third_person_standing",
    "first_person_hand",
    "health",
    "hunger",
    "air",
    "in_menu",
    "invisible",
    "light",
    "head_rotation_yaw",
    "head_rotation_pitch",
    "brush_left",
    "brush_right",
    "crawling",
    "spear_left",
    "spear_right",
];

/// `InterpolatorType` names, by ordinal (matches [`Interp::from_ordinal`]).
const INTERPOLATORS: [&str; 7] = [
    "poly_loop",
    "poly_single",
    "linear_loop",
    "linear_single",
    "no_interpolate",
    "trig_loop",
    "trig_single",
];

/// `AnimationType`, in Java's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimKind {
    Pose,
    CustomPose,
    Gesture,
    Layer,
    ValueLayer,
    Setup,
    Finish,
}

/// An `EditorAnim` as `AnimationsLoaderV1.loadAnimation` builds it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectAnim {
    /// The entry name in `animations/` (`EditorAnim.filename`).
    pub file_name: String,
    /// `displayName`: the `name` key ("Unnamed" by default), with a `g_` type prefix removed.
    /// `EditorAnim.getId()` is the part before the first `#`.
    pub display_name: String,
    pub kind: AnimKind,
    /// `VanillaPose` ordinal for `v_` files (see `anim::Pose::from_ordinal`).
    pub pose: Option<u8>,
    /// `additive` (`add`, default true).
    pub additive: bool,
    pub duration: i32,
    pub priority: i32,
    pub loop_: bool,
    pub interpolator: Interp,
    pub layer_default: f32,
    pub order: i32,
    pub is_property: bool,
    pub group: Option<String>,
    pub command: bool,
    pub layer_controlled: bool,
    pub max_value: i32,
    /// `interpolateVal`.
    pub interpolate_value: bool,
    pub must_finish: bool,
    pub hidden: bool,
    pub frames: Vec<ProjectFrame>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectFrame {
    /// In file order (Java keeps a `HashMap` keyed by element).
    pub components: Vec<FrameComponent>,
}

/// `AnimFrame.FrameData` for the element(s) with `store_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameComponent {
    pub store_id: i64,
    pub pos: [f32; 3],
    /// Degrees.
    pub rotation: [f32; 3],
    /// r, g, b in 0..=255.
    pub color: [f32; 3],
    pub show: bool,
    pub scale: [f32; 3],
}

/// Returns `None` where Java skips the file (`type == null`).
pub(super) fn load_animation(file: &str, data: Option<&JMap>, store_ids: &HashSet<i64>) -> Option<ProjectAnim> {
    let mut display_name = string(data, "name").unwrap_or_else(|| "Unnamed".into());
    let (prefix, rest) = match file.split_once('_') {
        Some((p, r)) => (p, r),
        None => (file, ""),
    };
    let (kind, pose) = match prefix {
        "v" => {
            let pose_name = rest.strip_suffix(".json").unwrap_or(rest);
            let o = pose_for(pose_name)?;
            (AnimKind::Pose, Some(o))
        }
        "c" => (AnimKind::CustomPose, None),
        "g" => {
            let kind = type_of(&display_name);
            display_name = clean_name(&display_name).to_string();
            (kind, None)
        }
        _ => return None,
    };
    let interpolator = string(data, "interpolator")
        .and_then(|n| INTERPOLATORS.iter().position(|i| i.eq_ignore_ascii_case(&n)))
        .and_then(|o| Interp::from_ordinal(o as u8))
        .unwrap_or(Interp::PolyLoop);
    let frames = data
        .and_then(|d| d.get("frames"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|f| load_frame(f, store_ids))
        .collect();
    Some(ProjectAnim {
        file_name: file.to_string(),
        display_name,
        kind,
        pose,
        additive: boolean(data, "additive", true),
        duration: int(data, "duration", 1000),
        priority: int(data, "priority", 0),
        loop_: boolean(data, "loop", false),
        interpolator,
        layer_default: float(data, "layerDefault", 0.0),
        order: int(data, "order", 0),
        is_property: boolean(data, "isProperty", false),
        group: string(data, "group"),
        command: boolean(data, "command", false),
        layer_controlled: boolean(data, "layerControlled", true),
        max_value: int(data, "maxValue", 100),
        interpolate_value: boolean(data, "interpolateVal", true),
        must_finish: boolean(data, "mustFinish", false),
        hidden: boolean(data, "hidden", false),
        frames,
    })
}

/// First `VanillaPose` (in ordinal order) whose lower-case name prefixes `pose_name`.
fn pose_for(pose_name: &str) -> Option<u8> {
    VANILLA_POSES
        .iter()
        .position(|p| pose_name.starts_with(p))
        .map(|o| o as u8)
}

fn type_of(name: &str) -> AnimKind {
    if name.starts_with(LAYER_PREFIX) {
        AnimKind::Layer
    } else if name.starts_with(VALUE_LAYER_PREFIX) {
        AnimKind::ValueLayer
    } else if name.starts_with(SETUP_PREFIX) {
        AnimKind::Setup
    } else if name.starts_with(FINISH_PREFIX) {
        AnimKind::Finish
    } else {
        AnimKind::Gesture
    }
}

fn clean_name(name: &str) -> &str {
    for p in [LAYER_PREFIX, VALUE_LAYER_PREFIX, SETUP_PREFIX, FINISH_PREFIX] {
        if let Some(s) = name.strip_prefix(p) {
            return s;
        }
    }
    name
}

fn load_frame(data: &JMap, store_ids: &HashSet<i64>) -> ProjectFrame {
    let components = data
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|c| {
            let m = Some(c);
            let store_id = long(m, "storeID", 0);
            // findPartRef: components naming no element are dropped.
            if !store_ids.contains(&store_id) {
                return None;
            }
            let rgb = hex_color(m, "color");
            Some(FrameComponent {
                store_id,
                pos: vec3(c.get("pos"), [0.0; 3]),
                rotation: vec3(c.get("rotation"), [0.0; 3]),
                color: [((rgb >> 16) & 0xff) as f32, ((rgb >> 8) & 0xff) as f32, (rgb & 0xff) as f32],
                show: boolean(m, "show", true),
                scale: vec3(c.get("scale"), [1.0; 3]),
            })
        })
        .collect();
    ProjectFrame { components }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pose_prefixes_match_in_ordinal_order() {
        assert_eq!(pose_for("global_Tail_uuid"), Some(12));
        assert_eq!(pose_for("sneaking_x"), Some(4));
        assert_eq!(pose_for("sneak_walk_x"), Some(18));
        assert_eq!(pose_for("crossbow_ch_left_x"), Some(30));
        assert_eq!(pose_for("spear_right_x"), Some(71));
        assert_eq!(pose_for("nonsense"), None);
    }
}
