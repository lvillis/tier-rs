#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};

use tier::{ConfigLoader, Layer, Patch, TierPatch};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PatchConfig {
    server: PatchServer,
    db: PatchDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PatchServer {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PatchDb {
    token: Option<String>,
}

impl Default for PatchConfig {
    fn default() -> Self {
        Self {
            server: PatchServer {
                host: "127.0.0.1".to_owned(),
                port: 3000,
            },
            db: PatchDb {
                token: Some("default-token".to_owned()),
            },
        }
    }
}

#[derive(Debug, Clone, TierPatch, Default)]
struct ServerPatch {
    port: Option<u16>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct AppPatch {
    #[tier(nested)]
    server: Option<ServerPatch>,
    #[tier(path = "db.token")]
    token: Patch<Option<String>>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct CheckedPathPatch {
    #[tier(path_expr = tier::path!(PatchConfig.db.token))]
    token: Patch<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PatternConfig {
    services: std::collections::BTreeMap<String, PatternService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PatternService {
    token: String,
}

#[test]
fn typed_patches_can_override_nested_fields_and_clear_optionals() {
    let patch = AppPatch {
        server: Some(ServerPatch { port: Some(9001) }),
        token: Patch::set(None),
    };

    let loaded = ConfigLoader::new(PatchConfig::default())
        .patch("typed-patch", &patch)
        .expect("patch layer is valid")
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.port, 9001);
    assert_eq!(loaded.db.token, None);
    assert!(
        loaded
            .report()
            .explain("server.port")
            .expect("server.port explanation")
            .steps
            .last()
            .expect("latest step")
            .source
            .to_string()
            .contains("typed-patch")
    );
}

#[test]
fn layer_can_be_constructed_from_a_typed_patch() {
    let patch = AppPatch {
        server: Some(ServerPatch { port: Some(7000) }),
        token: Patch::Unset,
    };

    let layer = Layer::from_patch("manual-patch", &patch).expect("layer from patch");
    let loaded = ConfigLoader::new(PatchConfig::default())
        .layer(layer)
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.port, 7000);
    assert_eq!(loaded.db.token.as_deref(), Some("default-token"));
}

#[test]
fn checked_path_macros_can_drive_sparse_patches() {
    assert_eq!(tier::path!(PatchConfig.db.token), "db.token");
    assert_eq!(
        tier::path_pattern!(PatternConfig.services.*.token),
        "services.*.token"
    );

    let loaded = ConfigLoader::new(PatchConfig::default())
        .patch(
            "checked-patch",
            &CheckedPathPatch {
                token: Patch::set(Some("from-checked-path".to_owned())),
            },
        )
        .expect("patch layer is valid")
        .load()
        .expect("config loads");

    assert_eq!(loaded.db.token.as_deref(), Some("from-checked-path"));
}

#[cfg(feature = "clap")]
mod clap_bridge {
    use clap::{Args, Parser};

    use super::*;

    #[derive(Debug, Clone, Args, TierPatch, Default)]
    struct ServerCli {
        #[arg(long)]
        port: Option<u16>,
    }

    #[derive(Debug, Clone, Parser, TierPatch)]
    struct AppCli {
        #[command(flatten)]
        #[tier(nested)]
        server: ServerCli,
        #[arg(long = "db-token")]
        #[tier(path_expr = tier::path!(PatchConfig.db.token))]
        token: Option<String>,
    }

    #[test]
    fn typed_clap_structs_can_apply_last_layer_overrides() {
        let cli = AppCli::parse_from(["app", "--port", "8123", "--db-token", "from-cli"]);

        let loaded = ConfigLoader::new(PatchConfig::default())
            .clap_overrides(&cli)
            .expect("typed clap overrides are valid")
            .load()
            .expect("config loads");

        assert_eq!(loaded.server.port, 8123);
        assert_eq!(loaded.db.token.as_deref(), Some("from-cli"));
        assert!(
            loaded
                .report()
                .explain("db.token")
                .expect("db.token explanation")
                .steps
                .last()
                .expect("latest step")
                .source
                .to_string()
                .contains("typed-clap")
        );
    }
}
