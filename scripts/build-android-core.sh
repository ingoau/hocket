#!/usr/bin/env bash
# Cross-compiles hocket-android (the UniFFI cdylib) for Android ABIs into the Gradle jniLibs dir.
#
#   scripts/build-android-core.sh [debug|release] [abi ...]
#
# ABIs default to arm64-v8a armeabi-v7a x86_64. Needs cargo-ndk, the Rust Android targets and an NDK
# under $ANDROID_HOME/ndk (or ANDROID_NDK_HOME). Respects CARGO_TARGET_DIR. Output:
#   android/core/src/main/jniLibs/<abi>/libhocket_android.so
#
# The script never installs anything: a missing NDK, cargo-ndk or Rust target fails with the command
# to run. Release builds are stripped (`profile.release.strip`), so the cdylib is small even when the
# cargo-ndk default changes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  # `ls` fails when the directory is absent; `export X=$(...)` would mask that, so resolve first.
  NDK_DIR="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
  if [ -z "$NDK_DIR" ] || [ ! -d "$NDK_DIR" ]; then
    echo "error: no Android NDK found under $ANDROID_HOME/ndk (set ANDROID_NDK_HOME, or install one with" >&2
    echo "       '\$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager \"ndk;27.2.12479018\"')" >&2
    exit 1
  fi
  export ANDROID_NDK_HOME="$NDK_DIR"
fi
[ -d "$ANDROID_NDK_HOME" ] || { echo "error: ANDROID_NDK_HOME=$ANDROID_NDK_HOME is not a directory" >&2; exit 1; }
PROFILE="${1:-debug}"
shift || true
ABIS=("$@")
[ ${#ABIS[@]} -eq 0 ] && ABIS=(arm64-v8a armeabi-v7a x86_64)

command -v cargo-ndk >/dev/null || { echo "error: cargo-ndk is not installed; run 'cargo install cargo-ndk'" >&2; exit 1; }
MISSING=()
for t in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android; do
  rustup target list --installed | grep -q "^$t\$" || MISSING+=("$t")
done
if [ ${#MISSING[@]} -gt 0 ]; then
  echo "error: Rust Android targets missing; run 'rustup target add ${MISSING[*]}'" >&2
  exit 1
fi

ARGS=()
[ "$PROFILE" = "release" ] && ARGS+=(--release --config 'profile.release.strip=true')
TARGET_ARGS=()
for abi in "${ABIS[@]}"; do TARGET_ARGS+=(-t "$abi"); done

OUT="$ROOT/android/core/src/main/jniLibs"
mkdir -p "$OUT"
# --platform 26 matches minSdk. cargo-ndk copies the cdylib per ABI.
cargo ndk "${TARGET_ARGS[@]}" --platform 26 -o "$OUT" build -p hocket-android "${ARGS[@]}"
echo "android core built ($PROFILE) for: ${ABIS[*]} -> $OUT"
ls -la "$OUT"/*/libhocket_android.so
