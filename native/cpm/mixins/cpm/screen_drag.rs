// The mouse dragged on CPM's own mod screen: moves the gesture menu's slider live, or turns its
// preview.
fn cpm_screen_drag(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    button: i32,
    delta: [f32; 2],
    canceled: &mut bool,
) {
    crate::game::cpm_screen::mouse_dragged(game, screen, button, delta, canceled);
}
