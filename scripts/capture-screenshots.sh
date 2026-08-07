#!/usr/bin/env bash
# Refresh README screenshots from a live Cofferly window.
# Requires a graphical session (macOS/Linux desktop).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${COFFERLY_CAPTURE_DIR:-$ROOT/docs/screenshots}"
DATA_ROOT="${COFFERLY_DATA_DIR:-/tmp/cofferly-capture-$$}"
BIN="${COFFERLY_BIN:-$ROOT/target/release/Cofferly}"

if [[ ! -x "$BIN" ]]; then
  echo "Building release binary..."
  cargo build --release
fi

mkdir -p "$OUT_DIR"

for target in story-unlock wallet settings; do
  echo "Capturing ${target}..."
  data_dir="${DATA_ROOT}-${target}"
  rm -rf "$data_dir"
  mkdir -p "$data_dir" "$OUT_DIR"
  # One target per process: Argon2 demo prep is heavy; multi-target can stall the UI thread.
  COFFERLY_DATA_DIR="$data_dir" \
  COFFERLY_CAPTURE="$target" \
  COFFERLY_CAPTURE_DIR="$OUT_DIR" \
    "$BIN"
done

echo "Wrote screenshots to $OUT_DIR"
ls -la "$OUT_DIR"/cofferly-story-unlock.png \
       "$OUT_DIR"/cofferly-wallet-screen.png \
       "$OUT_DIR"/cofferly-settings-screen.png
