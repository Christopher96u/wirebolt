#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

# Timing benchmarks target local Apple Silicon. GitHub's virtualized runners are several
# times slower, so CI skips them; run the full gate locally before review and release.
benchmarks=true
if [[ "${GITHUB_ACTIONS:-}" == true || "${WIREBOLT_SKIP_BENCHMARKS:-}" == 1 ]]; then
  benchmarks=false
fi

scripts/lint.sh
scripts/test.sh
# Checks the bridge and builds the harness for the launch smoke; it enforces no timings.
scripts/performance.sh smoke
if [[ "$benchmarks" == true ]]; then
  scripts/editor-performance.sh
fi
scripts/build-app.sh
if [[ "$benchmarks" == true ]]; then
  scripts/native-performance.sh
fi
build/performance-contract --launch-smoke build/Wirebolt.app/Contents/MacOS/Wirebolt
