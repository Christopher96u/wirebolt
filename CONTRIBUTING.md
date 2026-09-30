# Contributing to Wirebolt

Thanks for helping improve Wirebolt. Useful contributions include reproducible bug reports, documentation corrections, accessibility improvements and focused fixes.

## Before contributing code

Wirebolt uses the [MIT License](LICENSE). Submit only material you have the right to contribute under that license. Third-party dependencies keep their own license terms.

For a substantial feature, open a feature request explaining the workflow and expected behavior before building it. Small documentation corrections can be proposed directly.

## Report a bug

Use the [bug report form](https://github.com/Christopher96u/wirebolt/issues/new/choose). Include version/commit, macOS, chip, reproduction steps and expected versus actual behavior. Use demo data and redact credentials. Report vulnerabilities privately through [SECURITY.md](SECURITY.md), never in a public issue.

## Work locally

1. Fork the repository and create a branch from `main` for a focused change.
2. Follow [development setup](docs/development.md).
3. Add regression coverage for behavior changes; preserve the performance contract.
4. Run the relevant checks and `./scripts/check.sh` before requesting review of an app change.
5. Open a pull request against `main` explaining the user-visible problem, resulting behavior and validation.

For documentation-only changes, verify links, code examples, screenshots and UI labels. App benchmarks do not need to be rerun solely because prose changed.

## Review expectations

Keep changes scoped. Do not include credentials, real response data, generated bindings or build output. Document compatibility limits and avoid unsupported performance claims. Include before/after screenshots for interface changes.

Use clear, single-line commit subjects. Do not add `Co-authored-by` trailers to project commits.
