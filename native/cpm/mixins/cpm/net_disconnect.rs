// The window's connection to a server ends (net.rs: what it sent goes).
fn cpm_net_disconnect(game: &crate::game::Game<'_>) {
    let _ = game;
    crate::game::cpm_net::disconnecting();
}
