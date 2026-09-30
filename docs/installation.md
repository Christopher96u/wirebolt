# Install Wirebolt

[Documentation](README.md) · [Next: quick start](quick-start.md)

## Requirements

- macOS 15 (Sequoia) or later.
- An Apple Silicon Mac (M-series).
- Homebrew only if using the Homebrew installation method.

There is no account requirement. Network access is needed for downloads and remote APIs; workspace editing and the bundled help work locally.

## Homebrew

```sh
brew tap christopher96u/wirebolt https://github.com/Christopher96u/wirebolt
brew install --cask wirebolt
```

The tap is this source repository; its Cask lives in `Casks/wirebolt.rb`. Wirebolt is ad-hoc signed and not notarized, so the Cask removes the quarantine attribute after installing and the app opens without a Gatekeeper prompt.

To update:

```sh
brew update
brew upgrade --cask wirebolt
```

To uninstall and remove the tap:

```sh
brew uninstall --cask wirebolt
brew untap christopher96u/wirebolt
```

Add `--zap` to the uninstall command to also delete preferences, history and cookies. Workspace folders are always kept.

### Migrate from the beta tap

Betas were installed from `christopher96u/tap`, which no longer receives updates. Switch once:

```sh
brew uninstall --cask wirebolt && brew untap christopher96u/tap
brew tap christopher96u/wirebolt https://github.com/Christopher96u/wirebolt
brew install --cask wirebolt
```

Workspace folders are not affected.

## Direct download

1. Open [Wirebolt releases](https://github.com/Christopher96u/wirebolt/releases).
2. Download `Wirebolt-<version>-arm64.zip` from the release you want. Release candidates are marked **Pre-release**.
3. Extract the ZIP and move **Wirebolt.app** to **Applications**.
4. Open Wirebolt and follow the first-launch steps below.

## First launch

Downloaded builds are ad-hoc signed and not notarized, so macOS blocks the first launch:

1. Open Wirebolt. When macOS says it cannot verify the app, click **Done**.
2. Open **System Settings → Privacy & Security** and scroll to **Security**.
3. Click **Open Anyway** next to the Wirebolt message, then confirm with your password or Touch ID.

macOS remembers the approval. Only approve a package you intentionally downloaded from the releases page, and do not disable Gatekeeper globally.

If macOS reports a damaged package or offers no **Open Anyway** button, download it again and report the exact message with your macOS and app versions. [Building from source](development.md) is also available.

## Versions

The docs and screenshots follow the source branch. Features under **Unreleased** in the [changelog](../CHANGELOG.md) are not in a published build yet. To try current source changes, follow the development guide.

## Workspace location

Use **File → New Workspace…** to choose a folder or **File → Open Workspace…** to select a folder containing `wirebolt.toml`. The app remembers the last chosen workspace. Keep a backup or Git history of workspace files before trying a new release candidate.
