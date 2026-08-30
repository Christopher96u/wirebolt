#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" == "Darwin" ]]; then
  export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-15.0}"
fi

if command -v cargo >/dev/null 2>&1; then
  exec cargo "$@"
fi

if command -v mise >/dev/null 2>&1; then
  exec mise exec -- cargo "$@"
fi

echo "error: install Rust with mise before running this command" >&2
exit 1
