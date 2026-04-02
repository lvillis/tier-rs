set shell := ["bash", "-euo", "pipefail", "-c"]

ci:
  cargo fmt --all --check
  cargo check --workspace --all-features
  cargo check --workspace
  cargo check --workspace --no-default-features
  cargo check --workspace --no-default-features --features clap
  cargo check --workspace --no-default-features --features json
  cargo check --workspace --no-default-features --features schema
  cargo check --workspace --no-default-features --features watch
  cargo nextest run --workspace --all-features
  cargo nextest run --workspace
  cargo nextest run --workspace --no-default-features --no-tests pass
  cargo nextest run --workspace --no-default-features --features clap
  cargo nextest run --workspace --no-default-features --features schema
  cargo bench -p tier --bench core --no-run
  cargo bench -p tier --bench core --all-features --no-run
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace --doc --all-features
  cargo doc --workspace --no-deps --all-features

bench:
  cargo bench -p tier --bench core
  cargo bench -p tier --bench core --all-features

patch:
  cargo release patch --no-publish --execute

publish:
  cargo publish --workspace
