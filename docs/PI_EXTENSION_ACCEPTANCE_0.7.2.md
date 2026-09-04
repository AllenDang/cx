# pi-cx 0.7.2 local acceptance

Date: 2026-04-10  
Source baseline: working tree based on cx 0.7.2  
Target: darwin/arm64  
Language pack: 1.3.1  
Schema: 1

Verified locally:

- `cargo test --all`: 340 tests passed (194 unit plus integration suites).
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `npm run typecheck:pi`: passed.
- `npm run test:pi`: 14 tests passed.
- staged asset build produced `pi-cx-aarch64-apple-darwin.tar.gz` and SHA-256.
- `PI_CX_ASSET_DIR` staged install, per-file verification, `cx --version`, schema probe, grammar seed/repair test, real `cx_definition`, and `/cx-status` report passed.
- generated `vendor/pi-cx/` and `dist/` are ignored and are not tracked.

Release-only gates remain intentionally unclaimed until an immutable tag exists: GitHub asset URL installation without `PI_CX_ASSET_DIR`, provenance/attestation, new-machine Pi SDK smoke, and a recorded network-disabled nine-language fixture run.
