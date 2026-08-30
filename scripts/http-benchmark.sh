#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

exec scripts/cargo.sh bench -p wirebolt-core --bench http_engine
