// The model's quads drawn with the game's `native_draw_quad` (what player_submit.rs and
// first_person_arm.rs share), and as a player head's item quads (skull_hook.rs). Added to the game
// as `game::cpm_draw`.

/// Draws `quads` into `draw`, each with its sprite: a glowing or emissive one at full brightness,
/// the others at `draw.light` (left as it was). False, with nothing drawn, when the atlas lacks the
/// model's sprite ([`cpm_mod::SPRITE`](crate::game::cpm_mod::SPRITE)); a quad of a worn texture the
/// atlas lacks is left out (the rest of the model still draws). The armour glint needs the drawn
/// vertices: [`quads_sorted`] tags it.
pub(super) fn quads(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    quads: &crate::game::cpm_mod::Quads,
) -> bool {
    drawn_quads(game, draw, quads).is_some()
}

/// [`quads`], giving the glint rect of each quad drawn (four vertices each), in order.
fn drawn_quads(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    quads: &crate::game::cpm_mod::Quads,
) -> Option<Vec<Option<[f32; 4]>>> {
    use crate::game::cpm_mod as cpm;
    if !game.gui.atlas.sprites.contains_key(cpm::SPRITE) {
        return None;
    }
    let light = draw.light;
    let mut drawn = Vec::with_capacity(quads.quads.len());
    for q in &quads.quads {
        draw.light = if q.glow || q.emissive { cpm::FULL_BRIGHT } else { light };
        let sprite = quads.sprites.get(q.sprite).map_or(cpm::SPRITE, String::as_str);
        // False only for a sprite the atlas lacks: that quad is skipped.
        if game.native_draw_quad(draw, sprite, q.corners, q.uv, q.tint) {
            drawn.push(q.glint);
        }
    }
    draw.light = light;
    Some(drawn)
}

/// [`quads`] in the order CPM's `entityTranslucent` buffer is drawn ([`cpm_mod::back_to_front`]):
/// farthest from the camera first, so faces that lie on one another show the one CPM shows.
/// `BlockEntityDraw` keeps its pose to itself, so it is read back from a probe quad (the origin and
/// the three unit axes; a quad's corners go through the pose and nothing else), the probe taken
/// out, and the draw rebuilt with that pose, as `cpm_item_probe::draw_items` does. Falls back to
/// [`quads`] when the probe cannot be drawn (only when the atlas lacks the model's sprite, so that
/// draws nothing either). The glinting
/// quads' vertices are then tagged with the armour glint ([`glint_pass`]).
pub(super) fn quads_sorted(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    quads: &crate::game::cpm_mod::Quads,
) -> bool {
    use crate::game::cpm_mod as cpm;
    use crate::game::native_block_entity_draw::BlockEntityDraw;
    use glam::{Mat4, Vec3};
    let probe = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if !game.gui.atlas.sprites.contains_key(cpm::SPRITE) || !game.native_draw_quad(draw, cpm::SPRITE, probe, [0.0; 4], 0xFF_FFFF) {
        return crate::game::cpm_draw::quads(game, draw, quads);
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
    *draw = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, verts);
    let core_pose = cpm_core::glam::Mat4::from_cols_array(&pose.to_cols_array());
    let (solid, additive) = cpm::split_glow(&quads.quads, core_pose);
    let mut sorted = cpm::Quads { quads: solid, ..quads.clone() };
    cpm::back_to_front(&mut sorted.quads, core_pose);
    let start = n - 4;
    let Some(glints) = drawn_quads(game, draw, &sorted) else {
        return false;
    };
    if glints.iter().any(Option::is_some) {
        glint_pass(draw, start, &glints, (partial, game_time, light, block, pose, lights));
    }
    if !additive.is_empty() {
        glow_pass(game, draw, quads, additive, (partial, game_time, light, block, pose, lights));
    }
    true
}

/// Horizon's armour glint (`mobs::render::ARMOR_GLINT`, `EquipmentLayerRenderer.renderLayers`'
/// foil): the overlay on the glinting quads' vertices (from `start`, four a quad drawn, `glints`
/// in order), each with its place in its own texture as the `glint_uv` entity.wgsl scrolls the
/// glint by. The draw is rebuilt around them as [`quads_sorted`] does.
fn glint_pass(
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    start: usize,
    glints: &[Option<[f32; 4]>],
    (partial, game_time, light, block, pose, lights): (f32, i64, u32, [i32; 3], glam::Mat4, (glam::Vec3, glam::Vec3)),
) {
    use crate::game::native_block_entity_draw::BlockEntityDraw;
    let empty = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, Vec::new());
    let mut verts = std::mem::replace(draw, empty).into_vertices();
    for (quad, glint) in verts[start..].chunks_mut(4).zip(glints) {
        let Some(rect) = glint else { continue };
        for (v, uv) in quad.iter_mut().zip(crate::game::cpm_mod::glint_corners(*rect)) {
            v.layer = v.layer & !0xffff | horizon_client::mobs::render::ARMOR_GLINT;
            v.glint_uv = uv;
        }
    }
    *draw = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, verts);
}

/// CPM's eyes layer for the untinted glowing faces (`CustomRenderTypes.EYES`: additive, no depth
/// writes, after the rest) on Horizon's additive energy swirl pass (`ENERGY_SWIRL_OVERLAY`: one,
/// one blending, no depth writes, depth tested). That pass samples a sprite by the vertex UV's
/// place in it (`stage_light` of the sprite's origin in the light, of its size in the colour) and
/// halves the texel's colour, so each face is drawn twice. The faces are drawn as usual, then
/// their vertices retagged; the draw is rebuilt around them as [`quads_sorted`] does.
fn glow_pass(
    game: &crate::game::Game<'_>,
    draw: &mut crate::game::native_block_entity_draw::BlockEntityDraw,
    quads: &crate::game::cpm_mod::Quads,
    additive: Vec<crate::game::cpm_mod::Quad>,
    (partial, game_time, light, block, pose, lights): (f32, i64, u32, [i32; 3], glam::Mat4, (glam::Vec3, glam::Vec3)),
) {
    use crate::game::cpm_mod as cpm;
    use crate::game::native_block_entity_draw::BlockEntityDraw;
    let empty = || BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, Vec::new());
    let verts = std::mem::replace(draw, empty()).into_vertices();
    let start = verts.len();
    *draw = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, verts);
    let mut rects = Vec::new();
    for q in &additive {
        let sprite = quads.sprites.get(q.sprite).map_or(cpm::SPRITE, String::as_str);
        let Some(r) = game.gui.atlas.sprites.get(sprite) else { continue };
        for _ in 0..2 {
            if game.native_draw_quad(draw, sprite, q.corners, q.uv, 0xFF_FFFF) {
                rects.push([r.u0, r.v0, r.u1, r.v1]);
            }
        }
    }
    let mut verts = std::mem::replace(draw, empty()).into_vertices();
    for (quad, rect) in verts[start..].chunks_mut(4).zip(rects) {
        for v in quad {
            v.uv = cpm::swirl_uv(v.uv, rect);
            v.light = horizon_client::breaking::stage_light([rect[0], rect[1]]);
            v.color = horizon_client::breaking::stage_light([rect[2] - rect[0], rect[3] - rect[1]]);
            v.layer = horizon_client::gfx::ENERGY_SWIRL_OVERLAY;
            v.normal = [0; 4];
        }
    }
    *draw = BlockEntityDraw::posed(partial, game_time, light, block, pose, lights, verts);
}

/// A player head as the model's head (`RedirectHolderSkull`), when the head is the local player's
/// (`cpm_head_probe`): the item renderer's drawing (skull_hook.rs, set by frame.rs), given the
/// head's `profile`, the skull `head` part's matrix `head` and the atlas `sprites`. Each quad goes
/// to `sink` as the vanilla skull's faces do (`PlayerHeadSpecialRenderer`: the skin's
/// `entityTranslucent`, so improved transparency's stages), shaded and lit by the sink (the
/// wearer's light in the world, the GUI's in a slot); a glowing or emissive face at full block
/// light (the light emission a quad can carry; CPM's eyes layer has no item-renderer pass). False,
/// with nothing drawn (the vanilla skull), for another player's head, with no model or its
/// texture not in the game yet, or when the atlas lacks the model's sprite.
pub(crate) fn player_head(
    profile: &serde_json::Value,
    head: glam::Mat4,
    sprites: &std::collections::HashMap<String, horizon_client::hud::Rect>,
    sink: &mut dyn horizon_client::item_render::ItemSink,
) -> bool {
    use crate::game::cpm_mod as cpm;
    use glam::Vec3;
    use horizon_client::item_render::{Foil, ItemQuad};
    if !cpm::is_own_head(&crate::game::cpm_head_probe::profile(profile)) {
        return false;
    }
    let Some(quads) = cpm::skull_quads(cpm::clock_ms(std::time::Instant::now())) else {
        return false;
    };
    if !sprites.contains_key(cpm::SPRITE) {
        return false;
    }
    // A reflection reverses the winding, not the normals (special.rs `draw`).
    let flip = head.determinant() < 0.0;
    for q in &quads.quads {
        let name = quads.sprites.get(q.sprite).map_or(cpm::SPRITE, String::as_str);
        let Some(r) = sprites.get(name) else { continue };
        let pos = q.corners.map(|c| head.transform_point3(Vec3::from_array(c)));
        let normal = (pos[1] - pos[0]).cross(pos[2] - pos[0]).normalize_or_zero();
        let normal = if normal == Vec3::ZERO { (pos[2] - pos[1]).cross(pos[3] - pos[1]).normalize_or(Vec3::Y) } else { normal };
        let normal = if flip { -normal } else { normal };
        // native_draw_quad's mapping: texels of 16 a side, corners TL, BL, BR, TR.
        let [u0, v0, u1, v1] = q.uv.map(|t| t / 16.0);
        let local = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
        sink.quad(ItemQuad {
            pos,
            uv: local.map(|[u, v]| [r.u0 + (r.u1 - r.u0) * u, r.v0 + (r.v1 - r.v0) * v]),
            foil_uv: local,
            normal,
            color: 0xFF00_0000 | (q.tint & 0xFF_FFFF),
            foil: Foil::None,
            light_emission: if q.glow || q.emissive { 15 } else { 0 },
            cull_back: false,
            translucent: false,
            oit: true,
        });
    }
    true
}
