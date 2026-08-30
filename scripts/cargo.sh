#!/usr/bin/env bash
set -euo pipefail

if command -v cargo >/dev/null 2>&1; then
  exec cargo "$@"
fi

if command -v mise >/dev/null 2>&1; then
  exec mise exec -- cargo "$@"
fi

echo "error: install Rust with mise before running this command" >&2
exit 1
