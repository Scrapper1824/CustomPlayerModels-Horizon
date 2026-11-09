// The local player's downloaded skin, for the model stored in it (CPM's skin model): the raw PNG
// in the game's skin cache and the skin as the game draws it. Added to the game as
// `game::cpm_skin_probe`; frame.rs uses it.

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
    let file = cache_file(&location["skins/".len()..]);
    let png = std::fs::read(&file).unwrap_or_else(|e| {
        log::warn!("cpm: cannot read your skin {}: {e}", file.display());
        Vec::new()
    });
    // As the game draws it: the 64x64 layout, the opaque parts opaque.
    let texture = cpm_core::skin::Image::decode(&png)
        .ok()
        .and_then(|i| horizon_client::skins::process_legacy_skin(i.w, i.h, &i.rgba()))
        .map(|rgba| ([64, 64], rgba));
    cpm::set_skin(Some(cpm::SkinInput { location: location.to_owned(), slim: m.slim, png, texture }));
}
