#![cfg(feature = "toml")]

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::tempdir;

use tier::{
    ArgsSource, ConfigError, ConfigLoader, ConfigMetadata, ConfigWarning, EnvDecoder, EnvSource,
    FieldMetadata, FileFormat, FileSource, Layer, MergeStrategy, REPORT_FORMAT_VERSION, SourceKind,
    ValidationErrors,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AppConfig {
    server: ServerConfig,
    db: DbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ServerConfig {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DbConfig {
    url: String,
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MergeConfig {
    plugins: Vec<String>,
    headers: BTreeMap<String, String>,
    server: MergeServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WildcardMergeConfig {
    headers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MergeServer {
    tls: MergeTls,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MergeTls {
    cert: String,
    key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StringValueConfig {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct StructuredEnvConfig {
    no_proxy: Vec<String>,
    ports: Vec<u16>,
    labels: BTreeMap<String, u16>,
    words: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct ProxyCompatConfig {
    proxy: ProxyCompatSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct ProxyCompatSettings {
    url: Option<String>,
    no_proxy: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PortOnlyConfig {
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct OptionalTokenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OptionalStringConfig {
    value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct OptionalUsersConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    users: Option<Vec<UserRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UserArrayConfig {
    users: Vec<UserRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UserRecord {
    name: String,
    password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct WildcardCheckConfig {
    users: Vec<WildcardCheckUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WildcardCheckUser {
    enabled: bool,
    password: Option<String>,
    cert: Option<String>,
    key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AliasCollisionConfig {
    first: String,
    second: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AliasSecretConfig {
    server: AliasSecretServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AliasSecretServer {
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct AliasValidationConfig {
    server: AliasValidationServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct AliasValidationServer {
    token: Option<String>,
    cert: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct DynamicKeyConfig {
    headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DynamicValueConfig {
    value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TupleOverrideConfig {
    pair: (String, u16),
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_owned(),
                port: 3000,
            },
            db: DbConfig {
                url: "postgres://localhost/app".to_owned(),
                password: "default-secret".to_owned(),
            },
        }
    }
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            plugins: vec!["core".to_owned()],
            headers: BTreeMap::from([("x-default".to_owned(), "1".to_owned())]),
            server: MergeServer {
                tls: MergeTls {
                    cert: "default-cert.pem".to_owned(),
                    key: Some("default-key.pem".to_owned()),
                },
            },
        }
    }
}

impl Default for WildcardMergeConfig {
    fn default() -> Self {
        Self {
            headers: BTreeMap::from([(
                "svc".to_owned(),
                BTreeMap::from([("a".to_owned(), "1".to_owned())]),
            )]),
        }
    }
}

impl Default for StringValueConfig {
    fn default() -> Self {
        Self {
            value: "default".to_owned(),
        }
    }
}

impl Default for PortOnlyConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}

impl Default for OptionalStringConfig {
    fn default() -> Self {
        Self {
            value: Some("default".to_owned()),
        }
    }
}

impl Default for UserArrayConfig {
    fn default() -> Self {
        Self {
            users: vec![UserRecord {
                name: "alice".to_owned(),
                password: "array-secret".to_owned(),
            }],
        }
    }
}

impl Default for AliasCollisionConfig {
    fn default() -> Self {
        Self {
            first: "a".to_owned(),
            second: "b".to_owned(),
        }
    }
}

impl Default for AliasSecretConfig {
    fn default() -> Self {
        Self {
            server: AliasSecretServer {
                token: "alias-secret".to_owned(),
            },
        }
    }
}

impl Default for DynamicValueConfig {
    fn default() -> Self {
        Self {
            value: serde_json::json!({
                "legacy": {
                    "password": "before"
                }
            }),
        }
    }
}

impl Default for TupleOverrideConfig {
    fn default() -> Self {
        Self {
            pair: ("edge".to_owned(), 8080),
        }
    }
}

#[test]
fn loads_from_defaults_files_env_and_args() {
    let dir = tempdir().expect("temporary directory");
    let config_path = dir.path().join("app.toml");
    fs::write(
        &config_path,
        r#"
            [server]
            host = "0.0.0.0"
            port = 8000

            [db]
            url = "postgres://file/db"
            password = "file-secret"
        "#,
    )
    .expect("config file");

    let env = EnvSource::from_pairs([
        ("APP__SERVER__PORT", "9000"),
        ("APP__DB__PASSWORD", "env-secret"),
    ])
    .prefix("APP");

    let args = ArgsSource::from_args([
        "tier",
        "--config",
        config_path.to_str().expect("utf-8 path"),
        "--set",
        "server.host=\"127.0.0.2\"",
        "--set",
        "db.password=\"cli-secret\"",
    ]);

    let loaded = ConfigLoader::new(AppConfig::default())
        .env(env)
        .args(args)
        .secret_path("db.password")
        .validator("port-range", |config| {
            if config.server.port == 0 {
                return Err(ValidationErrors::from_message(
                    "server.port",
                    "port must be greater than zero",
                ));
            }
            Ok(())
        })
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.port, 9000);
    assert_eq!(loaded.server.host, "127.0.0.2");
    assert_eq!(loaded.db.url, "postgres://file/db");
    assert_eq!(loaded.db.password, "cli-secret");

    let explanation = loaded
        .report()
        .explain("server.port")
        .expect("port explanation");
    assert_eq!(explanation.steps.len(), 3);
    assert_eq!(explanation.steps[0].source.to_string(), "default(defaults)");
    assert_eq!(
        explanation.steps[1].source.to_string(),
        format!("file({})", config_path.display())
    );
    assert_eq!(
        explanation.steps[2].source.to_string(),
        "env(APP__SERVER__PORT)"
    );

    let password_explanation = loaded
        .report()
        .explain("db.password")
        .expect("password explanation");
    assert!(password_explanation.redacted);
    assert_eq!(
        password_explanation
            .final_value
            .expect("final value")
            .as_str(),
        Some("***redacted***")
    );

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("cli-secret"));
}

#[test]
fn parent_path_explanations_and_traces_redact_nested_secrets() {
    let loaded = ConfigLoader::new(AppConfig::default())
        .secret_path("db.password")
        .load()
        .expect("config loads");

    let explanation = loaded.report().explain("db").expect("db explanation");
    assert!(explanation.redacted);
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(|value| value.get("password"))
            .and_then(serde_json::Value::as_str),
        Some("***redacted***")
    );
    assert!(explanation.steps.iter().all(|step| {
        step.value
            .get("password")
            .and_then(serde_json::Value::as_str)
            == Some("***redacted***")
            && step.redacted
    }));

    let trace_steps = loaded.report().traces().get("db").expect("db trace");
    assert!(trace_steps.iter().all(|step| {
        step.value
            .get("password")
            .and_then(serde_json::Value::as_str)
            == Some("***redacted***")
            && step.redacted
    }));
}

#[test]
fn manual_secret_paths_are_canonicalized_through_alias_metadata() {
    let loaded = ConfigLoader::new(AliasSecretConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "server.token",
        )
        .alias("service.legacyToken")]))
        .secret_path("service.legacyToken")
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("alias-secret"));

    let explanation = loaded
        .report()
        .explain("service.legacyToken")
        .expect("alias explanation");
    assert_eq!(explanation.path, "server.token");
    assert!(explanation.redacted);
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(serde_json::Value::as_str),
        Some("***redacted***")
    );
}

#[test]
fn empty_manual_secret_paths_are_ignored() {
    let loaded = ConfigLoader::new(AppConfig::default())
        .secret_path("")
        .secret_path(".")
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("3000"));
    assert!(!rendered.contains("***redacted***"));

    let explanation = loaded
        .report()
        .explain("server.port")
        .expect("server.port explanation");
    assert!(!explanation.redacted);
}

#[test]
fn metadata_lookups_accept_alias_paths_including_wildcards() {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("server.tokens")
            .alias("server.legacyTokens")
            .merge_strategy(MergeStrategy::Append),
        FieldMetadata::new("users.*.password")
            .alias("users.*.legacyPassword")
            .secret(),
    ]);

    let tokens = metadata
        .field("server.legacyTokens")
        .expect("alias metadata lookup");
    assert_eq!(tokens.path, "server.tokens");
    assert_eq!(
        metadata.merge_strategy_for("server.legacyTokens"),
        Some(MergeStrategy::Append)
    );

    let password = metadata
        .field("users.0.legacyPassword")
        .expect("wildcard alias metadata lookup");
    assert_eq!(password.path, "users.*.password");
    assert!(password.secret);
}

#[test]
fn parent_path_explanations_use_layer_provenance_for_multi_entry_env_and_args() {
    let env_loaded = ConfigLoader::new(AppConfig::default())
        .env(
            EnvSource::from_pairs([
                ("APP__DB__URL", "postgres://env/db"),
                ("APP__DB__PASSWORD", "env-secret"),
            ])
            .prefix("APP"),
        )
        .load()
        .expect("env config loads");

    let env_explanation = env_loaded.report().explain("db").expect("db explanation");
    assert!(
        env_explanation
            .steps
            .iter()
            .any(|step| step.source.to_string() == "env(environment)")
    );

    let args_loaded = ConfigLoader::new(AppConfig::default())
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"db.url="postgres://args/db""#,
            "--set",
            r#"db.password="args-secret""#,
        ]))
        .load()
        .expect("args config loads");

    let args_explanation = args_loaded.report().explain("db").expect("db explanation");
    assert!(
        args_explanation
            .steps
            .iter()
            .any(|step| step.source.to_string() == "cli(arguments)")
    );
}

#[test]
fn applies_profile_placeholders_and_tracks_normalization() {
    let dir = tempdir().expect("temporary directory");
    let default_path = dir.path().join("default.toml");
    let profile_path = dir.path().join("{profile}.toml");

    fs::write(
        &default_path,
        r#"
            [server]
            host = " LOCALHOST "
            port = 8080

            [db]
            url = "postgres://default/db"
            password = "secret"
        "#,
    )
    .expect("default file");

    fs::write(
        dir.path().join("prod.toml"),
        r#"
            [server]
            port = 9090
        "#,
    )
    .expect("profile file");

    let loaded = ConfigLoader::new(AppConfig::default())
        .file(default_path)
        .optional_file(profile_path)
        .profile("prod")
        .normalizer("trim-host", |config| {
            config.server.host = config.server.host.trim().to_ascii_lowercase();
            Ok::<_, String>(())
        })
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.host, "localhost");
    assert_eq!(loaded.server.port, 9090);

    let explanation = loaded
        .report()
        .explain("server.host")
        .expect("host explanation");
    assert!(
        explanation
            .steps
            .iter()
            .any(|step| step.source.to_string() == "normalize(trim-host)")
    );
}

#[test]
fn normalization_traces_paths_removed_by_skip_serializing_if() {
    let loaded = ConfigLoader::new(OptionalTokenConfig {
        token: Some("seed".to_owned()),
    })
    .normalizer("clear-token", |config| {
        config.token = None;
        Ok::<_, String>(())
    })
    .load()
    .expect("config loads");

    let explanation = loaded.report().explain("token").expect("token explanation");
    let normalization_step = explanation
        .steps
        .iter()
        .find(|step| step.source.to_string() == "normalize(clear-token)")
        .expect("normalization step");

    assert_eq!(explanation.final_value, None);
    assert_eq!(normalization_step.value, serde_json::Value::Null);
}

#[test]
fn removed_array_paths_still_explain_leading_zero_indices() {
    let loaded = ConfigLoader::new(OptionalUsersConfig {
        users: Some(vec![UserRecord {
            name: "alice".to_owned(),
            password: "seed-secret".to_owned(),
        }]),
    })
    .normalizer("clear-users", |config| {
        config.users = None;
        Ok::<_, String>(())
    })
    .load()
    .expect("config loads");

    let explanation = loaded
        .report()
        .explain("users[00].password")
        .expect("removed array path explanation");
    let normalization_step = explanation
        .steps
        .iter()
        .find(|step| step.source.to_string() == "normalize(clear-users)")
        .expect("normalization step");

    assert_eq!(explanation.path, "users.0.password");
    assert_eq!(explanation.final_value, None);
    assert_eq!(normalization_step.value, serde_json::Value::Null);
}

#[test]
fn removed_object_paths_do_not_alias_numeric_keys() {
    let loaded = ConfigLoader::new(DynamicValueConfig {
        value: serde_json::json!({
            "00": {
                "password": "seed-secret"
            }
        }),
    })
    .normalizer("clear-value", |config| {
        config.value = serde_json::Value::Null;
        Ok::<_, String>(())
    })
    .load()
    .expect("config loads");

    assert!(loaded.report().explain("value.0.password").is_none());

    let explanation = loaded
        .report()
        .explain("value.00.password")
        .expect("exact numeric object-key path explanation");
    assert_eq!(explanation.path, "value.00.password");
    assert_eq!(explanation.final_value, None);
}

#[test]
fn present_object_paths_do_not_alias_numeric_keys_through_brackets() {
    let loaded = ConfigLoader::new(DynamicValueConfig {
        value: serde_json::json!({
            "0": {
                "password": "seed-secret"
            }
        }),
    })
    .load()
    .expect("config loads");

    assert!(loaded.report().explain("value[0].password").is_none());

    let explanation = loaded
        .report()
        .explain("value.0.password")
        .expect("exact numeric object-key path explanation");
    assert_eq!(explanation.path, "value.0.password");
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(serde_json::Value::as_str),
        Some("seed-secret")
    );
}

#[test]
fn rejects_object_keys_that_cannot_be_represented_in_paths() {
    let error = ConfigLoader::new(DynamicKeyConfig {
        headers: BTreeMap::from([("x.y".to_owned(), "value".to_owned())]),
    })
    .load()
    .expect_err("dotted object keys should be rejected");

    let ConfigError::InvalidPathKey { path, key, message } = error else {
        panic!("expected invalid path key error");
    };

    assert_eq!(path, "headers");
    assert_eq!(key, "x.y");
    assert!(message.contains("path separator"));
}

#[test]
fn rejects_object_keys_that_conflict_with_external_array_path_syntax() {
    let error = ConfigLoader::new(DynamicKeyConfig {
        headers: BTreeMap::from([("x[0]".to_owned(), "value".to_owned())]),
    })
    .load()
    .expect_err("bracketed object keys should be rejected");

    let ConfigError::InvalidPathKey { path, key, message } = error else {
        panic!("expected invalid path key error");
    };

    assert_eq!(path, "headers");
    assert_eq!(key, "x[0]");
    assert!(message.contains("array path syntax"));
}

#[test]
fn normalizers_cannot_introduce_unrepresentable_object_keys() {
    let error = ConfigLoader::new(DynamicKeyConfig::default())
        .normalizer("insert-dotted-key", |config| {
            config.headers.insert("x.y".to_owned(), "value".to_owned());
            Ok::<_, String>(())
        })
        .load()
        .expect_err("normalizers should not be able to introduce dotted keys");

    let ConfigError::InvalidPathKey { path, key, message } = error else {
        panic!("expected invalid path key error");
    };

    assert_eq!(path, "headers");
    assert_eq!(key, "x.y");
    assert!(message.contains("path separator"));
}

#[test]
fn normalizers_cannot_introduce_keys_that_conflict_with_external_array_path_syntax() {
    let error = ConfigLoader::new(DynamicKeyConfig::default())
        .normalizer("insert-bracket-key", |config| {
            config.headers.insert("x[0]".to_owned(), "value".to_owned());
            Ok::<_, String>(())
        })
        .load()
        .expect_err("normalizers should not be able to introduce bracketed keys");

    let ConfigError::InvalidPathKey { path, key, message } = error else {
        panic!("expected invalid path key error");
    };

    assert_eq!(path, "headers");
    assert_eq!(key, "x[0]");
    assert!(message.contains("array path syntax"));
}

#[test]
fn cli_overrides_reject_reserved_wildcard_key_segments() {
    let error = ConfigLoader::new(DynamicKeyConfig::default())
        .args(ArgsSource::from_args(["tier", "--set", "headers.*=value"]))
        .load()
        .expect_err("wildcard key segments should be rejected");

    let ConfigError::InvalidPathKey { path, key, message } = error else {
        panic!("expected invalid path key error");
    };

    assert_eq!(path, "headers");
    assert_eq!(key, "*");
    assert!(message.contains("wildcard"));
}

#[test]
fn validation_errors_are_returned_with_context() {
    let error = ConfigLoader::new(AppConfig::default())
        .validator("port-range", |config| {
            if config.server.port < 4_000 {
                return Err(ValidationErrors::from_message(
                    "server.port",
                    "port must be >= 4000",
                ));
            }
            Ok(())
        })
        .load()
        .expect_err("validation must fail");

    let message = error.to_string();
    assert!(message.contains("validator port-range failed"));
    assert!(message.contains("server.port"));
}

#[test]
fn deserialize_errors_include_the_last_source() {
    let error = ConfigLoader::new(PortOnlyConfig::default())
        .env(EnvSource::from_pairs([("APP_PORT", "abc")]).prefix("APP"))
        .load()
        .expect_err("deserialization must fail");

    let ConfigError::Deserialize {
        path,
        provenance,
        message,
    } = &error
    else {
        panic!("expected deserialize error");
    };

    assert_eq!(path, "port");
    assert_eq!(
        provenance.as_ref().map(ToString::to_string),
        Some("env(APP_PORT)".to_owned())
    );
    assert!(message.contains("invalid type"));
    assert!(error.to_string().contains("from env(APP_PORT)"));
}

#[test]
fn env_and_args_keep_string_inputs_but_still_coerce_numeric_targets() {
    let string_from_env = ConfigLoader::new(StringValueConfig::default())
        .env(EnvSource::from_pairs([("APP_VALUE", "false")]).prefix("APP"))
        .load()
        .expect("string env override should load");
    assert_eq!(string_from_env.value, "false");

    let string_from_args = ConfigLoader::new(StringValueConfig::default())
        .args(ArgsSource::from_args(["app", "--set", "value=false"]))
        .load()
        .expect("string CLI override should load");
    assert_eq!(string_from_args.value, "false");

    let port_from_env = ConfigLoader::new(PortOnlyConfig::default())
        .env(EnvSource::from_pairs([("APP_PORT", "9000")]).prefix("APP"))
        .load()
        .expect("numeric env override should still coerce");
    assert_eq!(port_from_env.port, 9000);

    let port_from_args = ConfigLoader::new(PortOnlyConfig::default())
        .args(ArgsSource::from_args(["app", "--set", "port=9100"]))
        .load()
        .expect("numeric CLI override should still coerce");
    assert_eq!(port_from_args.port, 9100);

    let optional_string_from_env = ConfigLoader::new(OptionalStringConfig::default())
        .env(EnvSource::from_pairs([("APP_VALUE", "\"null\"")]).prefix("APP"))
        .load()
        .expect("quoted null env override should stay a string");
    assert_eq!(optional_string_from_env.value.as_deref(), Some("null"));

    let optional_string_from_args = ConfigLoader::new(OptionalStringConfig::default())
        .args(ArgsSource::from_args(["app", "--set", r#"value="null""#]))
        .load()
        .expect("quoted null CLI override should stay a string");
    assert_eq!(optional_string_from_args.value.as_deref(), Some("null"));

    let whitespace_from_env = ConfigLoader::new(StringValueConfig::default())
        .env(EnvSource::from_pairs([("APP_VALUE", "   ")]).prefix("APP"))
        .load()
        .expect("whitespace-only env override should load");
    assert_eq!(whitespace_from_env.value, "   ");

    let whitespace_from_args = ConfigLoader::new(StringValueConfig::default())
        .args(ArgsSource::from_args(["app", "--set", "value=   "]))
        .load()
        .expect("whitespace-only CLI override should load");
    assert_eq!(whitespace_from_args.value, "   ");
}

#[test]
fn env_decoders_handle_common_structured_operational_formats() {
    let loaded = ConfigLoader::new(StructuredEnvConfig::default())
        .env_decoder("no_proxy", EnvDecoder::Csv)
        .env_decoder("ports", EnvDecoder::Csv)
        .env_decoder("labels", EnvDecoder::KeyValueMap)
        .env_decoder("words", EnvDecoder::Whitespace)
        .env(
            EnvSource::from_pairs([
                ("APP__NO_PROXY", "localhost,127.0.0.1,.internal.example.com"),
                ("APP__PORTS", "80,443"),
                ("APP__LABELS", "http=80,https=443"),
                ("APP__WORDS", "alpha beta   gamma"),
            ])
            .prefix("APP"),
        )
        .load()
        .expect("structured env overrides should decode");

    assert_eq!(
        loaded.no_proxy,
        vec![
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
            ".internal.example.com".to_owned()
        ]
    );
    assert_eq!(loaded.ports, vec![80, 443]);
    assert_eq!(
        loaded.labels,
        BTreeMap::from([("http".to_owned(), 80_u16), ("https".to_owned(), 443_u16),])
    );
    assert_eq!(
        loaded.words,
        vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()]
    );
}

#[test]
fn env_aliases_and_fallbacks_support_standard_operational_variables() {
    let env = EnvSource::from_pairs([
        ("HTTP_PROXY", "http://fallback-proxy:8080"),
        ("NO_PROXY", "localhost,127.0.0.1,.internal.example.com"),
        ("APP__PROXY__URL", "http://app-proxy:9090"),
    ])
    .prefix("APP")
    .with_fallback("HTTP_PROXY", "proxy.url")
    .with_fallback_decoder("NO_PROXY", "proxy.no_proxy", EnvDecoder::Csv);

    let loaded = ConfigLoader::new(ProxyCompatConfig::default())
        .env(env)
        .load()
        .expect("config loads");

    assert_eq!(loaded.proxy.url.as_deref(), Some("http://app-proxy:9090"));
    assert_eq!(
        loaded.proxy.no_proxy,
        vec![
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
            ".internal.example.com".to_owned(),
        ]
    );
}

#[test]
fn custom_env_decoders_can_handle_application_specific_formats() {
    let loaded = ConfigLoader::new(StructuredEnvConfig::default())
        .env_decoder_with("no_proxy", |raw| {
            Ok(Value::Array(
                raw.split(';')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(|segment| Value::String(segment.to_owned()))
                    .collect(),
            ))
        })
        .env(EnvSource::from_pairs([("APP__NO_PROXY", "localhost;.svc.internal")]).prefix("APP"))
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.no_proxy,
        vec!["localhost".to_owned(), ".svc.internal".to_owned()]
    );
}

#[test]
fn invalid_explicit_json_overrides_return_source_specific_errors() {
    let env_error = ConfigLoader::new(PortOnlyConfig::default())
        .env(EnvSource::from_pairs([("APP_PORT", "[1,]")]).prefix("APP"))
        .load()
        .expect_err("invalid explicit env JSON should fail");
    let arg_error = ConfigLoader::new(PortOnlyConfig::default())
        .args(ArgsSource::from_args(["tier", "--set", "port=[1,]"]))
        .load()
        .expect_err("invalid explicit arg JSON should fail");

    let env_message = env_error.to_string();
    let arg_message = arg_error.to_string();

    assert!(env_message.contains("invalid explicit JSON override"));
    assert!(env_message.contains("APP_PORT"));
    assert!(arg_message.contains("invalid explicit JSON override"));
    assert!(arg_message.contains("--set port=[1,]"));
}

#[test]
fn env_prefix_requires_a_separator_boundary() {
    let loaded = ConfigLoader::new(PortOnlyConfig::default())
        .env(EnvSource::from_pairs([("APPLICATION__PORT", "9000")]).prefix("APP"))
        .load()
        .expect("unrelated env vars should be ignored");

    assert_eq!(loaded.port, 3000);
}

#[test]
fn inferred_env_segments_reject_reserved_path_syntax() {
    let error = ConfigLoader::new(AppConfig::default())
        .env(EnvSource::from_pairs([("APP__SERVER.PORT", "9100")]).prefix("APP"))
        .load()
        .expect_err("reserved env path syntax should be rejected");

    let ConfigError::InvalidEnv {
        name,
        path,
        message,
    } = error
    else {
        panic!("expected invalid environment variable error");
    };

    assert_eq!(name, "APP__SERVER.PORT");
    assert_eq!(path, "server.port");
    assert!(message.contains("reserved path syntax"));
    assert!(message.contains("`.` is reserved"));
}

#[test]
fn env_prefix_respects_the_configured_separator() {
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
    struct NestedPortConfig {
        server: PortOnlyConfig,
    }

    let loaded = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("APP--SERVER--PORT", "9000")])
                .prefix("APP")
                .separator("--"),
        )
        .load()
        .expect("custom separator env vars should load");

    assert_eq!(loaded.server.port, 9000);
}

#[test]
fn custom_env_separator_does_not_accept_underscore_boundary_variants() {
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
    struct NestedPortConfig {
        server: PortOnlyConfig,
    }

    let loaded = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("APP__SERVER--PORT", "9000")])
                .prefix("APP")
                .separator("--"),
        )
        .load()
        .expect("mismatched separator variants should be ignored");

    assert_eq!(loaded.server.port, 3000);
}

#[test]
fn env_prefixes_with_trailing_separator_suffixes_are_normalized() {
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
    struct NestedPortConfig {
        server: PortOnlyConfig,
    }

    let dashed = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("APP--SERVER--PORT", "9100"), ("APP__SERVER__PORT", "9999")])
                .prefix("APP--")
                .separator("--"),
        )
        .load()
        .expect("custom separator suffixes should be accepted without broadening the prefix");
    assert_eq!(dashed.server.port, 9100);

    let underscored = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("APP__SERVER__PORT", "9200")])
                .prefix("APP__")
                .separator("__"),
        )
        .load()
        .expect("prefixed env vars should load even when the prefix includes the separator");

    assert_eq!(underscored.server.port, 9200);

    let single_underscore = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("APP__SERVER__PORT", "9300")])
                .prefix("APP_")
                .separator("__"),
        )
        .load()
        .expect("single underscore prefixes should still honor the configured separator");

    assert_eq!(single_underscore.server.port, 9300);
}

#[test]
fn empty_env_separator_keeps_the_existing_mapping_separator() {
    let loaded = ConfigLoader::new(PortOnlyConfig::default())
        .env(
            EnvSource::from_pairs([("APP__PORT", "9400")])
                .prefix("APP")
                .separator(""),
        )
        .load()
        .expect("empty separators should not invalidate env parsing");

    assert_eq!(loaded.port, 9400);
}

#[test]
fn empty_env_prefix_behaves_like_an_unprefixed_source() {
    let loaded = ConfigLoader::new(PortOnlyConfig::default())
        .env(EnvSource::from_pairs([("PORT", "9500")]).prefix(""))
        .load()
        .expect("empty prefixes should not filter out env vars");

    assert_eq!(loaded.port, 9500);
}

#[test]
fn separator_only_env_prefix_behaves_like_an_unprefixed_source() {
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
    struct NestedPortConfig {
        server: PortOnlyConfig,
    }

    let loaded = ConfigLoader::new(NestedPortConfig::default())
        .env(
            EnvSource::from_pairs([("SERVER--PORT", "9600")])
                .prefix("--")
                .separator("--"),
        )
        .load()
        .expect("separator-only prefixes should not filter out env vars");

    assert_eq!(loaded.server.port, 9600);
}

#[test]
fn wildcard_secret_paths_redact_array_items() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .secret_path("users.*.password")
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("array-secret"));

    let explanation = loaded
        .report()
        .explain("users.0.password")
        .expect("password explanation");
    assert!(explanation.redacted);
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(|value| value.as_str()),
        Some("***redacted***")
    );

    let bracket_explanation = loaded
        .report()
        .explain("users[0].password")
        .expect("bracket path explanation");
    assert_eq!(bracket_explanation.path, "users.0.password");
    assert!(bracket_explanation.redacted);
}

#[test]
fn dot_paths_with_leading_zero_array_indices_are_canonicalized_in_reports() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .load()
        .expect("config loads");

    let explanation = loaded
        .report()
        .explain("users.00.password")
        .expect("leading-zero dot path explanation");
    assert_eq!(explanation.path, "users.0.password");
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(serde_json::Value::as_str),
        Some("array-secret")
    );
}

#[test]
fn args_accept_bracket_array_paths() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[0].password="rotated-secret""#,
        ]))
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].password, "rotated-secret");

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .expect("bracket path explanation");
    assert_eq!(explanation.path, "users.0.password");
    assert!(explanation.steps.iter().any(|step| {
        step.source.to_string() == r#"cli(--set users[0].password="rotated-secret")"#
    }));
}

#[test]
fn bracket_array_indices_are_canonicalized() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[00].password="rotated-secret""#,
        ]))
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].password, "rotated-secret");

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .unwrap_or_else(|| {
            panic!(
                "canonical bracket path explanation: {:?}",
                loaded.report().traces()
            )
        });
    assert_eq!(explanation.path, "users.0.password");
    assert!(explanation.steps.iter().any(|step| {
        step.source.to_string() == r#"cli(--set users[00].password="rotated-secret")"#
    }));
}

#[test]
fn args_reject_malformed_external_array_paths() {
    for raw in [
        r#"headers[foo]="value""#,
        r#"users[0]password="value""#,
        r#"users]="value""#,
        r#"server..port="1""#,
    ] {
        let error = ConfigLoader::new(DynamicKeyConfig::default())
            .args(ArgsSource::from_args(["tier", "--set", raw]))
            .load()
            .expect_err("malformed bracket paths must fail");

        let ConfigError::InvalidArg { arg, .. } = error else {
            panic!("expected invalid arg error");
        };
        assert!(arg.contains(raw), "unexpected arg payload for {raw}: {arg}");
    }
}

#[test]
fn explain_rejects_malformed_external_array_paths() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .load()
        .expect("config loads");

    assert!(loaded.report().explain("users[foo].password").is_none());
    assert!(loaded.report().explain("users[0.password").is_none());
    assert!(loaded.report().explain("users[0]password").is_none());
    assert!(loaded.report().explain("users]").is_none());
    assert!(loaded.report().explain("server..port").is_none());
}

#[test]
fn env_accepts_indexed_array_paths() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .env(EnvSource::from_pairs([("APP__USERS__0__PASSWORD", "env-secret")]).prefix("APP"))
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].name, "alice");
    assert_eq!(loaded.users[0].password, "env-secret");

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .expect("bracket path explanation");
    assert_eq!(explanation.path, "users.0.password");
    assert!(
        explanation
            .steps
            .iter()
            .any(|step| step.source.to_string() == "env(APP__USERS__0__PASSWORD)")
    );
}

#[test]
fn env_index_paths_with_leading_zeroes_are_canonicalized() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .env(EnvSource::from_pairs([("APP__USERS__00__PASSWORD", "env-secret")]).prefix("APP"))
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].password, "env-secret");

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .unwrap_or_else(|| {
            panic!(
                "canonical bracket path explanation: {:?}",
                loaded.report().traces()
            )
        });
    assert_eq!(explanation.path, "users.0.password");
    assert!(
        explanation
            .steps
            .iter()
            .any(|step| step.source.to_string() == "env(APP__USERS__00__PASSWORD)")
    );

    let dot_explanation = loaded
        .report()
        .explain("users[00].password")
        .expect("leading-zero bracket path explanation");
    assert_eq!(dot_explanation.path, "users.0.password");
}

#[test]
fn concrete_metadata_paths_match_canonical_array_indices() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00.password",
        )
        .secret()]))
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("array-secret"));

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .expect("canonical bracket path explanation");
    assert!(explanation.redacted);
}

#[test]
fn concrete_alias_metadata_paths_match_canonical_array_indices() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00.password",
        )
        .alias("users.00.legacyPassword")
        .secret()]))
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[00].legacyPassword="rotated-secret""#,
        ]))
        .load()
        .expect("config loads");

    assert_eq!(loaded.users[0].password, "rotated-secret");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("rotated-secret"));
    assert!(!rendered.contains("legacyPassword"));
}

#[test]
fn concrete_secret_metadata_paths_stay_canonical_after_normalizer_creates_array_values() {
    let loaded = ConfigLoader::new(OptionalUsersConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00.password",
        )
        .secret()]))
        .normalizer("seed-user", |config| {
            config.users = Some(vec![UserRecord {
                name: "alice".to_owned(),
                password: "normalized-secret".to_owned(),
            }]);
            Ok::<_, String>(())
        })
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("normalized-secret"));

    let explanation = loaded
        .report()
        .explain("users[0].password")
        .expect("canonical bracket path explanation");
    assert!(explanation.redacted);
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(serde_json::Value::as_str),
        Some("***redacted***")
    );
}

#[test]
fn concrete_validation_metadata_paths_stay_canonical_after_normalizer_creates_array_values() {
    let error = ConfigLoader::new(OptionalUsersConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00.password",
        )
        .secret()
        .non_empty()]))
        .normalizer("seed-user", |config| {
            config.users = Some(vec![UserRecord {
                name: "alice".to_owned(),
                password: String::new(),
            }]);
            Ok::<_, String>(())
        })
        .load()
        .expect_err("declared validation must run after normalizer");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    let entry = errors
        .iter()
        .find(|entry| entry.rule.as_deref() == Some("non_empty"));
    let entry = entry.expect("non_empty validation error");
    assert_eq!(entry.path, "users.0.password");
    assert_eq!(
        entry.actual.as_ref().and_then(serde_json::Value::as_str),
        Some("***redacted***")
    );
}

#[test]
fn normalization_traces_new_paths_when_container_shape_changes() {
    let loaded = ConfigLoader::new(DynamicValueConfig::default())
        .normalizer("reshape-value", |config| {
            config.value = serde_json::json!([
                {
                    "password": "after"
                }
            ]);
            Ok::<_, String>(())
        })
        .load()
        .expect("config loads");

    let explanation = loaded
        .report()
        .explain("value[0].password")
        .expect("new array child path explanation");
    assert_eq!(
        explanation
            .final_value
            .as_ref()
            .and_then(serde_json::Value::as_str),
        Some("after")
    );
    assert!(
        explanation
            .steps
            .iter()
            .any(|step| step.source.name == "reshape-value")
    );
}

#[test]
fn concrete_merge_metadata_paths_match_canonical_array_indices() {
    ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00",
        )
        .merge_strategy(MergeStrategy::Replace)]))
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[00]={"name":"bob"}"#,
        ]))
        .load()
        .expect_err("replace merge should remove password and fail deserialization");
}

#[test]
fn concrete_deprecated_metadata_paths_match_canonical_array_indices() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.00.password",
        )
        .deprecated("use users.*.credential instead")]))
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[00].password="rotated-secret""#,
        ]))
        .load()
        .expect("config loads");

    assert!(loaded.report().warnings().iter().any(|warning| {
        matches!(
            warning,
            ConfigWarning::DeprecatedField(field)
                if field.path == "users.0.password"
                    && field.note.as_deref() == Some("use users.*.credential instead")
        )
    }));
}

#[test]
fn args_can_still_replace_whole_arrays() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users=[{"name":"bob","password":"replaced-secret"}]"#,
        ]))
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.users,
        vec![UserRecord {
            name: "bob".to_owned(),
            password: "replaced-secret".to_owned(),
        }]
    );
}

#[test]
fn indexed_array_patches_ignore_append_merge_strategy() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([
            FieldMetadata::new("users").merge_strategy(MergeStrategy::Append)
        ]))
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[0].password="patched-secret""#,
        ]))
        .load()
        .expect("indexed array patch should not append a partial item");

    assert_eq!(
        loaded.users,
        vec![UserRecord {
            name: "alice".to_owned(),
            password: "patched-secret".to_owned(),
        }]
    );
}

#[test]
fn indexed_array_patches_ignore_replace_merge_strategy() {
    let loaded = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([
            FieldMetadata::new("users").merge_strategy(MergeStrategy::Replace)
        ]))
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[0].password="patched-secret""#,
        ]))
        .load()
        .expect("indexed array patch should not replace the entire array");

    assert_eq!(
        loaded.users,
        vec![UserRecord {
            name: "alice".to_owned(),
            password: "patched-secret".to_owned(),
        }]
    );
}

#[test]
fn whole_array_overrides_still_replace_when_combined_with_indexed_item_patches() {
    let defaults = UserArrayConfig {
        users: vec![
            UserRecord {
                name: "alice".to_owned(),
                password: "default-a".to_owned(),
            },
            UserRecord {
                name: "carol".to_owned(),
                password: "default-c".to_owned(),
            },
        ],
    };

    let loaded = ConfigLoader::new(defaults)
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users=[{"name":"bob","password":"base-secret"}]"#,
            "--set",
            r#"users[0].password="patched-secret""#,
        ]))
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.users,
        vec![UserRecord {
            name: "bob".to_owned(),
            password: "patched-secret".to_owned(),
        }]
    );
}

#[test]
fn sparse_indexed_array_overrides_are_rejected_early() {
    let error = ConfigLoader::new(UserArrayConfig { users: vec![] })
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users[2].name="eve""#,
            "--set",
            r#"users[2].password="late-secret""#,
        ]))
        .load()
        .expect_err("sparse array overrides must fail early");

    let ConfigError::InvalidArg { arg, message } = error else {
        panic!("expected invalid arg error");
    };
    assert!(arg.starts_with("--set "));
    assert!(arg.contains("users[2]."));
    assert!(message.contains("sparse array override"));
    assert!(message.contains("index 2"));
    assert!(message.contains("index 0"));
}

#[test]
fn sparse_indexed_array_overrides_after_direct_array_resets_are_rejected_early() {
    let error = ConfigLoader::new(UserArrayConfig { users: vec![] })
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"users=[{"name":"bob","password":"base-secret"}]"#,
            "--set",
            r#"users[2].password="late-secret""#,
        ]))
        .load()
        .expect_err("sparse array overrides after direct replacements must fail early");

    let ConfigError::InvalidArg { arg, message } = error else {
        panic!("expected invalid arg error");
    };
    assert!(arg.starts_with("--set "));
    assert!(arg.contains("users[2].password"));
    assert!(message.contains("sparse array override"));
    assert!(message.contains("index 2"));
    assert!(message.contains("index 1"));
}

#[test]
fn wildcard_declared_validation_runs_for_array_items() {
    let error = ConfigLoader::new(UserArrayConfig {
        users: vec![UserRecord {
            name: String::new(),
            password: String::new(),
        }],
    })
    .metadata(ConfigMetadata::from_fields([
        FieldMetadata::new("users.*.name").non_empty(),
        FieldMetadata::new("users.*.password").secret().non_empty(),
    ]))
    .load()
    .expect_err("declared validation must run for array items");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert!(errors.iter().any(|error| error.path == "users.0.name"));
    assert!(errors.iter().any(|error| {
        error.path == "users.0.password"
            && error.actual.as_ref().and_then(|value| value.as_str()) == Some("***redacted***")
    }));
}

#[test]
fn canonical_alias_conflicts_are_rejected() {
    let error = ConfigLoader::new(StringValueConfig::default())
        .metadata(ConfigMetadata::from_fields([
            FieldMetadata::new("value").alias("legacy")
        ]))
        .layer(
            Layer::custom(
                "conflict",
                serde_json::json!({
                    "value": "canonical",
                    "legacy": "alias"
                }),
            )
            .expect("layer"),
        )
        .load()
        .expect_err("conflicting alias and canonical paths must fail");

    let ConfigError::PathConflict {
        first_path,
        second_path,
        canonical_path,
    } = error
    else {
        panic!("expected path conflict");
    };

    assert_eq!(first_path, "legacy");
    assert_eq!(second_path, "value");
    assert_eq!(canonical_path, "value");
}

#[test]
fn declared_validation_rules_return_structured_errors_and_redact_secrets() {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("server.host").non_empty(),
        FieldMetadata::new("server.port").min(1),
        FieldMetadata::new("db.password").secret().non_empty(),
    ]);
    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"server.host="""#,
        "--set",
        "server.port=0",
        "--set",
        r#"db.password="""#,
    ]);

    let error = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .args(args)
        .load()
        .expect_err("declared validation must fail");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert_eq!(errors.len(), 3);

    let host = errors
        .iter()
        .find(|error| error.path == "server.host")
        .expect("server.host validation error");
    assert_eq!(host.rule.as_deref(), Some("non_empty"));
    assert_eq!(
        host.actual.as_ref().and_then(|value| value.as_str()),
        Some("")
    );

    let port = errors
        .iter()
        .find(|error| error.path == "server.port")
        .expect("server.port validation error");
    assert_eq!(port.rule.as_deref(), Some("min"));
    assert_eq!(
        port.expected.as_ref().and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        port.actual.as_ref().and_then(|value| value.as_u64()),
        Some(0)
    );

    let password = errors
        .iter()
        .find(|error| error.path == "db.password")
        .expect("db.password validation error");
    assert_eq!(password.rule.as_deref(), Some("non_empty"));
    assert_eq!(
        password.actual.as_ref().and_then(|value| value.as_str()),
        Some("***redacted***")
    );
}

#[test]
fn invalid_declarative_numeric_bounds_return_structured_errors() {
    let error = ConfigLoader::new(PortOnlyConfig::default())
        .metadata(ConfigMetadata::from_fields([
            FieldMetadata::new("port").min(f64::NAN)
        ]))
        .load()
        .expect_err("invalid bounds must fail without panicking");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert_eq!(errors.len(), 1);
    let error = errors.iter().next().expect("validation error");
    assert_eq!(error.path, "port");
    assert_eq!(error.rule.as_deref(), Some("min"));
    assert!(error.message.contains("must be finite"));
    assert_eq!(
        error.expected.as_ref().and_then(|value| value.as_str()),
        Some("NaN")
    );
}

#[test]
fn declared_validation_supports_cross_field_checks_and_extended_rules() {
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct AdvancedValidationConfig {
        endpoint: AdvancedEndpoint,
        tls: AdvancedTls,
        runtime: AdvancedRuntime,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct AdvancedEndpoint {
        host: String,
        listen: String,
        ip: String,
        mode: String,
        unix_socket: Option<String>,
        port: Option<u16>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct AdvancedTls {
        enabled: bool,
        cert: Option<String>,
        key: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct AdvancedRuntime {
        state_dir: String,
    }

    impl Default for AdvancedValidationConfig {
        fn default() -> Self {
            Self {
                endpoint: AdvancedEndpoint {
                    host: "api.internal".to_owned(),
                    listen: "127.0.0.1:8080".to_owned(),
                    ip: "127.0.0.1".to_owned(),
                    mode: "memory".to_owned(),
                    unix_socket: None,
                    port: Some(8080),
                },
                tls: AdvancedTls {
                    enabled: false,
                    cert: None,
                    key: None,
                },
                runtime: AdvancedRuntime {
                    state_dir: "/var/lib/tier".to_owned(),
                },
            }
        }
    }

    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("endpoint.host").hostname(),
        FieldMetadata::new("endpoint.listen").socket_addr(),
        FieldMetadata::new("endpoint.ip").ip_addr(),
        FieldMetadata::new("endpoint.mode").one_of(["memory", "redis"]),
        FieldMetadata::new("runtime.state_dir").absolute_path(),
    ])
    .exactly_one_of(["endpoint.port", "endpoint.unix_socket"])
    .required_if("tls.enabled", true, ["tls.cert", "tls.key"]);

    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"endpoint.host="bad host""#,
        "--set",
        r#"endpoint.listen="localhost""#,
        "--set",
        r#"endpoint.ip="not-an-ip""#,
        "--set",
        r#"endpoint.mode="disk""#,
        "--set",
        r#"runtime.state_dir="relative/path""#,
        "--set",
        "endpoint.port=8080",
        "--set",
        r#"endpoint.unix_socket="/tmp/tier.sock""#,
        "--set",
        "tls.enabled=true",
    ]);

    let error = ConfigLoader::new(AdvancedValidationConfig::default())
        .metadata(metadata)
        .args(args)
        .load()
        .expect_err("advanced declared validation must fail");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("hostname"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("socket_addr"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("ip_addr"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("one_of"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("absolute_path"))
    );

    let exactly_one = errors
        .iter()
        .find(|error| error.rule.as_deref() == Some("exactly_one_of"))
        .expect("exactly one of error");
    assert_eq!(exactly_one.path, "");
    assert_eq!(
        exactly_one.related_paths,
        vec![
            "endpoint.port".to_owned(),
            "endpoint.unix_socket".to_owned()
        ]
    );

    let required_if = errors
        .iter()
        .find(|error| error.rule.as_deref() == Some("required_if"))
        .expect("required_if error");
    assert_eq!(
        required_if.related_paths,
        vec![
            "tls.enabled".to_owned(),
            "tls.cert".to_owned(),
            "tls.key".to_owned(),
        ]
    );
}

#[test]
fn wildcard_required_if_binds_to_the_matching_collection_item() {
    let error = ConfigLoader::new(WildcardCheckConfig {
        users: vec![
            WildcardCheckUser {
                enabled: true,
                password: Some("ok".to_owned()),
                cert: None,
                key: None,
            },
            WildcardCheckUser {
                enabled: true,
                password: None,
                cert: None,
                key: None,
            },
        ],
    })
    .metadata(ConfigMetadata::new().required_if("users.*.enabled", true, ["users.*.password"]))
    .load()
    .expect_err("missing password for a matched item should fail");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    let wildcard_error = errors
        .iter()
        .find(|entry| entry.rule.as_deref() == Some("required_if"))
        .expect("required_if error");
    assert_eq!(
        wildcard_error.related_paths,
        vec!["users.1.enabled".to_owned(), "users.1.password".to_owned()]
    );
    assert_eq!(
        wildcard_error
            .actual
            .as_ref()
            .and_then(|value| value.get("missing"))
            .and_then(serde_json::Value::as_array)
            .map(|values| values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()),
        Some(vec!["users.1.password"])
    );
}

#[test]
fn wildcard_required_with_binds_to_the_matching_collection_item() {
    let error = ConfigLoader::new(WildcardCheckConfig {
        users: vec![
            WildcardCheckUser {
                enabled: false,
                password: None,
                cert: Some("cert.pem".to_owned()),
                key: Some("key.pem".to_owned()),
            },
            WildcardCheckUser {
                enabled: false,
                password: None,
                cert: None,
                key: Some("key.pem".to_owned()),
            },
        ],
    })
    .metadata(ConfigMetadata::new().required_with("users.*.key", ["users.*.cert"]))
    .load()
    .expect_err("missing cert for a matched item should fail");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    let wildcard_error = errors
        .iter()
        .find(|entry| entry.rule.as_deref() == Some("required_with"))
        .expect("required_with error");
    assert_eq!(
        wildcard_error.related_paths,
        vec!["users.1.key".to_owned(), "users.1.cert".to_owned()]
    );
    assert_eq!(
        wildcard_error
            .actual
            .as_ref()
            .and_then(|value| value.get("missing"))
            .and_then(serde_json::Value::as_array)
            .map(|values| values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()),
        Some(vec!["users.1.cert"])
    );
}

#[test]
fn declared_checks_accept_alias_paths() {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("server.token").alias("service.legacyToken"),
        FieldMetadata::new("server.cert").alias("service.legacyCert"),
    ])
    .required_with("service.legacyToken", ["service.legacyCert"]);

    let error = ConfigLoader::new(AliasValidationConfig::default())
        .metadata(metadata)
        .args(ArgsSource::from_args([
            "tier",
            "--set",
            r#"service.legacyToken="secret""#,
        ]))
        .load()
        .expect_err("alias-based declared checks should fail when required fields are missing");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    let alias_error = errors
        .iter()
        .find(|entry| entry.rule.as_deref() == Some("required_with"))
        .expect("required_with error");
    assert_eq!(
        alias_error.related_paths,
        vec!["server.token".to_owned(), "server.cert".to_owned()]
    );
    assert_eq!(
        alias_error
            .actual
            .as_ref()
            .and_then(|value| value.get("missing"))
            .and_then(serde_json::Value::as_array)
            .map(|values| values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()),
        Some(vec!["server.cert"])
    );
}

#[test]
fn manual_metadata_drives_env_overrides_redaction_and_deprecation_warnings() {
    let env = EnvSource::from_pairs([
        ("DATABASE_URL", "postgres://env/db"),
        ("DB_PASSWORD", "env-secret"),
    ]);
    let args = ArgsSource::from_args(["tier", "--set", "server.port=7000"]);
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("db.url")
            .env("DATABASE_URL")
            .doc("Primary database connection URL"),
        FieldMetadata::new("db.password")
            .env("DB_PASSWORD")
            .secret(),
        FieldMetadata::new("server.port").deprecated("use server.bind_port instead"),
    ]);

    let loaded = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .env(env)
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(loaded.db.url, "postgres://env/db");
    assert_eq!(loaded.db.password, "env-secret");
    assert_eq!(loaded.server.port, 7000);

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("env-secret"));

    let warnings = loaded.report().warnings();
    assert!(warnings.iter().any(|warning| {
        warning
            .to_string()
            .contains("deprecated field `server.port`")
    }));
}

#[test]
fn duplicate_explicit_env_names_are_rejected() {
    let env = EnvSource::from_pairs([("DATABASE_URL", "postgres://env/db")]);
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("db.url").env("DATABASE_URL"),
        FieldMetadata::new("db.password").env("DATABASE_URL"),
    ]);

    let error = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .env(env)
        .load()
        .expect_err("duplicate explicit env names should fail");

    let ConfigError::MetadataConflict {
        kind,
        name,
        first_path,
        second_path,
    } = error
    else {
        panic!("expected metadata conflict");
    };

    assert_eq!(kind, "environment variable");
    assert_eq!(name, "DATABASE_URL");
    assert_eq!(
        [first_path.as_str(), second_path.as_str()],
        ["db.password", "db.url"]
    );
}

#[test]
fn wildcard_explicit_env_names_are_rejected() {
    let env = EnvSource::from_pairs([("APP_USER_PASSWORD", "secret")]);
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("users.*.password").env("APP_USER_PASSWORD")
    ]);

    let error = ConfigLoader::new(UserArrayConfig::default())
        .metadata(metadata)
        .env(env)
        .load()
        .expect_err("wildcard explicit env names should fail");

    let ConfigError::MetadataInvalid { path, message } = error else {
        panic!("expected metadata invalid error");
    };

    assert_eq!(path, "users.*.password");
    assert!(message.contains("wildcard"));
}

#[test]
fn duplicate_aliases_are_rejected() {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("first").alias("legacy"),
        FieldMetadata::new("second").alias("legacy"),
    ]);

    let error = ConfigLoader::new(AliasCollisionConfig::default())
        .metadata(metadata)
        .args(ArgsSource::from_args(["tier", "--set", "legacy=override"]))
        .load()
        .expect_err("duplicate aliases should fail");

    let ConfigError::MetadataConflict {
        kind,
        name,
        first_path,
        second_path,
    } = error
    else {
        panic!("expected metadata conflict");
    };

    assert_eq!(kind, "alias");
    assert_eq!(name, "legacy");
    assert_eq!(first_path, "first");
    assert_eq!(second_path, "second");
}

#[test]
fn wildcard_aliases_must_preserve_path_structure() {
    let metadata = ConfigMetadata::from_fields([FieldMetadata::new("db.password").alias("db.*")]);

    let error = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .load()
        .expect_err("lossy wildcard aliases should fail");

    let ConfigError::MetadataInvalid { path, message } = error else {
        panic!("expected metadata invalid error");
    };

    assert_eq!(path, "db.*");
    assert!(message.contains("preserve wildcard positions"));
}

#[test]
fn ambiguous_alias_patterns_are_rejected() {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("users.*.password").alias("users.*"),
        FieldMetadata::new("*.admin.token").alias("*.admin"),
    ]);

    let error = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .load()
        .expect_err("ambiguous alias patterns should fail");

    let ConfigError::MetadataInvalid { path, message } = error else {
        panic!("expected metadata invalid error");
    };

    assert!(path == "users.*" || path == "*.admin");
    assert!(message.contains("overlaps ambiguously"));
    assert!(message.contains("users.*"));
    assert!(message.contains("users.admin"));
}

#[test]
fn field_level_merge_strategies_control_layering() {
    let dir = tempdir().expect("temporary directory");
    let config_path = dir.path().join("merge.toml");
    fs::write(
        &config_path,
        r#"
            plugins = ["file"]

            [headers]
            x-file = "2"

            [server.tls]
            cert = "file-cert.pem"
        "#,
    )
    .expect("config file");

    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"plugins=["cli"]"#,
        "--set",
        r#"headers={"x-cli":"3"}"#,
    ]);
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("plugins").merge_strategy(MergeStrategy::Append),
        FieldMetadata::new("headers").merge_strategy(MergeStrategy::Merge),
        FieldMetadata::new("server.tls").merge_strategy(MergeStrategy::Replace),
    ]);

    let loaded = ConfigLoader::new(MergeConfig::default())
        .file(config_path)
        .args(args)
        .metadata(metadata)
        .load()
        .expect("config loads");

    assert_eq!(loaded.plugins, vec!["core", "file", "cli"]);
    assert_eq!(
        loaded.headers.get("x-default").map(String::as_str),
        Some("1")
    );
    assert_eq!(loaded.headers.get("x-file").map(String::as_str), Some("2"));
    assert_eq!(loaded.headers.get("x-cli").map(String::as_str), Some("3"));
    assert_eq!(loaded.server.tls.cert, "file-cert.pem");
    assert_eq!(loaded.server.tls.key, None);
}

#[test]
fn wildcard_merge_strategies_apply_to_concrete_paths() {
    let overlay = Layer::custom(
        "overlay",
        serde_json::json!({
            "headers": {
                "svc": { "b": "2" }
            }
        }),
    )
    .expect("custom layer");
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("headers.*").merge_strategy(MergeStrategy::Replace)
    ]);

    let loaded = ConfigLoader::new(WildcardMergeConfig::default())
        .layer(overlay)
        .metadata(metadata)
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.headers.get("svc"),
        Some(&BTreeMap::from([("b".to_owned(), "2".to_owned())]))
    );
}

#[test]
fn warns_on_unknown_fields_with_suggestions() {
    let dir = tempdir().expect("temporary directory");
    let config_path = dir.path().join("typo.toml");
    fs::write(
        &config_path,
        r#"
            [server]
            posrt = 8088
        "#,
    )
    .expect("config file");

    let loaded = ConfigLoader::new(AppConfig::default())
        .file(config_path)
        .warn_unknown_fields()
        .load()
        .expect("config loads with warning");

    assert_eq!(loaded.server.port, 3000);
    assert!(loaded.report().has_warnings());
    assert_eq!(loaded.report().warnings().len(), 1);

    let warning = loaded.report().warnings()[0].to_string();
    assert!(warning.contains("server.posrt"));
    assert!(warning.contains("server.port"));

    let doctor = loaded.report().doctor();
    assert!(doctor.contains("Warnings: 1"));
    assert!(doctor.contains("server.posrt"));
}

#[test]
fn unknown_field_suggestions_prefer_metadata_over_runtime_shape() {
    let error = ConfigLoader::new(OptionalTokenConfig::default())
        .env(EnvSource::from_pairs([("APP_TOKNE", "\"secret\"")]).prefix("APP"))
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new("token")]))
        .load()
        .expect_err("unknown fields should fail");

    let message = error.to_string();
    assert!(message.contains("tokne"));
    assert!(message.contains("token"));
}

#[test]
fn metadata_free_unknown_fields_still_get_shape_based_suggestions() {
    let error = ConfigLoader::new(OptionalTokenConfig::default())
        .env(EnvSource::from_pairs([("APP_TOKNE", "secret")]).prefix("APP"))
        .load()
        .expect_err("unknown fields should fail");

    let message = error.to_string();
    assert!(message.contains("tokne"));
    assert!(message.contains("token"));
}

#[test]
fn root_level_unknown_fields_preserve_source_information() {
    let error = ConfigLoader::new(AppConfig::default())
        .args(ArgsSource::from_args(["app", "--set", "serber.port=7000"]))
        .load()
        .expect_err("unknown fields should fail");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    let field = &fields[0];
    assert_eq!(field.path, "serber");
    let source = field.source.as_ref().expect("unknown field source");
    assert_eq!(source.kind, SourceKind::Arguments);
    assert_eq!(source.name, "--set serber.port=7000");
}

#[test]
fn metadata_unknown_fields_are_reported_before_deserialize_failures() {
    let error = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([FieldMetadata::new(
            "users.*.password",
        )]))
        .args(ArgsSource::from_args([
            "app",
            "--set",
            "users.0.passwrod=bad",
        ]))
        .load()
        .expect_err("unknown field should be reported before deserialize failure");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    let field = &fields[0];
    assert_eq!(field.path, "users.0.passwrod");
    assert_eq!(field.suggestion.as_deref(), Some("users.0.password"));
}

#[test]
fn parent_object_metadata_does_not_hide_child_unknown_fields() {
    let error = ConfigLoader::new(UserArrayConfig::default())
        .metadata(ConfigMetadata::from_fields([
            FieldMetadata::new("users.0").merge_strategy(MergeStrategy::Replace)
        ]))
        .args(ArgsSource::from_args([
            "app",
            "--set",
            "users.0.passwrod=bad",
        ]))
        .load()
        .expect_err("unknown child field should still be reported");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    let field = &fields[0];
    assert_eq!(field.path, "users.0.passwrod");
    assert_eq!(field.suggestion.as_deref(), Some("users.0.password"));
}

#[test]
fn metadata_free_unknown_fields_are_reported_before_deserialize_failures() {
    let error = ConfigLoader::new(UserArrayConfig::default())
        .args(ArgsSource::from_args([
            "app",
            "--set",
            "users.0.passwrod=bad",
        ]))
        .load()
        .expect_err("unknown field should be reported before deserialize failure");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    let field = &fields[0];
    assert_eq!(field.path, "users.0.passwrod");
    assert_eq!(field.suggestion.as_deref(), Some("users.0.password"));
}

#[test]
fn doctor_and_audit_outputs_are_structured() {
    let env = EnvSource::from_pairs([("APP__SERVER__PORT", "9100")]).prefix("APP");
    let loaded = ConfigLoader::new(AppConfig::default())
        .env(env)
        .secret_path("db.password")
        .load()
        .expect("config loads");

    let doctor = loaded.report().doctor_report();
    assert_eq!(doctor.format_version, REPORT_FORMAT_VERSION);
    assert_eq!(doctor.summary.source_count, 2);
    assert_eq!(doctor.summary.warning_count, 0);
    assert!(doctor.summary.trace_count >= 1);
    assert_eq!(doctor.summary.secret_path_count, 1);

    let doctor_json = loaded.report().doctor_json();
    assert_eq!(
        doctor_json["format_version"].as_u64(),
        Some(REPORT_FORMAT_VERSION as u64)
    );
    assert_eq!(doctor_json["summary"]["source_count"].as_u64(), Some(2));
    assert_eq!(
        doctor_json["summary"]["secret_path_count"].as_u64(),
        Some(1)
    );

    let audit_json = loaded.report().audit_json();
    assert_eq!(
        audit_json["format_version"].as_u64(),
        Some(REPORT_FORMAT_VERSION as u64)
    );
    assert_eq!(
        audit_json["traces"]["server.port"]["explanation"]["final_value"].as_i64(),
        Some(9100)
    );
    assert_eq!(
        audit_json["traces"]["db.password"]["explanation"]["final_value"].as_str(),
        Some("***redacted***")
    );
}

#[test]
fn root_path_can_be_explained_and_reports_latest_source() {
    let env = EnvSource::from_pairs([("APP__SERVER__PORT", "9100")]).prefix("APP");
    let loaded = ConfigLoader::new(AppConfig::default())
        .env(env)
        .load()
        .expect("config loads");

    let explanation = loaded.report().explain(".").expect("root explanation");
    assert_eq!(explanation.path, "");
    assert!(explanation.final_value.is_some());
    assert!(!explanation.steps.is_empty());

    let audit = loaded.report().audit_report();
    let latest = audit
        .traces
        .get("")
        .and_then(|trace| trace.last_source.as_ref())
        .expect("root last source");
    assert_eq!(latest.kind, SourceKind::Environment);
}

#[test]
fn denies_unknown_fields_by_default() {
    let dir = tempdir().expect("temporary directory");
    let config_path = dir.path().join("typo.toml");
    fs::write(
        &config_path,
        r#"
            [server]
            host = "0.0.0.0"
            porrt = 8088
        "#,
    )
    .expect("config file");

    let error = ConfigLoader::new(AppConfig::default())
        .file(config_path)
        .load()
        .expect_err("unknown fields should fail by default");

    let message = error.to_string();
    assert!(message.contains("unknown configuration fields"));
    assert!(message.contains("server.porrt"));
    assert!(message.contains("server.port"));
}

#[test]
fn tuple_extra_indices_are_reported_as_unknown_fields() {
    let error = ConfigLoader::new(TupleOverrideConfig::default())
        .args(ArgsSource::from_args(["app", "--set", "pair[2]=42"]))
        .load()
        .expect_err("extra tuple indices should be rejected as unknown fields");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].path, "pair.2");
}

#[test]
fn tuple_whole_array_overrides_reject_extra_indices_as_unknown_fields() {
    let error = ConfigLoader::new(TupleOverrideConfig::default())
        .args(ArgsSource::from_args([
            "app",
            "--set",
            r#"pair=["edge",8080,42]"#,
        ]))
        .load()
        .expect_err("extra tuple elements should be rejected as unknown fields");

    let ConfigError::UnknownFields { fields } = error else {
        panic!("expected unknown fields error");
    };

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].path, "pair.2");
}

#[test]
fn searches_candidate_files_in_order() {
    let dir = tempdir().expect("temporary directory");
    let missing_path = dir.path().join("missing.toml");
    let fallback_path = dir.path().join("fallback.toml");
    fs::write(
        &fallback_path,
        r#"
            [server]
            port = 7000
        "#,
    )
    .expect("fallback file");

    let loaded = ConfigLoader::new(AppConfig::default())
        .with_file(FileSource::search([missing_path, fallback_path]))
        .load()
        .expect("fallback file should be used");

    assert_eq!(loaded.server.port, 7000);
}

#[test]
fn loads_extensionless_file_with_explicit_format() {
    let dir = tempdir().expect("temporary directory");
    let config_path = dir.path().join("runtime");
    fs::write(
        &config_path,
        r#"
            [server]
            port = 6100
        "#,
    )
    .expect("config file");

    let loaded = ConfigLoader::new(AppConfig::default())
        .with_file(FileSource::new(config_path).format(FileFormat::Toml))
        .load()
        .expect("config should load with explicit format");

    assert_eq!(loaded.server.port, 6100);
}

#[test]
fn doctor_json_is_machine_readable() {
    let loaded = ConfigLoader::new(AppConfig::default())
        .validator("port-range", |config| {
            if config.server.port == 0 {
                return Err(ValidationErrors::from_message(
                    "server.port",
                    "port must be greater than zero",
                ));
            }
            Ok(())
        })
        .load()
        .expect("config loads");

    let doctor = loaded.report().doctor_json();
    assert_eq!(
        doctor["format_version"].as_u64(),
        Some(REPORT_FORMAT_VERSION as u64)
    );
    assert_eq!(doctor["sources"].as_array().map(Vec::len), Some(1));
    assert_eq!(doctor["validations"].as_array().map(Vec::len), Some(1));
    assert!(doctor["redacted_final"].is_object());
}
