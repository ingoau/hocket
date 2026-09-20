#!/usr/bin/env bash
# Regenerates every platform type/binding from crates/hocket-core/src/api.rs.
#
#   typeshare      -> desktop/src/core/api.ts                                   (TypeScript)
#   typeshare      -> android/core/src/main/java/app/hocket/core/api/Generated.kt (Kotlin, kotlinx.serialization)
#   uniffi-bindgen -> android/core/src/main/java/app/hocket/core/ffi/hocket_android.kt (Kotlin FFI glue)
#
# Respects CARGO_TARGET_DIR (defaults to <repo>/target). Generated files are gitignored.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
API="$ROOT/crates/hocket-core/src/api.rs"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

command -v typeshare >/dev/null || cargo install typeshare-cli

mkdir -p "$ROOT/desktop/src/core" "$ROOT/android/core/src/main/java/app/hocket/core/api"
typeshare "$API" --lang=typescript --output-file="$ROOT/desktop/src/core/api.ts"
typeshare "$API" --lang=kotlin --java-package=app.hocket.core.api --module-name=core \
  --output-file="$ROOT/android/core/src/main/java/app/hocket/core/api/Generated.kt"
# typeshare names the `FilterValue::List` variant `List`, which shadows kotlin.collections.List inside
# the sealed class; qualify the payload type.
sed -i 's/data class List(val data: List</data class List(val data: kotlin.collections.List</' \
  "$ROOT/android/core/src/main/java/app/hocket/core/api/Generated.kt"

# UniFFI Kotlin glue (from the compiled host cdylib; proc-macro metadata lives in the binary).
cargo build -p hocket-android --features bindgen --quiet
LIB="$TARGET_DIR/debug/libhocket_android.so"
[ -f "$LIB" ] || LIB="$TARGET_DIR/debug/libhocket_android.dylib"
[ -f "$LIB" ] || { echo "host cdylib not found under $TARGET_DIR/debug" >&2; exit 1; }
cargo run -p hocket-android --features bindgen --bin uniffi-bindgen --quiet -- \
  generate --library "$LIB" --language kotlin --out-dir "$ROOT/android/core/src/main/java" \
  --config "$ROOT/crates/hocket-android/uniffi.toml" --no-format
echo "bindings generated"
