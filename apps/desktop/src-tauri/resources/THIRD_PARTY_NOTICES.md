# Third-party notices

KVN itself is distributed under the MIT License (see `LICENSE`). Release
artifacts additionally contain the following third-party components.

## Xray-core (bundled engine sidecar)

Bundled since 0.1.0 as the `xray` sidecar next to the application binary.
The exact release tag and the SHA-256 digests of the upstream archives are
pinned in `core/xray.json` of the source repository; the shipped binary is
extracted unmodified from the official upstream release archive after
digest verification.

- Project: <https://github.com/XTLS/Xray-core>
- License: Mozilla Public License 2.0 —
  full text: [`licenses/xray-core-MPL-2.0.txt`](licenses/xray-core-MPL-2.0.txt)
- Corresponding source: the pinned release tag on the project page above.

Xray-core is used as a separate executable (sidecar); it is not linked into
KVN's own code.

## Geist Mono (interface font)

Self-hosted webfont for the terminal-style interface.

- Project: <https://github.com/vercel/geist-font>
- License: SIL Open Font License 1.1 —
  full text: [`licenses/geist-mono-OFL-1.1.txt`](licenses/geist-mono-OFL-1.1.txt)
