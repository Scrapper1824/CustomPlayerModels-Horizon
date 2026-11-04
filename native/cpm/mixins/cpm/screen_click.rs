// A mouse button pressed on CPM's own mod screen: the gist popup's or the gesture menu's controls.
fn cpm_screen_click(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    button: i32,
    canceled: &mut bool,
) {
    crate::game::cpm_screen::mouse_pressed(game, screen, button, canceled);
}
