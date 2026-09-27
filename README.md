# Wirebolt

A blazing-fast, native HTTP client for macOS: offline-first, Git-friendly, and free of telemetry.

Wirebolt currently includes a native request composer, incremental response viewer, versioned
file-based workspaces, environment variables, Keychain-backed secrets, layered proxy policy, and
explicit Git collaboration. Workspaces live locally and remain readable, deterministic TOML.
Git status, pull, commit, and push run only when requested; conflicts are surfaced without automatic
resolution, and commits created by Wirebolt include only its managed workspace documents.

Keyboard shortcuts:

- `⌘↩` sends the current request.
- `⌘S` saves it to the workspace.
- `⇧⌘N` creates a new request draft (`⌘N` keeps the native New Window behavior).
- `⇧⌘G` opens Git collaboration.

## Development

Requirements: macOS, `mise`, Rust 1.98.0, and Xcode 26.6.

```sh
./scripts/check.sh
./scripts/build-app.sh
open build/Wirebolt.app
```

Measure the performance contract on an idle Apple Silicon Mac:

```sh
./scripts/performance.sh check
```

Performance budgets distinguish the response engine's cold first viewport (50 ms)
from a complete native window's first content (300 ms, including AppKit/SwiftUI
construction and, for JSON responses, formatting). Native workloads in
`scripts/check.sh` enforce every reported budget, including initial display and
resizing, cold renderer switches, and scroll over 100,000-row documents. They also
check narrow/split-panel controls and preserve the reading position across wrapping
changes. Large response viewers draw a bounded first viewport while completing
the file index in the background. Narrow editor groups place the response below
the request to keep all controls reachable.

Benchmark the Rust HTTP engine over loopback:

```sh
./scripts/http-benchmark.sh
```

## Beta releases

On an Apple Silicon Mac with Python 3 and the authenticated GitHub CLI:

```sh
./scripts/package-beta.sh 0.1.0-beta.2
./scripts/publish-beta.sh 0.1.0-beta.2
```

Use a new version for each build. Packages target Apple Silicon and macOS 15 or later.
The package contains the app and dependency notices. The publisher uploads only the ZIP
and Cask to `Christopher96u/homebrew-tap`; the source repository remains private.
Release notes and the distribution repository description stay empty. An interrupted
publication can be retried with the same package without replacing an existing asset.
These betas use ad-hoc signing, so macOS requires approval on first launch.

### Proxy settings

Configure **Settings → Network** for the app default on this Mac, use the
**Workspace Settings** gear for a workspace override, or open a request's
**Settings** tab for a request override. The precedence is request → workspace
→ app default. New installations use the system proxy.

Choose **Inherit**, **System**, **Direct — No proxy**, or **Manual**. Inherit is
available at workspace and request scope; Direct bypasses parent proxies, and
System explicitly uses macOS settings. Manual supports HTTP, HTTPS and SOCKS,
with separate routes for HTTP and HTTPS destinations. An unmatched destination
connects directly; proxy connection failures never trigger a direct retry.

Save app/workspace changes using **Save**. For a request, use **Apply to request**
and then save the request with ⌘S. The indicator beside Send shows the current
policy and its source; the response's **Request** tab records the policy captured
for that execution. HTTP and WebSocket execution and Copy cURL use the same
inheritance. Reconnect an open WebSocket to apply a new policy.

App defaults stay local. Workspace/request proxy definitions are shareable files;
usernames and passwords are kept in Keychain, with only references in those files.
Use **Test connection** with an explicit URL for a cancellable HEAD probe without
request auth, headers, body or cookies. Host exclusions are not yet exposed.

### Sidebar ordering

Drag a request or subfolder to the upper or lower edge of a sibling row to reorder
items in the same folder. The insertion line shows the destination. Requests and
subfolders can be mixed, and their order is saved in the workspace TOML files.
Dropping in the center of a folder still moves the item into that folder.

### Markdown notes

Each request's Note tab offers **Edit** and **Preview**. The original Markdown is
saved with the request (Cmd+S) in its existing TOML `note` field. Preview supports
headings, emphasis, nested lists, quotes, links and fenced code using native text.
Images show their alternative text; previews do not fetch remote resources or run HTML.

Parsing runs off the main actor only while Preview is visible. The content cache
retains at most eight previews within an 8 MiB estimated memory budget; larger
previews remain viewable but are not cached. Editing does not continuously render
Markdown, and hidden notes are not parsed when switching requests or tabs.

### Workspace commands and help

Use **File → New Workspace / Open Workspace** (⌘O) to switch folders, and
**New Collection** to create a top-level collection. Unsaved request edits require
an explicit discard decision before switching. A failed open preserves the current
workspace. The last chosen folder opens on the next launch unless `--workspace`
provides an explicit path.

**Workspace → Git Collaboration** (⇧⌘G) exposes status, commit, pull and push.
Configure the repository and remote with Git first. **Help → Wirebolt Help**
provides offline instructions for requests, variables, proxy, export and shortcuts.

**Export Wirebolt JSON** preserves API Key and OAuth configuration with typed
Wirebolt authentication metadata and secret references. Reimport via
**Wirebolt / Legacy Collection v1 JSON**. Other clients may not understand the
Wirebolt extensions; exports do not resolve Keychain secrets or include history.
Workspace transport settings save automatically, even when the settings sheet closes.
