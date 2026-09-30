#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

version="${1:-}"
if [[ $# -ne 1 || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(rc|beta)\.[1-9][0-9]*)?$ ]]; then
  echo "usage: scripts/package-release.sh 1.0.0 | 1.0.0-rc.1" >&2
  exit 1
fi
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo "error: package this release on an Apple Silicon Mac" >&2
  exit 1
fi
if [[ "$(git rev-parse --is-shallow-repository)" != false ]]; then
  echo "error: the build number is the commit count; fetch the full history first" >&2
  exit 1
fi

output="$repo_dir/build/releases/$version"
if [[ -e "$output" ]]; then
  echo "error: $output already exists; remove it or use a new version" >&2
  exit 1
fi
source_commit="$(git rev-parse HEAD)"
source_state=clean
[[ -z "$(git status --porcelain)" ]] || source_state=dirty
build_number="$(git rev-list --count HEAD)"

mkdir -p "$repo_dir/build/releases"
stage="$(mktemp -d "$repo_dir/build/.release.XXXXXX")"
trap 'rm -rf "$stage"' EXIT

app="$stage/Wirebolt.app"
WIREBOLT_APP_DIR="$app" scripts/build-app.sh
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${version%%-*}" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $build_number" "$app/Contents/Info.plist"
scripts/cargo.sh metadata --format-version 1 --locked --filter-platform aarch64-apple-darwin >"$stage/dependencies.json"
python3 scripts/release-notices.py "$stage/dependencies.json" "$app/Contents/Resources/ThirdPartyNotices.txt"
codesign --force --sign - --timestamp=none "$app"
codesign --verify --deep --strict "$app"

python3 - "$app/Contents/MacOS/Wirebolt" <<'PY'
import subprocess, sys
binary = sys.argv[1]
assert subprocess.check_output(["lipo", "-archs", binary], text=True).strip() == "arm64"
for line in subprocess.check_output(["otool", "-L", binary], text=True).splitlines()[1:]:
    dependency = line.strip().split(" (", 1)[0]
    if not dependency.startswith(("/System/Library/", "/usr/lib/")):
        raise SystemExit(f"Unbundled dependency: {dependency}")
PY

mkdir "$stage/release"
archive="Wirebolt-$version-arm64.zip"
ditto --norsrc --noextattr --noacl --noqtn -c -k --keepParent "$app" "$stage/release/$archive"
cp "$app/Contents/Resources/ThirdPartyNotices.txt" "$stage/release/Wirebolt-$version-ThirdPartyNotices.txt"
checksum="$(shasum -a 256 "$stage/release/$archive" | cut -d ' ' -f 1)"
python3 scripts/cask.py "$version" "$stage/release/$archive" >"$stage/release/wirebolt.rb"
printf '%s %s\n' "$source_commit" "$source_state" >"$stage/release/source"
mv "$stage/release" "$output"
printf 'Package: %s\nBuild: %s (%s)\nSource: %s (%s)\nSHA-256: %s\n' \
  "$output/$archive" "${version%%-*}" "$build_number" "$source_commit" "$source_state" "$checksum"
