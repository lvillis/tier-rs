# tier

`tier` is a Rust configuration library for typed, layered application config.

It is designed for projects that want one `serde` config type fed by code
defaults, TOML files, environment variables, and CLI overrides, without
falling back to untyped value trees.

By default, `tier` only enables TOML file support. `derive`, `clap`, `schema`,
`watch`, `json`, and `yaml` are opt-in features.

Use `tier` when you want:

1. a Rust config library built around `serde` types
2. predictable layered config from defaults, files, env, and CLI
3. source tracing and validation instead of silent config drift
4. optional schema, docs, and reload support without a heavy default feature set

## Feature Flags

- `toml`: TOML file parsing and commented TOML examples
- `derive`: `#[derive(TierConfig)]` metadata generation
- `clap`: reusable config flags and diagnostics commands
- `schema`: JSON Schema, env docs, and machine-readable reports
- `watch`: native filesystem watcher backend
- `json`: JSON file parsing
- `yaml`: YAML file parsing

## Feature Map

- Loading: `ConfigLoader`, `FileSource`, `EnvSource`, `ArgsSource`
- Metadata: `ConfigMetadata`, `FieldMetadata`, `TierConfig`
- Diagnostics: `ConfigReport`, `doctor()`, `explain()`, `audit_report()`
- Schema and docs: `json_schema_*`, `annotated_json_schema_*`, `config_example_*`, `EnvDocOptions`
- Reload: `ReloadHandle`, `PollingWatcher`, `NativeWatcher`

## Input Semantics

- Env values and `--set key=value` overrides are string-first inputs
- Primitive targets such as `bool`, integers, floats, and `Option<T>` are coerced during deserialization
- Use explicit JSON syntax for arrays, objects, or quoted strings when you need structured inline values

## Quick Start

The smallest useful setup is defaults plus a TOML file:

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

assert!(loaded.report().doctor().contains("Sources:"));
# Ok::<(), tier::ConfigError>(())
```

## Examples

The crate ships with focused examples under [`examples/`](./examples):

- `basic.rs`: defaults + env + CLI layering
- `manual-metadata.rs`: explicit `ConfigMetadata`
- `derive.rs`: derive metadata and declarative validation
- `schema.rs`: schema, env docs, and commented TOML examples
- `clap.rs`: embedding `TierCli`
- `reload.rs`: polling reload
- `application.rs`: a fuller application setup

## Core Types

- `ConfigLoader<T>` builds a deterministic pipeline from defaults, files, env,
  CLI, and custom layers.
- `ConfigMetadata` carries env names, aliases, secrets, examples, merge rules,
  and declared validations.
- `LoadedConfig<T>` returns the final typed value with a `ConfigReport`.
- `ReloadHandle<T>` reuses the same loader closure for polling or native file
  watching.

## Highlights

- Typed loading with deterministic merge order and unknown field governance
- Metadata-driven env mapping, secret handling, and validation
- Field-level tracing, doctor output, and machine-readable audit/report data
- Optional schema/docs export, commented TOML examples, `clap`, and reload

## Example

```rust,no_run
# #[cfg(feature = "derive")] {
use serde::{Deserialize, Serialize};
use tier::{ArgsSource, ConfigLoader, EnvSource, Secret, TierConfig, ValidationErrors};

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct AppConfig {
    server: ServerConfig,
    db: DbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct ServerConfig {
    #[tier(
        env = "APP_SERVER_HOST",
        doc = "IP address or hostname to bind",
        example = "0.0.0.0"
    )]
    host: String,
    #[tier(deprecated = "use server.bind_port instead")]
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DbConfig {
    password: Secret<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 3000,
            },
            db: DbConfig {
                password: Secret::new("secret".into()),
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded = ConfigLoader::new(AppConfig::default())
        .derive_metadata()
        .file("config/default.toml")
        .optional_file("config/{profile}.toml")
        .env(EnvSource::prefixed("APP"))
        .args(ArgsSource::from_env())
        .profile("prod")
        .validator("port-range", |config| {
            if config.server.port == 0 {
                return Err(ValidationErrors::from_message(
                    "server.port",
                    "port must be greater than zero",
                ));
            }
            Ok(())
        })
        .load()?;

    println!("{}", loaded.report().doctor());
    Ok(())
}
# }
```

`derive_metadata()` applies metadata generated by `TierConfig`, including env
names, aliases, secret handling, `serde(default)` awareness, merge strategies,
declared validation rules, env docs, and deprecation warnings.

## Declarative Validation

`tier` supports metadata-driven field and cross-field validation alongside
custom validator hooks. Declared rules feed the loader, schema annotations,
env docs, and commented TOML examples from the same metadata source.

```rust
# #[cfg(feature = "derive")] {
use serde::{Deserialize, Serialize};
use tier::{ConfigError, ConfigLoader, TierConfig};

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct AppConfig {
    #[tier(non_empty, min_length = 3)]
    service_name: String,
    #[tier(min = 1, max = 65535)]
    port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            service_name: "api".to_owned(),
            port: 8080,
        }
    }
}

let error = ConfigLoader::new(AppConfig::default())
    .derive_metadata()
    .args(tier::ArgsSource::from_args([
        "app",
        "--set",
        r#"service_name="""#,
        "--set",
        "port=0",
    ]))
    .load()
    .expect_err("declared validation must fail");

assert!(matches!(error, ConfigError::DeclaredValidation { .. }));
# }
```

## Reload

`tier` always includes `ReloadHandle` and a polling watcher. With `watch`
enabled, it also exposes a native filesystem watcher. Reloads can emit
structured diffs and events, and watchers can either keep running or stop
after a failed reload.

```rust,no_run
# #[cfg(all(feature = "toml", feature = "watch"))] {
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tier::{ConfigError, ConfigLoader, ReloadEvent, ReloadHandle};

#[derive(Clone, Serialize, Deserialize)]
struct AppConfig {
    port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}

fn main() -> Result<(), ConfigError> {
    let handle =
        ReloadHandle::new(|| ConfigLoader::new(AppConfig::default()).file("app.toml").load())?;
    let _events = handle.subscribe();
    let _summary = handle.reload_detailed()?;
    let watcher = handle.start_native(["app.toml"], Duration::from_millis(100))?;
    watcher.stop();
    Ok(())
}
# }
```

## Schema Export

With `schema` enabled, `tier` can export a JSON Schema for a config type:

```rust
# #[cfg(feature = "schema")] {
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tier::json_schema_pretty;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct AppConfig {
    port: u16,
}

let schema = json_schema_pretty::<AppConfig>();
assert!(schema.contains("\"type\": \"object\""));
# }
```

## Clap Integration

With `clap` enabled, `tier` provides a reusable config flag group:

```rust
# #[cfg(feature = "clap")] {
use clap::Parser;
use tier::TierCli;

#[derive(Debug, Parser)]
struct AppCli {
    #[command(flatten)]
    config: TierCli,
}

let cli = AppCli::parse_from(["app", "--validate-config"]);
assert!(matches!(cli.config.command(), tier::TierCliCommand::ValidateConfig));

# #[cfg(feature = "schema")] {
let example = AppCli::parse_from(["app", "--print-config-example"]);
assert!(matches!(
    example.config.command(),
    tier::TierCliCommand::PrintConfigExample
));
# }
# }
```

## Environment Variable Docs

With `schema` enabled, `tier` can generate environment variable docs and
annotated JSON Schema. When `toml` is also enabled, it can render a commented
TOML example configuration:

```rust
# #[cfg(all(feature = "schema", feature = "derive", feature = "toml"))] {
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tier::{
    EnvDocOptions, TierConfig, annotated_json_schema_pretty, config_example_toml, env_docs_json,
    env_docs_markdown,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema, TierConfig)]
struct AppConfig {
    server: ServerConfig,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, TierConfig)]
struct ServerConfig {
    #[tier(
        env = "APP_SERVER_PORT",
        doc = "Port used for incoming traffic",
        example = "8080"
    )]
    port: u16,
}

let docs = env_docs_markdown::<AppConfig>(&EnvDocOptions::prefixed("APP"));
assert!(docs.contains("APP_SERVER_PORT"));

let docs_json = env_docs_json::<AppConfig>(&EnvDocOptions::prefixed("APP"));
assert!(docs_json.is_array());

let schema = annotated_json_schema_pretty::<AppConfig>();
assert!(schema.contains("\"x-tier-env\""));

let example = config_example_toml::<AppConfig>();
assert!(example.contains("[server]"));
# }
```

## Secrets

`tier::Secret<T>` is a strong typed wrapper for sensitive values. It redacts
`Debug` and `Display` output, and with the `schema` feature it marks fields as
`writeOnly` so the loader can auto-discover secret paths.

```rust
use serde::{Deserialize, Serialize};
use tier::Secret;

#[derive(Debug, Serialize, Deserialize)]
struct DbConfig {
    password: Secret<String>,
}

let password = Secret::new("super-secret".to_owned());
assert_eq!(format!("{password}"), "***redacted***");
```

## Status

This crate focuses on typed layered loading, metadata, diagnostics, validation,
schema/docs output, and reload support.

Deliberately out of scope in the current crate line:

1. remote configuration backends
2. derive-driven full CLI generation for application-specific flags
