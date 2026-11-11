// The local player's cape, as the game's `CapeLayer` sees it (game/perspective.rs
// `player_cape_geometry`, mobs/render.rs `cape`): the cape texture, its swing, the chest item's
// equipment layers and invisibility. Added to the game as `game::cpm_cape_probe`; player_submit.rs
// and equipment_probe.rs use it.

/// The cape's flap and leans (`AvatarRenderer.extractCapeState`, the game's `AvatarBob::cape`) at
/// `partial`; `None` when no cape layer is drawn: outside a world, or a spectator (who draws no
/// layers).
pub(super) fn motion(game: &crate::game::Game<'_>, partial: f32) -> Option<cpm_core::CapeState> {
    motion_of(game.player_model.as_ref()?, partial)
}

/// [`motion`] for the player `m` (the local player's model, or another player's mob).
pub(super) fn motion_of(m: &horizon_client::mobs::Mob, partial: f32) -> Option<cpm_core::CapeState> {
    if !horizon_client::mobs::render::layers_shown(m) {
        return None;
    }
    let a = f64::from(partial);
    let at = [
        m.previous.x + (m.position.x - m.previous.x) * a,
        m.previous.y + (m.position.y - m.previous.y) * a,
        m.previous.z + (m.position.z - m.previous.z) * a,
    ];
    let body = m.previous_body_yaw + horizon_client::mobs::living::wrap(m.body_yaw - m.previous_body_yaw) * partial;
    // LivingEntityRenderState.fallFlyingScale.
    let ticks = m.extra.fall_fly_ticks as f32 + partial;
    let gliding = (ticks * ticks / 100.0).clamp(0.0, 1.0);
    let [flap, lean, lean2] = m.extra.avatar.cape(partial, at, body, gliding);
    Some(cpm_core::CapeState { flap, lean, lean2 })
}

/// `isInvisible` for the player itself: `CapeLayer` draws no cape (the model can still be drawn,
/// see-through, for a team that sees its invisible friends).
pub(super) fn invisible(game: &crate::game::Game<'_>) -> bool {
    game.own_invisible()
}

/// The player's cape texture, when the atlas has it.
pub(super) fn sprite(game: &crate::game::Game<'_>) -> Option<String> {
    sprite_of(game, game.player_model.as_ref()?)
}

/// [`sprite`] for the player `m`.
pub(super) fn sprite_of(game: &crate::game::Game<'_>, m: &horizon_client::mobs::Mob) -> Option<String> {
    let cape = m.cape.as_deref()?;
    game.gui.atlas.sprites.contains_key(cape).then(|| cape.to_owned())
}

/// Whether the chest item's equipment asset has a `wings` layer (no cape) and a `humanoid` one
/// (the cape moved out over it), as `player_cape_geometry` reads them.
pub(super) fn chest_layers(game: &crate::game::Game<'_>) -> (bool, bool) {
    chest_layers_of(game, game.armor[1].as_ref().map(|s| &*s.item))
}

/// [`chest_layers`] for the chest item `chest` (another player's: others_probe.rs).
pub(super) fn chest_layers_of(game: &crate::game::Game<'_>, chest: Option<&str>) -> (bool, bool) {
    let layers = chest
        .and_then(|item| game.res.items().get(item))
        .and_then(|i| i.equip_asset.as_ref())
        .and_then(|asset| game.armor_stand_assets.equipment.get(asset));
    (
        layers.is_some_and(|k| k.contains_key("wings")),
        layers.is_some_and(|k| k.contains_key("humanoid")),
    )
}
