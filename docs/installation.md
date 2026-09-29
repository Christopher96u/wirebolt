# Install Wirebolt

[Documentation](README.md) · [Next: quick start](quick-start.md)

## Requirements

- macOS 15 (Sequoia) or later.
- An Apple Silicon Mac (M-series).
- Homebrew only if using the Homebrew installation method.

There is no account requirement. Network access is needed for downloads and remote APIs; workspace editing and the bundled help work locally.

## Homebrew

```sh
brew install --cask Christopher96u/tap/wirebolt
```

To update:

```sh
brew update
brew upgrade --cask wirebolt
```

## Direct download

1. Open [Wirebolt beta releases](https://github.com/Christopher96u/homebrew-tap/releases).
2. Choose the desired prerelease and download `Wirebolt-<version>-arm64.zip`.
3. Extract the ZIP and move **Wirebolt.app** to **Applications**.
4. Open Wirebolt.

The download repository is separate from the source repository. Choose a release explicitly: GitHub's `releases/latest` URL does not reliably select a prerelease.

## First launch

Current beta packages use ad-hoc signing and are not notarized. If macOS blocks opening the downloaded app, attempt to open it once, then use **System Settings → Privacy & Security → Open Anyway**, if offered, and confirm the system dialog. Only approve the package you intentionally downloaded from the release repository. Do not disable Gatekeeper globally.

If macOS reports a damaged package or offers no approval option, download it again and report the exact message with your macOS and app versions. [Building from source](development.md) is also available.

## Versions

The docs and screenshots follow the source branch. Features under **Unreleased** in the [changelog](../CHANGELOG.md) are not a promise about an older ZIP. To try current source changes, follow the development guide.

## Workspace location

Use **File → New Workspace…** to choose a folder or **File → Open Workspace…** to select a folder containing `wirebolt.toml`. The app remembers the last chosen workspace. Keep a backup or Git history of workspace files before trying a new beta.
