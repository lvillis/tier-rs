#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RawPathConfig {
    proxy: RawProxyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RawProxyConfig {
    r#type: String,
}

impl Default for RawPathConfig {
    fn default() -> Self {
        Self {
            proxy: RawProxyConfig {
                r#type: "http".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, TierPatch, Default)]
struct RawPathPatch {
    #[tier(path_expr = tier::path!(RawPathConfig.proxy.r#type))]
    kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RawPatternConfig {
    services: std::collections::BTreeMap<String, RawPatternService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RawPatternService {
    r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OptionalPatternConfig {
    services: Option<std::collections::BTreeMap<String, PatternService>>,
}

#[derive(Debug)]
struct BoxedPatternConfig {
    services: Box<[PatternService; 1]>,
}

#[derive(Debug)]
struct SharedPatternConfig {
    services: Arc<Vec<PatternService>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ArrayPatchConfig {
    users: Vec<PatternService>,
}

impl Default for ArrayPatchConfig {
    fn default() -> Self {
        Self {
            users: vec![PatternService {
                token: "seed".to_owned(),
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct NumericObjectKeyConfig {
    value: serde_json::Value,
}

impl Default for NumericObjectKeyConfig {
    fn default() -> Self {
        Self {
            value: serde_json::json!({
                "0": {
                    "password": "seed-secret"
                }
            }),
        }
    }
}

#[derive(Debug, Clone, TierPatch, Default)]
struct OverlappingPatch {
    #[tier(path = "db.token")]
    token: Option<String>,
    db: Option<PatchDb>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct DuplicatePathPatch {
    port: Option<u16>,
    #[tier(path = "port")]
    other_port: Option<u16>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct CanonicalDuplicateArrayPatch {
    #[tier(path = "users[0].name")]
    first: Option<String>,
    #[tier(path = "users[00].name")]
    second: Option<String>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct CanonicalOverlappingArrayPatch {
    #[tier(path = "users[0]")]
    first: Option<PatternService>,
    #[tier(path = "users[00].token")]
    second: Option<String>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct ArrayItemPatch {
    #[tier(path = "users.0.token")]
    token: Option<String>,
}

#[derive(Debug, Clone, TierPatch, Default)]
struct NumericObjectKeyPatch {
    #[tier(path = "value.0.password")]
    password: Option<String>,
}

#[cfg(feature = "clap")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DeferredArrayShapeConfig {
    users: serde_json::Value,
}

#[cfg(feature = "clap")]
impl Default for DeferredArrayShapeConfig {
    fn default() -> Self {
        Self {
            users: serde_json::json!({}),
        }
    }
}

#[cfg(feature = "clap")]
#[derive(Debug, Clone, TierPatch, Default)]
struct DeferredArrayShapePatch {
    users: Patch<serde_json::Value>,
}

#[cfg(feature = "clap")]
#[derive(Debug, Clone, TierPatch, Default)]
struct DeferredArrayItemPatch {
    #[tier(path = "users.0.token")]
    token: Option<String>,
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
fn standalone_patch_layers_preserve_numeric_object_keys_without_shape_context() {
    let layer = Layer::from_patch(
        "manual-patch",
        &NumericObjectKeyPatch {
            password: Some("patched-secret".to_owned()),
        },
    )
    .expect("layer from patch");

    let loaded = ConfigLoader::new(NumericObjectKeyConfig::default())
        .layer(layer)
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.value,
        serde_json::json!({
            "0": {
                "password": "patched-secret"
            }
        })
    );
    assert!(loaded.report().explain("value[0].password").is_none());
    let explanation = loaded
        .report()
        .explain("value.0.password")
        .expect("numeric object-key explanation");
    assert_eq!(explanation.path, "value.0.password");
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

#[test]
fn checked_path_macros_strip_raw_identifier_prefixes() {
    assert_eq!(tier::path!(RawPathConfig.proxy.r#type), "proxy.type");
    assert_eq!(
        tier::path_pattern!(RawPatternConfig.services.*.r#type),
        "services.*.type"
    );

    let loaded = ConfigLoader::new(RawPathConfig::default())
        .patch(
            "raw-path-patch",
            &RawPathPatch {
                kind: Some("https".to_owned()),
            },
        )
        .expect("patch layer is valid")
        .load()
        .expect("config loads");

    assert_eq!(loaded.proxy.r#type, "https");
}

#[test]
fn checked_pattern_paths_support_optional_collections() {
    assert_eq!(
        tier::path_pattern!(OptionalPatternConfig.services.*.token),
        "services.*.token"
    );
}

#[test]
fn checked_pattern_paths_support_boxed_and_shared_collections() {
    assert_eq!(
        tier::path_pattern!(BoxedPatternConfig.services.*.token),
        "services.*.token"
    );
    assert_eq!(
        tier::path_pattern!(SharedPatternConfig.services.*.token),
        "services.*.token"
    );
}

#[test]
fn typed_patches_keep_existing_array_index_semantics_when_shape_is_an_array() {
    let loaded = ConfigLoader::new(ArrayPatchConfig::default())
        .patch(
            "array-patch",
            &ArrayItemPatch {
                token: Some("patched-array-token".to_owned()),
            },
        )
        .expect("patch layer is valid")
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].token, "patched-array-token");
}

#[test]
fn typed_patches_preserve_numeric_object_keys_when_defaults_define_object_shape() {
    let loaded = ConfigLoader::new(NumericObjectKeyConfig::default())
        .patch(
            "numeric-object-key-patch",
            &NumericObjectKeyPatch {
                password: Some("patched-secret".to_owned()),
            },
        )
        .expect("patch layer is valid")
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.value,
        serde_json::json!({
            "0": {
                "password": "patched-secret"
            }
        })
    );
    assert!(loaded.report().explain("value[0].password").is_none());
    let explanation = loaded
        .report()
        .explain("value.0.password")
        .expect("numeric object-key explanation");
    assert_eq!(explanation.path, "value.0.password");
}

#[test]
fn typed_patches_preserve_numeric_object_keys_when_prior_layers_define_object_shape() {
    let loaded = ConfigLoader::new(NumericObjectKeyConfig {
        value: serde_json::json!({}),
    })
    .layer(
        Layer::custom(
            "shape-layer",
            serde_json::json!({
                "value": {
                    "0": {
                        "password": "seed-secret"
                    }
                }
            }),
        )
        .expect("shape layer"),
    )
    .patch(
        "numeric-object-key-patch",
        &NumericObjectKeyPatch {
            password: Some("patched-secret".to_owned()),
        },
    )
    .expect("patch layer is valid")
    .load()
    .expect("config loads");

    assert_eq!(
        loaded.value,
        serde_json::json!({
            "0": {
                "password": "patched-secret"
            }
        })
    );
}

#[cfg(feature = "clap")]
#[test]
fn typed_clap_overrides_preserve_array_shape_from_prior_typed_clap_layers() {
    let loaded = ConfigLoader::new(DeferredArrayShapeConfig::default())
        .clap_overrides(&DeferredArrayShapePatch {
            users: Patch::set(serde_json::json!([{ "token": "seed-token" }])),
        })
        .expect("shape-defining typed clap overrides are valid")
        .clap_overrides(&DeferredArrayItemPatch {
            token: Some("patched-token".to_owned()),
        })
        .expect("follow-up typed clap overrides are valid")
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.users,
        serde_json::json!([{ "token": "patched-token" }])
    );
    let explanation = loaded
        .report()
        .explain("users[0].token")
        .expect("users[0].token explanation");
    assert_eq!(explanation.path, "users.0.token");
}

#[test]
fn overlapping_parent_and_child_patch_paths_are_rejected() {
    let error = Layer::from_patch(
        "overlapping-patch",
        &OverlappingPatch {
            token: Some("child".to_owned()),
            db: Some(PatchDb {
                token: Some("parent".to_owned()),
            }),
        },
    )
    .expect_err("overlapping patch paths should not be order-dependent");

    let message = error.to_string();
    assert!(message.contains("overlapping-patch"));
    assert!(message.contains("db"));
    assert!(message.contains("db.token"));
    assert!(message.contains("overlap"));
}

#[test]
fn duplicate_patch_paths_are_rejected() {
    let error = Layer::from_patch(
        "duplicate-patch",
        &DuplicatePathPatch {
            port: Some(8080),
            other_port: Some(9090),
        },
    )
    .expect_err("duplicate patch paths should be rejected");

    let message = error.to_string();
    assert!(message.contains("duplicate-patch"));
    assert!(message.contains("port"));
    assert!(message.contains("duplicate patch path"));
}

#[test]
fn canonical_duplicate_array_patch_paths_are_rejected() {
    let error = Layer::from_patch(
        "duplicate-array-patch",
        &CanonicalDuplicateArrayPatch {
            first: Some("first".to_owned()),
            second: Some("second".to_owned()),
        },
    )
    .expect_err("canonical duplicate array paths should be rejected");

    let message = error.to_string();
    assert!(message.contains("duplicate-array-patch"));
    assert!(message.contains("users.0.name"));
    assert!(message.contains("duplicate patch path"));
}

#[test]
fn canonical_overlapping_array_patch_paths_are_rejected() {
    let error = Layer::from_patch(
        "overlapping-array-patch",
        &CanonicalOverlappingArrayPatch {
            first: Some(PatternService {
                token: "parent".to_owned(),
            }),
            second: Some("child".to_owned()),
        },
    )
    .expect_err("canonical overlapping array paths should be rejected");

    let message = error.to_string();
    assert!(message.contains("overlapping-array-patch"));
    assert!(message.contains("users.0"));
    assert!(message.contains("users.0.token"));
    assert!(message.contains("overlap"));
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

    #[test]
    fn typed_clap_overrides_win_over_env_sources() {
        let cli = AppCli::parse_from(["app", "--port", "8123", "--db-token", "from-cli"]);

        let loaded = ConfigLoader::new(PatchConfig::default())
            .env(
                tier::EnvSource::from_pairs([
                    ("APP__SERVER__PORT", "9000"),
                    ("APP__DB__TOKEN", "from-env"),
                ])
                .prefix("APP"),
            )
            .clap_overrides(&cli)
            .expect("typed clap overrides are valid")
            .load()
            .expect("config loads");

        assert_eq!(loaded.server.port, 8123);
        assert_eq!(loaded.db.token.as_deref(), Some("from-cli"));
        let explanation = loaded
            .report()
            .explain("server.port")
            .expect("server.port explanation");
        let port_step = explanation.steps.last().expect("latest step");
        assert!(port_step.source.to_string().contains("typed-clap"));
    }
}
