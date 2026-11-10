// The drive's disk slots and the grid's view slots.
fn observe_block_entity_container(
    block: &str,
    container: &mut Option<horizon_client::containers::ModContainer>,
) {
    crate::game::refinedstorage_state::on_block_entity_container(block, container);
}
