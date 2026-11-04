// The gesture menu key went down while playing: ask for the menu (the frame hook opens it).
fn cpm_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    use crate::game::cpm_mod as cpm;
    let allowed = cpm::menu_key_allowed(game.native_playing(), game.native_mod_screen().is_some());
    if !pressed || !game.native_on(cpm::ID) || !allowed {
        return;
    }
    if game.native_mod_key(cpm::ID, cpm::MENU_KEY, cpm::DEFAULT_MENU_KEY) == key {
        cpm::request_menu();
    }
}
