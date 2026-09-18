#!/usr/bin/env bash
# Regenerates every platform type/binding from crates/hocket-core/src/api.rs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
API="$ROOT/crates/hocket-core/src/api.rs"

command -v typeshare >/dev/null || cargo install typeshare-cli

mkdir -p "$ROOT/desktop/src/core" "$ROOT/android/core/src/main/java/app/hocket/core/api"
typeshare "$API" --lang=typescript --output-file="$ROOT/desktop/src/core/api.ts"
typeshare "$API" --lang=kotlin --java-package=app.hocket.core.api --module-name=core \
  --output-file="$ROOT/android/core/src/main/java/app/hocket/core/api/Generated.kt"

# UniFFI Kotlin glue (from the compiled host cdylib; proc-macro metadata lives in the binary).
cargo build -p hocket-android --features bindgen --quiet
LIB="$ROOT/target/debug/libhocket_android.so"
[ -f "$LIB" ] || LIB="$ROOT/target/debug/libhocket_android.dylib"
cargo run -p hocket-android --features bindgen --bin uniffi-bindgen --quiet -- \
  generate --library "$LIB" --language kotlin --out-dir "$ROOT/android/core/src/main/java" \
  --config "$ROOT/crates/hocket-android/uniffi.toml" --no-format
echo "bindings generated"
