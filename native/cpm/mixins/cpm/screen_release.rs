// A mouse button released on CPM's own mod screen: ends the gesture menu's slider drag.
fn cpm_screen_release(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::cpm_screen::mouse_released(game, screen, button, canceled);
}
