// The local player's state the CPM poses read, from the game's own copy of it (`Game::player`).
// Added to the game as `game::cpm_pose_probe`; player_submit.rs uses it.

/// What the render state (`NativeMobRender`) does not say about the local player.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PoseFlags {
    pub sneaking: bool,
    pub sprinting: bool,
    /// `isVisuallySwimming`: swimming, or crawling in a low gap (CPM's `swimming`).
    pub swimming: bool,
    /// Elytra flight.
    pub fall_flying: bool,
    /// Creative (or spectator) flight.
    pub creative_flying: bool,
    pub sleeping: bool,
    /// In a vehicle.
    pub riding: bool,
    pub on_fire: bool,
    /// Fallen more than 4 blocks (vanilla's fall distance, reset in water and on ladders).
    pub falling: bool,
    /// Air supply, 0..1 of vanilla's 300 ticks.
    pub air: Option<f32>,
    /// Standing on something. The render state's `on_ground` is not kept up to date for this
    /// window's player, so jumps (CPM's JUMPING pose) are seen from the motion's own flag.
    pub on_ground: bool,
}

/// The local player's flags; all off outside a world.
pub(super) fn flags(game: &crate::game::Game<'_>) -> PoseFlags {
    let Some(p) = game.player.as_ref() else {
        return PoseFlags::default();
    };
    PoseFlags {
        sneaking: p.local_crouching,
        sprinting: p.sprinting,
        swimming: p.pose == horizon_client::pose::Pose::Swimming
            || (!p.fall_flying && p.pose == horizon_client::pose::Pose::FallFlying),
        fall_flying: p.fall_flying,
        creative_flying: p.flying,
        sleeping: p.sleeping,
        riding: p.is_passenger,
        on_fire: p.fire_ticks > 0,
        falling: p.fall_distance > cpm_core::FALL_DISTANCE,
        air: Some(p.air.clamp(0, 300) as f32 / 300.0),
        on_ground: p.ground.on_ground,
    }
}
