// A character typed on CPM's own mod screen: into the gist popup's URL field (the gesture menu
// swallows it).
fn cpm_screen_char(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    c: char,
    canceled: &mut bool,
) {
    crate::game::cpm_screen::char_typed(game, screen, c, canceled);
}
