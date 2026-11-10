// The `/refinedstorage` command bodies.
fn perform_command(
    game: &mut crate::game::Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::refinedstorage_state::on_perform_command(game, player, command, ran);
}
