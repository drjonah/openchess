#!/usr/bin/env bash
# Local cutechess SPRT / gauntlet harness (P8-03).
# Usage:
#   ./testing/sprt.sh [--st SECONDS] [--games N] [--engine-a PATH] [--engine-b PATH]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ST=8
GAMES=200
ELO0=0
ELO1=5
ALPHA=0.05
BETA=0.05
ENGINE_A="${ROOT}/target/release/openchess"
ENGINE_B="${ROOT}/target/release/openchess"
BOOK="${ROOT}/testing/books/openings.epd"
CONCURRENCY=1
HASH=64

while [[ $# -gt 0 ]]; do
  case "$1" in
    --st) ST="$2"; shift 2 ;;
    --games) GAMES="$2"; shift 2 ;;
    --engine-a) ENGINE_A="$2"; shift 2 ;;
    --engine-b) ENGINE_B="$2"; shift 2 ;;
    --book) BOOK="$2"; shift 2 ;;
    --concurrency) CONCURRENCY="$2"; shift 2 ;;
    --hash) HASH="$2"; shift 2 ;;
    --elo0) ELO0="$2"; shift 2 ;;
    --elo1) ELO1="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,5p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 1
      ;;
  esac
done

if ! command -v cutechess-cli >/dev/null 2>&1; then
  echo "cutechess-cli not found on PATH." >&2
  echo "Install from https://github.com/cutechess/cutechess then re-run." >&2
  exit 1
fi

if [[ ! -x "$ENGINE_A" ]]; then
  echo "engine A missing: $ENGINE_A" >&2
  echo "Build with: cargo build --release" >&2
  exit 1
fi
if [[ ! -x "$ENGINE_B" ]]; then
  echo "engine B missing: $ENGINE_B" >&2
  exit 1
fi
if [[ ! -f "$BOOK" ]]; then
  echo "opening book missing: $BOOK" >&2
  exit 1
fi

echo "==> SPRT smoke"
echo "    A=$ENGINE_A"
echo "    B=$ENGINE_B"
echo "    st=${ST}s games=$GAMES elo0=$ELO0 elo1=$ELO1"

# Each engine gets a unique name so cutechess can report results clearly.
cutechess-cli \
  -engine name=OpenChessA cmd="$ENGINE_A" option.Hash="$HASH" \
  -engine name=OpenChessB cmd="$ENGINE_B" option.Hash="$HASH" \
  -each proto=uci tc="${ST}+0.1" \
  -openings file="$BOOK" format=epd order=random \
  -games "$GAMES" \
  -concurrency "$CONCURRENCY" \
  -ratinginterval 10 \
  -sprt elo0="$ELO0" elo1="$ELO1" alpha="$ALPHA" beta="$BETA" \
  -recover \
  -pgnout "${ROOT}/testing/last-sprt.pgn"
