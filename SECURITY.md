# Security policy

## Report a vulnerability

Report privately through [GitHub private vulnerability reporting](https://github.com/Christopher96u/wirebolt/security/advisories/new) (**Security → Report a vulnerability**). Do not open public issues, discussions or pull requests for vulnerabilities.

Include:

- Wirebolt version (**About Wirebolt**, for example `1.0.0 (35)`) or source commit, macOS version and chip
- Steps to reproduce with synthetic data, expected versus actual behavior and impact
- Proof of concept, if you have one

Never include live credentials, cookies, private workspace files or real response bodies.

## Supported versions

Only the latest [release](https://github.com/Christopher96u/wirebolt/releases), including release candidates, is supported. Fixes land on `main` and ship in the next release.

## Response

Wirebolt is maintained by one person. The goal is to acknowledge reports within 7 days and keep you updated while the report is triaged. Once a fix is released, the advisory is published and reporters are credited unless they prefer otherwise.

## Scope

Wirebolt is local-first: no accounts, telemetry or cloud sync. In scope:

- Secret material leaking out of Keychain into workspace files, Git, history, logs, exports or raw-request views
- Unsafe handling of imported files (cURL, HAR, Postman, Wirebolt JSON and other supported formats)
- Proxy, TLS validation or certificate handling that differs from the configured policy
- Markdown notes loading remote content, or Markdown or HTML response previews executing scripts

Out of scope: servers you send requests to, issues requiring an already compromised Mac or user account, and the ad-hoc signed (not notarized) packaging, which is documented.
