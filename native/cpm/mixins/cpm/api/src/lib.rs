//! CPM's trait-mod entry point (`[api]`). It overrides no hook: all of CPM is in its mixin
//! helpers. It is here so `horizon_client::mod_api::mods()` lists CPM: the game reserves the
//! `horizon/modimage` sprite that `native_mod_image` fills with the model's texture only when
//! that list is not empty (`HudAtlas::build_over_with_image`, at startup and on a resource
//! reload), and a mixin-only bundle is not in it.
#![forbid(unsafe_code)]

use horizon_client::mod_api::Hooks;

/// CPM's entry in the game's mod list. Every `Hooks` method keeps its default (no effect).
struct Cpm;

impl Hooks for Cpm {}

/// Called once by the launcher's generated game executable.
pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Cpm)
}
