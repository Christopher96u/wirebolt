#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
output="${1:-$repo_dir/build/docs-screenshots}"
mkdir -p "$output" build/docs-screenshots-runner
output="$(cd "$output" && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/wirebolt-docs.XXXXXX")"
server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$fixture"
}
trap cleanup EXIT

scripts/generate-bindings.sh
# shellcheck source=scripts/app-sources.sh
source scripts/app-sources.sh
swiftc -Onone -parse-as-library -swift-version 6 -target arm64-apple-macosx15.0 \
  -o build/docs-screenshots-runner/capture \
  apple/Documentation/Screenshots.swift apple/Tests/NativeTestSupport.swift "${wirebolt_app_inputs[@]}"

python3 -B - "$repo_dir" "$fixture/server-url" <<'PY' &
import importlib.util
import pathlib
import sys
from http.server import ThreadingHTTPServer

spec = importlib.util.spec_from_file_location("wirebolt_demo", pathlib.Path(sys.argv[1]) / "docs/examples/demo-server.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
with ThreadingHTTPServer(("127.0.0.1", 0), module.DemoAPI) as server:
    pathlib.Path(sys.argv[2]).write_text(f"http://127.0.0.1:{server.server_port}")
    server.serve_forever()
PY
server_pid=$!
for _ in {1..50}; do
  [[ -s "$fixture/server-url" ]] && break
  kill -0 "$server_pid" 2>/dev/null || exit 1
  sleep 0.1
done
[[ -s "$fixture/server-url" ]] || {
  echo "Documentation server did not start" >&2
  exit 1
}
base_url="$(cat "$fixture/server-url")"
build/docs-screenshots-runner/capture "$output" "$fixture" "$base_url"
git rev-parse HEAD >"$output/source-commit.txt"
