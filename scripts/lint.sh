#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

scripts/cargo.sh fmt --all --check
scripts/cargo.sh clippy --workspace --all-targets --all-features -- -D warnings
shellcheck scripts/*.sh
shfmt -d -i 2 -ci scripts
