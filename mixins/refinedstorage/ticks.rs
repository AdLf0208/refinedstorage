// The cable, the disk drive and the grid tick.
fn observe_block_entity_ticks(block: &str, ticks: &mut bool) {
    crate::game::refinedstorage_state::on_block_entity_ticks(block, ticks);
}
