#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/lint.sh
scripts/test.sh
scripts/performance.sh smoke
scripts/editor-performance.sh
scripts/build-app.sh
scripts/native-performance.sh
build/performance-contract --launch-smoke build/Wirebolt.app/Contents/MacOS/Wirebolt
