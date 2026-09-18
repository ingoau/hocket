#!/usr/bin/env bash
# Cross-compiles hocket-android (the UniFFI cdylib) for Android ABIs into the Gradle jniLibs dir.
#
#   scripts/build-android-core.sh [debug|release] [abi ...]
#
# ABIs default to arm64-v8a armeabi-v7a x86_64. Needs cargo-ndk, the Rust Android targets and an NDK
# under $ANDROID_HOME/ndk (or ANDROID_NDK_HOME). Respects CARGO_TARGET_DIR. Output:
#   android/core/src/main/jniLibs/<abi>/libhocket_android.so
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}"
PROFILE="${1:-debug}"
shift || true
ABIS=("$@")
[ ${#ABIS[@]} -eq 0 ] && ABIS=(arm64-v8a armeabi-v7a x86_64)

command -v cargo-ndk >/dev/null || cargo install cargo-ndk
for t in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android; do
  rustup target list --installed | grep -q "$t" || rustup target add "$t"
done

ARGS=()
[ "$PROFILE" = "release" ] && ARGS+=(--release)
TARGET_ARGS=()
for abi in "${ABIS[@]}"; do TARGET_ARGS+=(-t "$abi"); done

OUT="$ROOT/android/core/src/main/jniLibs"
mkdir -p "$OUT"
# --platform 26 matches minSdk. cargo-ndk strips symbols in release and copies the cdylib per ABI.
cargo ndk "${TARGET_ARGS[@]}" --platform 26 -o "$OUT" build -p hocket-android "${ARGS[@]}"
echo "android core built ($PROFILE) for: ${ABIS[*]} -> $OUT"
ls -la "$OUT"/*/libhocket_android.so
