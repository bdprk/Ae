#!/usr/bin/env bash
# Linux-only gate: build, then verify generated C under AddressSanitizer.
# Run from WSL2; this is the check Windows cannot perform.
set -euo pipefail

cd "$(dirname "$0")/.."

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/root/.cache/ae-target}"
export AE_CC="${AE_CC:-gcc}"

cargo build --locked

created=0
if [ ! -e target ]; then
    ln -s "$CARGO_TARGET_DIR" target
    created=1
fi
cleanup() {
    if [ "$created" = 1 ]; then rm -f target; fi
}
trap cleanup EXIT

python3 scripts/verify_generated_c.py --sanitize
