// The players' downloaded skins, for the models stored in them (CPM's skin models): the raw PNG
// in the game's skin cache and the skin as the game draws it, for the local player and the other
// players in the world. Added to the game as `game::cpm_skin_probe`; frame.rs uses it.

/// Where the game's skin downloader keeps a skin's file: `<game dir>/assets/skins/<2 hex>/<digest>`
/// (game.rs makes `skins::Downloader` with `current_dir()/assets/skins`; skins.rs
/// `Downloader::state` names the file by the digest of `skins::skin_location`). The file holds the
/// PNG as downloaded, before `process_legacy_skin` (which clears the alpha the data lives in).
fn cache_file(digest: &str) -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("assets/skins")
        .join(if digest.len() > 2 { &digest[..2] } else { "xx" })
        .join(digest)
}

/// Gives the runtime the local player's skin when it changed (`cpm_mod::set_skin`). The player's
/// skin is a downloaded one (`skins/<digest>`, the texture `remote.rs` `drawn_skin` gives once it is
/// in the atlas) or none: a default or offline skin holds no model. Outside a world the last one is
/// kept.
pub(super) fn update(game: &crate::game::Game<'_>) {
    use crate::game::cpm_mod as cpm;
    let Some(m) = game.player_model.as_ref() else { return };
    let Some(location) = m.skin.as_deref().filter(|s| s.starts_with("skins/")) else {
        if cpm::skin_set() {
            cpm::set_skin(None);
        }
        return;
    };
    if cpm::skin_known(location, m.slim) {
        return;
    }
    let file = skin_file(location);
    let png = std::fs::read(&file).unwrap_or_else(|e| {
        log::warn!("cpm: cannot read your skin {}: {e}", file.display());
        Vec::new()
    });
    let texture = skin_texture(&png);
    cpm::set_skin(Some(cpm::SkinInput { location: location.to_owned(), slim: m.slim, png, texture }));
}

/// The cache file of any player's downloaded skin `location` (`skins/<digest>`).
fn skin_file(location: &str) -> std::path::PathBuf {
    cache_file(location.strip_prefix("skins/").unwrap_or(location))
}

/// A skin PNG as the game draws it: the 64x64 layout, the opaque parts opaque.
fn skin_texture(png: &[u8]) -> Option<([u32; 2], Vec<u8>)> {
    cpm_core::skin::Image::decode(png)
        .ok()
        .and_then(|i| horizon_client::skins::process_legacy_skin(i.w, i.h, &i.rgba()))
        .map(|rgba| ([64, 64], rgba))
}

/// How the runtime reads the other players' skins (on its loading threads, never this one).
pub(super) const READER: crate::game::cpm_mod::SkinReader =
    crate::game::cpm_mod::SkinReader { file: skin_file, texture: skin_texture };

/// The other players in the world (`cpm_mod::others_tick`): every player entity the game holds
/// (the local player is not one of them), by its UUID (`persist.uuids`, from the spawn packet),
/// with its downloaded skin once that is in the atlas (`remote.rs` `drawn_skin` gives
/// `skins/<digest>` only then; a default or offline skin holds no model). Nearest to the camera
/// (the player's eyes) first: the [`cpm_mod::max_others`](crate::game::cpm_mod::max_others) cap
/// keeps the nearest.
pub(super) fn others(game: &crate::game::Game<'_>) -> Vec<crate::game::cpm_mod::OtherPlayer> {
    use crate::game::cpm_mod::{OtherPlayer, OtherSkin};
    let eye = game.eye_position().unwrap_or_default();
    let away = |m: &horizon_client::mobs::Mob| {
        let d = [m.position.x - eye[0], m.position.y - eye[1], m.position.z - eye[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32
    };
    let mut players: Vec<_> = game
        .mobs
        .iter()
        .filter(|m| &*m.kind.id == "minecraft:player")
        .filter_map(|m| {
            let uuid = horizon_client::lan::playerdata::ints_uuid(*game.persist.uuids.get(&m.id)?);
            let skin = m
                .skin
                .as_deref()
                .filter(|s| s.starts_with("skins/"))
                .map(|s| OtherSkin { location: s.to_owned(), slim: m.slim });
            Some(OtherPlayer { uuid, skin, distance: away(m) })
        })
        .collect();
    players.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    players
}
