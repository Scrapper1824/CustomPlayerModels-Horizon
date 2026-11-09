// The local player, drawn with the loaded model instead of the vanilla one. Anything missing (no
// model, its texture not uploaded, the sprite unknown) leaves `drawn` false: the vanilla player.
fn cpm_player_submit(
    game: &crate::game::Game<'_>,
    player: &crate::game::native_mob_draw::NativeMobRender<'_>,
    who: &crate::game::native_mob_draw::NativePlayerRender<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    drawn: &mut bool,
) {
    use crate::game::{cpm_mod as cpm, cpm_pose_probe as probe};
    if *drawn || !who.local || !game.native_on(cpm::ID) {
        return;
    }
    let flags = probe::flags(game);
    let held = crate::game::cpm_item_probe::held(game);
    let mut input = cpm_core::PoseInput {
        time_ms: cpm::clock_ms(std::time::Instant::now()),
        walk_position: player.walk_position,
        walk_speed: player.walk_speed,
        head_yaw: player.head_yaw,
        pitch: player.pitch,
        age: player.age,
        on_ground: flags.on_ground,
        falling: flags.falling,
        sneaking: flags.sneaking,
        sprinting: flags.sprinting,
        swimming: flags.swimming,
        fall_flying: flags.fall_flying,
        creative_flying: flags.creative_flying,
        sleeping: flags.sleeping,
        riding: flags.riding,
        on_fire: flags.on_fire,
        air: flags.air,
        // Set from what is worn (`cpm::quads`).
        equipment: cpm_core::Equipment::default(),
        left_handed: held.left_handed,
        // Hurt, dying, health, hunger, light and the action states (`cpm::action_input`).
        ..Default::default()
    };
    cpm::action_input(&crate::game::cpm_action_probe::state(game, &held, player.partial), &mut input);
    // The cape layer: its swing, and none on an invisible player (cape_probe.rs).
    input.cape = crate::game::cpm_cape_probe::motion(game, player.partial);
    input.invisible = crate::game::cpm_cape_probe::invisible(game);
    // PARROT_LEFT / PARROT_RIGHT (parrot_probe.rs).
    [input.parrot_left, input.parrot_right] = crate::game::cpm_parrot_probe::on_shoulders(game);
    // The game leaves out its armour, elytra and cape layers for a player a mod drew: the model
    // draws them (its own armour, elytra and cape parts, else the vanilla pieces on its body).
    let mut worn = crate::game::cpm_equipment_probe::worn(game);
    // A worn skull hides the head of a model with hideHeadIfSkull.
    worn.skull = held.skull;
    // The gesture menu's preview poses the model with the same input.
    cpm::record_input(&input);
    let Some(quads) = cpm::quads(&input, &worn) else {
        return;
    };
    *drawn = crate::game::cpm_draw::quads_sorted(game, draw, &quads);
    // The game leaves out its held item, head item and shoulder parrot layers too: the items go
    // where the model holds them (its item parts, else its arms and head), the parrots on its
    // shoulder parts (else where vanilla puts them).
    if *drawn {
        crate::game::cpm_item_probe::draw_items(game, draw, &quads.items, &held, player.id, player.walk_position);
        crate::game::cpm_parrot_probe::draw_parrots(game, draw, &quads.shoulders, player.partial);
    }
}
