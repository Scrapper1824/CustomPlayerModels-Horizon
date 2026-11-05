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

/// [`quads`] in the order CPM's `entityTranslucent` buffer is drawn ([`cpm_mod::back_to_front`]):
/// farthest from the camera first, so faces that lie on one another show the one CPM shows.
/// `BlockEntityDraw` keeps its pose to itself, so it is read back from a probe quad (the origin and
/// the three unit axes; a quad's corners go through the pose and nothing else), the probe taken
/// out, and the draw rebuilt with that pose, as `cpm_item_probe::draw_items` does. Falls back to
/// [`quads`] in submission order when the probe cannot be drawn.
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
    if !crate::game::cpm_draw::quads(game, draw, &sorted) {
        return false;
    }
    if !additive.is_empty() {
        glow_pass(game, draw, quads, additive, (partial, game_time, light, block, pose, lights));
    }
    true
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
