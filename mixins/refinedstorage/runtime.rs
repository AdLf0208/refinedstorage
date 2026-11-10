// Refined Storage slice 1 for Horizon: the item network core's pure model.
//
// Ported from Refined Storage 2.0.9 (refinedmods/refinedstorage2, MIT); this is
// a clean-room rewrite from the mod's observed behaviour, its data files and
// its public documentation. Local-only until the maintainer approves
// publishing (ARCHITECTURE.md §8).
//
// This module is the standalone, unit-tested core: the network's connected
// component and its on/off rule, a storage disk's capacity and contents, the
// aggregated view a grid shows, and the reconciliation of the grid's menu
// slots against the network. Everything that needs the game (block entities,
// menus, item components, commands) lives in `support.rs` and is injected as
// `crate::game::refinedstorage_state`.

/// A 1k storage disk's capacity in items (upstream `ItemStorageVariant.ONE_K`).
pub const CAPACITY_1K: u64 = 1024;
/// The grid's menu shows a 9x6 grid of aggregated entries.
pub const GRID_SLOTS: usize = 54;
/// The disk drive's slots (upstream `AMOUNT_OF_DISKS` is 8; the pin's menu
/// types only show 9/18/27/45/54 slots, so the port shows one row of nine and
/// records the difference).
pub const DRIVE_SLOTS: usize = 9;
/// The most blocks one flood fill visits; a network larger than this is cut.
pub const NETWORK_CAP: usize = 512;

/// The four network blocks of the slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Controller,
    Cable,
    DiskDrive,
    Grid,
}

impl Kind {
    /// The block id this kind is, exactly as the content mod declares it.
    pub fn block_id(self) -> &'static str {
        match self {
            Kind::Controller => "refinedstorage:controller",
            Kind::Cable => "refinedstorage:cable",
            Kind::DiskDrive => "refinedstorage:disk_drive",
            Kind::Grid => "refinedstorage:grid",
        }
    }

    /// The kind a block id stands for, or none for any other block.
    pub fn from_block(block: &str) -> Option<Kind> {
        [Kind::Controller, Kind::Cable, Kind::DiskDrive, Kind::Grid]
            .into_iter()
            .find(|kind| kind.block_id() == block)
    }
}

/// The six face neighbours of a position, in the order
/// `north, east, south, west, up, down`.
pub fn neighbours(pos: [i32; 3]) -> [[i32; 3]; 6] {
    [
        [pos[0], pos[1], pos[2] - 1],
        [pos[0] + 1, pos[1], pos[2]],
        [pos[0], pos[1], pos[2] + 1],
        [pos[0] - 1, pos[1], pos[2]],
        [pos[0], pos[1] + 1, pos[2]],
        [pos[0], pos[1] - 1, pos[2]],
    ]
}

/// One connected network: the blocks reachable from some start block through
/// face-adjacent network blocks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    pub nodes: Vec<(Kind, [i32; 3])>,
    /// True when the flood fill hit [`NETWORK_CAP`] and the list is cut.
    #[allow(
        dead_code,
        reason = "the runtime tests read it; slice 1 has no oversized network"
    )]
    pub truncated: bool,
}

impl Network {
    /// A network with a controller is online; without one nothing in it works
    /// (upstream: the controller is the network's root, and every node is
    /// inactive without it).
    pub fn online(&self) -> bool {
        self.nodes.iter().any(|(kind, _)| *kind == Kind::Controller)
    }

    /// The block positions of one kind, in visit order.
    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.nodes
            .iter()
            .filter(move |(k, _)| *k == kind)
            .map(|(_, pos)| *pos)
    }

    /// How many blocks of one kind the network holds.
    pub fn count(&self, kind: Kind) -> usize {
        self.nodes.iter().filter(|(k, _)| *k == kind).count()
    }
}

/// Flood fill the network `start` belongs to. `at` answers the kind of the
/// block at a position from the world (none for a non-network or unloaded
/// block); `start` itself must be a network block.
pub fn network(start: [i32; 3], mut at: impl FnMut([i32; 3]) -> Option<Kind>) -> Network {
    let mut out = Network::default();
    if at(start).is_none() {
        return out;
    }
    let mut seen = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    seen.insert(start);
    queue.push_back(start);
    while let Some(pos) = queue.pop_front() {
        let Some(kind) = at(pos) else {
            continue;
        };
        if out.nodes.len() >= NETWORK_CAP {
            out.truncated = true;
            break;
        }
        out.nodes.push((kind, pos));
        for next in neighbours(pos) {
            if seen.insert(next) && at(next).is_some() {
                queue.push_back(next);
            }
        }
    }
    out
}

/// What a stored entry must answer for the network to treat it as one resource.
pub trait Stored: Clone {
    /// The identity of the resource: two entries with the same key are
    /// different amounts of the same thing (the item id and its components).
    fn key(&self) -> String;
    /// How many items the entry holds.
    fn count(&self) -> u32;
    /// The same resource with another count.
    fn with_count(&self, count: u32) -> Self;
}

/// One storage disk's contents: at most `capacity` items over any number of
/// resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk<T> {
    capacity: u64,
    entries: Vec<T>,
}

impl<T: Stored> Disk<T> {
    /// An empty disk of `capacity` items.
    pub fn new(capacity: u64) -> Self {
        Disk {
            capacity,
            entries: Vec::new(),
        }
    }

    /// The disk's capacity in items.
    #[allow(dead_code, reason = "the commands report it; the tick reads `free`")]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Everything the disk holds, one entry per resource, in insertion order.
    pub fn entries(&self) -> &[T] {
        &self.entries
    }

    /// Replace the contents (a disk read back from its item component).
    pub fn set_entries(&mut self, entries: Vec<T>) {
        self.entries = entries;
        self.trim();
    }

    /// The items the disk holds.
    pub fn total(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| u64::from(entry.count()))
            .sum()
    }

    /// The items the disk can still take.
    pub fn free(&self) -> u64 {
        self.capacity.saturating_sub(self.total())
    }

    /// How many of `key` the disk holds.
    #[allow(
        dead_code,
        reason = "the runtime tests read it; the grid aggregates instead"
    )]
    pub fn amount(&self, key: &str) -> u64 {
        self.entries
            .iter()
            .filter(|entry| entry.key() == key)
            .map(|entry| u64::from(entry.count()))
            .sum()
    }

    /// Insert `stack` and answer what did not fit (a zero-count stack when all
    /// of it did).
    pub fn insert(&mut self, stack: T) -> T {
        let mut left = stack.count();
        if left == 0 {
            return stack;
        }
        let key = stack.key();
        let mut room = self.free();
        for entry in &mut self.entries {
            if left == 0 || room == 0 {
                break;
            }
            if entry.key() != key {
                continue;
            }
            let moved = room.min(u64::from(left));
            if moved > 0 {
                *entry = entry.with_count(entry.count() + moved as u32);
                left -= moved as u32;
                room -= moved;
            }
        }
        if left > 0 && room > 0 && self.entries.len() < MAX_RESOURCES {
            let moved = room.min(u64::from(left)) as u32;
            self.entries.push(stack.with_count(moved));
            left -= moved;
        }
        stack.with_count(left)
    }

    /// Take up to `want` of `key`, answering the stack taken (none when the
    /// disk does not hold it).
    pub fn extract(&mut self, key: &str, want: u32) -> Option<T> {
        let mut taken: Option<T> = None;
        let mut left = want;
        let mut index = 0;
        while index < self.entries.len() && left > 0 {
            if self.entries[index].key() == key {
                let have = self.entries[index].count();
                let moved = have.min(left);
                let entry = self.entries[index].clone();
                taken = Some(match taken {
                    Some(previous) => previous.with_count(previous.count() + moved),
                    None => entry.with_count(moved),
                });
                left -= moved;
                if moved == have {
                    self.entries.remove(index);
                    continue;
                }
                self.entries[index] = self.entries[index].with_count(have - moved);
            }
            index += 1;
        }
        taken
    }

    /// Empty the disk, answering what it held.
    #[allow(
        dead_code,
        reason = "the port's API; slice 1 empties a disk only by taking items"
    )]
    pub fn drain(&mut self) -> Vec<T> {
        std::mem::take(&mut self.entries)
    }

    /// Drop empty entries and merge entries of one resource; keep capacity.
    fn trim(&mut self) {
        let mut merged: Vec<T> = Vec::new();
        for entry in std::mem::take(&mut self.entries) {
            if entry.count() == 0 {
                continue;
            }
            let key = entry.key();
            match merged.iter_mut().find(|kept| kept.key() == key) {
                Some(kept) => *kept = kept.with_count(kept.count().saturating_add(entry.count())),
                None => merged.push(entry),
            }
        }
        let mut room = self.capacity;
        merged.retain_mut(|entry| {
            let kept = u64::from(entry.count()).min(room);
            *entry = entry.with_count(kept as u32);
            room -= kept;
            kept > 0
        });
        self.entries = merged;
    }
}

/// Insert into the first disk that can take the whole stack, answering what
/// did not fit. A disk takes a whole stack or nothing, so a network never
/// spreads one stack over two disks (upstream inserts into one storage at a
/// time).
pub fn insert<T: Stored>(disks: &mut [Disk<T>], stack: T) -> T {
    let count = stack.count();
    if count == 0 {
        return stack;
    }
    let key = stack.key();
    if !disks
        .iter()
        .any(|disk| disk.free() >= u64::from(count) && disk.fits(&key))
    {
        return stack;
    }
    for disk in disks {
        if disk.free() < u64::from(count) || !disk.fits(&key) {
            continue;
        }
        return disk.insert(stack);
    }
    stack
}

impl<T: Stored> Disk<T> {
    /// Whether the disk can hold more of `key`: it already does, or it has
    /// room for a new resource (one entry per disk resource, so a disk that
    /// holds `NETWORK` resources does not accept another).
    fn fits(&self, key: &str) -> bool {
        self.entries.iter().any(|entry| entry.key() == key) || self.entries.len() < MAX_RESOURCES
    }
}

/// The most distinct resources one disk tracks (it mirrors the number of
/// entries a menu can show, and keeps a disk's item component small).
pub const MAX_RESOURCES: usize = 256;

/// Take up to `want` of `key` from the disks, answering the stack taken.
pub fn extract<T: Stored>(disks: &mut [Disk<T>], key: &str, want: u32) -> Option<T> {
    let mut left = want;
    let mut taken: Option<T> = None;
    for disk in disks {
        if left == 0 {
            break;
        }
        if let Some(part) = disk.extract(key, left) {
            left -= part.count();
            taken = Some(match taken {
                Some(previous) => previous.with_count(previous.count() + part.count()),
                None => part,
            });
        }
    }
    taken
}

/// How many of `key` the disks hold together.
#[allow(
    dead_code,
    reason = "the runtime tests read it; `info` sums the disks instead"
)]
pub fn amount<T: Stored>(disks: &[Disk<T>], key: &str) -> u64 {
    disks.iter().map(|disk| disk.amount(key)).sum()
}

/// Every resource the disks hold, one entry per resource with its full total.
pub fn aggregate<T: Stored>(disks: &[Disk<T>]) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for disk in disks {
        for entry in disk.entries() {
            let key = entry.key();
            match out.iter_mut().find(|kept| kept.key() == key) {
                Some(kept) => *kept = kept.with_count(kept.count().saturating_add(entry.count())),
                None => out.push(entry.clone()),
            }
        }
    }
    out
}

/// Whether a resource's key matches a grid search: a case-insensitive
/// substring of the item id (`minecraft:diamond` matches `diamond`, `MI`,
/// `minecraft`).
pub fn matches(key: &str, search: &str) -> bool {
    let search = search.trim().to_ascii_lowercase();
    search.is_empty() || key.to_ascii_lowercase().contains(&search)
}

/// The grid's view of the network: one entry per matching resource, sorted by
/// amount ascending and then by identity (upstream's default grid sorting is
/// quantity, ascending), each entry capped to one stack so it fits a menu
/// slot. `max_stack` says how many items one stack of a resource holds in
/// this game (64, 16, 1...).
pub fn view<T: Stored>(
    disks: &[Disk<T>],
    search: &str,
    slots: usize,
    max_stack: impl Fn(&T) -> u32,
) -> Vec<T> {
    let mut entries: Vec<T> = aggregate(disks)
        .into_iter()
        .filter(|entry| matches(&entry.key(), search))
        .collect();
    entries.sort_by(|a, b| {
        a.count()
            .cmp(&b.count())
            .then_with(|| a.key().cmp(&b.key()))
    });
    entries.truncate(slots);
    entries
        .into_iter()
        .map(|entry| {
            let cap = max_stack(&entry).max(1);
            entry.with_count(entry.count().min(cap))
        })
        .collect()
}

/// What a player's click did to one grid slot, as the next tick sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delta<T> {
    /// The player took this much out of the slot: take it off the network.
    Withdraw { slot: usize, stack: T },
    /// The player put this much into the slot: insert it into the network.
    Deposit { slot: usize, stack: T },
}

/// Compare a grid's slots with what the port last put there and answer the
/// network operations a player's clicks and drags became. Only slots the port
/// owns are compared: a slot it marked `owned` (a stack the network could not
/// take) belongs to the player until they empty it.
pub fn deltas<T: Stored>(
    before: &[Option<T>],
    after: &[Option<T>],
    owned: &[bool],
) -> Vec<Delta<T>> {
    let mut out = Vec::new();
    for index in 0..before.len().max(after.len()) {
        if owned.get(index).copied().unwrap_or(false) {
            continue;
        }
        let old = before.get(index).and_then(|slot| slot.as_ref());
        let new = after.get(index).and_then(|slot| slot.as_ref());
        match (old, new) {
            (None, None) => {}
            (Some(old), None) => out.push(Delta::Withdraw {
                slot: index,
                stack: old.clone(),
            }),
            (None, Some(new)) => out.push(Delta::Deposit {
                slot: index,
                stack: new.clone(),
            }),
            (Some(old), Some(new)) if old.key() == new.key() => match new.count().cmp(&old.count())
            {
                std::cmp::Ordering::Less => out.push(Delta::Withdraw {
                    slot: index,
                    stack: old.with_count(old.count() - new.count()),
                }),
                std::cmp::Ordering::Greater => out.push(Delta::Deposit {
                    slot: index,
                    stack: new.with_count(new.count() - old.count()),
                }),
                std::cmp::Ordering::Equal => {}
            },
            (Some(old), Some(new)) => {
                out.push(Delta::Withdraw {
                    slot: index,
                    stack: old.clone(),
                });
                out.push(Delta::Deposit {
                    slot: index,
                    stack: new.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test resource: `key` is the item id, counts free-form.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestItem {
        id: &'static str,
        count: u32,
    }

    impl TestItem {
        fn new(id: &'static str, count: u32) -> Self {
            TestItem { id, count }
        }
    }

    impl Stored for TestItem {
        fn key(&self) -> String {
            self.id.to_owned()
        }
        fn count(&self) -> u32 {
            self.count
        }
        fn with_count(&self, count: u32) -> Self {
            TestItem { id: self.id, count }
        }
    }

    /// One stack of a test item holds 64.
    fn stack_of(_entry: &TestItem) -> u32 {
        64
    }

    fn disk_with(id: &'static str, count: u32) -> Disk<TestItem> {
        let mut disk = Disk::new(CAPACITY_1K);
        assert_eq!(disk.insert(TestItem::new(id, count)).count(), 0);
        disk
    }

    // --- network -----------------------------------------------------------

    #[test]
    fn kind_maps_the_four_blocks_and_nothing_else() {
        assert_eq!(
            Kind::from_block("refinedstorage:controller"),
            Some(Kind::Controller)
        );
        assert_eq!(Kind::from_block("refinedstorage:cable"), Some(Kind::Cable));
        assert_eq!(
            Kind::from_block("refinedstorage:disk_drive"),
            Some(Kind::DiskDrive)
        );
        assert_eq!(Kind::from_block("refinedstorage:grid"), Some(Kind::Grid));
        assert_eq!(Kind::from_block("minecraft:stone"), None);
        assert_eq!(Kind::from_block("refinedstorage:importer"), None);
        assert_eq!(Kind::Controller.block_id(), "refinedstorage:controller");
    }

    #[test]
    fn a_controller_cable_drive_and_grid_are_one_online_network() {
        let nodes = [
            (Kind::Controller, [0, 64, 0]),
            (Kind::Cable, [1, 64, 0]),
            (Kind::DiskDrive, [2, 64, 0]),
            (Kind::Grid, [3, 64, 0]),
        ];
        let net = network(nodes[1].1, |pos| {
            nodes
                .iter()
                .find(|(_, at)| *at == pos)
                .map(|(kind, _)| *kind)
        });
        assert_eq!(net.nodes.len(), 4);
        assert!(net.online());
        assert_eq!(net.count(Kind::Cable), 1);
        assert_eq!(
            net.of_kind(Kind::DiskDrive).collect::<Vec<_>>(),
            vec![[2, 64, 0]]
        );
    }

    #[test]
    fn a_network_without_a_controller_is_offline() {
        let nodes = [
            (Kind::Cable, [0, 64, 0]),
            (Kind::DiskDrive, [1, 64, 0]),
            (Kind::Grid, [2, 64, 0]),
        ];
        let net = network([0, 64, 0], |pos| {
            nodes
                .iter()
                .find(|(_, at)| *at == pos)
                .map(|(kind, _)| *kind)
        });
        assert_eq!(net.nodes.len(), 3);
        assert!(!net.online(), "no controller means no network");
    }

    #[test]
    fn a_gap_or_a_diagonal_disconnects_a_network() {
        let nodes = [
            (Kind::Controller, [0, 64, 0]),
            (Kind::Grid, [2, 64, 0]),
            (Kind::DiskDrive, [1, 65, 0]),
        ];
        let at = |pos| {
            nodes
                .iter()
                .find(|(_, at)| *at == pos)
                .map(|(kind, _)| *kind)
        };
        let net = network([0, 64, 0], at);
        assert_eq!(net.nodes, vec![(Kind::Controller, [0, 64, 0])]);
        assert!(net.online());
        assert_eq!(
            net.count(Kind::Grid),
            0,
            "one block gap is not a connection"
        );
        let other = network([2, 64, 0], at);
        assert!(!other.online(), "a lone grid is its own offline network");
    }

    #[test]
    fn six_faces_are_the_neighbours() {
        assert_eq!(
            neighbours([0, 0, 0]),
            [
                [0, 0, -1],
                [1, 0, 0],
                [0, 0, 1],
                [-1, 0, 0],
                [0, 1, 0],
                [0, -1, 0]
            ]
        );
    }

    #[test]
    fn the_flood_fill_stops_at_the_cap() {
        // A wall of `NETWORK_CAP + 8` cables: the fill reports the cut.
        let at = |pos: [i32; 3]| {
            (pos[1] == 0 && pos[2] == 0 && (0..NETWORK_CAP as i32 + 8).contains(&pos[0]))
                .then_some(Kind::Cable)
        };
        let net = network([0, 0, 0], at);
        assert_eq!(net.nodes.len(), NETWORK_CAP);
        assert!(net.truncated);
    }

    // --- one disk ----------------------------------------------------------

    #[test]
    fn a_disk_takes_items_up_to_its_capacity() {
        let mut disk = Disk::<TestItem>::new(CAPACITY_1K);
        assert_eq!(disk.free(), 1024);
        assert_eq!(
            disk.insert(TestItem::new("minecraft:stone", 1000)).count(),
            0
        );
        assert_eq!(disk.total(), 1000);
        let left = disk.insert(TestItem::new("minecraft:dirt", 64));
        assert_eq!(left.count(), 40, "1024 - 1000 leaves 24 free");
        assert_eq!(disk.total(), 1024);
        assert_eq!(disk.free(), 0);
        assert_eq!(disk.amount("minecraft:stone"), 1000);
        assert_eq!(disk.amount("minecraft:dirt"), 24);
    }

    #[test]
    fn one_resource_accumulates_and_empty_entries_disappear() {
        let mut disk = disk_with("minecraft:diamond", 10);
        assert_eq!(
            disk.insert(TestItem::new("minecraft:diamond", 5)).count(),
            0
        );
        assert_eq!(disk.entries().len(), 1);
        assert_eq!(disk.amount("minecraft:diamond"), 15);
        let taken = disk.extract("minecraft:diamond", 15).expect("held");
        assert_eq!(taken.count(), 15);
        assert!(disk.entries().is_empty(), "an emptied resource is gone");
        assert!(disk.extract("minecraft:diamond", 1).is_none());
    }

    #[test]
    fn extraction_is_partial_when_the_disk_is_short() {
        let mut disk = disk_with("minecraft:diamond", 7);
        let taken = disk.extract("minecraft:diamond", 64).expect("held");
        assert_eq!(taken.count(), 7);
        assert!(disk.entries().is_empty());
    }

    #[test]
    fn a_disk_keeps_a_bounded_number_of_resources() {
        let mut disk = Disk::<TestItem>::new(CAPACITY_1K);
        for index in 0..MAX_RESOURCES {
            let id: &'static str = Box::leak(format!("test:item{index}").into_boxed_str());
            assert_eq!(disk.insert(TestItem::new(id, 1)).count(), 0);
        }
        assert_eq!(disk.entries().len(), MAX_RESOURCES);
        let leftover = disk.insert(TestItem::new("test:one_too_many", 1));
        assert_eq!(leftover.count(), 1, "a full disk refuses a new resource");
    }

    #[test]
    fn a_saved_disk_never_exceeds_its_capacity() {
        let mut disk = Disk::<TestItem>::new(16);
        disk.set_entries(vec![
            TestItem::new("minecraft:stone", 10),
            TestItem::new("minecraft:dirt", 10),
            TestItem::new("minecraft:air", 0),
        ]);
        assert_eq!(
            disk.total(),
            16,
            "a hostile item component is cut to capacity"
        );
        assert_eq!(disk.entries().len(), 2);
    }

    // --- a network of disks ------------------------------------------------

    #[test]
    fn inserting_uses_one_disk_that_fits_the_whole_stack() {
        let mut disks = [disk_with("minecraft:stone", 1000), Disk::new(CAPACITY_1K)];
        let leftover = insert(&mut disks, TestItem::new("minecraft:dirt", 64));
        assert_eq!(leftover.count(), 0);
        assert_eq!(
            disks[0].amount("minecraft:dirt"),
            0,
            "the full disk is skipped"
        );
        assert_eq!(disks[1].amount("minecraft:dirt"), 64);
    }

    #[test]
    fn inserting_refuses_when_no_disk_has_room() {
        let mut disks = [disk_with("minecraft:stone", CAPACITY_1K as u32)];
        let leftover = insert(&mut disks, TestItem::new("minecraft:dirt", 1));
        assert_eq!(leftover.count(), 1);
        assert_eq!(disks[0].total(), 1024);
    }

    #[test]
    fn extraction_crosses_disks_and_stops_when_short() {
        let mut disks = [
            disk_with("minecraft:diamond", 10),
            disk_with("minecraft:diamond", 10),
        ];
        let taken = extract(&mut disks, "minecraft:diamond", 64).expect("held");
        assert_eq!(taken.count(), 20);
        assert_eq!(amount(&disks, "minecraft:diamond"), 0);
        assert!(extract(&mut disks, "minecraft:diamond", 1).is_none());
    }

    #[test]
    fn aggregation_sums_a_resource_over_disks() {
        let disks = [
            disk_with("minecraft:diamond", 10),
            disk_with("minecraft:diamond", 5),
            disk_with("minecraft:gold_ingot", 3),
        ];
        let mut all = aggregate(&disks);
        all.sort_by_key(|entry| entry.id);
        assert_eq!(
            all,
            vec![
                TestItem::new("minecraft:diamond", 15),
                TestItem::new("minecraft:gold_ingot", 3)
            ]
        );
        assert_eq!(amount(&disks, "minecraft:diamond"), 15);
    }

    // --- the grid's view ---------------------------------------------------

    #[test]
    fn the_view_sorts_by_amount_and_then_id() {
        let disks = [
            disk_with("minecraft:diamond", 10),
            disk_with("minecraft:gold_ingot", 10),
            disk_with("minecraft:dirt", 5),
        ];
        let view = view(&disks, "", GRID_SLOTS, stack_of);
        assert_eq!(
            view,
            vec![
                TestItem::new("minecraft:dirt", 5),
                TestItem::new("minecraft:diamond", 10),
                TestItem::new("minecraft:gold_ingot", 10)
            ]
        );
    }

    #[test]
    fn the_view_caps_a_slot_to_one_stack_but_keeps_the_total() {
        let disks = [disk_with("minecraft:diamond", 100)];
        let view = view(&disks, "", GRID_SLOTS, stack_of);
        assert_eq!(view, vec![TestItem::new("minecraft:diamond", 64)]);
        assert_eq!(amount(&disks, "minecraft:diamond"), 100);
    }

    #[test]
    fn the_search_is_a_case_insensitive_substring_of_the_id() {
        let disks = [
            disk_with("minecraft:diamond", 10),
            disk_with("minecraft:diamond_block", 1),
            disk_with("refinedstorage:1k_storage_disk", 1),
        ];
        let ids: Vec<&str> = view(&disks, "DIAMOND", GRID_SLOTS, stack_of)
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(
            ids,
            vec!["minecraft:diamond_block", "minecraft:diamond"],
            "smallest amount first, `_block` sorts before the bare id"
        );
        assert!(
            view(&disks, "storage", GRID_SLOTS, stack_of)
                .iter()
                .all(|entry| entry.id.contains("storage"))
        );
        assert!(view(&disks, "nothing_matches", GRID_SLOTS, stack_of).is_empty());
        assert!(
            matches("minecraft:diamond", "  "),
            "an empty search lists all"
        );
    }

    #[test]
    fn the_view_shows_at_most_the_slots_it_has() {
        let mut disks = vec![Disk::new(CAPACITY_1K * 4)];
        for index in 0..GRID_SLOTS + 5 {
            let id: &'static str = Box::leak(format!("test:item{index}").into_boxed_str());
            disks[0].insert(TestItem::new(id, 1));
        }
        assert_eq!(view(&disks, "", GRID_SLOTS, stack_of).len(), GRID_SLOTS);
        assert_eq!(view(&disks, "", 10, stack_of).len(), 10);
    }

    // --- grid slot reconciliation -----------------------------------------

    #[test]
    fn taking_a_view_stack_withdraws_it_from_the_network() {
        let before = [Some(TestItem::new("minecraft:diamond", 64))];
        let after = [None];
        assert_eq!(
            deltas(&before, &after, &[false]),
            vec![Delta::Withdraw {
                slot: 0,
                stack: TestItem::new("minecraft:diamond", 64)
            }]
        );
    }

    #[test]
    fn a_partial_take_withdraws_only_the_difference() {
        let before = [Some(TestItem::new("minecraft:diamond", 64))];
        let after = [Some(TestItem::new("minecraft:diamond", 4))];
        assert_eq!(
            deltas(&before, &after, &[false]),
            vec![Delta::Withdraw {
                slot: 0,
                stack: TestItem::new("minecraft:diamond", 60)
            }]
        );
    }

    #[test]
    fn placing_a_stack_into_an_empty_view_slot_deposits_it() {
        let before = [None];
        let after = [Some(TestItem::new("minecraft:dirt", 10))];
        assert_eq!(
            deltas(&before, &after, &[false]),
            vec![Delta::Deposit {
                slot: 0,
                stack: TestItem::new("minecraft:dirt", 10)
            }]
        );
    }

    #[test]
    fn adding_to_a_view_stack_deposits_the_difference() {
        let before = [Some(TestItem::new("minecraft:dirt", 10))];
        let after = [Some(TestItem::new("minecraft:dirt", 12))];
        assert_eq!(
            deltas(&before, &after, &[false]),
            vec![Delta::Deposit {
                slot: 0,
                stack: TestItem::new("minecraft:dirt", 2)
            }]
        );
    }

    #[test]
    fn swapping_a_view_stack_withdraws_then_deposits() {
        let before = [Some(TestItem::new("minecraft:diamond", 3))];
        let after = [Some(TestItem::new("minecraft:dirt", 5))];
        assert_eq!(
            deltas(&before, &after, &[false]),
            vec![
                Delta::Withdraw {
                    slot: 0,
                    stack: TestItem::new("minecraft:diamond", 3)
                },
                Delta::Deposit {
                    slot: 0,
                    stack: TestItem::new("minecraft:dirt", 5)
                }
            ]
        );
    }

    #[test]
    fn an_unchanged_slot_is_no_work() {
        let before = [Some(TestItem::new("minecraft:diamond", 64)), None];
        let after = before.clone();
        assert!(deltas(&before, &after, &[false, false]).is_empty());
    }

    #[test]
    fn a_slot_the_network_could_not_take_is_the_player_s() {
        let before = [Some(TestItem::new("minecraft:dirt", 10))];
        let after = [None];
        assert!(
            deltas(&before, &after, &[true]).is_empty(),
            "emptying an owned slot does not take dirt off the network"
        );
    }

    #[test]
    fn a_fresh_grid_with_items_but_no_memory_does_not_deposit_them() {
        // The first tick after a load passes an empty `before`: the view is
        // rebuilt rather than deposited, so the caller must not use `deltas`
        // then. This pins the contract: equal-length slices only.
        let before: [Option<TestItem>; 0] = [];
        let after = [Some(TestItem::new("minecraft:dirt", 10))];
        assert_eq!(
            deltas(&before, &after, &[]),
            vec![Delta::Deposit {
                slot: 0,
                stack: TestItem::new("minecraft:dirt", 10)
            }],
            "an empty `before` is a deposit, which is why the glue skips the first tick"
        );
    }

    // --- whole flow --------------------------------------------------------

    #[test]
    fn a_grid_view_round_trip_conserves_items() {
        let mut disks = [disk_with("minecraft:diamond", 100)];
        // The view shows one capped stack.
        let shown = view(&disks, "", GRID_SLOTS, stack_of);
        // The player shift-clicks all 64 out of slot 0.
        let before = [Some(shown[0].clone())];
        let after: [Option<TestItem>; 1] = [None];
        for delta in deltas(&before, &after, &[false]) {
            if let Delta::Withdraw { stack, .. } = delta {
                let taken = extract(&mut disks, &stack.key(), stack.count()).expect("held");
                assert_eq!(taken.count(), 64);
            }
        }
        assert_eq!(amount(&disks, "minecraft:diamond"), 36);
        assert_eq!(view(&disks, "", GRID_SLOTS, stack_of)[0].count(), 36);
        // Depositing 36 back fills the disk to 72.
        let before = [Some(view(&disks, "", GRID_SLOTS, stack_of)[0].clone())];
        let after = [Some(TestItem::new("minecraft:diamond", 72))];
        for delta in deltas(&before, &after, &[false]) {
            if let Delta::Deposit { stack, .. } = delta {
                assert_eq!(insert(&mut disks, stack).count(), 0);
            }
        }
        assert_eq!(amount(&disks, "minecraft:diamond"), 72);
    }
}
