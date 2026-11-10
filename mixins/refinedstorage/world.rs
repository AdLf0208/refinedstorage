// The world opened or closed: the disk cache belongs to one world.
fn observe_world_lifecycle(game: &mut crate::game::Game<'_>, phase: WorldPhase) {
    crate::game::refinedstorage_state::on_world_lifecycle(game, phase);
}
