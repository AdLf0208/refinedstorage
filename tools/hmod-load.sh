#!/usr/bin/env bash
# Does the packaged content mod load on the shared, unmodded game, and are its
# ids absent without it? The second part is the negative control: a `.hmod`
# check that passes on a game which already knows the ids would prove nothing.
#
# CTL/GAME default to the campaign's shared binaries at the pin.
#   tools/hmod-load.sh          PASS/FAIL
set -euo pipefail
export LC_ALL=C

CLONE=${CAMPAIGN_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd)}
PORT=$CLONE/ports/refinedstorage
HMOD=${HMOD:-$PORT/content/refinedstorage.hmod}
RES=${RES:-${HORIZON_RES:-${HORIZON_RES:-$HOME/.local/horizon-client/26.3}}}
if [ -z "${CTL:-}" ] || [ -z "${GAME:-}" ]; then
    eval "$("$CLONE/ports/tools/shared-bin.sh" env)"
fi
SOCK=${SOCK:-/tmp/opencode/refinedstorage-hmod.sock}
WORLD=RefinedStorageHmod

[ -f "$HMOD" ] || { echo "missing content mod: $HMOD" >&2; exit 1; }
[ -x "$CTL" ] || { echo "missing horizon-ctl: $CTL" >&2; exit 1; }
[ -x "$GAME" ] || { echo "missing horizon-client: $GAME" >&2; exit 1; }

SCRATCH=$(mktemp -d /tmp/opencode/refinedstorage-hmod-home.XXXXXX)
export HORIZON_HOME=$SCRATCH/game
export HORIZON_LAUNCHER_HOME=$SCRATCH/launcher
export XDG_DATA_HOME=$SCRATCH/data
export XDG_CONFIG_HOME=$SCRATCH/config
export HORIZON_NO_AUDIO=1
mkdir -p "$(dirname "$SOCK")" "$XDG_CONFIG_HOME"
rm -f "$SOCK"
ctl() { "$CTL" --socket "$SOCK" "$@"; }
trap 'ctl quit >/dev/null 2>&1 || true; sleep 1; rm -rf "$SCRATCH" 2>/dev/null || true' EXIT

launch() { # launch [dev-mod args...]
    rm -f "$SOCK"
    cd "$SCRATCH"
    "$CTL" --socket "$SOCK" launch \
        --game "$GAME" --world "$WORLD" --seed 20261105 --creative --headless --wait \
        --resources "$RES" "$@" >/dev/null
    ctl wait-ready 120
    ctl wait 40
}

echo "== the packaged .hmod loads (block placed, item given) =="
launch --dev-mod "$HMOD"
ctl command "execute in minecraft:the_end run tp @s 0.5 64 3.5"
ctl wait 10
ctl command "execute in minecraft:the_end run setblock 0 64 0 refinedstorage:controller"
ctl wait 6
ctl block 0 64 0 | jq -e '.block == "refinedstorage:controller"' >/dev/null \
    || { echo "FAIL: the controller did not place: $(ctl block 0 64 0)" >&2; exit 1; }
ctl give refinedstorage:1k_storage_disk 1
ctl wait 6
ctl inventory | jq -e '[.hotbar[] | select(.item == "refinedstorage:1k_storage_disk")] | length == 1' >/dev/null \
    || { echo "FAIL: the disk was not given: $(ctl inventory)" >&2; exit 1; }
echo "PASS: the packaged .hmod loaded (block refinedstorage:controller placed, item refinedstorage:1k_storage_disk given)"
ctl quit
sleep 3

echo "== without --dev-mod the same ids are unknown (negative control) =="
launch
ctl command "execute in minecraft:the_end run tp @s 0.5 64 3.5"
ctl wait 10
ctl command "execute in minecraft:the_end run setblock 0 64 0 refinedstorage:controller"
ctl wait 6
ctl messages 4 | jq -r '.chat[]' | grep -q "Unknown block type 'refinedstorage:controller'" \
    || { echo "FAIL: the id was known without the mod: $(ctl messages 4)" >&2; exit 1; }
echo "  setblock: unknown block type (as expected)"
give=$(ctl give refinedstorage:1k_storage_disk 1 || true)
echo "  give: $give"
jq -e '.error | contains("unknown item refinedstorage:1k_storage_disk")' <<<"$give" >/dev/null \
    || { echo "FAIL: the item was known without the mod: $give" >&2; exit 1; }
echo "PASS: without --dev-mod the controller and the disk are unknown (the check is not vacuous)"
