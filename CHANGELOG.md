# Changelog

Changes under **Unreleased** describe the source branch and may not be included in downloadable betas. Published binaries are listed in the [distribution releases](https://github.com/Christopher96u/homebrew-tap/releases). This file does not reconstruct unverified release history.

## Unreleased

### Added

- Local Markdown Edit/Preview for request notes.
- Explicit New/Open Workspace and New Collection commands.
- Reachable Git Collaboration commands and local in-app help.
- User documentation, native screenshots and a local quick-start API example.

### Changed

- The bundle identifier is now `io.github.christopher96u.wirebolt`. Preferences, the remembered workspace and Keychain items stored under `local.wirebolt.app` are not migrated: reopen the workspace and re-enter stored credentials.

### Fixed

- Saving an open request after moving it across collections targets the new location.
- Deleting a folder closes descendant request sessions.
- Pending workspace transport edits persist after settings closes.
- Wirebolt JSON export preserves API key and OAuth configuration through native metadata.
- Export failures retain more useful error messages.
- Ambiguous accessibility labels include more context.

### Clarified

- Privacy settings describe behaviors instead of presenting inactive switches.
- Advanced authentication export portability and local credential provisioning have explicit limits.
