// A storage disk clicked onto a drive goes in; every other click is the
// block's (the drive and the grid open their menus).
fn observe_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    face: [i32; 3],
    canceled: &mut bool,
) {
    crate::game::refinedstorage_state::on_right_click_block(game, pos, face, canceled);
}
