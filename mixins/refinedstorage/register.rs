// `/refinedstorage info|filter`.
fn register_commands(commands: &mut Vec<NativeCommand>) {
    crate::game::refinedstorage_state::on_register_commands(commands);
}
