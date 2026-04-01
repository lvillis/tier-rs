# tier

`tier` is a Rust configuration library for typed, layered application config.

It is built for projects that want one `serde` config type fed by code
defaults, TOML files, environment variables, and CLI overrides.

By default, `tier` only enables TOML support. `derive`, `clap`, `schema`,
`watch`, `json`, and `yaml` stay opt-in.

## What It Does

- Loads config from defaults, files, env, and CLI in a predictable order
- Deserializes directly into typed Rust structs
- Treats env and `--set` inputs as strings first, with primitive coercion at deserialize time
- Tracks where each value came from and surfaces validation and warning output
- Optionally adds derive metadata, schema/docs export, and runtime reload

## Quick Start

```rust,no_run
use serde::{Deserialize, Serialize};
use tier::ConfigLoader;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}

let loaded = ConfigLoader::new(AppConfig::default())
    .file("config/app.toml")
    .load()?;

assert_eq!(loaded.config().port, 3000);
# Ok::<(), tier::ConfigError>(())
```

## Feature Flags

- `toml`: TOML file parsing and commented TOML examples
- `derive`: `#[derive(TierConfig)]` metadata generation
- `clap`: reusable config flags and diagnostics commands
- `schema`: JSON Schema, env docs, and machine-readable reports
- `watch`: native filesystem watcher backend
- `json`: JSON file parsing
- `yaml`: YAML file parsing

## Examples And Docs

- Crate documentation: [`crates/tier/README.md`](./crates/tier/README.md)
- Examples: [`crates/tier/examples`](./crates/tier/examples)
- API docs: <https://docs.rs/tier>

## Scope

Current scope stays focused on typed layered loading, metadata, diagnostics,
validation, schema/docs output, and reload support.

Out of scope for the current crate line:

- remote configuration backends
- full application-specific CLI generation from derive metadata
