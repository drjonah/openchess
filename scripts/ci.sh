#!/usr/bin/env bash
# One-command correctness gate (P8-00).
# Usage: ./scripts/ci.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo test (unit + integration)"
cargo test

echo "==> bench signature"
cargo test --lib tools::bench::tests::bench_is_deterministic -- --exact

echo "==> UCI smoke"
printf 'uci\nisready\nposition startpos\ngo depth 4\nquit\n' | cargo run -q -- 2>/dev/null | tee /tmp/openchess-uci-smoke.txt
grep -q 'uciok' /tmp/openchess-uci-smoke.txt
grep -q 'readyok' /tmp/openchess-uci-smoke.txt
grep -q 'bestmove' /tmp/openchess-uci-smoke.txt

echo "==> All gates passed"
