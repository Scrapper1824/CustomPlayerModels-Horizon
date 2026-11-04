// What the local player wears, for the model's armour and elytra, from the game's own copy of it
// (`Game::armor`, head to feet) and its atlas. Added to the game as `game::cpm_equipment_probe`;
// player_submit.rs uses it.
//
// Each armour piece gets the textures Horizon's `HumanoidArmorLayer` draws for the player
// (game/armor_stands.rs `armor_layers`), as far as a vanilla piece goes: its layers by
// `cpm_mod::armor_layers` (the names Horizon's mob path gives them), an OptiFine CIT armour rule's
// texture in place of a layer, then its trim. The elytra's texture is `WingsLayer`'s
// (game/perspective.rs `wings_geometry`): a CIT elytra rule's, else the player's cape, else the
// elytra's own. Only textures the atlas has are kept, so a slot with none is not worn. The cape's
// texture and the chest item's wings / humanoid layers come from cape_probe.rs.

/// The local player's worn armour and elytra; nothing outside a world.
pub(super) fn worn(game: &crate::game::Game<'_>) -> crate::game::cpm_mod::Worn {
    use crate::game::cpm_mod as cpm;
    use horizon_client::cit::Kind;
    let mut worn = cpm::Worn::default();
    if game.player.is_none() {
        return worn;
    }
    let sprites = &game.gui.atlas.sprites;
    let assets = &game.armor_stand_assets;
    for (i, slot) in cpm::ARMOR_SLOTS.into_iter().enumerate() {
        let Some(stack) = game.armor[i].as_ref() else {
            continue;
        };
        let Some(asset) = cpm::armor_asset(&stack.item, slot) else {
            continue;
        };
        let kind = cpm::armor_kind(slot);
        let dyed = stack.extra.as_ref().and_then(|e| e.dyed_color).map(|c| c as u32 & 0xFF_FFFF);
        let rule = game.cit_worn_rule(Kind::Armor, stack);
        let mut layers = Vec::new();
        for mut layer in cpm::armor_layers(&stack.item, slot, dyed) {
            // cit_armor::worn_layer: the rule's texture when the atlas has it, else the layer's own.
            if let Some(cit) = horizon_client::cit_armor::worn_layer_sprite(rule, asset, kind, &layer.sprite)
                .filter(|s| sprites.contains_key(s))
            {
                layer.sprite = cit;
            }
            if sprites.contains_key(&layer.sprite) {
                layers.push(layer);
            }
        }
        // The trim over the armour (prepared into the atlas each frame by `prepare_trims`).
        let trim = stack.extra.as_ref().and_then(|e| e.trim.as_ref()).and_then(|(material, pattern)| {
            let id = format!("minecraft:{asset}");
            let overrides = assets.trim_overrides.get(&id).map_or(&[][..], Vec::as_slice);
            horizon_client::trims::TrimTexture::resolve(&assets.trims, overrides, material, pattern, kind)
        });
        if !layers.is_empty()
            && let Some(t) = trim
        {
            let sprite = t.sprite();
            if sprites.contains_key(&sprite) {
                layers.push(cpm::WornLayer { sprite, tint: 0xFF_FFFF });
            }
        }
        worn.armor[i] = layers;
    }
    if let Some(stack) = game.armor[1].as_ref().filter(|s| cpm::is_elytra(&s.item)) {
        worn.elytra = true;
        let rule = game.cit_worn_rule(Kind::Elytra, stack);
        let cape = game.player_model.as_ref().and_then(|m| m.cape.as_deref());
        let known = |s: &str| sprites.contains_key(s).then(|| s.to_owned());
        worn.elytra_sprite =
            horizon_client::cit_armor::wings(rule, cape, &known).or_else(|| known(cpm::ELYTRA_SPRITE));
    }
    // The cape layer's texture and the chest item's layers (cape_probe.rs).
    worn.cape_sprite = crate::game::cpm_cape_probe::sprite(game);
    (worn.chest_has_wings, worn.chest_humanoid) = crate::game::cpm_cape_probe::chest_layers(game);
    worn
}
