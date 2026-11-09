// A player head (`PlayerHeadSpecialRenderer`, which every held, GUI, dropped and worn player head
// draws through: `submit_special` for the heads mobs, armour stands and the local player wear)
// drawn as the model's head when it is the local player's (`PlayerHeadSpecialRendererMixin`,
// `CustomHeadLayerMixin`). Not a named hook point but the game's own function
// (item_render/special.rs, `submit`), so it holds for the pinned commit only.
//
// The skull model's `head` part is placed as the vanilla draw places it (`root`, then `head`, at
// `pose`); the game crate's drawing (skull_hook.rs) decides whose head it is and draws it there.
// Cancelled (the vanilla skull left out) only when it drew.
fn cpm_player_head(
    renderer: &ItemRenderer,
    model: &SpecialModel,
    argument: &Value,
    pose: Mat4,
    sprites: &HashMap<String, Rect>,
    sink: &mut dyn ItemSink,
) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow;
    // A head with no profile is steve's: never the player's.
    if !matches!(model.kind, SpecialKind::PlayerHead) || argument.is_null() {
        return ControlFlow::Continue(());
    }
    let Some(draw) = super::cpm_skull_hook::get() else {
        return ControlFlow::Continue(());
    };
    let Some(root) = renderer.part("player") else {
        return ControlFlow::Continue(());
    };
    let Some((_, head)) = root.children.iter().find(|(name, _)| name == "head") else {
        return ControlFlow::Continue(());
    };
    let m = part_matrix(part_matrix(pose, &root.pose), &head.pose);
    if draw(argument, m, sprites, sink) { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
}
