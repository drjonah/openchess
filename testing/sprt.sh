#!/usr/bin/env bash
# Local cutechess SPRT / gauntlet harness (P8-03 / M2-01).
# Usage:
#   ./testing/sprt.sh [--st SECONDS] [--games N] [--engine-a PATH] [--engine-b PATH]
#   ./testing/sprt.sh --book testing/books/openings.epd   # smoke book
# Default openings: UHO-class 8mvs_+90_+99.epd (OwnBook forced false).
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
# Strength default: UHO / 8-moves class (M2-01). Smoke: --book testing/books/openings.epd
BOOK="${ROOT}/testing/books/8mvs_+90_+99.epd"
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
      cat <<'EOF'
Usage: ./testing/sprt.sh [options]
  --st SECONDS          Time control base seconds (default 8)
  --games N             Max games (default 200)
  --engine-a PATH       Baseline binary
  --engine-b PATH       Candidate binary
  --book PATH           Opening EPD (default: testing/books/8mvs_+90_+99.epd)
  --concurrency N       Parallel games
  --hash MB             UCI Hash per engine
  --elo0 / --elo1       SPRT Elo bounds (default 0 / 5)
OwnBook is always forced false. See testing/README.md.
EOF
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

echo "==> SPRT"
echo "    A=$ENGINE_A"
echo "    B=$ENGINE_B"
echo "    book=$BOOK"
echo "    st=${ST}s games=$GAMES elo0=$ELO0 elo1=$ELO1 OwnBook=false"

# Each engine gets a unique name so cutechess can report results clearly.
# OwnBook is forced off: strength SPRT must measure search + eval only, with the
# EPD suite (below) providing fixed, shared openings (P10-07 / M2-01). The
# interactive TUI / GUI / Lichess play path keeps OwnBook on by default.
cutechess-cli \
  -engine name=OpenChessA cmd="$ENGINE_A" option.Hash="$HASH" option.OwnBook=false \
  -engine name=OpenChessB cmd="$ENGINE_B" option.Hash="$HASH" option.OwnBook=false \
  -each proto=uci tc="${ST}+0.1" \
  -openings file="$BOOK" format=epd order=random \
  -games "$GAMES" \
  -concurrency "$CONCURRENCY" \
  -ratinginterval 10 \
  -sprt elo0="$ELO0" elo1="$ELO1" alpha="$ALPHA" beta="$BETA" \
  -recover \
  -pgnout "${ROOT}/testing/last-sprt.pgn"
