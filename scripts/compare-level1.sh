#!/usr/bin/env bash
# Record the shipped build's first level in a browser and the port's first level
# offscreen, then write side-by-side sheets and MP4s for review.
#
#   scripts/compare-level1.sh [out-dir] [level-index]
#
# The reference is the WebGL build under DD_REF_BUILD (default
# ~/Documents/refs/DeadlyDescent). It runs in Chromium inside a nested
# kwin_wayland, so capturing it does not disturb the desktop; frames are grabbed
# with spectacle on a timer (Chromium in SwiftShader is far below real time, so a
# timer is the honest cadence). The port's side is the deterministic offscreen
# capture: DD_CAPTURE frames -> mp4.
set -euo pipefail

out="${1:-/tmp/dd-compare}"
level="${2:-0}"
ref_build="${DD_REF_BUILD:-$HOME/Documents/refs/DeadlyDescent}"
port_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
assets="${DD_ASSETS:-$HOME/Documents/refs/DeadlyDescent/extracted}"
ref_frames="${DD_REF_SECONDS:-150}"
ref_every="${DD_REF_EVERY:-2}"

mkdir -p "$out/ref" "$out/port" /tmp/dd-run
chmod 700 /tmp/dd-run

echo "== port: level $level offscreen =="
(
    cd "$port_dir"
    DD_ASSETS="$assets" DD_CAPTURE="$out/port" DD_AUTOPLAY="$level" \
        DD_CAPTURE_FRAMES="${DD_PORT_FRAMES:-600}" DD_CAPTURE_W=640 DD_CAPTURE_H=360 \
        cargo run --quiet --release 2>/dev/null || \
    DD_ASSETS="$assets" DD_CAPTURE="$out/port" DD_AUTOPLAY="$level" \
        DD_CAPTURE_FRAMES="${DD_PORT_FRAMES:-600}" DD_CAPTURE_W=640 DD_CAPTURE_H=360 \
        cargo run --quiet 2>/dev/null
)

echo "== reference: shipped build in Chromium =="
python3 -m http.server "${DD_REF_PORT:-8123}" --directory "$ref_build" >/tmp/dd-http.log 2>&1 &
HTTP=$!
trap 'kill $HTTP 2>/dev/null || true' EXIT
sleep 1

timeout $((ref_frames + 120)) dbus-run-session -- bash -c '
set -u
export XDG_RUNTIME_DIR=/tmp/dd-run
kwin_wayland --virtual --xwayland --width 1280 --height 720 --socket dd-ref >/tmp/dd-kwin.log 2>&1 &
KW=$!
sleep 5
DISP=":$(ls /tmp/.X11-unix/ 2>/dev/null | head -1 | tr -d X)"
CHROME="$HOME/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome"
[ -x "$CHROME" ] || CHROME=$(command -v chromium)
DISPLAY="$DISP" "$CHROME" \
  --no-sandbox --disable-gpu-sandbox --enable-unsafe-swiftshader \
  --ozone-platform=x11 --disable-features=UseOzonePlatform \
  --use-gl=angle --use-angle=swiftshader \
  --window-size=1280,720 --window-position=0,0 \
  --autoplay-policy=no-user-gesture-required \
  --user-data-dir=/tmp/dd-chrome \
  "http://127.0.0.1:'"${DD_REF_PORT:-8123}"'/index.html" >/tmp/dd-chrome.log 2>&1 &
CH=$!
sleep 3
i=0
end=$(( $(date +%s) + '"$ref_frames"' ))
while [ "$(date +%s)" -lt "$end" ]; do
  shot="'"$out"'/ref/ref_$(printf "%03d" $i).png"
  WAYLAND_DISPLAY=dd-ref QT_QPA_PLATFORM=wayland timeout 20 spectacle -b -n -f -o "$shot" >/dev/null 2>&1
  [ -s "$shot" ] || rm -f "$shot"
  i=$((i + 1))
  sleep '"$ref_every"'
done
kill $CH 2>/dev/null
kill $KW 2>/dev/null
' 2>/dev/null || true

for f in "$out"/ref/ref_*.png; do
    [ -s "$f" ] || continue
    magick "$f" -resize 640x360! "${f%.png}.tmp.png" 2>/dev/null && mv "${f%.png}.tmp.png" "$f"
done

echo "== videos and sheets =="
if compgen -G "$out/port/frame_*.png" >/dev/null; then
    ffmpeg -y -loglevel error -framerate 30 -i "$out/port/frame_%04d.png" \
        -pix_fmt yuv420p "$out/port.mp4" </dev/null
fi
if compgen -G "$out/ref/ref_*.png" >/dev/null; then
    ffmpeg -y -loglevel error -framerate "${DD_REF_VIDEO_FPS:-2}" -pattern_type glob \
        -i "$out/ref/ref_*.png" -pix_fmt yuv420p "$out/reference.mp4" </dev/null
fi

mapfile -t refs < <(ls "$out"/ref/ref_*.png 2>/dev/null | sort)
mapfile -t mines < <(ls "$out"/port/frame_*.png 2>/dev/null | sort)
echo "reference frames: ${#refs[@]}, port frames: ${#mines[@]}"
if [ "${#refs[@]}" -gt 0 ] && [ "${#mines[@]}" -gt 0 ]; then
    pairs=5
    sheets=()
    for i in $(seq 0 $((pairs - 1))); do
        r=$(( i * (${#refs[@]} - 1) / (pairs - 1) ))
        m=$(( i * (${#mines[@]} - 1) / (pairs - 1) ))
        magick "${refs[$r]}" -bordercolor '#3a3a3a' -border 2 "$out/_a.png"
        magick "${mines[$m]}" -bordercolor '#3a3a3a' -border 2 "$out/_b.png"
        magick "$out/_a.png" "$out/_b.png" +append "$out/sheet_$i.png"
        sheets+=("$out/sheet_$i.png")
    done
    montage -tile 1x -geometry +2+2 -background '#111' "${sheets[@]}" "$out/all.png"
    rm -f "$out/_a.png" "$out/_b.png"
fi
ls -la "$out"/*.mp4 "$out/all.png" 2>/dev/null | awk '{print $5, $9}'
