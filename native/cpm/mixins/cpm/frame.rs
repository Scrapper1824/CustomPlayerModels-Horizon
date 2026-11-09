// Once a frame: load the selected model and keep its texture in the mod image sprite.
fn cpm_frame(game: &mut crate::game::Game<'_>) {
    use crate::game::cpm_mod as cpm;
    // A finished conversion (from its own thread): its result line, and a too-big model's gist
    // popup, which `cpm::pending_gist_popup` holds until its screen can open. The settings page
    // is rebuilt so it shows the outcome (the game rebuilds it only on open and after a click).
    if cpm::poll_convert() {
        crate::game::cpm_convert::refresh_page(game);
    }
    // Link downloads a model asked for: each on a thread of its own (gist_fetch.rs).
    for (link, urls) in cpm::take_fetch_requests() {
        crate::game::cpm_gist_fetch::fetch(link, urls);
    }
    // The gist popup: open its screen when due, run what its input asked for, drop it once closed.
    crate::game::cpm_gist::frame(game);
    // The gesture menu (the menu key or `Gestures…`): open its screen when asked for and the game
    // lets it, drop it once closed (or the mod is off).
    crate::game::cpm_menu::frame(game);
    // Player heads: the item renderer's drawing of the player's own (once), and who the player is
    // (no one while the mod is off: every head is then vanilla).
    horizon_client::item_render::cpm_skull_hook::set(crate::game::cpm_draw::player_head);
    crate::game::cpm_head_probe::update(game);
    if !game.native_on(cpm::ID) {
        return;
    }
    // The model chosen on the settings page (kept as the `model` setting), once.
    cpm::restore_once(|| {
        let data = game.native_data_dir(cpm::ID);
        cpm::set_data_dir(data.clone());
        let dir = cpm::models_dir(&data);
        cpm::selection_from_setting(&dir, game.native_setting(cpm::ID, cpm::MODEL_SETTING))
    });
    // The skin's model (drawn when nothing is chosen): read again when the skin changes.
    crate::game::cpm_skin_probe::update(game);
    cpm::ensure_loaded();
    // A model that just loaded gets its kept parameters; later changes are kept as they happen.
    cpm::restore_params(|key| game.native_setting(cpm::ID, key));
    if let Some((key, value)) = cpm::take_params_to_save() {
        game.native_set_setting(cpm::ID, &key, &value);
    }
    // The Models picker's reads (its icons, the clicked model) that ended: the image changes.
    cpm::poll_picker();
    // Animated textures: a frame that changed pixels makes the image due.
    cpm::tick_textures(std::time::Instant::now());
    if let Some((width, height, rgba)) = cpm::texture_upload(std::time::Instant::now())
        && !game.native_mod_image(width, height, &rgba)
    {
        cpm::refuse(format!("cpm: {}: the game refused the {width}x{height} texture", cpm::selected_name()));
    }
}
