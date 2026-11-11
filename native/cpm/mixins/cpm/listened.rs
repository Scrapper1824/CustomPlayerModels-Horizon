// Ask the game to pose players for native_player_submit, while this mod is on and has a model to
// draw: the local player's, or another player's.
fn cpm_listened(game: &crate::game::Game<'_>, listened: &mut bool) {
    use crate::game::cpm_mod as cpm;
    if game.native_on(cpm::ID) && (cpm::has_model() || cpm::others_drawable()) {
        *listened = true;
    }
}
