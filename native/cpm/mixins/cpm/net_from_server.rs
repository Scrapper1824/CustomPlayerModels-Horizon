// A mod message from the server, on the window (net.rs: the other players' models).
fn cpm_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {
    let _ = game;
    crate::game::cpm_net::from_server(channel, data);
}
