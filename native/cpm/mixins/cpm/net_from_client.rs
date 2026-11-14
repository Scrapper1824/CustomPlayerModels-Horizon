// A mod message from a player's client, on the server (net.rs: the relay).
fn cpm_net_from_client(game: &mut crate::game::Game<'_>, player: u128, channel: &str, data: &[u8]) {
    let _ = game;
    crate::game::cpm_net::from_client(player, channel, data);
}
