// The wheel turned over CPM's own mod screen: zooms the gesture menu's preview.
fn cpm_screen_scroll(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    scroll: [f64; 2],
    canceled: &mut bool,
) {
    crate::game::cpm_screen::mouse_scrolled(game, screen, scroll, canceled);
}
