set shell := ["bash", "-euo", "pipefail", "-c"]

ci:
  cargo fmt --all --check
  cargo check --workspace --all-features --locked
  cargo check --workspace --locked
  cargo check --workspace --no-default-features --locked
  cargo check --workspace --no-default-features --features clap --locked
  cargo check --workspace --no-default-features --features json --locked
  cargo check --workspace --no-default-features --features schema --locked
  cargo check --workspace --no-default-features --features watch --locked
  cargo nextest run --workspace --all-features --locked
  cargo nextest run --workspace --locked
  cargo nextest run --workspace --no-default-features --locked --no-tests pass
  cargo nextest run --workspace --no-default-features --features clap --locked
  cargo nextest run --workspace --no-default-features --features schema --locked
  cargo bench -p tier --bench core --no-run --locked
  cargo bench -p tier --bench core --all-features --no-run --locked
  cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
  cargo test --workspace --doc --all-features --locked
  cargo doc --workspace --no-deps --all-features --locked

bench:
  cargo bench -p tier --bench core --locked
  cargo bench -p tier --bench core --all-features --locked

patch:
  cargo release patch --no-publish --execute
