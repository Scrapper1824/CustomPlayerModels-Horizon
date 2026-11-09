// The local player's shoulder parrots, from the game's own copy of them (the player model's
// `shoulder_parrots`, which `tick_player_model` keeps from `ShoulderEntityLeft` / `Right`), and
// their drawing at the model's shoulder anchors. Added to the game as `game::cpm_parrot_probe`;
// player_submit.rs uses it.
//
// The game leaves out its shoulder-parrot layer (`shoulder_parrots_geometry`) for a player a mod
// drew; this draws the same parrots (Horizon's parrot model, variant texture and on-shoulder
// animation: `mobs::render::shoulder_parrot`), placed as CPM's `ParrotOnShoulderLayerMixin` does.

/// Which shoulders hold a parrot: left, right (PARROT_LEFT / PARROT_RIGHT); none outside a world.
pub(super) fn on_shoulders(game: &crate::game::Game<'_>) -> [bool; 2] {
    match (game.player.as_ref(), game.player_model.as_ref()) {
        (Some(_), Some(m)) => m.extra.shoulder_parrots.map(|v| v.is_some()),
        _ => [false; 2],
    }
}

/// Draws the local player's shoulder parrots at `shoulders` (each the parrot model's space after
/// the layer's translate to the player's draw frame), beside `draw` (item_probe.rs
/// `draw_beside`). Nothing is drawn without a parrot type or its variant texture, or for a player
/// whose layers are not drawn (a spectator).
pub(super) fn draw_parrots(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    shoulders: &[cpm_core::ShoulderAnchor],
    partial: f32,
) {
    use glam::{Mat4, Vec3};
    let Some(m) = game.player_model.as_ref() else {
        return;
    };
    if !horizon_client::mobs::render::layers_shown(m) {
        return;
    }
    let parrots: Vec<_> = shoulders
        .iter()
        .filter_map(|a| m.extra.shoulder_parrots[usize::from(!a.left)].map(|variant| (a, variant)))
        .collect();
    if parrots.is_empty() {
        return;
    }
    let Some(parrot) = game.res.mob_types().get("minecraft:parrot") else {
        return;
    };
    let light = draw.light;
    let atlas = &game.gui.atlas;
    let lookup = |name: &str| atlas.sprites.get(name).map(|r| [r.u0, r.v0, r.u1, r.v1]);
    crate::game::cpm_item_probe::draw_beside(game, draw, |pose, verts| {
        for (anchor, variant) in parrots {
            // `shoulder_parrot` applies the layer's translate itself (its own crouch test): taken
            // back out here, so the parrot lands at the anchor, which already has it.
            let x = if anchor.left { 0.4 } else { -0.4 };
            let y = if m.extra.crouching { -1.3 } else { -1.5 };
            let at = pose
                * Mat4::from_cols_array(&anchor.transform.to_cols_array())
                * Mat4::from_translation(Vec3::new(-x, -y, 0.0));
            horizon_client::mobs::render::shoulder_parrot(m, parrot, variant, anchor.left, partial, at, &lookup, light, verts);
        }
    });
}
