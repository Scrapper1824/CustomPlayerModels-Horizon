// Draws CPM's own mod screen (the gist popup or the gesture menu) when it is open.
fn cpm_screen_render(
    game: &crate::game::Game<'_>,
    batch: &mut horizon_client::mod_api::HudBatch,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
) {
    crate::game::cpm_screen::render(game, batch, screen);
}
