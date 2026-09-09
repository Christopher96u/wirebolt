#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
version="${1:-}"
if [[ $# -ne 1 || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+-beta\.[1-9][0-9]*$ ]]; then
  echo "usage: scripts/publish-beta.sh 0.1.0-beta.1" >&2
  exit 1
fi

distribution_repository="Christopher96u/homebrew-tap"
author_name="$(git config user.name)"
output="$repo_dir/build/releases/$version"
archive="$output/Wirebolt-$version-arm64.zip"
recipe="$output/wirebolt.rb"
[[ -f "$archive" && -f "$recipe" ]] || {
  echo "error: run scripts/package-beta.sh first" >&2
  exit 1
}
[[ "$(gh repo view "$distribution_repository" --json isPrivate --jq .isPrivate)" == false ]] || {
  echo "error: the distribution repository must exist and be public" >&2
  exit 1
}

cmp "$recipe" <(python3 scripts/beta-cask.py "$version" "$archive")
author_email="$(gh api users/Christopher96u --jq '(.id | tostring) + "+Christopher96u@users.noreply.github.com"')"

stage="$(mktemp -d "$repo_dir/build/.publish-beta.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
gh repo clone "$distribution_repository" "$stage/tap"
cd "$stage/tap"
git config user.name "$author_name"
git config user.email "$author_email"
if git rev-parse --verify HEAD >/dev/null 2>&1; then
  git ls-files | while IFS= read -r path; do
    [[ "$path" == Casks/wirebolt.rb ]] || {
      echo "error: unexpected public file: $path" >&2
      exit 1
    }
  done
fi
if git rev-parse --verify "refs/tags/v$version" >/dev/null 2>&1; then
  if ! git rev-parse --verify HEAD >/dev/null 2>&1 || ! cmp -s "$recipe" <(git show HEAD:Casks/wirebolt.rb); then
    cmp "$recipe" <(git show "refs/tags/v$version:Casks/wirebolt.rb")
    git checkout --detach "refs/tags/v$version"
  fi
else
  mkdir -p Casks
  cp "$recipe" Casks/wirebolt.rb
  git add -- Casks/wirebolt.rb
  git diff --cached --check
  [[ "$(git diff --cached --name-only)" == Casks/wirebolt.rb ]]
  git commit -m "$version"
  [[ "$(git log -1 --format=%B)" == "$version" ]]
  git tag "v$version"
  git push origin "refs/tags/v$version"
fi
if ! gh release view "v$version" --repo "$distribution_repository" >/dev/null 2>&1; then
  gh release create "v$version" --repo "$distribution_repository" --verify-tag --prerelease --draft --title "$version" --notes ""
fi
asset_name="$(basename "$archive")"
if gh release view "v$version" --repo "$distribution_repository" --json assets --jq '.assets[].name' | grep -Fxq "$asset_name"; then
  mkdir "$stage/download"
  gh release download "v$version" --repo "$distribution_repository" --pattern "$asset_name" --dir "$stage/download"
  cmp "$archive" "$stage/download/$asset_name"
else
  gh release upload "v$version" "$archive" --repo "$distribution_repository"
fi
gh release edit "v$version" --repo "$distribution_repository" --draft=false
git push origin HEAD:refs/heads/main
printf 'brew install --cask Christopher96u/tap/wirebolt\n'
