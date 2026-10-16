# Contributing to KVN

Thanks for taking the time to contribute. This document covers the
practical bits; the architecture lives in [`docs/`](docs/) (ADRs included).

## Ground rules

- **Pull requests only** — `main` is protected, history is linear, and
  pull requests are squash-merged.
- **[Conventional Commits](https://www.conventionalcommits.org/)** — the PR
  title becomes the squash commit subject, and it drives releases
  (`feat:` → minor, `fix:` → patch, `!` → major). CI checks it.
- **DCO** — every commit needs a `Signed-off-by` line. Use
  `git commit -s`; CI checks it (Dependabot is exempt).
- Be kind: see [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
- Security issues go through [`SECURITY.md`](SECURITY.md), never a public
  issue.

## Development setup

Requirements: Rust (the pinned toolchain in `rust-toolchain.toml` is
installed automatically by rustup), Node 22+ with pnpm 10, and on Linux the
WebKitGTK build dependencies (see `.github/workflows/ci.yml`).

```bash
# 1. Engine sidecar: release builds bundle a pinned Xray-core (ADR 0005).
#    Fetch + verify it once (SHA-256 checked against core/xray.json):
./scripts/fetch-xray.sh --bundle        # binary for the Tauri bundle
./scripts/fetch-xray.sh                 # engine in .tools/ for live tests

# 2. Frontend + app
cd apps/desktop
pnpm install
pnpm build                              # type-check + bundle UI
cd ../.. && cargo run -p kvn-desktop    # or: cd apps/desktop && pnpm tauri dev
```

## Checks before pushing

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                  # engine integration tests use .tools/xray-*/xray
cd apps/desktop && pnpm build
```

On macOS and Linux, `XRAY_BIN=.tools/xray-<platform>/xray cargo test -p
kvn-engine` pins the engine explicitly.

## Project layout

- `crates/kvn-core` — profiles, subscription parsing, Xray config
  generation, the connection state machine (no I/O).
- `crates/kvn-engine` — engine process management, health/traffic probes,
  verified installation, the pin manifest (`core/xray.json`).
- `apps/desktop` — Tauri shell (Rust) and the vanilla-TS frontend.
- `core/xray.json` — the engine pin: bumping the engine is a one-file PR
  (usually opened automatically by `.github/workflows/core-update.yml`).
- `docs/adr/` — architecture decision records; read the relevant one before
  large changes, and add a new one for architectural shifts.

## Releasing

Releases are automated: merge conventional commits to `main`,
release-plz opens the release PR, merging it tags the release and
creates a *draft* GitHub release; `.github/workflows/release.yml` builds
attested artifacts for all platforms into that draft, and publishing the
draft is the promotion step (the published bytes are exactly the built
ones). Maintainers do this; nothing to do in normal contributions.

Notes for maintainers:

- The release PR is opened with the repository `GITHUB_TOKEN`, so GitHub
  does not trigger CI on it and the `ci-gate` check never reports; merge it
  with the ruleset bypass. The PR contains only the version bump, the
  regenerated `Cargo.lock` and the `CHANGELOG.md` entry produced by
  release-plz.
- Merging the release PR makes release-plz create the tag and the draft
  release and dispatch `release.yml` for that tag. If the automatic
  dispatch ever fails, rebuild manually with
  `gh workflow run release.yml -f tag=vX.Y.Z`.
- Once the draft has all platform assets (roughly 30–40 minutes), review
  the checksums and publish the draft.
