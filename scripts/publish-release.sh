#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
version="${1:-}"
if [[ $# -ne 1 || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(rc|beta)\.[1-9][0-9]*)?$ ]]; then
  echo "usage: scripts/publish-release.sh 1.0.0 | 1.0.0-rc.1" >&2
  exit 1
fi

repository="Christopher96u/wirebolt"
tag="v$version"
output="$repo_dir/build/releases/$version"
archive="$output/Wirebolt-$version-arm64.zip"
notices="$output/Wirebolt-$version-ThirdPartyNotices.txt"
recipe="$output/wirebolt.rb"
[[ -f "$archive" && -f "$notices" && -f "$recipe" && -f "$output/source" ]] || {
  echo "error: run scripts/package-release.sh $version first" >&2
  exit 1
}
read -r source_commit source_state <"$output/source"
[[ "$source_state" == clean ]] || {
  echo "error: the package was built from a dirty tree; package again from a clean main" >&2
  exit 1
}
cmp "$recipe" <(python3 scripts/cask.py "$version" "$archive")
[[ "$(gh repo view "$repository" --json visibility --jq .visibility)" == PUBLIC ]] || {
  echo "error: $repository must be public before publishing" >&2
  exit 1
}

prerelease=--prerelease=false
latest=--latest=true
if [[ "$version" == *-* ]]; then
  prerelease=--prerelease=true
  latest=--latest=false
fi

stage="$(mktemp -d "$repo_dir/build/.publish.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
python3 - "$version" CHANGELOG.md >"$stage/notes.md" <<'PY'
import re, sys
version, path = sys.argv[1:]
lines = open(path, encoding="utf-8").read().splitlines()
heading = re.compile(r"## " + re.escape(version) + r"(\s.*)?")
start = next((i for i, line in enumerate(lines) if heading.fullmatch(line)), None)
if start is None:
    raise SystemExit(f"error: CHANGELOG.md has no '## {version}' section")
end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))
notes = "\n".join(lines[start + 1:end]).strip()
if not notes:
    raise SystemExit(f"error: the CHANGELOG.md section for {version} is empty")
print(notes)
print("""
## Install

Requires an Apple Silicon Mac with macOS 15 or later.

```sh
brew tap christopher96u/wirebolt https://github.com/Christopher96u/wirebolt
brew install --cask wirebolt
```

Or download the ZIP below. Wirebolt is ad-hoc signed and not notarized; see
[installation](https://github.com/Christopher96u/wirebolt/blob/main/docs/installation.md) for first-launch steps.""")
PY

[[ "$(git symbolic-ref --short HEAD)" == main ]] || {
  echo "error: publish from the main branch" >&2
  exit 1
}
[[ -z "$(git status --porcelain)" ]] || {
  echo "error: the working tree must be clean" >&2
  exit 1
}
git fetch --quiet origin main
remote_main="$(git rev-parse origin/main)"
remote_tag="$(git ls-remote origin "refs/tags/$tag")"
if [[ -n "$remote_tag" ]]; then
  # Fails rather than overwriting a different local tag.
  git fetch --quiet origin "refs/tags/$tag:refs/tags/$tag"
fi

check_release_commit() {
  # The release commit must add exactly the packaged Cask on top of the packaged source.
  local commit="$1"
  [[ "$(git rev-parse "$commit^")" == "$source_commit" ]] || {
    echo "error: $tag does not follow the packaged commit $source_commit" >&2
    exit 1
  }
  [[ "$(git diff --name-only "$commit^" "$commit")" == Casks/wirebolt.rb ]]
  cmp "$recipe" <(git show "$commit:Casks/wirebolt.rb")
}

if [[ -n "$remote_tag" ]]; then
  # Already pushed: verify it and resume at the GitHub release.
  release_commit="$(git rev-parse "refs/tags/$tag^{commit}")"
  check_release_commit "$release_commit"
  git merge-base --is-ancestor "$release_commit" "$remote_main" || {
    echo "error: $tag is not on origin/main" >&2
    exit 1
  }
  [[ "$(git rev-parse HEAD)" == "$remote_main" ]] || {
    echo "error: local main must equal origin/main" >&2
    exit 1
  }
else
  if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    # A previous run committed and tagged locally but did not push.
    release_commit="$(git rev-parse "refs/tags/$tag^{commit}")"
    [[ "$(git rev-parse HEAD)" == "$release_commit" && "$source_commit" == "$remote_main" ]] || {
      echo "error: local $tag is not an unpushed release commit on top of origin/main" >&2
      exit 1
    }
    check_release_commit "$release_commit"
  else
    [[ "$(git rev-parse HEAD)" == "$remote_main" ]] || {
      echo "error: local main must equal origin/main" >&2
      exit 1
    }
    [[ "$(git rev-parse HEAD)" == "$source_commit" ]] || {
      echo "error: main is not the packaged commit $source_commit; package again" >&2
      exit 1
    }
    mkdir -p Casks
    cp "$recipe" Casks/wirebolt.rb
    git add -- Casks/wirebolt.rb
    git diff --cached --check
    [[ "$(git diff --cached --name-only)" == Casks/wirebolt.rb ]]
    git commit --quiet -m "Release $tag"
    git tag -a "$tag" -m "Wirebolt $version"
    release_commit="$(git rev-parse HEAD)"
  fi
  git push --atomic origin HEAD:refs/heads/main "refs/tags/$tag"
fi

if ! gh release view "$tag" --repo "$repository" >/dev/null 2>&1; then
  gh release create "$tag" --repo "$repository" --verify-tag --draft "$prerelease" \
    --title "Wirebolt $version" --notes-file "$stage/notes.md"
fi
mkdir "$stage/download"
for asset in "$archive" "$notices"; do
  asset_name="$(basename "$asset")"
  if gh release view "$tag" --repo "$repository" --json assets --jq '.assets[].name' | grep -Fxq "$asset_name"; then
    gh release download "$tag" --repo "$repository" --pattern "$asset_name" --dir "$stage/download"
    cmp "$asset" "$stage/download/$asset_name"
  else
    gh release upload "$tag" "$asset" --repo "$repository"
  fi
done
gh release edit "$tag" --repo "$repository" --draft=false "$prerelease" "$latest" \
  --title "Wirebolt $version" --notes-file "$stage/notes.md"
printf 'Published https://github.com/%s/releases/tag/%s\n' "$repository" "$tag"
printf 'brew tap christopher96u/wirebolt https://github.com/Christopher96u/wirebolt\nbrew install --cask wirebolt\n'
