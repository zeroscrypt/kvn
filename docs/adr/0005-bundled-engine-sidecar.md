# ADR 0005 — Engine distribution: bundled sidecar with a pinned manifest

- Status: accepted (2026-10-16)
- Supersedes: the "install on first run" flow of
  [ADR 0003](0003-engine-distribution-pinned-release.md); the pinning and
  digest verification principles of ADR 0003 remain in force.

## Context

ADR 0003 pinned Xray-core to an exact release with SHA-256 verification and
installed it in-app. That kept releases clean but made every first
connection depend on a ~10 MB download from GitHub, and gave the app no
engine at all when GitHub was unreachable or blocked — the exact networks
this tool exists for.

## Decision

1. **Bundled sidecar.** Release artifacts ship the pinned Xray-core as a
   Tauri `externalBin` sidecar (`xray` next to the main binary). Builds
   fetch it with `scripts/fetch-xray.sh --bundle`; CI runs that before any
   desktop build. Sidecar binaries are build inputs and are never committed.
2. **Single-source manifest.** `core/xray.json` becomes the only place that
   pins the engine: release tag, repository, per-platform archive names,
   SHA-256 digests (from the release `.dgst` files) and Rust target
   triples. The Rust crate embeds it at compile time, and the fetch/update
   scripts and CI read the same file.
3. **Discovery order** stays deterministic: user-supplied path (ADR 0003)
   → engine installed in the app-data directory → bundled sidecar →
   `xray` on `PATH`. The Settings panel names the active source.
4. **In-app installer remains** as the update/repair path for the installed
   copy; it always installs the pinned release, digest-verified.
5. **Update transparency, not silent updates.** Settings shows the pinned
   version and runs an informational check against the latest Xray-core
   release on GitHub. Engine upgrades arrive with a new KVN release, not
   through the app replacing its own tunnel engine. A scheduled workflow
   (`.github/workflows/core-update.yml`) detects new upstream releases,
   refreshes the manifest digests and opens a reviewed bump PR.

## Consequences

- First run works offline, with no silent downloads, on supported
  platforms.
- Building from source now requires fetching the sidecar once; this is
  documented and automated in CI.
- MPL-2.0 compliance for Xray-core: license text and third-party notices
  ship in the bundle resources (`THIRD_PARTY_NOTICES.md`).
- Engine bumps are deliberate, reviewed and reproducible: the workflow
  changes exactly one file (`core/xray.json`), and the app reports the
  pinned version in Settings and diagnostics.
