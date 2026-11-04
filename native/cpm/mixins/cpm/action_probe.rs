// The local player's action, hurt and dying state the CPM poses read, from the game's own copy
// of it: the swing (`Game::swing`), the item in use (`Game::using`), the hands, the player's
// motion, health and food, and the light at its block. Added to the game as
// `game::cpm_action_probe`; player_submit.rs uses it, `cpm_mod::action_input` maps it.
//
// The use animation follows Horizon's own arm poses and first-person item poses (mobs/anim.rs,
// game.rs `hand_geometry`): a blocking item, a spear, the bow / crossbow / trident / spyglass /
// goat horn / brush by id, else the `consumable` component's animation, then any mod's
// `native_use_animation`. Reaches into the pinned Horizon's game module, as the other probes do.

use horizon_client::items::ItemStack;
use horizon_world::consumable::UseAnimation;

/// The local player's action state at `partial`; all off outside a world.
pub(super) fn state(
    game: &crate::game::Game<'_>,
    held: &crate::game::cpm_item_probe::Held<'_>,
    partial: f32,
) -> crate::game::cpm_mod::ActionState {
    use crate::game::cpm_mod::{ActionState, Pull, Use};
    use cpm_core::Hand;
    use horizon_client::pose::Pose;
    let Some(p) = game.player.as_ref() else {
        return ActionState::default();
    };
    let hand = |h: horizon_client::first_person::Hand| match h {
        horizon_client::first_person::Hand::Main => Hand::Main,
        horizon_client::first_person::Hand::Off => Hand::Off,
    };
    let charged =
        |s: Option<&ItemStack>| s.is_some_and(|s| &*s.item == "minecraft:crossbow" && s.data().is_some_and(|d| !d.charged.is_empty()));
    let using = game.using.and_then(|(slot, ticks)| {
        let stack = game.stack_in(slot)?;
        let pull = match &*stack.item {
            "minecraft:bow" => Pull::Bow,
            "minecraft:crossbow" => Pull::Crossbow(crate::game::Game::charge_ticks(stack)),
            _ => Pull::None,
        };
        Some(Use {
            hand: if slot == crate::game::shield::OFFHAND_SLOT { Hand::Off } else { Hand::Main },
            kind: use_kind(game, stack),
            ticks,
            pull,
        })
    });
    let max_health = p
        .attributes
        .lookup(p.attributes.key("minecraft:max_health").as_ref())
        .and_then(|h| p.attributes.instance(&h).ok())
        .map_or(20.0, |i| i.cached_value() as f32);
    let block = [p.position.x.floor() as i32, p.position.y.floor() as i32, p.position.z.floor() as i32];
    let (block_light, sky_light) = game.native_light(block);
    ActionState {
        swing: game.swing.current.map(|c| (hand(c.hand), game.swing.animation(partial))),
        swinging: game.swing.swinging(),
        using,
        holding: [held.main.is_some(), held.off.is_some()],
        charged: [charged(held.main), charged(held.off)],
        on_climbable: on_climbable(game, block),
        dy: p.position.y - p.old_position.y,
        visually_swimming: p.pose == Pose::Swimming || (!p.fall_flying && p.pose == Pose::FallFlying),
        in_water: p.fluids.was_touching_water,
        spin_attack: p.pose == Pose::SpinAttack,
        ticks_frozen: p.ticks_frozen,
        health: p.health.health(),
        max_health,
        food: p.health.food.food,
        light: [block_light, sky_light],
        // The live player's, not the render state's copy (its death time is never set for this
        // window's player).
        hurt_time: p.health.hurt_animation_time,
        dead: p.health.dead_or_dying(),
    }
}

/// `ItemStack.getUseAnimation` as Horizon draws it, for CPM (`HandAnimation`): `None` for an
/// item CPM has no pose for. A blocking item is checked before the `consumable` component, as
/// Horizon's `tick_used_item` treats it (it blocks); vanilla checks `consumable` first, so a
/// data-driven stack with both reads as BLOCK here and EAT in Java.
fn use_kind(game: &crate::game::Game<'_>, stack: &ItemStack) -> Option<cpm_core::UseKind> {
    use cpm_core::UseKind;
    let mut animation = if game.blocks_attacks(stack).is_some() {
        UseAnimation::Block
    } else if horizon_client::spear::Kinetic::of(&stack.item).is_some() {
        UseAnimation::Spear
    } else {
        match &*stack.item {
            "minecraft:bow" => UseAnimation::Bow,
            "minecraft:crossbow" => UseAnimation::Crossbow,
            "minecraft:trident" => UseAnimation::Trident,
            "minecraft:spyglass" => UseAnimation::Spyglass,
            "minecraft:goat_horn" => UseAnimation::TootHorn,
            "minecraft:brush" => UseAnimation::Brush,
            _ => game
                .pack
                .item_renderer
                .models
                .components(&horizon_client::item_render::stack_view(stack))
                .get("minecraft:consumable")
                .map_or(UseAnimation::None, horizon_client::first_person::consumable_animation),
        }
    };
    game.native_use_animation(stack, &mut animation);
    Some(match animation {
        UseAnimation::Eat | UseAnimation::Drink => UseKind::Eat,
        UseAnimation::Block => UseKind::Block,
        UseAnimation::Bow => UseKind::Bow,
        UseAnimation::Trident => UseKind::Trident,
        UseAnimation::Crossbow => UseKind::Crossbow,
        UseAnimation::Spyglass => UseKind::Spyglass,
        UseAnimation::TootHorn => UseKind::Horn,
        UseAnimation::Brush => UseKind::Brush,
        UseAnimation::Spear => UseKind::Spear,
        UseAnimation::None | UseAnimation::Bundle => return None,
    })
}

/// `Player.onClimbable` (`LivingEntity.onClimbable`): not for a spectator or a creative flyer;
/// the block at the feet is `#climbable` (not one an elytra glides through while gliding), or an
/// open trapdoor over a ladder facing the same way.
fn on_climbable(game: &crate::game::Game<'_>, at: [i32; 3]) -> bool {
    let Some(p) = game.player.as_ref() else {
        return false;
    };
    if p.spectator || p.flying {
        return false;
    }
    let state = game.world.block(at[0], at[1], at[2]);
    let name = &game.res.facts().get(state).name;
    if game.block_in_tag("minecraft:climbable", name) {
        return !(p.fall_flying && game.block_in_tag("minecraft:can_glide_through", name));
    }
    let property = |s, key: &str| {
        game.pack.assets.registry.state(s).and_then(|s| s.properties.get(key)).map(|v| v.to_string())
    };
    if !name.ends_with("_trapdoor") || property(state, "open").as_deref() != Some("true") {
        return false;
    }
    let below = game.world.block(at[0], at[1] - 1, at[2]);
    &*game.res.facts().get(below).name == "minecraft:ladder" && property(below, "facing") == property(state, "facing")
}
