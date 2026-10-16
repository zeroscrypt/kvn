# Security Policy

## Reporting a vulnerability

Please use **GitHub's private vulnerability reporting** on this repository
("Security" tab → "Report a vulnerability"). Do not open a public issue for
security problems.

You can expect an acknowledgement within a few days. There is no bounty
program; fixes are released as soon as they are ready.

## Scope notes for this project

KVN is a client application: it stores VPN credentials locally and runs a
local proxy. The threat model, the storage rules (0600 files, no logging of
secrets) and the review checklist live in
[`docs/SECURITY.md`](docs/SECURITY.md) — please read it before reporting;
it explains what is in scope.

Known design facts, not vulnerabilities:

- **Release builds are unsigned for macOS** (no Apple Developer
  certificate). This is documented in the release notes; verify downloads
  with the published `SHA256SUMS` and the GitHub build provenance
  attestation.
- The bundled Xray-core is a **separate process** (sidecar) pinned by
  SHA-256 in `core/xray.json`; it is not linked into the application.

## Supported versions

Only the latest release receives fixes. Pre-1.0 releases are marked as
such.
