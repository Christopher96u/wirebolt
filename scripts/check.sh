#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/lint.sh
scripts/test.sh
scripts/performance.sh smoke
scripts/build-app.sh
