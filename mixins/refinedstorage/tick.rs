// One server tick of a network block.
fn observe_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {
    crate::game::refinedstorage_state::on_block_entity_tick(game, pos, entity);
}
