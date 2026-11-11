// What the local player wears, for the model's armour and elytra, from the game's own copy of it
// (`Game::armor`, head to feet) and its atlas. Added to the game as `game::cpm_equipment_probe`;
// player_submit.rs uses it.
//
// Each armour piece gets the textures Horizon's `HumanoidArmorLayer` draws for the player
// (game/armor_stands.rs `armor_layers`), as far as a vanilla piece goes: its layers by
// `cpm_mod::armor_layers` (the names Horizon's mob path gives them), an OptiFine CIT armour rule's
// texture in place of a layer, then its trim. A foil stack (`ItemStack::has_foil` with the item's
// default glint) glints on its first layer and trim, and a layer whose texture has an OptiFine
// emissive overlay (`mob_data().emissive`) is drawn again with it (`cpm_mod::piece_layers` puts
// them in order). The elytra's texture is `WingsLayer`'s (game/perspective.rs `wings_geometry`): a
// CIT elytra rule's, else the player's cape, else the elytra's own; it glints when enchanted
// (`wings_overlay`) and has the elytra texture's emissive overlay unless it is the cape. Only
// textures the atlas has are kept, so a slot with none is not worn. The cape's texture and the
// chest item's wings / humanoid layers come from cape_probe.rs.

/// The local player's worn armour and elytra; nothing outside a world.
pub(super) fn worn(game: &crate::game::Game<'_>) -> crate::game::cpm_mod::Worn {
    if game.player.is_none() {
        return crate::game::cpm_mod::Worn::default();
    }
    let cape = game.player_model.as_ref().and_then(|m| m.cape.as_deref());
    let mut worn = worn_of(game, game.armor.each_ref().map(Option::as_ref), cape);
    // The cape layer's texture and the chest item's layers (cape_probe.rs).
    worn.cape_sprite = crate::game::cpm_cape_probe::sprite(game);
    (worn.chest_has_wings, worn.chest_humanoid) = crate::game::cpm_cape_probe::chest_layers(game);
    worn
}

/// The armour and elytra of the worn stacks `armor` (head to feet) on a player whose cape is
/// `cape` (its texture, which an elytra takes): the local player's ([`worn`]) or another
/// player's (others_probe.rs). The cape layer's fields are left for the caller.
pub(super) fn worn_of(
    game: &crate::game::Game<'_>,
    armor: [Option<&horizon_client::items::ItemStack>; 4],
    cape: Option<&str>,
) -> crate::game::cpm_mod::Worn {
    use crate::game::cpm_mod as cpm;
    use horizon_client::cit::Kind;
    let mut worn = cpm::Worn::default();
    let sprites = &game.gui.atlas.sprites;
    let assets = &game.armor_stand_assets;
    // `ItemStack.hasFoil` with the item's default glint (game/item_draw.rs `has_foil`).
    let foil = |stack: &horizon_client::items::ItemStack| {
        let glint = game.res.items().enchantments.props.get(&*stack.item).is_some_and(|p| p.glint);
        stack.has_foil(glint)
    };
    let emissive = game.res.mob_data().emissive.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    // A texture's emissive overlay, when the atlas has it.
    let glow = |texture: &str| emissive.overlay(texture).filter(|e| sprites.contains_key(*e)).map(str::to_owned);
    for (i, slot) in cpm::ARMOR_SLOTS.into_iter().enumerate() {
        let Some(stack) = armor[i] else {
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
            // The overlay of the layer's own texture, even under a CIT rule, as Horizon looks it up.
            let overlay = glow(&layer.sprite);
            // cit_armor::worn_layer: the rule's texture when the atlas has it, else the layer's own.
            if let Some(cit) = horizon_client::cit_armor::worn_layer_sprite(rule, asset, kind, &layer.sprite)
                .filter(|s| sprites.contains_key(s))
            {
                layer.sprite = cit;
            }
            if sprites.contains_key(&layer.sprite) {
                layers.push((layer, overlay));
            }
        }
        // The trim over the armour (prepared into the atlas each frame by `prepare_trims`).
        let trim = stack.extra.as_ref().and_then(|e| e.trim.as_ref()).and_then(|(material, pattern)| {
            let id = format!("minecraft:{asset}");
            let overrides = assets.trim_overrides.get(&id).map_or(&[][..], Vec::as_slice);
            horizon_client::trims::TrimTexture::resolve(&assets.trims, overrides, material, pattern, kind)
        });
        let trim = trim.map(|t| t.sprite()).filter(|s| sprites.contains_key(s));
        worn.armor[i] = cpm::piece_layers(layers, trim, foil(stack));
    }
    if let Some(stack) = armor[1].filter(|s| cpm::is_elytra(&s.item)) {
        worn.elytra = true;
        let rule = game.cit_worn_rule(Kind::Elytra, stack);
        let known = |s: &str| sprites.contains_key(s).then(|| s.to_owned());
        worn.elytra_sprite =
            horizon_client::cit_armor::wings(rule, cape, &known).or_else(|| known(cpm::ELYTRA_SPRITE));
        worn.elytra_glint = foil(stack);
        // The elytra texture's overlay, not a cape's (`wings_geometry`).
        if cape.is_none_or(|c| !sprites.contains_key(c)) {
            worn.elytra_emissive = glow(cpm::ELYTRA_SPRITE);
        }
    }
    worn
}
