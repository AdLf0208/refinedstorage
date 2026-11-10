#!/usr/bin/env bash
# Compile/package ladder for the AE2 embedded-content bundle.
# Runs on either box; `tools/horizon.py` defaults to the campaign pin checkouts.
# usage: tools/ladder-embed.sh <version> <hnmod-name>
set -uo pipefail
VER=${1:?version}
HN=${2:?hnmod name}
PORT=$(cd "$(dirname "$0")/.." && pwd)
cd "$PORT" || exit 1
ID=$(basename "$PORT")
CLONE=$(cd "$PORT/../.." && pwd)
PIN=$CLONE/ports/_shared/pins/horizon-1d63e0a
EV=$CLONE/ports/_shared/evidence/$ID
mkdir -p "$EV"
export CARGO_INCREMENTAL=0
export HORIZON_MIXINS=$CLONE/ports/_shared/pins/horizon-mixins-5a64af9
export CARGO_TARGET_DIR="${HORIZON_TARGET_DIR:-$PORT/target/horizon-cargo}/carry-tools"
CARRY=(cargo run --locked --offline --quiet \
    --manifest-path "$PIN/crates/cargo-horizon-mod/Cargo.toml" --)
failstep() { echo "STEP-FAILED($ID): $*" >&2; exit 90; }

echo "===== $ID: rebuild content .hmod ====="
(cd content && "${CARRY[@]}" horizon-mod build) 2>&1 | tee "$EV/ladder-package-hmod.log" \
    || failstep "hmod build"
cp -f "content/target/horizon-mod/$ID.hmod" "content/$ID.hmod" || failstep "copy hmod"
(cd content && "${CARRY[@]}" horizon-mod check "$ID.hmod") 2>&1 | tee -a "$EV/ladder-package-hmod.log" \
    || failstep "hmod check"

echo "===== $ID: embed the built content in the bundle ====="
mkdir -p "mixins/$ID/data"
cp -f "content/$ID.hmod" "mixins/$ID/data/$ID-content.hmod" || failstep "embed copy"
cmp "content/$ID.hmod" "mixins/$ID/data/$ID-content.hmod" || failstep "embed cmp"

echo "===== $ID: package .hnmod ====="
python3 tools/package.py 2>&1 | tee "$EV/ladder-package-hnmod.log" || failstep "hnmod package"
cp -f "target/$ID-$VER.hnmod" "$HN" || failstep "copy hnmod"
API=notorchmaster
[ "$ID" = torchmaster ] && API=api
python3 - "$HN" "$ID" "$API" <<'PY' || failstep "hnmod contents"
import sys, zipfile
hn, mid, api = sys.argv[1], sys.argv[2], sys.argv[3]
names = zipfile.ZipFile(hn).namelist()
must = [f"mixins/{mid}/data/{mid}-content.hmod", "horizon-native.json"]
if api == "api":
    must.append(f"mixins/{mid}/api/src/lib.rs")
missing = [m for m in must if m not in names]
if missing:
    sys.exit(f"missing from {hn}: {missing} (entries={names})")
print(f"hnmod: {len(names)} entries, carries {must}")
PY

echo "===== $ID: fmt ====="
cargo fmt --all --check 2>&1 | tee "$EV/ladder-fmt.log" || failstep "fmt"
echo "===== $ID: clippy -D warnings ====="
cargo clippy --all-targets --locked --offline -- -D warnings 2>&1 | tee "$EV/ladder-clippy.log" \
    || failstep "clippy"
echo "===== $ID: runtime cargo test ====="
cargo test --locked --offline 2>&1 | tee "$EV/ladder-test.log" || failstep "cargo test"

for hstep in stage check test build; do
    echo "===== $ID: horizon $hstep ====="
    python3 tools/horizon.py "$hstep" --horizon "$PIN" 2>&1 | tee "$EV/ladder-$hstep.json" \
        || failstep "horizon $hstep"
done

echo "===== $ID: artifacts ====="
for f in "content/$ID.hmod" "$HN"; do
    [ -f "$f" ] || failstep "missing $f"
    size=$(stat -c %s "$f")
    sha=$(sha256sum "$f" | cut -d' ' -f1)
    entries=$(python3 -c 'import sys,zipfile; print(len(zipfile.ZipFile(sys.argv[1]).namelist()))' "$f")
    echo "$f size=$size sha256=$sha entries=$entries" | tee -a "$EV/ladder-artifacts.log"
done
echo "embedded data: $(stat -c %s "mixins/$ID/data/$ID-content.hmod") B" | tee -a "$EV/ladder-artifacts.log"
echo "LADDER-BUILD-OK $ID"
