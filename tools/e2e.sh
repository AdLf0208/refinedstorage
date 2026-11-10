#!/usr/bin/env bash
# In-game check for Refined Storage slice 1: the item network core.
#
# The End arena (stone floor at y=63, the network at y=64):
#   controller 0,64,0 - cable 1,64,0 - disk drive 2,64,0 - grid 3,64,0
# Every mod command is typed with `ctl chat "/cmd"` because `ctl command` runs
# the console path and bypasses the native `perform_command` hook.
#
# Default run:
#  - the cable's six connection bits follow the blocks beside it, and the grid
#    reports `active=true` while its network has a controller;
#  - the grid opens a 9x6 menu (54 container slots) that is empty with no disk;
#  - a storage disk right-clicked onto the drive goes into it (and leaves the
#    hand), and the network then reports 1 disk;
#  - 5 diamonds and 3 gold deposited *through the grid's menu* are in the
#    network: the grid lists both and `/refinedstorage info` totals 8 items in
#    2 resources;
#  - the search filters the listing (`gold` lists only the gold), and clearing
#    it lists both again;
#  - a shift-click on the diamond view slot withdraws 5 into the inventory and
#    the network totals 3 items in 1 resource;
#  - a broken controller takes the network offline (the grid lists nothing while
#    the disk still holds the gold), and putting it back lists the gold again;
#  - an online network with no disk drive lists nothing while the first grid
#    still lists its gold (the listing comes from the drive's disk);
#  - a stick right-clicked onto the drive is not a disk and does not enter it;
#  - a vanilla barrel is not a network block (and `/refinedstorage info` says
#    so);
#  - the disk keeps its contents across `save` + relaunch.
#
# SELFTEST=1 flips the "offline lists nothing" assertion (it must FAIL), which
# proves the scenario can fail.
set -euo pipefail
export LC_ALL=C

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/refinedstorage
CTL=${CTL:-$PORT/target/horizon-cargo/fast/horizon-ctl}
GAME=${GAME:-$PORT/target/horizon-cargo/fast/horizon-client}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
SOCK=${SOCK:-/tmp/opencode/refinedstorage-e2e.sock}
SELFTEST=${SELFTEST:-0}
KEEP=${KEEP:-0}
WORLD=RefinedStorageE2E
# EMBEDDED=1 launches the native bundle alone (no --dev-mod).
EMBEDDED=${EMBEDDED:-0}
DEV_ARGS=()
[ "$EMBEDDED" = 1 ] || DEV_ARGS=(--dev-mod "$PORT/content")

[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }
[ -d "$RES/assets" ] || { echo "missing prepared resources: $RES" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/refinedstorage-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"

ctl() { "$CTL" --socket "$SOCK" "$@"; }
say() { ctl chat "/$1" >/dev/null; }
fail() { echo "FAIL: $*" >&2; ctl logs --lines 60 >&2 || true; exit 1; }

# The non-null slots of the container at a position, as JSON.
slots() { ctl container "$1" "$2" "$3" | jq -c '[.slots[] | select(. != null)]'; }
# The open menu's container slots (the grid's view, the drive's disks).
view() { ctl screen | jq -c '[.slots[] | select(.slot >= 300 and .item != null)]'; }
count_of() { # count_of ITEM
    ctl inventory | jq -r --arg item "$1" \
        '[((.hotbar // [])[] | select(.item == $item) | .count), ((.main // [])[] | select(.item == $item) | .count)] | add // 0'
}
# The first player-inventory slot (0-35 in a container menu) holding ITEM.
inv_slot() { # inv_slot ITEM
    ctl screen | jq -r --arg item "$1" \
        '[.slots[] | select(.slot < 36 and .item == $item)][0].slot // empty'
}
expect_slots() { # expect_slots JSON
    local got want
    got=$(view | jq -S -c .)
    want=$(jq -S -c . <<<"$1")
    [ "$got" = "$want" ] || fail "the grid's view is $got, expected $want"
    echo "  grid view: $got"
}
info() { # info X Y Z: the /refinedstorage info line
    say "refinedstorage info $1 $2 $3"
    ctl wait 4 >/dev/null
    ctl messages 6 | jq -r '.chat[]' | grep -m1 -E 'Refined Storage network|No Refined Storage' || true
}
expect_info() { # expect_info X Y Z TEXT
    local line
    line=$(info "$1" "$2" "$3")
    echo "  info($1,$2,$3): $line"
    case "$line" in
        *"$4"*) ;;
        *) fail "expected /refinedstorage info to say '$4', got '$line'" ;;
    esac
}
block() { ctl block "$1" "$2" "$3" | jq -r '.state'; }
expect_state() { # expect_state X Y Z TEXT
    local got
    got=$(block "$1" "$2" "$3")
    case "$got" in
        *"$4"*) ;;
        *) fail "the block at $1,$2,$3 is $got, expected $4" ;;
    esac
    echo "  block($1,$2,$3): $got"
}

cleanup() {
    local status=$?
    ctl quit >/dev/null 2>&1 || true
    sleep 1
    if [ "$KEEP" = 1 ]; then
        echo "kept scratch: $SCRATCH" >&2
    else
        rm -rf "$SCRATCH" 2>/dev/null || true
    fi
    echo "EXIT:$status"
}
trap cleanup EXIT

launch() {
    rm -f "$SOCK"
    cd "$SCRATCH"
    "$CTL" --socket "$SOCK" launch \
        --game "$GAME" --world "$WORLD" --seed 20261105 --creative --offscreen --wait \
        --resources "$RES" "${DEV_ARGS[@]}" >/dev/null
    ctl wait-ready 120
    if [ "$EMBEDDED" = 1 ]; then
        cache=$SCRATCH/.horizon-cache/refinedstorage-embedded-content.hmod
        [ -f "$cache" ] || fail "the bundle did not write its embedded content to $cache"
        cmp -s "$cache" "$PORT/content/refinedstorage.hmod" \
            || fail "the embedded content differs from content/refinedstorage.hmod"
        echo "  embedded content: $(wc -c <"$cache" | tr -d ' ') B, identical to content/refinedstorage.hmod"
    fi
    ctl wait 40
}

# The first launch only: a stone floor and a clear volume in The End.
arena() {
    ctl command "execute in minecraft:the_end run tp @s 40.5 64 3.5"
    local loaded=false
    for _ in $(seq 1 120); do
        ctl wait 5
        if [ "$(ctl block 40 63 0 | jq -r '.loaded // false')" = true ]; then
            loaded=true
            break
        fi
    done
    [ "$loaded" = true ] || fail "the End arena chunk never loaded"
    ctl command "execute in minecraft:the_end run fill 32 63 -8 56 63 24 minecraft:stone"
    ctl command "execute in minecraft:the_end run fill 32 64 -8 56 74 24 minecraft:air"
    ctl command "execute in minecraft:the_end run kill @e[type=item]"
    ctl wait 20
}

echo "== launch (The End arena, content + native bundle) =="
launch
arena

echo "== the network is built: controller, cable, disk drive, grid =="
ctl command "execute in minecraft:the_end run setblock 40 64 0 refinedstorage:controller"
ctl command "execute in minecraft:the_end run setblock 41 64 0 refinedstorage:cable"
ctl command "execute in minecraft:the_end run setblock 42 64 0 refinedstorage:disk_drive"
ctl command "execute in minecraft:the_end run setblock 43 64 0 refinedstorage:grid"
ctl wait 12
expect_state 41 64 0 'west=true'
expect_state 41 64 0 'east=true'
expect_state 41 64 0 'north=false'
expect_state 43 64 0 'active=true'
expect_info 43 64 0 "online (4 nodes, 1 controller, 1 drive, 1 grid); 0 disks, 0 items in 0 resources"

echo "== the grid opens its 9x6 menu, empty with no disk =="
ctl command "execute in minecraft:the_end run tp @s 42.5 64 2.5"
ctl wait 4
ctl interact 43 64 0
ctl wait 8
ctl screen | jq -e '.screen == "container"' >/dev/null \
    || fail "the grid did not open a container menu: $(ctl screen | jq -c '{screen,slots:(.slots|length)}')"
ctl screen | jq -e '[.slots[] | select(.slot >= 300)] | length == 54' >/dev/null \
    || fail "the grid menu does not have 54 container slots: $(ctl screen | jq -c '[.slots[] | select(.slot >= 300)] | length')"
expect_slots '[]'
ctl close
ctl wait 4

echo "== a storage disk right-clicked onto the drive goes in, out of the hand =="
ctl give refinedstorage:1k_storage_disk 1
ctl wait 4
ctl select refinedstorage:1k_storage_disk
ctl wait 4
ctl command "execute in minecraft:the_end run tp @s 41.5 64 2.5"
ctl wait 4
ctl interact 42 64 0
ctl wait 8
[ "$(count_of refinedstorage:1k_storage_disk)" = 0 ] \
    || fail "the disk stayed in the hand: $(ctl inventory | jq -c '[.hotbar[] | select(.item == "refinedstorage:1k_storage_disk")]')"
slots 42 64 0 | jq -e '[.[] | select(.item == "refinedstorage:1k_storage_disk")] | length == 1' >/dev/null \
    || fail "the drive does not hold the disk: $(slots 42 64 0)"
echo "  drive: $(slots 42 64 0)"
expect_info 43 64 0 "1 disks, 0 items in 0 resources"

echo "== items deposited through the grid's menu are on the network =="
ctl give minecraft:diamond 5
ctl give minecraft:gold_ingot 3
ctl wait 4
ctl command "execute in minecraft:the_end run tp @s 42.5 64 2.5"
ctl wait 4
ctl interact 43 64 0
ctl wait 8
diamond_slot=$(inv_slot minecraft:diamond)
[ -n "$diamond_slot" ] || fail "no diamond stack in the menu's player inventory: $(ctl screen | jq -c '[.slots[] | select(.item != null)]')"
ctl click-slot "$diamond_slot"
ctl wait 2
ctl click-slot 300
ctl wait 8
gold_slot=$(inv_slot minecraft:gold_ingot)
[ -n "$gold_slot" ] || fail "no gold stack in the menu's player inventory"
ctl click-slot "$gold_slot"
ctl wait 2
ctl click-slot 301
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3},{"slot":301,"item":"minecraft:diamond","count":5}]'
[ "$(count_of minecraft:diamond)" = 0 ] || fail "5 diamonds are still in the inventory"
expect_info 43 64 0 "1 disks, 8 items in 2 resources"

echo "== the search filters the grid's listing =="
say "refinedstorage filter 43 64 0 gold"
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
say "refinedstorage filter 43 64 0 diamond"
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:diamond","count":5}]'
ctl messages 6 | jq -r '.chat[]' | grep -q 'Grid search: diamond' \
    || fail "no search feedback in chat"
say "refinedstorage filter 43 64 0"
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3},{"slot":301,"item":"minecraft:diamond","count":5}]'

echo "== a shift-click on the diamond view withdraws it into the inventory =="
ctl click-slot 301 shift
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
[ "$(count_of minecraft:diamond)" = 5 ] \
    || fail "the withdrawn diamonds are not in the inventory: $(ctl inventory | jq -c '[.hotbar // [], .main // []]')"
expect_info 43 64 0 "1 disks, 3 items in 1 resources"
ctl close
ctl wait 4

echo "== a broken controller takes the network offline; the disk keeps the gold =="
ctl command "gamemode survival"
ctl give minecraft:diamond_pickaxe 1
ctl wait 4
ctl select minecraft:diamond_pickaxe
ctl command "execute in minecraft:the_end run tp @s 40.5 64 2.5"
ctl wait 6
ctl break 40 64 0
ctl wait 12
ctl command "clear @s refinedstorage:controller"
ctl command "execute in minecraft:the_end run kill @e[type=item]"
expect_state 43 64 0 'active=false'
expect_info 43 64 0 "offline (3 nodes, 0 controller, 1 drive, 1 grid)"
ctl command "gamemode creative"
ctl wait 4
ctl interact 43 64 0
ctl wait 8
if [ "$SELFTEST" = 1 ]; then
    echo "(selftest: asserting the offline grid still lists the gold)"
    expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
else
    expect_slots '[]'
fi
ctl close
ctl wait 4
# Put the controller back: the gold comes back, so it lived in the disk.
ctl command "execute in minecraft:the_end run setblock 40 64 0 refinedstorage:controller"
ctl wait 12
expect_state 43 64 0 'active=true'
expect_info 43 64 0 "online (4 nodes, 1 controller, 1 drive, 1 grid); 1 disks, 3 items in 1 resources"
ctl interact 43 64 0
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
ctl close
ctl wait 4

echo "== an online network with no disk drive lists nothing, beside it =="
ctl command "execute in minecraft:the_end run setblock 48 64 0 refinedstorage:controller"
ctl command "execute in minecraft:the_end run setblock 49 64 0 refinedstorage:cable"
ctl command "execute in minecraft:the_end run setblock 50 64 0 refinedstorage:grid"
ctl wait 12
expect_state 50 64 0 'active=true'
expect_info 50 64 0 "online (3 nodes, 1 controller, 0 drive, 1 grid); 0 disks, 0 items in 0 resources"
ctl command "execute in minecraft:the_end run tp @s 49.5 64 2.5"
ctl wait 4
ctl interact 50 64 0
ctl wait 8
expect_slots '[]'
ctl close
ctl wait 4
# The first network still lists its gold.
ctl command "execute in minecraft:the_end run tp @s 42.5 64 2.5"
ctl wait 4
ctl interact 43 64 0
ctl wait 8
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
ctl close
ctl wait 4

echo "== a stick is not a storage disk: the drive takes nothing =="
ctl give minecraft:stick 1
ctl wait 4
ctl select minecraft:stick
ctl wait 4
ctl command "execute in minecraft:the_end run tp @s 41.5 64 2.5"
ctl wait 4
ctl interact 42 64 0
ctl wait 8
[ "$(count_of minecraft:stick)" = 1 ] || fail "the drive took the stick"
slots 42 64 0 | jq -e '[.[] | select(.item == "minecraft:stick")] | length == 0' >/dev/null \
    || fail "the drive holds a stick: $(slots 42 64 0)"
ctl screen | jq -e '.screen == "container"' >/dev/null \
    || fail "the stick click did not open the drive's own menu"
echo "  drive after a stick: $(slots 42 64 0)"
ctl close
ctl wait 4

echo "== a vanilla barrel is not a network block =="
ctl command "execute in minecraft:the_end run setblock 56 64 0 minecraft:barrel"
ctl wait 6
expect_info 56 64 0 "No Refined Storage network block there."

echo "== the disk keeps its contents across a relaunch =="
ctl command "execute in minecraft:the_end run tp @s 42.5 64 2.5"
ctl save
ctl wait 10
ctl quit
sleep 3
launch
ctl wait 20
expect_info 43 64 0 "online (4 nodes, 1 controller, 1 drive, 1 grid); 1 disks, 3 items in 1 resources"
ctl interact 43 64 0
ctl wait 10
expect_slots '[{"slot":300,"item":"minecraft:gold_ingot","count":3}]'
ctl close
ctl wait 4

echo "PASS: the cable follows its neighbours and the grid is active with a controller, the disk went into the drive and the grid listed what was deposited through its menu, the search filtered it, a shift-click withdrew the diamonds, breaking the controller took the network offline and putting it back restored the disk's gold, a network without a drive listed nothing, a stick did not enter the drive, a vanilla barrel is not a network block, and the disk kept its 3 gold across a relaunch"
# The cleanup trap prints EXIT:<status> as the last line.
