// Whose a player head is: the local player's identities, read from the game, and a head's
// `minecraft:profile`. Added to the game as `game::cpm_head_probe`; frame.rs and draw.rs use it.

/// Gives the runtime the local player's identities (`cpm_mod::set_own_identities`): none outside a
/// world or while the mod is off.
/// - **On a server:** the name it was joined with; the signed-in account's UUID when that is the
///   account joined with, else the name's offline UUID.
/// - **In a world of its own:** the world's player (its UUID and the name it acts with), and the
///   signed-in account.
pub(super) fn update(game: &crate::game::Game<'_>) {
    use crate::game::cpm_mod::{self as cpm, Identity};
    if game.player.is_none() || !game.native_on(cpm::ID) {
        cpm::set_own_identities(Vec::new());
        return;
    }
    let account = horizon_client::auth::current();
    let mut own = Vec::new();
    if let Some(remote) = game.remote.as_ref() {
        let name = remote.session.name.clone();
        let id = match account.as_ref() {
            Some(a) if a.name == name => a.uuid,
            _ => horizon_protocol::crypto::offline_uuid(&name),
        };
        own.push(Identity { id: Some(id), name });
    } else {
        own.push(Identity {
            id: game.level.player_uuid.map(horizon_client::lan::playerdata::ints_uuid),
            name: game.acting_player_name().to_owned(),
        });
        if let Some(a) = account.as_ref() {
            own.push(Identity { id: Some(a.uuid), name: a.name.clone() });
        }
    }
    cpm::set_own_identities(own);
}

/// What a head's `minecraft:profile` (as `scoreboard::tag_to_json` gives it) names
/// (`ResolvableProfile`): a bare name, or a compound's `id` (four ints or a UUID string) and
/// `name`.
pub(super) fn profile(v: &serde_json::Value) -> crate::game::cpm_mod::HeadProfile {
    use crate::game::cpm_mod::HeadProfile;
    use serde_json::Value;
    match v {
        Value::String(name) => HeadProfile { id: None, name: Some(name.clone()) },
        Value::Object(_) => HeadProfile {
            id: v.get("id").and_then(uuid),
            name: v.get("name").and_then(Value::as_str).map(str::to_owned),
        },
        _ => HeadProfile::default(),
    }
}

/// A profile's `id`: an int array (`UUIDUtil.CODEC`) or a UUID string, with or without dashes.
fn uuid(v: &serde_json::Value) -> Option<u128> {
    match v {
        serde_json::Value::Array(a) => {
            let ints: Vec<i32> = a.iter().map(|x| x.as_i64().map(|i| i as i32)).collect::<Option<_>>()?;
            let ints: [i32; 4] = ints.try_into().ok()?;
            Some(horizon_client::lan::playerdata::ints_uuid(ints))
        }
        serde_json::Value::String(s) => horizon_client::auth::parse_uuid(s),
        _ => None,
    }
}
