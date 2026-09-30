# Releasing a beta

[Documentation](README.md) · Maintainer guide

## Prepare

1. Run `./scripts/check.sh` on the candidate revision.
2. Verify installation and a representative request on the supported macOS target.
3. Move the appropriate Unreleased changes into a versioned changelog entry.
4. Check screenshots and docs against the packaged revision.
5. Confirm GitHub private vulnerability reporting is enabled (**Settings → Code security**); [SECURITY.md](../SECURITY.md) links to it.

## Package

On an Apple Silicon Mac with Python 3:

```sh
./scripts/package-beta.sh 0.1.0-beta.3
```

The version above is an example; choose an unused beta version. The script refuses to overwrite an existing output directory. It builds an arm64 app, sets version metadata, includes third-party notices, verifies ad-hoc signing and creates a ZIP and Homebrew Cask under `build/releases/<version>/`.

Current packages are ad-hoc signed rather than notarized. Do not describe them as notarized until the distribution process changes.

## Publish deliberately

Publishing requires an authenticated GitHub CLI and writes to the public distribution repository:

```sh
./scripts/publish-beta.sh 0.1.0-beta.3
```

This pushes a distribution tag, uploads the ZIP, publishes a prerelease and updates the Cask in `Christopher96u/homebrew-tap`. It does not tag this source repository; tag the released commit (`git tag v0.1.0-beta.3 && git push origin v0.1.0-beta.3`) so bug reports map to source. Publication can be retried without replacing an existing asset; the script compares an already uploaded archive.

**The current script creates empty release notes.** After publishing, edit the release to add the version's changelog, requirements, installation instructions and known limitations.

Verify the asset download and Homebrew installation after publication. Link prereleases through the releases list or an explicit tag rather than `releases/latest`.
