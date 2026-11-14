// Models shared in Horizon-hosted worlds: the `cpm:sync` mod messages (docs/native-mods.md, "Mod
// messages"), both halves. Added to the game as `game::cpm_net`; the net_* hook helpers call it, and
// its logic (the messages, the host's relay, what a client sends and receives) is in runtime.rs.
//
// Where each half runs: every single-player world runs on an integrated server ("Server thread"), with
// the window joined to it as a player (integrated.rs), so the server half runs there whether the world
// is open to LAN or not, and on a Horizon (dedicated) server; the window's client half talks to it with
// `native_net_send_to_server` as it would to any server. The host is thus a player like the others.
use crate::game::Game;
use crate::game::cpm_mod as cpm;

/// Once a tick, in every `Game`: the window's sends its model (the client half), a server's relays
/// what its players sent (the server half).
pub(super) fn tick(game: &mut Game<'_>) {
    if game.native_window() {
        let on = game.native_on(cpm::ID);
        cpm::net_client_tick(on, online(game), |frame| game.native_net_send_to_server(cpm::SYNC_CHANNEL, frame));
    } else {
        cpm::relay_tick(std::time::Instant::now(), |to, frame| {
            match game.native_net_send_to_player(to, cpm::SYNC_CHANNEL, frame) {
                Ok(()) => true,
                Err(e) => {
                    log::debug!("cpm: a message to a player waits: {e}");
                    false
                }
            }
        });
    }
}

/// Whether the window is in a server's play state that carries mod messages: connected, joined,
/// not being reconfigured (a proxy moving the player to another server reconfigures them, with no
/// disconnect), and the server announced mod messages (`remote.rs`, `Event::ModNet`; a
/// reconfiguration clears it until the next server announces them again).
fn online(game: &Game<'_>) -> bool {
    game.remote.as_ref().is_some_and(|r| r.joined && r.reconfiguring.is_none() && r.mod_net)
}

/// A mod message from a player's client (the server half): ours go to the relay.
pub(super) fn from_client(player: u128, channel: &str, data: &[u8]) {
    if channel == cpm::SYNC_CHANNEL {
        cpm::relay_receive(player, data, std::time::Instant::now());
    }
}

/// A mod message from the server (the client half): ours go to the other players' models.
pub(super) fn from_server(channel: &str, data: &[u8]) {
    if channel == cpm::SYNC_CHANNEL {
        cpm::net_client_receive(data);
    }
}

/// A player joined the server.
pub(super) fn logged_in(player: u128) {
    cpm::relay_joined(player);
}

/// A player left the server: their model goes, and the others are told.
pub(super) fn logged_out(player: u128) {
    cpm::relay_left(player);
}

/// The window's connection ends: what the server sent goes, and the model is sent again on the next
/// join.
pub(super) fn disconnecting() {
    cpm::net_client_reset();
}
