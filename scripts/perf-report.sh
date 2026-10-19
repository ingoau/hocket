#!/usr/bin/env bash
# Isolated Criterion + Vitest benches for the CPU/RAM work on the hot paths.
# Writes parseable logs under /opt/cursor/artifacts when that directory exists,
# otherwise ./target/perf-report.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -d /opt/cursor/artifacts ]]; then
  OUT=/opt/cursor/artifacts
else
  OUT="$ROOT/target/perf-report"
  mkdir -p "$OUT"
fi
cd "$ROOT"

echo "== cargo test (queue JSON bound) =="
cargo test -p hocket-core --lib queue_view_json_window_is_bounded -- --nocapture | tee "$OUT/queue-json-size.txt"

echo "== cargo bench -p hocket-core --bench perf =="
cargo bench -p hocket-core --bench perf | tee "$OUT/criterion-perf.txt"
if [[ -d "$ROOT/target/criterion" ]]; then
  mkdir -p "$OUT/criterion"
  cp -a "$ROOT/target/criterion/." "$OUT/criterion/"
fi

echo "== vitest bench =="
(cd "$ROOT/desktop" && pnpm exec vitest bench --run) | tee "$OUT/vitest-bench.txt"

echo "reports in $OUT"
