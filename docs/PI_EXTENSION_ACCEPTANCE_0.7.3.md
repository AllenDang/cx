# pi-cx 0.7.3 acceptance

Target matrix: macOS, Linux, and Windows on arm64/x86_64.  
Language pack: 1.16.1.  
Schema: 1.

Required local gates:

- `cargo check`
- `cargo fmt --all -- --check`
- `cargo test --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `npm run typecheck:pi`
- `npm run test:pi`
- native staged asset install and `npm run verify:pi`

Release CI builds the six normal cx assets and six matching Pi assets. Native host/target combinations run executable smoke checks; cross targets verify the target-specific file set, manifest, byte sizes, and digests. Postinstall repeats native binary version/schema verification on the destination machine. Release CI intentionally does not publish crates.io or update Homebrew.

Record the immutable tag, workflow run, six asset URLs, and representative clean-machine installs here after release.
