// A player joined the server (net.rs: the relay).
fn cpm_net_joined(game: &mut crate::game::Game<'_>, player: u128) {
    let _ = game;
    crate::game::cpm_net::logged_in(player);
}
