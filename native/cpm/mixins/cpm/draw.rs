// The model's quads drawn with the game's `native_draw_quad`: what player_submit.rs and
// first_person_arm.rs share. Added to the game as `game::cpm_draw`.

/// Draws `quads` into `draw`, each with its sprite: a glowing one at full brightness, the others
/// at `draw.light` (left as it was). False, with nothing drawn, when the atlas lacks the model's
/// sprite ([`cpm_mod::SPRITE`](crate::game::cpm_mod::SPRITE)); a quad of a worn texture the atlas
/// lacks is left out (the rest of the model still draws).
pub(super) fn quads(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    quads: &crate::game::cpm_mod::Quads,
) -> bool {
    use crate::game::cpm_mod as cpm;
    if !game.gui.atlas.sprites.contains_key(cpm::SPRITE) {
        return false;
    }
    let light = draw.light;
    for q in &quads.quads {
        draw.light = if q.glow { cpm::FULL_BRIGHT } else { light };
        let sprite = quads.sprites.get(q.sprite).map_or(cpm::SPRITE, String::as_str);
        // False only for a sprite the atlas lacks: that quad is skipped.
        let _ = game.native_draw_quad(draw, sprite, q.corners, q.uv, q.tint);
    }
    draw.light = light;
    true
}
