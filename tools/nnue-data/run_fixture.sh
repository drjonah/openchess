#!/usr/bin/env bash
# Q2-01 end-to-end fixture: build quiet-position Bullet text on a developer machine.
# Usage (from repo root): ./tools/nnue-data/run_fixture.sh
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${1:-tools/nnue-data/out/fixture_bullet.txt}"
OPENINGS="${2:-tools/nnue-data/fixtures/openings.epd}"

echo "==> openchess nnue-data fixture"
cargo run -q -- nnue-data fixture --openings "$OPENINGS" --output "$OUT"

echo "==> validate"
cargo run -q -- nnue-data validate "$OUT"

echo "==> Q2-01 fixture OK: $OUT"
