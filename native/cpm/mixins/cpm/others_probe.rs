// Another player in the world, as the game knows them: their mob (`Game::mobs`, kept from the
// server's packets), its synced flags, pose, swing and item in use, what it holds and wears, its
// cape and shoulder parrots. Added to the game as `game::cpm_others_probe`; player_submit.rs uses
// it (`submit`) for the players other than the local one (whose own probes read the local player).
//
// The stacks are those Horizon's own layers draw for another player (game/mob_blocks.rs
// `held_item_geometry`, `head_item_geometry`, game/armor_stands.rs `worn_armor_layers`,
// game/perspective.rs `player_elytra_geometry`, `player_cape_geometry`): the held stacks the
// equipment packets left, enchanted when the gear is, a charged crossbow loaded; the worn stacks
// (or a bare stack of the worn item when only its id is known). What a client is not told of
// another player (hunger, creative flight, the fall distance, gestures; Horizon does not keep
// another player's air either) reads as off (`cpm_mod::remote_input`).

use horizon_client::items::ItemStack;
use horizon_client::mobs::Mob;

/// The player mob drawn as `id` (`NativeMobRender::id`); none for one the game does not hold.
pub(super) fn mob<'a>(game: &'a crate::game::Game<'_>, id: u32) -> Option<&'a Mob> {
    game.mobs.iter().find(|m| m.id == id && &*m.kind.id == "minecraft:player")
}

/// What the player holds and wears, as owned stacks (some are made here).
pub(super) struct Stacks {
    pub main: Option<ItemStack>,
    pub off: Option<ItemStack>,
    /// Head to feet.
    pub armor: [Option<ItemStack>; 4],
}

/// The player's held and worn stacks, as Horizon's layers draw them for another player.
pub(super) fn stacks(m: &Mob) -> Stacks {
    use horizon_client::mobs::{gear, render};
    let held = |off: bool| {
        let mut stack = render::held_item_stack(&m.extra, off)?;
        // The gear's enchantments are not tracked, only that it has some.
        if gear::enchanted(m, if off { gear::OFF } else { gear::MAIN }) {
            stack.enchant("minecraft:unbreaking", 1);
        }
        if &*stack.item == "minecraft:crossbow" && m.extra.flag2 {
            stack.data_mut().charged = vec![ItemStack::new("minecraft:arrow", 1)];
        }
        Some(stack)
    };
    let e = &m.extra;
    let ids = [e.head.as_deref(), e.chest.as_deref(), e.legs.as_deref(), e.feet.as_deref()];
    let armor = std::array::from_fn(|i| e.armor[i].clone().or_else(|| ids[i].map(|id| ItemStack::new(id, 1))));
    Stacks { main: held(false), off: held(true), armor }
}

/// The player's held and head items (item_probe.rs `Held`), from `stacks`.
pub(super) fn held<'a>(game: &crate::game::Game<'_>, m: &Mob, stacks: &'a Stacks) -> crate::game::cpm_item_probe::Held<'a> {
    let worn = stacks.armor[0].as_ref();
    // Helmets are drawn as armour, not by CustomHeadLayer.
    let head = worn.filter(|s| game.res.items().get(&s.item).and_then(|i| i.equip_asset.as_ref()).is_none());
    crate::game::cpm_item_probe::Held {
        main: stacks.main.as_ref(),
        off: stacks.off.as_ref(),
        head,
        left_handed: m.main_arm_left(),
        skull: worn.is_some_and(|s| crate::game::armor_stands::skull_kind(&s.item).is_some()),
    }
}

/// The player's armour, elytra and cape (equipment_probe.rs `worn_of`, cape_probe.rs).
pub(super) fn worn(game: &crate::game::Game<'_>, m: &Mob, stacks: &Stacks, held: &crate::game::cpm_item_probe::Held<'_>) -> crate::game::cpm_mod::Worn {
    let mut worn = crate::game::cpm_equipment_probe::worn_of(game, stacks.armor.each_ref().map(Option::as_ref), m.cape.as_deref());
    worn.cape_sprite = crate::game::cpm_cape_probe::sprite_of(game, m);
    (worn.chest_has_wings, worn.chest_humanoid) =
        crate::game::cpm_cape_probe::chest_layers_of(game, stacks.armor[1].as_ref().map(|s| &*s.item));
    worn.skull = held.skull;
    worn
}

/// The player's state at the frame of `render` (`cpm_mod::RemoteState`).
pub(super) fn state(
    game: &crate::game::Game<'_>,
    m: &Mob,
    render: &crate::game::native_mob_draw::NativeMobRender<'_>,
    held: &crate::game::cpm_item_probe::Held<'_>,
    time_ms: u64,
) -> crate::game::cpm_mod::RemoteState {
    use crate::game::cpm_mod::{Pull, RemoteState, Use};
    use cpm_core::Hand;
    use horizon_client::pose::Pose;
    let e = &m.extra;
    let left = m.main_arm_left();
    // The arm the swing moves (`swing_right_arm`), as a hand.
    let swing_hand = |right: bool| if right != left { Hand::Main } else { Hand::Off };
    let anim = horizon_client::mobs::anim::state(m, render.partial);
    let using = (e.using_item > 0)
        .then(|| {
            let (hand, stack) = if e.using_off_hand { (Hand::Off, held.off?) } else { (Hand::Main, held.main?) };
            let pull = match &*stack.item {
                "minecraft:bow" => Pull::Bow,
                "minecraft:crossbow" => Pull::Crossbow(crate::game::Game::charge_ticks(stack)),
                _ => Pull::None,
            };
            Some(Use { hand, kind: crate::game::cpm_action_probe::use_kind(game, stack), ticks: e.using_item as u32, pull })
        })
        .flatten();
    let charged =
        |s: Option<&ItemStack>| s.is_some_and(|s| &*s.item == "minecraft:crossbow" && s.data().is_some_and(|d| !d.charged.is_empty()));
    let block = [m.position.x.floor() as i32, m.position.y.floor() as i32, m.position.z.floor() as i32];
    let (block_light, sky_light) = game.native_light(block);
    let pose = e.avatar_pose;
    RemoteState {
        time_ms,
        walk_position: render.walk_position,
        walk_speed: render.walk_speed,
        head_yaw: render.head_yaw,
        pitch: render.pitch,
        age: render.age,
        on_ground: render.on_ground,
        hurt_time: render.hurt_time,
        dead: !m.alive(),
        health: {
            let max = horizon_client::mobs::finalize::max_health(m);
            (max > 0.0).then(|| (m.health / max).clamp(0.0, 1.0))
        },
        crouching: e.crouching,
        sprinting: e.sprinting,
        visually_swimming: pose == Pose::Swimming || (!e.fall_flying && pose == Pose::FallFlying),
        fall_flying: e.fall_flying,
        sleeping: pose == Pose::Sleeping,
        riding: e.passenger,
        on_fire: m.fire_ticks > 0,
        spin_attack: e.auto_spin_attack || pose == Pose::SpinAttack,
        // Horizon keeps another player's fluids each tick.
        in_water: m.fluids.in_water,
        ticks_frozen: m.ticks_frozen,
        air: m.air,
        on_climbable: !e.spectator && crate::game::cpm_action_probe::climbable_at(game, block, e.fall_flying),
        dy: m.position.y - m.previous.y,
        left_handed: left,
        swing: e.swing_right_arm.filter(|_| anim.swing > 0.0).map(|right| (swing_hand(right), anim.swing)),
        swinging: e.swing_right_arm.is_some(),
        using,
        holding: [held.main.is_some(), held.off.is_some()],
        charged: [charged(held.main), charged(held.off)],
        light: Some([block_light, sky_light]),
        cape: crate::game::cpm_cape_probe::motion_of(m, render.partial),
        invisible: m.is_invisible(),
        parrots: e.shoulder_parrots.map(|v| v.is_some()),
    }
}

/// The owner of a stack another player holds (`held_item_geometry`): at the player, facing with
/// its body, its main arm, and in use when it is the hand in use.
pub(super) fn owner(m: &Mob, stack: &ItemStack, held: &crate::game::cpm_item_probe::Held<'_>) -> horizon_assets::item_model::OwnerState {
    let mut owner =
        crate::game::item_draw::owner_at([m.position.x, m.position.y, m.position.z], m.body_yaw, &m.kind.id);
    owner.main_arm_left = m.main_arm_left();
    let in_hand = if m.extra.using_off_hand { held.off } else { held.main };
    if m.extra.using_item > 0 && in_hand.is_some_and(|s| std::ptr::eq(s, stack)) {
        owner.using_this_item = true;
        owner.use_duration = 72_000;
        owner.use_item_remaining_ticks = 72_000 - m.extra.using_item;
        owner.crossbow_charge_duration = 25;
    }
    owner
}

// Another player, drawn with their model (`cpm::other_quads`, its own region of the mod image) as
// the local player is: the same quads, sort, glow, glint, items and parrots, from what the game
// knows of them (others_probe.rs), never the local player's probes.
pub(super) fn submit(
    game: &crate::game::Game<'_>,
    player: &crate::game::native_mob_draw::NativeMobRender<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    drawn: &mut bool,
) {
    use crate::game::cpm_mod as cpm;
    use crate::game::cpm_others_probe as others;
    // Only a player whose model is in the image the game holds is read at all (most players draw
    // vanilla): their UUID first, then their mob.
    let Some(ints) = game.persist.uuids.get(&player.id) else { return };
    let uuid = horizon_client::lan::playerdata::ints_uuid(*ints);
    if !cpm::other_shown(uuid) {
        return;
    }
    let Some(m) = others::mob(game, player.id) else { return };
    let stacks = others::stacks(m);
    let held = others::held(game, m, &stacks);
    let time_ms = cpm::clock_ms(std::time::Instant::now());
    let input = cpm::remote_input(&others::state(game, m, player, &held, time_ms));
    let worn = others::worn(game, m, &stacks, &held);
    let Some(quads) = cpm::other_quads(uuid, &input, &worn) else {
        return;
    };
    *drawn = crate::game::cpm_draw::quads_sorted(game, draw, &quads);
    if *drawn {
        let owner = |stack: &horizon_client::items::ItemStack| others::owner(m, stack, &held);
        crate::game::cpm_item_probe::draw_items_owned(game, draw, &quads.items, &held, player.id, player.walk_position, &owner);
        crate::game::cpm_parrot_probe::draw_parrots_of(game, draw, &quads.shoulders, player.partial, m);
    }
}
