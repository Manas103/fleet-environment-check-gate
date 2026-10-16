#!/bin/bash
set -eu
SIMDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIR="$(dirname "$SIMDIR")"
cd "$DIR"
source "$HOME/.cargo/env"
mkdir -p "$DIR/docs"
cargo test --release 2>&1 | tee "$DIR/docs/test_output.txt"
