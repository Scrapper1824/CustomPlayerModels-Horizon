//! Gesture buttons and parameters: a port of CPM's `parts/anim/menu/*` (button data and their
//! `*ParameterValueAction`s), the parameter bytes of `ParameterDetails` / `AnimationEngine`
//! (`gestureData`), and the button list `LegacyAnimationParser.register` builds for legacy models.
//!
//! The parameter layout is Java's: byte 0 is the custom pose id, byte 1 the gesture id, then the
//! model's own parameters. The engine keeps the selected custom pose and the playing gesture
//! separately (they are time-dependent and not persisted), so bytes 0 and 1 always hold their
//! defaults here.

use super::{AnimSet, GestureKind, Pose};

/// What a button does in the gesture menu (`GestureButtonType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonKind {
    /// `CustomPoseGestureButtonData(false)`: plays a gesture.
    Gesture,
    /// `CustomPoseGestureButtonData(true)`: selects a custom pose.
    CustomPose,
    /// `BoolParameterToggleButtonData`: flips a parameter bit.
    Toggle,
    /// `ValueParameterButtonData`: a slider from 0 to `max`.
    Slider { max: u8 },
    /// `DropdownButtonData` / `LegacyDropdownButtonData`: one of `options`.
    Dropdown { options: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub name: String,
    pub kind: ButtonKind,
    /// `GestureGui` leaves it out: hidden, command-controlled or a model property
    /// (`!isProperty() && canShow()` fails).
    pub hidden: bool,
    /// The model's order value (new format: the button's position in the file, which the
    /// exporter sorted by order).
    pub order: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// Gesture playing, custom pose selected, or toggle on.
    Active(bool),
    /// Slider position, 0..=max.
    Value(u8),
    /// Dropdown option index.
    Selected(usize),
}

/// Legacy (`ANIMATION_DATA`) button metadata of a gesture or custom pose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ButtonMeta {
    /// `ANIMATION_DATA_ORDER`.
    pub order: i32,
    /// `ANIMATION_DATA_GROUP`: layers sharing a group become one dropdown.
    pub group: Option<String>,
    /// `ANIMATION_BUTTON_HIDDEN`.
    pub hidden: bool,
    /// `ANIMATION_DATA_IS_PROPERTY`.
    pub property: bool,
    /// `ANIMATION_DATA_COMMAND`.
    pub command: bool,
    /// `ANIMATION_SLIDER_OPT` max value; 0 means 255.
    pub max_value: u8,
}

/// When a gesture / layer entry plays (`AnimationTrigger` subclasses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    /// `GestureTrigger`: gesture id (parameter 1). Played through the engine's active gesture.
    Gesture(u8),
    /// `LayerTrigger`: `bitmask` ? `(p & value) == value` : `p == value`.
    Layer { param: usize, value: u8, bitmask: bool },
    /// `ValueTrigger`: progress = parameter / 256.
    Value { param: usize },
}

/// What pressing / setting a button changes (the button's `CommandAction`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Index into `AnimSet::gestures` (`None`: the model has no clip for it).
    Gesture(Option<usize>),
    /// Custom pose name.
    CustomPose(String),
    /// `BitmaskParameterValueAction`.
    Bool { param: usize, mask: u8 },
    /// `ValueParameterValueAction`.
    Value { param: usize, max: u8 },
    /// `DropdownButtonData`: the parameter holds the option index.
    Dropdown { param: usize },
    /// `LegacyDropdownButtonData`: option `i` (1..) sets `params[i - 1]` to 1, the others to 0;
    /// option 0 is "none".
    LegacyDropdown { params: Vec<usize> },
}

/// A model's buttons, their actions and the parameter defaults (`ParameterDetails.syncDefault`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Controls {
    pub buttons: Vec<Button>,
    /// Same length as `buttons`.
    pub actions: Vec<Action>,
    pub defaults: Vec<u8>,
}

/// `ValueParameterButtonData` max: 0 (unset) means 255.
pub(crate) fn slider_max(raw: u8) -> u8 {
    if raw == 0 { 255 } else { raw }
}

/// Java `Math.round` of a non-negative float.
fn round(v: f32) -> u32 {
    (v + 0.5).floor() as u32
}

/// `ValueParameterValueAction.getValue`: parameter byte -> slider value.
pub(crate) fn byte_to_slider(b: u8, max: u8) -> u8 {
    round(b as f32 * max as f32 / 255.0).min(max as u32) as u8
}

/// `ValueParameterValueAction.setValue`: slider value -> parameter byte.
pub(crate) fn slider_to_byte(v: u8, max: u8) -> u8 {
    round(v.min(max) as f32 * 255.0 / max.max(1) as f32).min(255) as u8
}

enum Item {
    Gesture(usize),
    Pose(String, ButtonMeta),
    Group(String, Vec<usize>),
}

/// `LegacyAnimationParser.addGesture` / `register`: assigns layer and value-layer parameters
/// (2 + index, in decode order), sets each entry's [`Cond`], and builds the buttons. Custom
/// poses and gestures are sorted by order (ties by name: Java's order is `HashMap` order);
/// layers sharing a group become one dropdown placed at its first member, options `""` (none)
/// then the members in that order.
pub(crate) fn legacy(set: &mut AnimSet) -> Controls {
    let mut defaults = vec![0u8, 0];
    let mut params = vec![None; set.gestures.len()];
    for (gi, g) in set.gestures.iter_mut().enumerate() {
        match g.kind {
            GestureKind::Layer | GestureKind::Value => {
                let param = defaults.len();
                defaults.push(g.default_value);
                params[gi] = Some(param);
                g.cond = Some(if g.kind == GestureKind::Layer {
                    Cond::Layer { param, value: 1, bitmask: true }
                } else {
                    Cond::Value { param }
                });
            }
            _ => {}
        }
    }

    let mut items: Vec<(i32, String, Item)> = Vec::new();
    for (gi, g) in set.gestures.iter().enumerate() {
        if matches!(g.kind, GestureKind::Gesture | GestureKind::Layer | GestureKind::Value) {
            items.push((g.meta.order, g.name.clone(), Item::Gesture(gi)));
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for (p, _) in &set.poses {
        if let Pose::Custom(n) = p {
            if !seen.contains(&n.as_str()) {
                seen.push(n);
                let meta = set.pose_meta.iter().find(|(m, _)| m == n).map(|(_, m)| m.clone()).unwrap_or_default();
                items.push((meta.order, n.clone(), Item::Pose(n.clone(), meta)));
            }
        }
    }
    items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

    // Group layers (the first member's slot holds the group).
    let mut grouped: Vec<Item> = Vec::new();
    for (_, _, it) in items {
        if let Item::Gesture(gi) = it {
            let g = &set.gestures[gi];
            if let (GestureKind::Layer, Some(group)) = (g.kind, &g.meta.group) {
                let existing = grouped.iter_mut().find_map(|x| match x {
                    Item::Group(n, m) if n == group => Some(m),
                    _ => None,
                });
                match existing {
                    Some(m) => m.push(gi),
                    None => grouped.push(Item::Group(group.clone(), vec![gi])),
                }
                continue;
            }
        }
        grouped.push(it);
    }

    let mut c = Controls { defaults, ..Default::default() };
    for it in grouped {
        let (button, action) = match it {
            Item::Gesture(gi) => {
                let g = &set.gestures[gi];
                let m = &g.meta;
                let hidden = m.hidden || m.command || m.property;
                let param = params[gi].unwrap_or(0);
                let (kind, action) = match g.kind {
                    GestureKind::Gesture => (ButtonKind::Gesture, Action::Gesture(Some(gi))),
                    GestureKind::Layer => (ButtonKind::Toggle, Action::Bool { param, mask: 1 }),
                    _ => {
                        let max = slider_max(m.max_value);
                        (ButtonKind::Slider { max }, Action::Value { param, max })
                    }
                };
                (Button { name: g.name.clone(), kind, hidden, order: m.order }, action)
            }
            Item::Pose(name, m) => (
                Button { name: name.clone(), kind: ButtonKind::CustomPose, hidden: m.hidden || m.command, order: m.order },
                Action::CustomPose(name),
            ),
            Item::Group(name, members) => {
                let gs = members.iter().map(|&gi| &set.gestures[gi]);
                let hidden = gs.clone().any(|g| g.meta.command || g.meta.property);
                let order = gs.clone().map(|g| g.meta.order).max().unwrap_or(0);
                let options = std::iter::once(String::new()).chain(gs.map(|g| g.name.clone())).collect();
                let params = members.iter().map(|&gi| params[gi].unwrap_or(0)).collect();
                (
                    Button { name, kind: ButtonKind::Dropdown { options }, hidden, order },
                    Action::LegacyDropdown { params },
                )
            }
        };
        c.buttons.push(button);
        c.actions.push(action);
    }
    c
}
