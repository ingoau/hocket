#!/usr/bin/env bash
# Builds the Pebble watchapp and stages it for the Android app to bundle.
#
#   scripts/build-pebble.sh
#
# Needs the Pebble SDK (`uv tool install pebble-tool && pebble sdk install latest`). Output:
#   pebble/build/pebble.pbw                     the watchapp
#   pebble/build/android-assets/hocket.pbw      picked up by android/app as an asset when present
#
# Without this the APK has no watchapp and Settings → Pebble watch hides Install on watch.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v pebble >/dev/null || { echo "error: pebble not found (uv tool install pebble-tool && pebble sdk install latest)" >&2; exit 1; }
cd "$ROOT/pebble"
pebble build
mkdir -p build/android-assets
cp build/pebble.pbw build/android-assets/hocket.pbw
echo "staged $ROOT/pebble/build/android-assets/hocket.pbw"
