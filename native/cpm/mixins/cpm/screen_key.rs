// A key pressed on CPM's own mod screen (the gist popup's editing keys; the gesture menu swallows
// keys); Escape is left to the game, which closes the screen.
fn cpm_screen_key(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    key: &str,
    modifiers: u32,
    canceled: &mut bool,
) {
    crate::game::cpm_screen::key_pressed(game, screen, key, modifiers, canceled);
}
