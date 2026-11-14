// A player left the server (net.rs: the relay drops them and tells the others).
fn cpm_net_left(game: &mut crate::game::Game<'_>, player: u128) {
    let _ = game;
    crate::game::cpm_net::logged_out(player);
}
