// The player-head drawing CPM hands the item renderer: the item renderer is in the game's client
// library (`horizon-client-lib`), which cannot reach the mod's own code in the game crate above it.
// So the game crate gives it a function here (frame.rs, once), and player_head.rs, at the head of
// `item_render::special::submit`, asks it to draw each player head in place of the vanilla skull.
// Added to the library as `item_render::cpm_skull_hook` (public, for the game crate).

/// Draws the model's head for a player head whose `minecraft:profile` is the first argument, under
/// the skull `head` part's matrix (blocks, the part's frame: the second argument), with the atlas
/// sprites given, into the sink. True when it drew (the vanilla skull is then left out).
pub type DrawHead = fn(
    &serde_json::Value,
    glam::Mat4,
    &std::collections::HashMap<String, crate::hud::Rect>,
    &mut dyn super::ItemSink,
) -> bool;

static DRAW: std::sync::OnceLock<DrawHead> = std::sync::OnceLock::new();

/// Sets the drawing (the first call wins; later ones change nothing).
pub fn set(draw: DrawHead) {
    let _ = DRAW.set(draw);
}

/// The drawing, once set.
pub fn get() -> Option<DrawHead> {
    DRAW.get().copied()
}
