# Refined Storage slice 1: the content mod

`refinedstorage.hmod` carries the five items and four blocks of the slice and
their assets. It is built from this folder:

```sh
/tmp/opencode/ironfurnaces-work/cargo-horizon-mod build   # from this folder
# -> target/horizon-mod/refinedstorage.hmod (copied beside this file)
```

Built with the pin's `cargo-horizon-mod` (built from Horizon `1d63e0a5` by the
ironfurnaces job; SHA-256
`7d8ff73160073f7aa4c7c6cb3f8c1007d2f4936fd553bcfab9508896e284fb3b` for the
`horizon-port-pack` binary of the same tree). The assets are carried from the
exact ATM10 jar (see
`ports/_shared/carry-work/refinedstorage/SOURCE.md`); the blockstates and the
disk-drive/cable item and block models are this port's own because upstream
bakes them in code.

| Id | Kind | Template | Shape |
|---|---|---|---|
| `refinedstorage:controller` | block + item | `minecraft:stone` | declares `energy_type` (`off`/`on`) |
| `refinedstorage:cable` | block + item | `minecraft:stone` | declares six connection booleans, `occludes: false`, core outline, no collision |
| `refinedstorage:disk_drive` | block + item | `minecraft:barrel` (opens its menu) | 9 disk slots, `generic_9x1` menu |
| `refinedstorage:grid` | block + item | `minecraft:barrel` (opens its menu) | declares `active`, 54-slot view, `generic_9x6` menu |
| `refinedstorage:1k_storage_disk` | item | `minecraft:paper` | stack size 1, contents in `refinedstorage:storage` |

No recipes: the crafting chain (processors, quartz-enriched iron, machine
casing, cores, disk parts) and the NeoForge `#c:` tags are the next slice.

`tools/hmod-load.sh` proves the packaged `.hmod` loads on the shared unmodded
game and that the same ids are unknown without it.
