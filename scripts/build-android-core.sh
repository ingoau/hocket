#!/usr/bin/env bash
# Cross-compiles hocket-android for Android ABIs into the Gradle jniLibs dir.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort | tail -1)}"
PROFILE="${1:-debug}"
ARGS=()
[ "$PROFILE" = "release" ] && ARGS+=(--release)
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o "$ROOT/android/core/src/main/jniLibs" \
  build -p hocket-android "${ARGS[@]}"
echo "android core built ($PROFILE)"
