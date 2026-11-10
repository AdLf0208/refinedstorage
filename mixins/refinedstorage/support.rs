// Game-side glue for Refined Storage slice 1: the item network core.
//
// Injected into `crate::game` as `refinedstorage_state`; it needs the game, so
// it is not part of the standalone library (only `runtime.rs` is).
//
// The slice: a **Controller** (the network's root: a connected component with
// one is online, without one it is off), **Cables** (the connected component's
// wiring), a **Disk Drive** (nine slots of storage disks), the **1k Storage
// Disk** (the items live in its item component, so it carries them when moved)
// and the **Grid** (a 9x6 menu that searches, withdraws and deposits across
// every disk in the network).
//
// The game gives each block entity its own slots, so the grid cannot write a
// drive's slots. The port keeps a per-disk cache in this module: a drive's
// tick mirrors its disk items into the cache (or writes a change back), and
// the grid's tick aggregates the cache of every drive in its network and
// reconciles the menu slots the player changed. A drive is with its disk at
// most one tick behind a grid change, and the disk's item component is the
// truth whenever the drive is loaded and not mid-write.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use horizon_client::containers::{ModBlockEntity, ModContainer};
use horizon_client::items::ItemStack;
use horizon_client::text::Text;
use horizon_world::nbt::{Compound, JavaString, Tag};
use horizon_world::registry::BlockStateId;

use crate::game::native_hooks::NativeScreen;
use crate::game::native_lifecycle::WorldPhase;
use crate::game::native_server::{NativeCommand, NativeServerPlayer};
use crate::game::refinedstorage::{
    CAPACITY_1K, Disk, Delta, GRID_SLOTS, Kind, Stored, deltas, extract, insert, network, view,
};

/// The five content ids of the slice, exactly as the content mod declares
/// them.
const CABLE: &str = "refinedstorage:cable";
const DISK_DRIVE: &str = "refinedstorage:disk_drive";
const GRID: &str = "refinedstorage:grid";
const DISK: &str = "refinedstorage:1k_storage_disk";
/// The item component that carries a disk's contents (`native_component_types`).
const COMPONENT: &str = "refinedstorage:storage";

/// The drive's menu shows one row of nine (upstream has eight disks; the pin's
/// menu types only show 9/18/27/45/54 slots, and a menu is how a player moves
/// disks in and out).
const DRIVE_MENU: &str = "minecraft:generic_9x1";
const GRID_MENU: &str = "minecraft:generic_9x6";

/// A disk cache key: the dimension, the drive's position and its slot.
type DiskKey = (String, [i32; 3], usize);

/// Every cache key a set of drives can hold.
fn drive_keys(dimension: &str, drives: &[[i32; 3]]) -> Vec<DiskKey> {
    drives
        .iter()
        .flat_map(|drive| {
            (0..crate::game::refinedstorage::DRIVE_SLOTS)
                .map(move |slot| (dimension.to_owned(), *drive, slot))
        })
        .collect()
}

/// The port's cross-hook state. Everything in it is derived from loaded blocks
/// and their block entities, so a new world drops it.
struct Core {
    /// Each loaded drive slot that holds a storage disk.
    disks: HashMap<DiskKey, Disk<ItemStack>>,
    /// Disk caches a grid changed and their drive has not written back yet.
    dirty: HashSet<DiskKey>,
    /// Each grid's transient search, by dimension and position (upstream's
    /// search box is the client's; the pin's mod menus have no text field, so
    /// `/refinedstorage filter` sets it).
    filters: HashMap<(String, [i32; 3]), String>,
    /// The connection bits each cable's state was last written with.
    connections: HashMap<(String, [i32; 3]), u8>,
    /// The `active` state each grid was last written with.
    active: HashMap<(String, [i32; 3]), bool>,
}

impl Default for Core {
    fn default() -> Self {
        Core {
            disks: HashMap::new(),
            dirty: HashSet::new(),
            filters: HashMap::new(),
            connections: HashMap::new(),
            active: HashMap::new(),
        }
    }
}

static CORE: Mutex<Option<Core>> = Mutex::new(None);

fn core() -> std::sync::MutexGuard<'static, Option<Core>> {
    CORE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn with_core<R>(f: impl FnOnce(&mut Core) -> R) -> R {
    let mut guard = core();
    f(guard.get_or_insert_with(Core::default))
}

// --- items on the network --------------------------------------------------

/// One stack with another count, keeping its identity (components included).
fn counted(stack: &ItemStack, count: u32) -> ItemStack {
    ItemStack {
        item: stack.item.clone(),
        count,
        damage: stack.damage,
        extra: stack.extra.clone(),
    }
}

/// An item stack as the network sees it: two stacks are one resource when
/// their ids, damage and components are all equal, and their counts add.
impl Stored for ItemStack {
    fn key(&self) -> String {
        horizon_world::snbt_text::to_snbt(&horizon_client::save::item_to_nbt(&counted(self, 1)))
    }

    fn count(&self) -> u32 {
        self.count
    }

    fn with_count(&self, count: u32) -> Self {
        counted(self, count)
    }
}

/// A disk item's contents from its component; an empty disk when it has none
/// (a freshly made or creative-given disk).
fn read_disk(game: &crate::game::Game<'_>, stack: &ItemStack) -> Disk<ItemStack> {
    let mut disk = Disk::new(CAPACITY_1K);
    let Some(Tag::List(entries)) = game.native_component(stack, COMPONENT).ok().flatten() else {
        return disk;
    };
    let mut stored = Vec::with_capacity(entries.len());
    for entry in entries {
        let Tag::String(text) = entry else { continue };
        let Ok((tag, _)) = horizon_world::nbt::parse_snbt(&text.to_string_lossy()) else {
            continue;
        };
        if let Some(stack) = horizon_client::save::item_from_nbt(&tag) {
            stored.push(stack);
        }
    }
    disk.set_entries(stored);
    disk
}

/// Write a disk's contents into its item component (an empty disk loses the
/// component rather than keeping an empty list).
fn write_disk(game: &crate::game::Game<'_>, stack: &mut ItemStack, disk: &Disk<ItemStack>) {
    if disk.entries().is_empty() {
        let _ = game.native_remove_component(stack, COMPONENT);
        return;
    }
    let entries = disk
        .entries()
        .iter()
        .map(|entry| {
            Tag::String(JavaString::from(
                horizon_world::snbt_text::to_snbt(&horizon_client::save::item_to_nbt(entry))
                    .as_str(),
            ))
        })
        .collect();
    let _ = game.native_set_component(stack, COMPONENT, Tag::List(entries));
}

/// The disk items in a drive's slots.
fn is_disk(stack: &ItemStack) -> bool {
    &*stack.item == DISK
}

// --- blocks ----------------------------------------------------------------

fn state_name(game: &crate::game::Game<'_>, state: BlockStateId) -> String {
    game.res.facts().get(state).name.to_string()
}

/// The network kind of the loaded block at `pos`, none when it is not loaded
/// or not a network block.
fn block_kind(game: &crate::game::Game<'_>, pos: [i32; 3]) -> Option<Kind> {
    if !game.world.loaded(pos[0], pos[2]) {
        return None;
    }
    let state = game.world.block(pos[0], pos[1], pos[2]);
    Kind::from_block(&state_name(game, state))
}

/// The network the block at `start` belongs to, read from the loaded level.
fn network_at(game: &crate::game::Game<'_>, start: [i32; 3]) -> crate::game::refinedstorage::Network {
    network(start, |pos| block_kind(game, pos))
}

// --- block-entity declaration ---------------------------------------------

/// A network block gets a block entity (`native_block_entity`, API 15).
pub fn on_block_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if Kind::from_block(block).is_some() {
        *entity = Some((block.to_owned(), Compound::new()));
    }
}

/// The blocks that tick: the cable (connections), the drive (its disks) and
/// the grid (its view). The controller holds no state yet.
pub fn on_block_entity_ticks(block: &str, ticks: &mut bool) {
    *ticks = matches!(Kind::from_block(block), Some(Kind::Cable | Kind::DiskDrive | Kind::Grid));
}

/// The drive's and the grid's slots, and the vanilla menus that show them
/// (`native_block_entity_container`, API 16).
pub fn on_block_entity_container(block: &str, container: &mut Option<ModContainer>) {
    let (slots, menu) = match Kind::from_block(block) {
        Some(Kind::DiskDrive) => (crate::game::refinedstorage::DRIVE_SLOTS, DRIVE_MENU),
        Some(Kind::Grid) => (GRID_SLOTS, GRID_MENU),
        _ => return,
    };
    let all: Vec<usize> = (0..slots).collect();
    *container = Some(ModContainer {
        slots,
        // No hopper reaches a grid's view or a drive's disk slots: neither is
        // a container upstream.
        faces: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        // The menu's clicks may put items into every slot (a deposit into the
        // grid, a disk into the drive) and take them out.
        inputs: all.clone(),
        outputs: all,
        menu: Some(menu.to_owned()),
    });
}

// --- the ticks -------------------------------------------------------------

/// One server tick of a network block (`native_block_entity_tick`, API 17).
#[allow(
    private_interfaces,
    reason = "the hook helper sits one module away, inside the game crate"
)]
pub fn on_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut ModBlockEntity,
) {
    match entity.id.as_str() {
        CABLE => cable_tick(game, pos, entity),
        DISK_DRIVE => drive_tick(game, pos, entity),
        GRID => grid_tick(game, pos, entity),
        _ => {}
    }
}

/// A cable's six connection bits, from the blocks beside it (upstream's
/// `computeConnections`: a neighbour with a network node accepts the
/// connection, and every network block of the slice has one).
fn cable_tick(game: &mut crate::game::Game<'_>, pos: [i32; 3], _entity: &mut ModBlockEntity) {
    let mut bits = 0u8;
    for (index, at) in crate::game::refinedstorage::neighbours(pos)
        .into_iter()
        .enumerate()
    {
        if block_kind(game, at).is_some() {
            bits |= 1 << index;
        }
    }
    let dimension = game.native_dimension().to_owned();
    let key = (dimension, pos);
    let previous = with_core(|core| core.connections.get(&key).copied());
    if previous == Some(bits) {
        return;
    }
    // The declared state properties, in `neighbours`' order.
    const NAMES: [&str; 6] = ["north", "east", "south", "west", "up", "down"];
    for (index, name) in NAMES.into_iter().enumerate() {
        let value = if bits & (1 << index) != 0 {
            "true"
        } else {
            "false"
        };
        game.native_set_block_property(pos, name, value);
    }
    with_core(|core| core.connections.insert(key, bits));
}

/// A drive's tick: mirror its disk items into the cache, or write a change the
/// grid made back into the disk's component.
fn drive_tick(game: &mut crate::game::Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    let dimension = game.native_dimension().to_owned();
    for slot in 0..crate::game::refinedstorage::DRIVE_SLOTS {
        let key = (dimension.clone(), pos, slot);
        let disk = entity
            .items
            .get(slot)
            .and_then(|held| held.as_ref())
            .filter(|held| is_disk(held))
            .cloned();
        let Some(mut item) = disk else {
            with_core(|core| {
                core.disks.remove(&key);
                core.dirty.remove(&key);
            });
            continue;
        };
        let dirty = with_core(|core| core.dirty.remove(&key));
        if dirty {
            let cached = with_core(|core| core.disks.get(&key).cloned());
            if let Some(cached) = cached {
                write_disk(game, &mut item, &cached);
                entity.items[slot] = Some(item);
            }
        } else {
            let cached = read_disk(game, &item);
            with_core(|core| core.disks.insert(key, cached));
        }
    }
}

/// The view slots a grid last put in its menu, and which of them hold a stack
/// the network refused (the player's own until they empty it).
struct ViewState {
    slots: Vec<Option<ItemStack>>,
    owned: Vec<bool>,
    initialised: bool,
}

/// The saved compound key the grid's view marks live under.
fn view_key() -> JavaString {
    JavaString::from("RefinedStorageView")
}

fn view_state(entity: &ModBlockEntity) -> ViewState {
    let mut state = ViewState {
        slots: vec![None; GRID_SLOTS],
        owned: vec![false; GRID_SLOTS],
        initialised: false,
    };
    if let Some(Tag::List(marks)) = entity.nbt.get(&view_key()) {
        state.initialised = true;
        for (index, mark) in marks.iter().enumerate().take(GRID_SLOTS) {
            let Tag::String(text) = mark else { continue };
            let text = text.to_string_lossy();
            let (owned, body) = match text.strip_prefix('o') {
                Some(body) => (true, body),
                None => (false, text.strip_prefix('n').unwrap_or(&text)),
            };
            state.owned[index] = owned;
            if body.is_empty() {
                continue;
            }
            let Ok((tag, _)) = horizon_world::nbt::parse_snbt(body) else {
                continue;
            };
            state.slots[index] = horizon_client::save::item_from_nbt(&tag).filter(|s| s.count > 0);
        }
    }
    state
}

fn save_view(entity: &mut ModBlockEntity, slots: &[Option<ItemStack>], owned: &[bool]) {
    let marks = (0..GRID_SLOTS)
        .map(|index| {
            let mark = slots
                .get(index)
                .and_then(|slot| slot.as_ref())
                .filter(|stack| stack.count > 0)
                .map(|stack| {
                    let prefix = if owned.get(index).copied().unwrap_or(false) {
                        'o'
                    } else {
                        'n'
                    };
                    format!(
                        "{prefix}{}",
                        horizon_world::snbt_text::to_snbt(
                            &horizon_client::save::item_to_nbt(stack)
                        )
                    )
                })
                .unwrap_or_default();
            Tag::String(JavaString::from(mark.as_str()))
        })
        .collect();
    entity.nbt.insert(view_key(), Tag::List(marks));
}

/// A grid's tick: reconcile what the player did to the menu's slots with the
/// network, then show the network's resources again.
fn grid_tick(game: &mut crate::game::Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    let dimension = game.native_dimension().to_owned();
    let net = network_at(game, pos);
    let online = net.online();
    let drives: Vec<[i32; 3]> = net.of_kind(Kind::DiskDrive).collect();
    let keys: Vec<DiskKey> = drive_keys(&dimension, &drives);
    // Only the slots the drives already mirror, in the same order, so a
    // change written back lands in the disk it came from.
    let (used, mut disks): (Vec<DiskKey>, Vec<Disk<ItemStack>>) = with_core(|core| {
        keys.iter()
            .filter(|key| core.disks.contains_key(*key))
            .map(|key| (key.clone(), core.disks[key].clone()))
            .unzip()
    });

    let mut state = view_state(entity);
    let after: Vec<Option<ItemStack>> = (0..GRID_SLOTS)
        .map(|index| {
            entity
                .items
                .get(index)
                .and_then(|slot| slot.as_ref())
                .filter(|stack| stack.count > 0)
                .cloned()
        })
        .collect();
    if state.initialised {
        for delta in deltas(&state.slots, &after, &state.owned) {
            match delta {
                Delta::Withdraw { stack, .. } => {
                    // The player took it out of the menu already; take it off
                    // the network. A network that no longer holds it (a disk
                    // pulled in the same tick) keeps what it has.
                    extract(&mut disks, &stack.key(), stack.count);
                }
                Delta::Deposit { slot, stack } => {
                    let taken = if online {
                        insert(&mut disks, stack.clone())
                    } else {
                        stack.clone()
                    };
                    if taken.count() > 0 {
                        // The network could not take it (offline, or full): the
                        // stack stays in the slot as the player's own.
                        state.owned[slot] = true;
                        entity.items[slot] = Some(taken);
                    }
                }
            }
        }
    }
    // Any slot the player emptied stops being theirs.
    for (index, flag) in state.owned.iter_mut().enumerate() {
        if *flag && after[index].is_none() {
            *flag = false;
        }
    }

    // Write the network's changes back into the drives' caches.
    with_core(|core| {
        for (key, disk) in used.iter().zip(disks.iter()) {
            core.disks.insert(key.clone(), disk.clone());
            core.dirty.insert(key.clone());
        }
    });

    // Rebuild the view: the player's own stacks stay, the rest shows the
    // network, in the grid's sort order.
    let filter = with_core(|core| {
        core.filters
            .get(&(dimension.clone(), pos))
            .cloned()
            .unwrap_or_default()
    });
    let entries = if online {
        view(&disks, &filter, GRID_SLOTS, |stack| game.max_stack(stack))
    } else {
        Vec::new()
    };
    let mut slots: Vec<Option<ItemStack>> = vec![None; GRID_SLOTS];
    let mut owned = vec![false; GRID_SLOTS];
    let mut entries = entries.into_iter();
    for index in 0..GRID_SLOTS {
        if state.owned[index] && let Some(keep) = after[index].clone() {
            slots[index] = Some(keep);
            owned[index] = true;
            continue;
        }
        slots[index] = entries.next();
    }
    entity.items = slots.clone();
    save_view(entity, &slots, &owned);

    // The block state says whether the network works: an offline grid is dark.
    let previous = with_core(|core| core.active.get(&(dimension.clone(), pos)).copied());
    if previous != Some(online) {
        game.native_set_block_property(pos, "active", if online { "true" } else { "false" });
        with_core(|core| core.active.insert((dimension, pos), online));
    }
}

// --- the menus' layouts on the client --------------------------------------

/// The grid's and the drive's screens (`native_screen`, API 19). A mod menu
/// type shows on a Horizon client through this layout only, and the game
/// lists a menu's slots from it.
pub fn on_screen(block: &str, screen: &mut Option<NativeScreen>) {
    match Kind::from_block(block) {
        Some(Kind::Grid) => {
            // Nine columns by six rows, as upstream's grid shows, over the
            // vanilla 9x6 panel; the player's inventory sits below.
            let slots = (0..GRID_SLOTS)
                .map(|index| {
                    let (column, row) = (index % 9, index / 9);
                    Some([8.0 + 18.0 * column as f32, 18.0 + 18.0 * row as f32])
                })
                .collect();
            *screen = Some(NativeScreen {
                width: 176.0,
                height: 222.0,
                slots,
                inventory_y: 140.0,
                inventory_x: 8.0,
                own_labels: false,
                own_background: false,
            });
        }
        Some(Kind::DiskDrive) => {
            let slots = (0..crate::game::refinedstorage::DRIVE_SLOTS)
                .map(|index| Some([8.0 + 18.0 * index as f32, 16.0]))
                .collect();
            *screen = Some(NativeScreen {
                width: 176.0,
                height: 133.0,
                slots,
                inventory_y: 84.0,
                inventory_x: 8.0,
                own_labels: false,
                own_background: false,
            });
        }
        _ => {}
    }
}

// --- player interaction ----------------------------------------------------

/// A storage disk right-clicked onto a drive goes into its first free slot;
/// every other click is left to the block (a barrel-template block opens its
/// menu). `native_right_click_block`, API 21.
#[allow(
    private_interfaces,
    reason = "the hook helper sits one module away, inside the game crate"
)]
pub fn on_right_click_block(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    _face: [i32; 3],
    canceled: &mut bool,
) {
    if block_kind(game, pos) != Some(Kind::DiskDrive) {
        return;
    }
    let Some(held) = game.held().cloned() else {
        return;
    };
    if !is_disk(&held) {
        return;
    }
    let mut disk = held.clone();
    disk.count = 1;
    // The engine's own helper: it marks the chunk and tells comparators, as
    // the drive's `setChanged` would.
    let inserted = game
        .native_with_mod_block_entity(pos, |block_entity| {
            let slot = (0..crate::game::refinedstorage::DRIVE_SLOTS)
                .find(|slot| block_entity.items.get(*slot).is_none_or(Option::is_none))?;
            block_entity.items[slot] = Some(disk.clone());
            Some(slot)
        })
        .flatten();
    if inserted.is_none() {
        game.native_feedback(Text::literal("The disk drive is full."));
        *canceled = true;
        return;
    }
    consume_held(game, held);
    game.native_feedback(Text::literal("Storage disk inserted."));
    *canceled = true;
}

/// Replace the acting player's held stack (their state is swapped in).
fn replace_held(game: &mut crate::game::Game<'_>, stack: Option<ItemStack>) {
    let slot = game.held_slot();
    *game.held_mut() = stack;
    if game.lan_acting.is_none() {
        game.hand_stack_replaced(slot);
    }
}

/// Take one item from the acting player's held stack. A disk is moved into
/// the drive, not used, so creative mode does not keep a free copy.
fn consume_held(game: &mut crate::game::Game<'_>, mut stack: ItemStack) {
    stack.count = stack.count.saturating_sub(1);
    replace_held(game, (stack.count > 0).then_some(stack));
}

/// A world opened or closed: everything in the cache is from that world.
#[allow(
    private_interfaces,
    reason = "the hook helper sits one module away, inside the game crate"
)]
pub fn on_world_lifecycle(_game: &mut crate::game::Game<'_>, phase: WorldPhase) {
    match phase {
        WorldPhase::WorldOpenReady => {
            let mut guard = core();
            *guard = Some(Core::default());
        }
        WorldPhase::WorldCloseFinal | WorldPhase::WorldCloseFailed => {
            let mut guard = core();
            *guard = None;
        }
        _ => {}
    }
}

// --- commands --------------------------------------------------------------

/// Register the disk's item component before content loads (`component_types`,
/// API 65).
pub fn register_component_types(types: &mut Vec<String>) {
    types.push(COMPONENT.to_owned());
}

/// `/refinedstorage info|filter` (`register_commands`, API 8).
pub fn on_register_commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::new("refinedstorage info <pos:block_pos>"));
    commands.push(NativeCommand::new(
        "refinedstorage filter <pos:block_pos> <text:text>",
    ));
}

fn parse_pos(words: &[&str]) -> Option<[i32; 3]> {
    Some([
        words.first()?.parse().ok()?,
        words.get(1)?.parse().ok()?,
        words.get(2)?.parse().ok()?,
    ])
}

/// The `/refinedstorage` commands (`perform_command`, API 8).
#[allow(
    private_interfaces,
    reason = "the hook helper sits one module away, inside the game crate"
)]
pub fn on_perform_command(
    game: &mut crate::game::Game<'_>,
    _player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    let words: Vec<&str> = command.split_whitespace().collect();
    match words.as_slice() {
        ["refinedstorage", "info", x, y, z] => {
            *ran = true;
            let Some(pos) = parse_pos(&[*x, *y, *z]) else {
                game.native_feedback(Text::literal(
                    "Usage: /refinedstorage info <x> <y> <z>",
                ));
                return;
            };
            let block = block_kind(game, pos);
            let Some(_kind) = block else {
                game.native_feedback(Text::literal(
                    "No Refined Storage network block there.",
                ));
                return;
            };
            let net = network_at(game, pos);
            let dimension = game.native_dimension().to_owned();
            let drives: Vec<[i32; 3]> = net.of_kind(Kind::DiskDrive).collect();
            let (disks, items, resources) = {
                let keys: Vec<DiskKey> = drive_keys(&dimension, &drives);
                with_core(|core| {
                    let found: Vec<Disk<ItemStack>> = keys
                        .iter()
                        .filter_map(|key| core.disks.get(key).cloned())
                        .collect();
                    let items: u64 = found.iter().map(Disk::total).sum();
                    let resources = crate::game::refinedstorage::aggregate(&found).len();
                    (found.len(), items, resources)
                })
            };
            let online = net.online();
            let state = if online { "online" } else { "offline" };
            game.native_feedback(Text::literal(format!(
                "Refined Storage network at {},{},{}: {state} ({} nodes, {} controller, {} drive, {} grid); {} disks, {items} items in {resources} resources",
                pos[0],
                pos[1],
                pos[2],
                net.nodes.len(),
                net.count(Kind::Controller),
                drives.len(),
                net.count(Kind::Grid),
                disks
            )));
        }
        ["refinedstorage", "filter", x, y, z, rest @ ..] => {
            *ran = true;
            let Some(pos) = parse_pos(&[*x, *y, *z]) else {
                game.native_feedback(Text::literal(
                    "Usage: /refinedstorage filter <x> <y> <z> [text]",
                ));
                return;
            };
            if block_kind(game, pos) != Some(Kind::Grid) {
                game.native_feedback(Text::literal("No grid there."));
                return;
            }
            let filter = rest.join(" ");
            let dimension = game.native_dimension().to_owned();
            with_core(|core| {
                core.filters.retain(|(dim, at), _| !(dim == &dimension && *at == pos));
                if !filter.is_empty() {
                    core.filters.insert((dimension, pos), filter.clone());
                }
            });
            if filter.is_empty() {
                game.native_feedback(Text::literal("Grid search cleared."));
            } else {
                game.native_feedback(Text::literal(format!("Grid search: {filter}")));
            }
        }
        _ => {}
    }
}
