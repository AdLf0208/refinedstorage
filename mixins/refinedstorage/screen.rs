// The grid's and the drive's menu layouts (API 19).
fn observe_screen(block: &str, screen: &mut Option<NativeScreen>) {
    crate::game::refinedstorage_state::on_screen(block, screen);
}
