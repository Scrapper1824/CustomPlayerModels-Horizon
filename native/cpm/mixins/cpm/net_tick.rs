// Once a tick, in the window's game and a server's: the `cpm:sync` messages (net.rs).
fn cpm_net_tick(game: &mut crate::game::Game<'_>) {
    crate::game::cpm_net::tick(game);
}
