# refinedstorage (Horizon port)

A Horizon port of [refinedstorage](refinedmods/refinedstorage2) — MIT.

**Status: first slices.** What is below is what the port actually does today.

## Implemented so far

- Network** (`runtime::network`): the connected component of the four network
- blocks over face-adjacent positions, capped at 512 blocks. A component with a
- controller is **online**; without one nothing works (upstream: the controller
- is the network's root and every node is inactive without it). The grid's
- `active` block state and the `/refinedstorage info` line both read it.
- Cable connections** (upstream `computeConnections`): the block declares six
- boolean state properties (`north/east/south/west/up/down`), and the cable's
- tick writes each one from whether its neighbour is a network block. That is

## Not yet ported

- No energy.** The controller is the network's on/off root but consumes and
- stores nothing; its `energy_type` state has two values (`off`/`on`) where
- upstream has four energy levels. No FE hook is needed or used.
- The rest of the mod does not exist**: crafters, exporters/importers,
- external storage, interfaces, relays, network transmitters/receivers,
- security manager, wireless transmitter/grid, portable grid, crafting/pattern
- grids, autocrafting, fluid storage and disks, the other disk sizes (4k/16k/
- k/creative), storage blocks, upgrades, the config file, and the coloured

## Install

Build the bundle with a native-mod capable launcher, or add the file from the
registry once it is listed. Pinned to Horizon `1d63e0a` and horizon-mixins
`5a64af93` for game 26.3.

## Build from source

```sh
cargo test
python3 tools/horizon.py check --horizon <horizon checkout>
python3 tools/horizon.py build --horizon <horizon checkout>
python3 tools/package.py
```

## Credits and licence

Original mod: refinedmods/refinedstorage2. Port: AdLf0208. Licence: MIT — see [LICENSE](LICENSE);
the original notices are preserved.
