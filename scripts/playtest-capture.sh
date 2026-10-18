#!/usr/bin/env bash
# Playtest capture: run a level under autoplay and grab the window on a timer.
#
#   scripts/playtest-capture.sh <level> [seconds] [fps] [out-dir] [binary]
#
# The offscreen renderer (`DD_CAPTURE`) draws the UI but not the wgpu callback the
# 3D viewport paints through, so a *world* playtest grabs the live window instead.
# Grabs go to PNGs named frame_0000.png … for review or `montage`/`ffmpeg`.
set -euo pipefail

level="${1:-0}"
seconds="${2:-20}"
fps="${3:-2}"
out="${4:-/tmp/opencode/dd/playtest}"
bin="${5:-$(ls -t /tmp/opencode/dd/deadly-descent-r* 2>/dev/null | head -1)}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${DD_ASSETS:=$HOME/Documents/refs/DeadlyDescent/extracted}"
export DD_ASSETS

mkdir -p "$out"
rm -f "$out"/frame_*.png

DD_AUTOPLAY="$level" "$bin" >/dev/null 2>&1 &
game=$!
trap 'kill "$game" 2>/dev/null || true' EXIT

frames=$((seconds * fps))
interval=$(awk "BEGIN{printf \"%.3f\", 1/$fps}")
for ((i = 0; i < frames; i++)); do
    sleep "$interval"
    shot="$out/frame_$(printf '%04d' "$i").png"
    spectacle -b -n -f -o "$shot" >/dev/null 2>&1 || true
    [ -s "$shot" ] || rm -f "$shot"
done

kill "$game" 2>/dev/null || true
echo "captured $(ls "$out"/frame_*.png 2>/dev/null | wc -l) frames into $out"
