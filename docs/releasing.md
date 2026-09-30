# Releasing Wirebolt

[Documentation](README.md) · Maintainer guide

Releases are published from this repository: the ZIP is attached to a GitHub Release, and the Homebrew Cask lives at `Casks/wirebolt.rb`, so the repository doubles as the tap. Versions are `X.Y.Z` or prereleases `X.Y.Z-rc.N` (`X.Y.Z-beta.N` is still accepted). Prereleases are marked as such on GitHub.

`Casks/` does not exist until the first release is published. Until then, `brew tap` succeeds but installing reports that the cask does not exist. Homebrew 6 and later refuse short names from untrusted third-party taps, so install instructions use the full name `christopher96u/wirebolt/wirebolt`, which trusts only this cask.

## Prepare

1. Run `./scripts/check.sh` on the candidate revision.
2. Verify installation and a representative request on the supported macOS target.
3. Rename the Unreleased changelog section to `## <version>` and add a new empty `## Unreleased` above it. The publish script uses that section as the release notes.
4. Set the Cargo workspace version in `Cargo.toml` to the release version and the base version (`X.Y.Z`) in `apple/WireboltApp/Info.plist`.
5. Check screenshots and docs against the packaged revision.
6. Confirm GitHub private vulnerability reporting is enabled (**Settings → Code security**); [SECURITY.md](../SECURITY.md) links to it.
7. Merge to `main`, then check out a clean `main` equal to `origin/main`.

## Package

On an Apple Silicon Mac with Python 3:

```sh
./scripts/package-release.sh 1.0.0-rc.1
```

The script refuses to overwrite an existing output directory. It builds an arm64 app, sets `CFBundleShortVersionString` to `X.Y.Z` and `CFBundleVersion` to the commit count, includes third-party notices, verifies ad-hoc signing and writes the ZIP, notices, Cask and source commit under `build/releases/<version>/`.

Packages are ad-hoc signed, not notarized. The Cask removes the quarantine attribute in `postflight_steps` (Homebrew 6.0.16 or later) and says so in its caveats. Do not describe builds as notarized until the distribution process changes.

## Publish deliberately

Publishing requires an authenticated GitHub CLI, a public repository, and a clean `main` equal to `origin/main` at the packaged commit:

```sh
./scripts/publish-release.sh 1.0.0-rc.1
```

The script commits `Casks/wirebolt.rb` as `Release v<version>`, tags `v<version>`, pushes `main` and the tag together, then creates the GitHub Release with the ZIP and notices. Release notes come from the matching changelog section plus installation instructions. Prereleases are not marked as latest.

Re-running is safe: an existing tag or release is verified against the package instead of being replaced, missing assets are uploaded, and uploaded assets are compared byte for byte. If `main` moved after packaging, the script stops; package again.

## Verify

```sh
brew tap christopher96u/wirebolt https://github.com/Christopher96u/wirebolt
brew install --cask christopher96u/wirebolt/wirebolt
```

Open Wirebolt and check **About Wirebolt**. Also download the ZIP from the release page and confirm the first-launch steps in [installation](installation.md#first-launch).
