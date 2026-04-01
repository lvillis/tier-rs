#![cfg(feature = "toml")]

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};
use tempfile::tempdir;

use tier::{
    ArgsSource, ConfigError, ConfigLoader, ConfigMetadata, EnvSource, FieldMetadata, FileFormat,
    FileSource, MergeStrategy, REPORT_FORMAT_VERSION, ValidationErrors,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PortOnlyConfig {
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
struct OptionalTokenConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
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
