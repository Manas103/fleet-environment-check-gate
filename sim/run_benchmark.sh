#!/bin/bash
set -eu
SIMDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIR="$(dirname "$SIMDIR")"
BIN="$DIR/target/x86_64-unknown-linux-musl/release/fleetcheck"
mkdir -p "$DIR/docs"
exec > >(tee "$DIR/docs/benchmark_output.txt") 2>&1

echo "=== fleetcheck static binary ==="
file "$BIN"
ldd "$BIN" || true
echo

echo "=== clean fleet (60 hosts, expect 0/600 failing) ==="
bash "$DIR/sim/start_sshd.sh" clean
sleep 0.3
set +e
"$BIN" --contract "$DIR/contract/contract.toml" --inventory "$DIR/sim/hosts.csv" \
  --out "$DIR/docs/clean_report.json" --timeout 5 --concurrency 8
CLEAN_EXIT=$?
set -e
echo "clean run exit code: $CLEAN_EXIT"
echo

echo "=== faulted fleet (60 hosts, 120 seeded misconfigurations, expect 120/600 failing) ==="
bash "$DIR/sim/start_sshd.sh" faulted
sleep 0.3
set +e
"$BIN" --contract "$DIR/contract/contract.toml" --inventory "$DIR/sim/hosts.csv" \
  --out "$DIR/docs/faulted_report.json" --timeout 5 --concurrency 8
FAULTED_EXIT=$?
set -e
echo "faulted run exit code: $FAULTED_EXIT"
echo

echo "=== cross-check: detected failures vs. the seeded fault manifest ==="
python3 "$DIR/sim/verify_faults.py"

bash "$DIR/sim/stop_sshd.sh"
