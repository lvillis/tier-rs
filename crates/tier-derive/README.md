# tier-derive

`tier-derive` provides `#[derive(TierConfig)]` for the
[`tier`](https://docs.rs/tier) Rust configuration library.

Use it when you want config metadata to stay next to your `serde` types instead
of being repeated in manual builder code. It derives env names, aliases,
examples, validation rules, merge behavior, and secret handling from the same
type definitions you already use for deserialization.

It generates structured `TierMetadata` implementations for nested config
structs, newtype wrappers, and serde-driven enums.

Supported metadata includes:

- field attributes such as `secret`, `env`, `doc`, `example`, `deprecated`,
  and `merge`
- declared validation such as `non_empty`, numeric and length bounds,
  `one_of`, hostname/IP/socket/path rules, and cross-field checks
- serde-aware naming and shape rules including `rename`, `rename_all`,
  `rename_all_fields`, `alias`, `default`, tagging, `skip`, and `flatten`
- automatic `tier::Secret<T>` detection

Most users should depend on `tier` with the `derive` feature enabled and use
the re-exported `tier::TierConfig` derive macro.
