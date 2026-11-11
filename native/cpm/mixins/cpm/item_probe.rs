// The local player's held and head items, from the game's own copy of them (the hotbar's
// selected slot, the off hand, `Game::armor`), and their drawing at the model's item anchors.
// Added to the game as `game::cpm_item_probe`; player_submit.rs uses it.
//
// The stacks are those Horizon's own `ItemInHandLayer` and `CustomHeadLayer` ports draw for the
// local player (game/mob_blocks.rs `held_item_geometry`, `head_item_geometry`): the selected slot
// and the off hand; the head slot unless it is a helmet (drawn as armour).

use horizon_client::items::ItemStack;

/// What the local player holds and wears on its head.
#[derive(Clone, Copy, Default)]
pub(super) struct Held<'a> {
    pub main: Option<&'a ItemStack>,
    pub off: Option<&'a ItemStack>,
    /// A head-slot item `CustomHeadLayer` draws (a pumpkin, skull, banner, any non-armour item).
    pub head: Option<&'a ItemStack>,
    /// The main hand setting is left.
    pub left_handed: bool,
    /// A skull or head is worn (`AnimationState.hasSkullOnHead`, for hide-skull).
    pub skull: bool,
}

/// The local player's held and head items; none outside a world.
pub(super) fn held<'a>(game: &'a crate::game::Game<'_>) -> Held<'a> {
    if game.player.is_none() {
        return Held::default();
    }
    let worn = game.armor[0].as_ref();
    let head = worn.filter(|s| game.res.items().get(&s.item).and_then(|i| i.equip_asset.as_ref()).is_none());
    Held {
        main: game.stack_in(game.selected),
        off: game.stack_in(crate::game::shield::OFFHAND_SLOT),
        head,
        left_handed: game.settings.main_hand_left,
        skull: worn.is_some_and(|s| crate::game::armor_stands::skull_kind(&s.item).is_some()),
    }
}

/// Draws each of `items` whose slot holds a stack, as Horizon's own layers draw the local
/// player's (game/mob_blocks.rs): held items resolved with the player as their owner
/// (`Game::player_owner`: the item in use, its use ticks, the main arm, a cast rod) and the hand
/// for CIT rules; a worn skull or head with the skull renderer (`submit_special`). `walk`: the
/// player's walk position, which a worn dragon or piglin head animates with. Drawn beside `draw`
/// ([`draw_beside`]).
pub(super) fn draw_items(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    items: &[crate::game::cpm_mod::ItemDraw],
    held: &Held<'_>,
    id: u32,
    walk: f32,
) {
    draw_items_owned(game, draw, items, held, id, walk, &|stack| game.player_owner(Some(stack)));
}

/// [`draw_items`] for any player: `owner` gives a held stack's owner (the local player's, or
/// another player's as Horizon's `held_item_geometry` makes it: others_probe.rs).
pub(super) fn draw_items_owned(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    items: &[crate::game::cpm_mod::ItemDraw],
    held: &Held<'_>,
    id: u32,
    walk: f32,
    owner: &dyn Fn(&ItemStack) -> horizon_assets::item_model::OwnerState,
) {
    use crate::game::cpm_mod as cpm;
    use glam::Mat4;
    use horizon_assets::item_model::{DisplayContext, SpecialKind, SpecialModel};
    let stacks: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let stack = match item.slot {
                cpm_core::ItemSlot::MainHand => held.main,
                cpm_core::ItemSlot::OffHand => held.off,
                cpm_core::ItemSlot::Head => held.head,
            };
            stack.map(|s| (item, s))
        })
        .collect();
    if stacks.is_empty() {
        return;
    }
    let light = draw.light;
    let lights = horizon_client::block_entity_render::diffuse_lights(game.dim == crate::game::Dim::Nether);
    draw_beside(game, draw, |pose, verts| {
        for (item, stack) in stacks {
            let t = pose * Mat4::from_cols_array(&item.transform.to_cols_array());
            let skull = if item.skull { crate::game::armor_stands::skull_kind(&stack.item) } else { None };
            if let Some(kind) = skull {
                // head_item_geometry's skull path (CustomHeadLayer, the player's skull offset 0).
                let special = SpecialModel {
                    kind: if kind == "player" {
                        SpecialKind::PlayerHead
                    } else {
                        SpecialKind::Head { kind: kind.to_owned(), texture: None, animation: walk }
                    },
                };
                let mut sink = horizon_client::item_render::WorldSink {
                    out: &mut *verts,
                    light,
                    lights: Some(lights),
                    glint: game.glint(),
                    tier: 0,
                };
                let profile = stack
                    .extra
                    .as_deref()
                    .and_then(|d| d.other.iter().find(|(k, _)| k == "minecraft:profile"))
                    .map(|(_, t)| horizon_client::scoreboard::tag_to_json(t))
                    .unwrap_or(serde_json::Value::Null);
                let has_foil = stack.data().is_some_and(|d| !d.enchantments.is_empty());
                game.pack.item_renderer.submit_special(
                    &special,
                    &profile,
                    has_foil,
                    t,
                    &game.gui.atlas.sprites,
                    &game.item_host(),
                    &mut sink,
                );
                continue;
            }
            let display = match item.display {
                cpm::ItemDisplay::ThirdPersonLeftHand => DisplayContext::ThirdPersonLeftHand,
                cpm::ItemDisplay::ThirdPersonRightHand => DisplayContext::ThirdPersonRightHand,
                cpm::ItemDisplay::Head => DisplayContext::Head,
            };
            let hand = match item.slot {
                cpm_core::ItemSlot::OffHand => horizon_client::cit::Hand::Off,
                _ => horizon_client::cit::Hand::Main,
            };
            let owner = owner(stack);
            let state = game.resolve_item_in_hand(stack, display, Some(&owner), id as i32 + item.seed, hand);
            // An empty stack or one with no model draws nothing.
            game.submit_item_world(&state, t, light, &mut *verts);
        }
    });
}

/// Runs `add` with the pose of `draw` (its frame to the vertices' space) and its vertices, to add
/// vertices of its own beside them (the items here, the shoulder parrots in parrot_probe.rs).
///
/// `BlockEntityDraw` has no matrix multiply and keeps its vertices to itself, so: its pose is read
/// back from a probe quad drawn under it (a quad's corners go through the pose and nothing else),
/// its vertices are taken out with `into_vertices`, `add` adds under the pose times its own
/// transforms (exact for any affine transform), and the draw is rebuilt with `posed` around them,
/// its pose, light and level lights as they were. Nothing is run when the probe's sprite is
/// missing. This reaches into the pinned Horizon's game module (as the other probes do).
pub(super) fn draw_beside(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    add: impl FnOnce(glam::Mat4, &mut Vec<horizon_client::mesher::Vertex>),
) {
    use crate::game::cpm_mod as cpm;
    use crate::game::native_block_entity_draw::BlockEntityDraw;
    use glam::{Mat4, Vec3};
    // The probe: the origin and the three unit axes, in that order.
    let probe = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if !game.native_draw_quad(draw, cpm::SPRITE, probe, [0.0; 4], 0xFF_FFFF) {
        return;
    }
    let (partial, game_time, light) = (draw.partial, draw.game_time, draw.light);
    let lights = horizon_client::block_entity_render::diffuse_lights(game.dim == crate::game::Dim::Nether);
    let block = game.player.as_ref().map_or([0; 3], |p| {
        [p.position.x.floor() as i32, p.position.y.floor() as i32, p.position.z.floor() as i32]
    });
    let taken = std::mem::replace(draw, BlockEntityDraw::posed(partial, game_time, light, block, Mat4::IDENTITY, lights, Vec::new()));
    let mut verts = taken.into_vertices();
    let n = verts.len();
    let at: Vec<Vec3> = verts[n - 4..].iter().map(|v| Vec3::from_array(v.pos)).collect();
    verts.truncate(n - 4);
    let pose = Mat4::from_cols((at[1] - at[0]).extend(0.0), (at[2] - at[0]).extend(0.0), (at[3] - at[0]).extend(0.0), at[0].extend(1.0));
    add(pose, &mut verts);
    *draw = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, verts);
}
