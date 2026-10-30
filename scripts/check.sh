#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/lint.sh
scripts/test.sh
scripts/performance.sh smoke
scripts/editor-performance.sh
scripts/build-app.sh
# Native workloads enforce timings for local Apple Silicon. GitHub's virtualized runners
# are several times slower, so CI skips them; run the full gate locally before release.
if [[ "${GITHUB_ACTIONS:-}" != true && "${WIREBOLT_SKIP_NATIVE_PERFORMANCE:-}" != 1 ]]; then
  scripts/native-performance.sh
fi
build/performance-contract --launch-smoke build/Wirebolt.app/Contents/MacOS/Wirebolt
