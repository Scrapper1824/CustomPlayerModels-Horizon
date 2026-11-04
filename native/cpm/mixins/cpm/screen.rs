// CPM's own mod screen, which the gist popup (`game::cpm_gist`) and the gesture menu
// (`game::cpm_menu`) share: Horizon gives a mod one screen, so one of them is shown at a time
// (`cpm_mod::shown_screen`), and each screen hook goes to that one. Added to the game as
// `game::cpm_screen`.
use crate::game::Game;
use crate::game::cpm_mod as cpm;
use horizon_client::mod_api::{HudBatch, NativeScreenView};

/// Which of CPM's screens `screen` is: CPM's own mod screen, with that screen shown.
fn owner(game: &Game<'_>, screen: &NativeScreenView<'_>) -> Option<cpm::ModScreen> {
    if screen.id == "mod" && game.native_mod_screen() == Some(cpm::ID) {
        cpm::shown_screen()
    } else {
        None
    }
}

pub(super) fn render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    match owner(game, screen) {
        Some(cpm::ModScreen::Gist) => crate::game::cpm_gist::render(game, batch, screen),
        Some(cpm::ModScreen::Menu) => crate::game::cpm_menu::render(game, batch, screen),
        None => {}
    }
}

pub(super) fn mouse_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    match owner(game, screen) {
        Some(cpm::ModScreen::Gist) => crate::game::cpm_gist::mouse_pressed(screen, button, canceled),
        Some(cpm::ModScreen::Menu) => crate::game::cpm_menu::mouse_pressed(game, screen, button, canceled),
        None => {}
    }
}

/// Only the menu takes releases (the gist popup never did).
pub(super) fn mouse_released(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    if owner(game, screen) == Some(cpm::ModScreen::Menu) {
        crate::game::cpm_menu::mouse_released(button, canceled);
    }
}

/// Only the menu takes drags (its sliders and its preview).
pub(super) fn mouse_dragged(
    game: &Game<'_>,
    screen: &NativeScreenView<'_>,
    button: i32,
    delta: [f32; 2],
    canceled: &mut bool,
) {
    if owner(game, screen) == Some(cpm::ModScreen::Menu) {
        crate::game::cpm_menu::mouse_dragged(screen, button, delta, canceled);
    }
}

/// Only the menu takes the wheel (its preview's zoom).
pub(super) fn mouse_scrolled(game: &Game<'_>, screen: &NativeScreenView<'_>, scroll: [f64; 2], canceled: &mut bool) {
    if owner(game, screen) == Some(cpm::ModScreen::Menu) {
        crate::game::cpm_menu::mouse_scrolled(screen, scroll, canceled);
    }
}

pub(super) fn key_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, key: &str, modifiers: u32, canceled: &mut bool) {
    match owner(game, screen) {
        Some(cpm::ModScreen::Gist) => crate::game::cpm_gist::key_pressed(key, modifiers, canceled),
        Some(cpm::ModScreen::Menu) => crate::game::cpm_menu::key_pressed(key, canceled),
        None => {}
    }
}

pub(super) fn char_typed(game: &Game<'_>, screen: &NativeScreenView<'_>, c: char, canceled: &mut bool) {
    match owner(game, screen) {
        Some(cpm::ModScreen::Gist) => crate::game::cpm_gist::char_typed(c, canceled),
        Some(cpm::ModScreen::Menu) => *canceled = true,
        None => {}
    }
}
