// The first-person arm (the bare main hand, and the arms under a held map), drawn with the
// loaded model's arm instead of the vanilla one. The game's `first_person_arm` gets the arm's
// pose `m`, swing and equip motion included, which maps the arm's model space (blocks, Y down,
// before its pivot: `mobs::render_model`'s `base`) to the hand pass's camera-relative space,
// the space of `out`; `cpm::arm_quads` is in that model space (the model's first-person hand
// transform for that arm included), so the quads are drawn under `m`.
// No model, its texture not uploaded or the sprite refused: the vanilla arm (not cancelled).
fn cpm_first_person_arm(
    game: &crate::game::Game<'_>,
    left: bool,
    m: glam::Mat4,
    light: u32,
    out: &mut Vec<horizon_client::mesher::Vertex>,
) -> std::ops::ControlFlow<()> {
    use crate::game::{cpm_draw, cpm_mod as cpm, native_block_entity_draw::BlockEntityDraw};
    use std::ops::ControlFlow;
    if !game.native_on(cpm::ID) {
        return ControlFlow::Continue(());
    }
    let Some(quads) = cpm::arm_quads(left) else {
        return ControlFlow::Continue(());
    };
    // Shaded as the third-person model is (mod_entity_draw): the level's two lights.
    let lights = horizon_client::block_entity_render::diffuse_lights(game.dim == crate::game::Dim::Nether);
    let start = out.len();
    let mut draw = BlockEntityDraw::posed(0.0, game.world.game_time, light, [0; 3], m, lights, std::mem::take(out));
    let drawn = cpm_draw::quads(game, &mut draw, &quads);
    *out = draw.into_vertices();
    if !drawn {
        out.truncate(start);
        return ControlFlow::Continue(());
    }
    ControlFlow::Break(())
}
