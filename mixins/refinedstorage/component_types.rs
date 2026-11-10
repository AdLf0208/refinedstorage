// Before content loads: register `refinedstorage:storage` for item stacks.
fn register_component_types(types: &mut Vec<String>) {
    crate::game::refinedstorage_state::register_component_types(types);
}
