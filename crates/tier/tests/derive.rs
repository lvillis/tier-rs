#![cfg(feature = "derive")]

use serde::{Deserialize, Serialize};

use tier::{
    ArgsSource, ConfigLoader, EnvSource, MergeStrategy, Secret, TierConfig, TierMetadata,
    ValidationCheck, ValidationRule,
};

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedConfig {
    server: DerivedServer,
    #[serde(rename = "database")]
    db: DerivedDb,
    #[serde(flatten)]
    common: DerivedCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedServer {
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
struct DerivedDb {
    password: Secret<String>,
    #[tier(secret)]
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedCommon {
    #[tier(secret)]
    api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[serde(rename_all = "camelCase")]
struct SerdeNamingConfig {
    /// Primary host configured through serde rename rules.
    #[serde(
        rename(serialize = "bindHost", deserialize = "bind-host"),
        alias = "bind_addr"
    )]
    host_name: String,
    #[serde(alias = "legacyPort")]
    port_value: u16,
    #[serde(skip_deserializing)]
    skipped_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct AliasDrivenConfig {
    #[serde(alias = "service")]
    server: AliasDrivenServer,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[serde(rename_all = "camelCase")]
struct AliasDrivenServer {
    #[serde(alias = "legacyPort")]
    bind_port: u16,
    #[serde(alias = "legacyToken")]
    auth_token: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[serde(default, rename_all = "camelCase")]
struct SerdeDefaultMergeConfig {
    #[tier(merge = "append", example = "[\"edge\"]")]
    peers: Vec<String>,
    #[tier(merge = "replace")]
    tls: SerdeDefaultTls,
    #[serde(default = "default_region")]
    region_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[serde(default)]
struct SerdeDefaultTls {
    cert_path: String,
    key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct WrappedTls(SerdeDefaultTls);

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedValidationConfig {
    #[tier(non_empty, min_length = 3, max_length = 32)]
    service_name: String,
    #[tier(min = -10, max = 100)]
    health_score: i32,
    #[tier(one_of("memory", "redis"), non_empty)]
    backend: String,
    #[tier(hostname)]
    hostname: String,
    #[tier(socket_addr)]
    listen_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[tier(at_least_one_of("port", "unix_socket"))]
#[tier(required_if(path = "tls.enabled", equals = true, requires("tls.cert", "tls.key")))]
struct DerivedCrossValidationConfig {
    port: Option<u16>,
    unix_socket: Option<String>,
    tls: DerivedTlsValidationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedTlsValidationConfig {
    enabled: bool,
    cert: Option<String>,
    key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", rename_all_fields = "camelCase")]
enum BackendConfig {
    Memory {
        max_items: usize,
    },
    #[serde(alias = "legacy-redis")]
    Redis {
        endpoint_url: String,
        auth_token: Secret<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct ExternalEnumConfig {
    backend: BackendConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[serde(
    tag = "kind",
    content = "config",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum CacheLayer {
    Memory { max_items: usize },
    Disk { path_root: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct AdjacentEnumConfig {
    cache: CacheLayer,
}

fn default_region() -> String {
    "ap-southeast-1".to_owned()
}

impl Default for AliasDrivenConfig {
    fn default() -> Self {
        Self {
            server: AliasDrivenServer {
                bind_port: 3000,
                auth_token: Secret::new("derived-alias-secret".to_owned()),
            },
        }
    }
}

impl Default for DerivedConfig {
    fn default() -> Self {
        Self {
            server: DerivedServer {
                host: "127.0.0.1".to_owned(),
                port: 3000,
            },
            db: DerivedDb {
                password: Secret::new("derived-password-secret".to_owned()),
                token: "derived-token-secret".to_owned(),
            },
            common: DerivedCommon {
                api_key: "derived-api-key-secret".to_owned(),
            },
        }
    }
}

impl Default for SerdeDefaultMergeConfig {
    fn default() -> Self {
        Self {
            peers: vec!["core".to_owned()],
            tls: SerdeDefaultTls::default(),
            region_name: default_region(),
        }
    }
}

impl Default for SerdeDefaultTls {
    fn default() -> Self {
        Self {
            cert_path: "default-cert.pem".to_owned(),
            key_path: Some("default-key.pem".to_owned()),
        }
    }
}

impl Default for ExternalEnumConfig {
    fn default() -> Self {
        Self {
            backend: BackendConfig::Memory { max_items: 64 },
        }
    }
}

#[test]
fn derive_metadata_collects_structured_field_metadata() {
    let metadata = DerivedConfig::metadata();
    let paths = DerivedConfig::secret_paths();

    let host = metadata.field("server.host").expect("server.host metadata");
    assert_eq!(host.env.as_deref(), Some("APP_SERVER_HOST"));
    assert_eq!(host.doc.as_deref(), Some("IP address or hostname to bind"));
    assert_eq!(host.example.as_deref(), Some("0.0.0.0"));

    let port = metadata.field("server.port").expect("server.port metadata");
    assert_eq!(
        port.deprecated.as_deref(),
        Some("use server.bind_port instead")
    );

    assert!(paths.contains(&"database.password".to_owned()));
    assert!(paths.contains(&"database.token".to_owned()));
    assert!(paths.contains(&"api_key".to_owned()));
}

#[test]
fn loader_can_use_derived_metadata_for_redaction_env_mapping_and_warnings() {
    let env = EnvSource::from_pairs([("APP_SERVER_HOST", "\"0.0.0.0\"")]);
    let args = ArgsSource::from_args(["tier", "--set", "server.port=9001"]);
    let loaded = ConfigLoader::new(DerivedConfig::default())
        .derive_metadata()
        .env(env)
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.host, "0.0.0.0");
    assert_eq!(loaded.server.port, 9001);

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("derived-password-secret"));
    assert!(!rendered.contains("derived-token-secret"));
    assert!(!rendered.contains("derived-api-key-secret"));

    let warnings = loaded.report().warnings();
    assert!(warnings.iter().any(|warning| {
        warning
            .to_string()
            .contains("deprecated field `server.port`")
    }));
}

#[test]
fn derive_metadata_tracks_serde_rename_rules_aliases_and_skip_deserializing() {
    let metadata = SerdeNamingConfig::metadata();

    let host = metadata.field("bindHost").expect("bindHost metadata");
    assert!(host.aliases.contains(&"bind-host".to_owned()));
    assert!(host.aliases.contains(&"bind_addr".to_owned()));
    assert_eq!(
        host.doc.as_deref(),
        Some("Primary host configured through serde rename rules.")
    );

    let port = metadata.field("portValue").expect("portValue metadata");
    assert!(port.aliases.contains(&"legacyPort".to_owned()));

    assert!(metadata.field("skippedValue").is_none());
}

#[test]
fn loader_canonicalizes_serde_alias_paths_for_traces_and_redaction() {
    let args = ArgsSource::from_args([
        "tier",
        "--set",
        "service.legacyPort=9010",
        "--set",
        "service.legacyToken=\"rotated-secret\"",
    ]);

    let loaded = ConfigLoader::new(AliasDrivenConfig::default())
        .derive_metadata()
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(loaded.server.bind_port, 9010);
    assert_eq!(loaded.server.auth_token.expose_ref(), "rotated-secret");

    let explanation = loaded
        .report()
        .explain("server.bindPort")
        .expect("canonical explain path");
    assert!(explanation.steps.iter().any(|step| {
        step.source
            .to_string()
            .contains("--set service.legacyPort=9010")
    }));

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("bindPort"));
    assert!(rendered.contains("authToken"));
    assert!(!rendered.contains("service"));
    assert!(!rendered.contains("legacyToken"));
    assert!(!rendered.contains("rotated-secret"));
}

#[test]
fn derive_metadata_tracks_serde_defaults_and_merge_strategies() {
    let metadata = SerdeDefaultMergeConfig::metadata();

    let peers = metadata.field("peers").expect("peers metadata");
    assert!(peers.has_default);
    assert_eq!(peers.merge, MergeStrategy::Append);
    assert_eq!(peers.example.as_deref(), Some("[\"edge\"]"));

    let tls = metadata.field("tls").expect("tls metadata");
    assert!(tls.has_default);
    assert_eq!(tls.merge, MergeStrategy::Replace);

    let region = metadata.field("regionName").expect("regionName metadata");
    assert!(region.has_default);
}

#[test]
fn derive_metadata_supports_newtype_wrappers() {
    let metadata = WrappedTls::metadata();
    assert!(metadata.field("cert_path").is_some());
    assert!(metadata.field("key_path").is_some());
}

#[test]
fn derive_metadata_collects_declared_validation_rules() {
    let metadata = DerivedValidationConfig::metadata();

    let service_name = metadata
        .field("service_name")
        .expect("service_name metadata");
    assert_eq!(
        service_name.validations,
        vec![
            ValidationRule::NonEmpty,
            ValidationRule::MinLength(3),
            ValidationRule::MaxLength(32),
        ]
    );

    let health_score = metadata
        .field("health_score")
        .expect("health_score metadata");
    assert_eq!(
        health_score.validations,
        vec![
            ValidationRule::Min((-10).into()),
            ValidationRule::Max(100.into())
        ]
    );

    let backend = metadata.field("backend").expect("backend metadata");
    assert_eq!(
        backend.validations,
        vec![
            ValidationRule::NonEmpty,
            ValidationRule::OneOf(vec!["memory".into(), "redis".into()]),
        ]
    );

    let hostname = metadata.field("hostname").expect("hostname metadata");
    assert_eq!(hostname.validations, vec![ValidationRule::Hostname]);

    let listen_addr = metadata.field("listen_addr").expect("listen_addr metadata");
    assert_eq!(listen_addr.validations, vec![ValidationRule::SocketAddr]);
}

#[test]
fn derive_metadata_collects_cross_field_validation_checks() {
    let metadata = DerivedCrossValidationConfig::metadata();

    assert_eq!(
        metadata.checks(),
        &[
            ValidationCheck::AtLeastOneOf {
                paths: vec!["port".to_owned(), "unix_socket".to_owned()],
            },
            ValidationCheck::RequiredIf {
                path: "tls.enabled".to_owned(),
                equals: true.into(),
                requires: vec!["tls.cert".to_owned(), "tls.key".to_owned()],
            },
        ]
    );
}

#[test]
fn derive_metadata_supports_external_enums_and_variant_aliases() {
    let metadata = ExternalEnumConfig::metadata();

    let endpoint = metadata
        .field("backend.redis.endpointUrl")
        .expect("redis endpoint metadata");
    assert!(
        endpoint
            .aliases
            .contains(&"backend.legacy-redis.endpointUrl".to_owned())
    );

    let token = metadata
        .field("backend.redis.authToken")
        .expect("redis token metadata");
    assert!(token.secret);
    assert!(
        token
            .aliases
            .contains(&"backend.legacy-redis.authToken".to_owned())
    );
}

#[test]
fn derive_metadata_supports_adjacent_tagged_enums() {
    let metadata = AdjacentEnumConfig::metadata();

    assert!(metadata.field("cache.config.maxItems").is_some());
    assert!(metadata.field("cache.config.pathRoot").is_some());
}

#[test]
fn loader_canonicalizes_external_enum_variant_aliases() {
    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"backend.legacy-redis.endpointUrl="redis://localhost""#,
        "--set",
        r#"backend.legacy-redis.authToken="redis-secret""#,
    ]);

    let loaded = ConfigLoader::new(ExternalEnumConfig::default())
        .derive_metadata()
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.backend,
        BackendConfig::Redis {
            endpoint_url: "redis://localhost".to_owned(),
            auth_token: Secret::new("redis-secret".to_owned()),
        }
    );

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("endpointUrl"));
    assert!(!rendered.contains("legacy-redis"));
    assert!(!rendered.contains("redis-secret"));
}
