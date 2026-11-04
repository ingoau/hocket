#!/usr/bin/env bash
# Cross-compiles hocket-android (the UniFFI cdylib) for Android ABIs into the Gradle jniLibs dir.
#
#   scripts/build-android-core.sh [debug|release] [abi ...]
#
# ABIs default to arm64-v8a armeabi-v7a x86_64. Needs the Rust Android targets and an NDK
# under $ANDROID_HOME/ndk (or ANDROID_NDK_HOME). Respects CARGO_TARGET_DIR. Output:
#   android/core/src/main/jniLibs/<abi>/libhocket_android.so
#
# The script never installs anything: a missing NDK or Rust target fails with the command
# to run. Release builds are stripped (`profile.release.strip`), so the cdylib is small even when the
# toolchain default changes.
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

# --platform 26 matches minSdk; the NDK's <triple><api>-clang wrappers bake it in.
API=26
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
[ -d "$TOOLCHAIN" ] || { echo "error: no NDK LLVM toolchain at $TOOLCHAIN" >&2; exit 1; }

triple() {
  case "$1" in
    arm64-v8a) echo aarch64-linux-android ;;
    armeabi-v7a) echo armv7-linux-androideabi ;;
    x86_64) echo x86_64-linux-android ;;
    x86) echo i686-linux-android ;;
    *) echo "error: unknown ABI '$1' (arm64-v8a, armeabi-v7a, x86_64, x86)" >&2; exit 1 ;;
  esac
}

MISSING=()
for t in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android; do
  rustup target list --installed | grep -q "^$t\$" || MISSING+=("$t")
done
if [ ${#MISSING[@]} -gt 0 ]; then
  echo "error: Rust Android targets missing; run 'rustup target add ${MISSING[*]}'" >&2
  exit 1
fi

# One cargo invocation for every ABI, so cargo builds them in parallel (cargo-ndk runs one cargo per
# ABI, back to back, and release hocket-core is a long single-threaded codegen-units=1 compile). The
# per-target CC/AR/linker env is what cargo-ndk would set, pointed at the NDK's clang wrappers.
TARGET_ARGS=()
for abi in "${ABIS[@]}"; do
  t="$(triple "$abi")"
  TARGET_ARGS+=(--target "$t")
  # armv7's clang wrapper is named armv7a-…; the Rust triple has no "a".
  clang="$TOOLCHAIN/${t/armv7-/armv7a-}$API-clang"
  [ -x "$clang" ] || { echo "error: NDK clang wrapper missing: $clang" >&2; exit 1; }
  u="${t//-/_}"
  U="$(echo "$u" | tr '[:lower:]' '[:upper:]')"
  export "CC_$u=$clang" "CXX_$u=$clang++" "AR_$u=$TOOLCHAIN/llvm-ar" "RANLIB_$u=$TOOLCHAIN/llvm-ranlib"
  export "CARGO_TARGET_${U}_LINKER=$clang" "CARGO_TARGET_${U}_AR=$TOOLCHAIN/llvm-ar"
done

ARGS=()
[ "$PROFILE" = "release" ] && ARGS+=(--release --config 'profile.release.strip=true')
cargo build -p hocket-android "${TARGET_ARGS[@]}" "${ARGS[@]}"

TARGET_DIR="${CARGO_TARGET_DIR:-$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')}"
OUT="$ROOT/android/core/src/main/jniLibs"
for abi in "${ABIS[@]}"; do
  mkdir -p "$OUT/$abi"
  cp "$TARGET_DIR/$(triple "$abi")/$PROFILE/libhocket_android.so" "$OUT/$abi/"
done
echo "android core built ($PROFILE) for: ${ABIS[*]} -> $OUT"
ls -la "$OUT"/*/libhocket_android.so
