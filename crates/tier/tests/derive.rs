#![cfg(feature = "derive")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use tier::{
    ArgsSource, ConfigError, ConfigLoader, ConfigMetadata, EnvSource, FieldMetadata, MergeStrategy,
    Secret, SourceKind, TierConfig, TierMetadata, ValidationCheck, ValidationRule,
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
    #[tier(pattern = "^[a-z0-9-]+$")]
    service_slug: String,
    #[tier(min = -10, max = 100)]
    health_score: i32,
    #[tier(min_items = 1, max_items = 4)]
    ports: Vec<u16>,
    #[tier(min_properties = 1, max_properties = 3)]
    labels: BTreeMap<String, String>,
    #[tier(multiple_of = 4)]
    worker_count: u16,
    #[tier(unique_items)]
    tags: Vec<String>,
    #[tier(one_of("memory", "redis"), non_empty)]
    backend: String,
    #[tier(hostname)]
    hostname: String,
    #[tier(url)]
    service_url: String,
    #[tier(email)]
    contact_email: String,
    #[tier(socket_addr)]
    listen_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq, Default)]
struct DerivedEnvDecodeConfig {
    #[tier(env = "APP_PORTS", env_decode = "csv")]
    ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum LeafBackend {
    Memory,
    Redis,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct LeafEnumConfig {
    #[tier(env = "APP_BACKEND", doc = "Selected backend mode", leaf)]
    backend: LeafBackend,
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

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[tier(at_least_one_of_expr(
    tier::path!(DerivedExprCrossValidationConfig.port),
    tier::path!(DerivedExprCrossValidationConfig.unix_socket)
))]
#[tier(required_if(
    path_expr = tier::path!(DerivedExprCrossValidationConfig.tls.enabled),
    equals = true,
    requires_expr(
        tier::path!(DerivedExprCrossValidationConfig.tls.cert),
        tier::path!(DerivedExprCrossValidationConfig.tls.key)
    )
))]
struct DerivedExprCrossValidationConfig {
    port: Option<u16>,
    unix_socket: Option<String>,
    tls: DerivedTlsValidationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq, Default)]
struct DerivedSourcePolicyConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[tier(sources("env", "cli"))]
    token: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InternalCacheLayer {
    Memory { max_items: usize },
    Disk { path_root: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct InternalEnumConfig {
    cache: InternalCacheLayer,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(untagged)]
enum UntaggedEndpoint {
    Tcp { port: u16 },
    Unix { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct UntaggedEnumConfig {
    endpoint: UntaggedEndpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AcronymBackend {
    HTTPServer { bind_port: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct AcronymEnumConfig {
    backend: AcronymBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(tag = "kind")]
enum InternalOverlapMode {
    Text {
        #[tier(non_empty)]
        value: String,
    },
    Count {
        #[tier(min = 1)]
        value: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct InternalOverlapConfig {
    mode: InternalOverlapMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(tag = "kind")]
enum InternalAliasConflictMode {
    A {
        #[serde(alias = "legacy")]
        first: String,
    },
    B {
        #[serde(alias = "legacy")]
        second: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct InternalAliasConflictConfig {
    mode: InternalAliasConflictMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
#[serde(tag = "kind")]
enum InternalEnvConflictMode {
    A {
        #[tier(env = "APP_VALUE")]
        first: String,
    },
    B {
        #[tier(env = "APP_VALUE")]
        second: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig, PartialEq, Eq)]
struct InternalEnvConflictConfig {
    mode: InternalEnvConflictMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedCollectionConfig {
    users: Vec<DerivedCollectionUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedCollectionUser {
    name: String,
    password: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedCollectionValidatedConfig {
    users: Vec<DerivedCollectionValidatedUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DerivedCollectionValidatedUser {
    #[tier(non_empty)]
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct SecretMergeConfig {
    #[tier(merge = "replace")]
    token: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RootMetadataInner {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct RootMetadataOuter {
    #[tier(env = "OUTER_VALUE", doc = "outer doc", example = "outer-example")]
    inner: RootMetadataInner,
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

impl Default for InternalEnumConfig {
    fn default() -> Self {
        Self {
            cache: InternalCacheLayer::Memory { max_items: 64 },
        }
    }
}

impl Default for UntaggedEnumConfig {
    fn default() -> Self {
        Self {
            endpoint: UntaggedEndpoint::Tcp { port: 7000 },
        }
    }
}

impl Default for AcronymEnumConfig {
    fn default() -> Self {
        Self {
            backend: AcronymBackend::HTTPServer { bind_port: 8080 },
        }
    }
}

impl Default for InternalOverlapConfig {
    fn default() -> Self {
        Self {
            mode: InternalOverlapMode::Text {
                value: "hello".to_owned(),
            },
        }
    }
}

impl Default for InternalAliasConflictConfig {
    fn default() -> Self {
        Self {
            mode: InternalAliasConflictMode::A {
                first: "hello".to_owned(),
            },
        }
    }
}

impl Default for InternalEnvConflictConfig {
    fn default() -> Self {
        Self {
            mode: InternalEnvConflictMode::A {
                first: "hello".to_owned(),
            },
        }
    }
}

impl Default for DerivedCollectionConfig {
    fn default() -> Self {
        Self {
            users: vec![DerivedCollectionUser {
                name: "alice".to_owned(),
                password: Secret::new("derived-collection-secret".to_owned()),
            }],
        }
    }
}

impl Default for DerivedCollectionValidatedConfig {
    fn default() -> Self {
        Self {
            users: vec![DerivedCollectionValidatedUser {
                name: String::new(),
            }],
        }
    }
}

impl Default for SecretMergeConfig {
    fn default() -> Self {
        Self {
            token: Secret::new("seed".to_owned()),
        }
    }
}

impl TierMetadata for RootMetadataInner {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("")
            .env("INNER_VALUE")
            .doc("inner doc")
            .example("inner-example")
            .secret()])
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
fn derive_metadata_preserves_explicit_merge_on_secret_fields() {
    let metadata = SecretMergeConfig::metadata();
    let token = metadata.field("token").expect("token metadata");

    assert!(token.secret);
    assert_eq!(token.merge, MergeStrategy::Replace);
}

#[test]
fn field_level_metadata_overrides_type_level_root_metadata() {
    let metadata = RootMetadataOuter::metadata();
    let inner = metadata.field("inner").expect("inner metadata");

    assert!(inner.secret);
    assert_eq!(inner.env.as_deref(), Some("OUTER_VALUE"));
    assert_eq!(inner.doc.as_deref(), Some("outer doc"));
    assert_eq!(inner.example.as_deref(), Some("outer-example"));
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
fn derive_metadata_can_configure_env_decoders() {
    let loaded = ConfigLoader::new(DerivedEnvDecodeConfig::default())
        .derive_metadata()
        .env(EnvSource::from_pairs([("APP_PORTS", "80,443")]))
        .load()
        .expect("config loads");

    assert_eq!(loaded.ports, vec![80, 443]);
}

#[test]
fn leaf_enums_do_not_require_enum_level_tier_metadata() {
    let metadata = LeafEnumConfig::metadata();
    let backend = metadata.field("backend").expect("backend metadata");
    assert_eq!(backend.env.as_deref(), Some("APP_BACKEND"));
    assert_eq!(backend.doc.as_deref(), Some("Selected backend mode"));

    let loaded = ConfigLoader::new(LeafEnumConfig {
        backend: LeafBackend::Memory,
    })
    .derive_metadata()
    .env(EnvSource::from_pairs([("APP_BACKEND", "Redis")]))
    .load()
    .expect("config loads");

    assert_eq!(loaded.backend, LeafBackend::Redis);
}

#[test]
fn derive_metadata_supports_collection_item_paths_and_redaction() {
    let metadata = DerivedCollectionConfig::metadata();
    let password = metadata
        .field("users.0.password")
        .expect("users.0.password metadata");
    assert!(password.secret);

    let loaded = ConfigLoader::new(DerivedCollectionConfig::default())
        .derive_metadata()
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("derived-collection-secret"));

    let explanation = loaded
        .report()
        .explain("users.0.password")
        .expect("password explanation");
    assert!(explanation.redacted);
}

#[test]
fn metadata_field_prefers_more_specific_matches() {
    let metadata = tier::ConfigMetadata::from_fields([
        tier::FieldMetadata::new("headers.*").doc("generic"),
        tier::FieldMetadata::new("headers.service").doc("specific"),
    ]);

    let field = metadata
        .field("headers.service")
        .expect("headers.service metadata");
    assert_eq!(field.doc.as_deref(), Some("specific"));
}

#[test]
fn metadata_field_prefers_earlier_specific_segments_when_patterns_tie() {
    let metadata = tier::ConfigMetadata::from_fields([
        tier::FieldMetadata::new("users.*.password").doc("leaf specific"),
        tier::FieldMetadata::new("users.admin.*").doc("prefix specific"),
    ]);

    let field = metadata
        .field("users.admin.password")
        .expect("users.admin.password metadata");
    assert_eq!(field.doc.as_deref(), Some("prefix specific"));
}

#[test]
fn merge_strategy_prefers_earlier_specific_segments_when_patterns_tie() {
    let metadata = tier::ConfigMetadata::from_fields([
        tier::FieldMetadata::new("users.*.password").merge_strategy(tier::MergeStrategy::Append),
        tier::FieldMetadata::new("users.admin.*").merge_strategy(tier::MergeStrategy::Replace),
    ]);

    assert_eq!(
        metadata.merge_strategy_for("users.admin.password"),
        Some(tier::MergeStrategy::Replace)
    );
}

#[test]
fn derive_metadata_runs_declared_validations_for_collection_items() {
    let error = ConfigLoader::new(DerivedCollectionValidatedConfig::default())
        .derive_metadata()
        .load()
        .expect_err("declared validation must run for collection items");

    let tier::ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert!(errors.iter().any(|error| error.path == "users.0.name"));
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

    let service_slug = metadata
        .field("service_slug")
        .expect("service_slug metadata");
    assert_eq!(
        service_slug.validations,
        vec![ValidationRule::Pattern("^[a-z0-9-]+$".to_owned())]
    );

    let ports = metadata.field("ports").expect("ports metadata");
    assert_eq!(
        ports.validations,
        vec![ValidationRule::MinItems(1), ValidationRule::MaxItems(4)]
    );

    let labels = metadata.field("labels").expect("labels metadata");
    assert_eq!(
        labels.validations,
        vec![
            ValidationRule::MinProperties(1),
            ValidationRule::MaxProperties(3),
        ]
    );

    let worker_count = metadata
        .field("worker_count")
        .expect("worker_count metadata");
    assert_eq!(
        worker_count.validations,
        vec![ValidationRule::MultipleOf(4u8.into())]
    );

    let tags = metadata.field("tags").expect("tags metadata");
    assert_eq!(tags.validations, vec![ValidationRule::UniqueItems]);

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

    let service_url = metadata.field("service_url").expect("service_url metadata");
    assert_eq!(service_url.validations, vec![ValidationRule::Url]);

    let contact_email = metadata
        .field("contact_email")
        .expect("contact_email metadata");
    assert_eq!(contact_email.validations, vec![ValidationRule::Email]);

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
fn derive_metadata_supports_checked_path_container_validations() {
    let metadata = DerivedExprCrossValidationConfig::metadata();

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
fn checked_path_container_validations_enforce_runtime_behavior() {
    let error = ConfigLoader::new(DerivedExprCrossValidationConfig {
        port: None,
        unix_socket: None,
        tls: DerivedTlsValidationConfig {
            enabled: true,
            cert: None,
            key: None,
        },
    })
    .derive_metadata()
    .load()
    .expect_err("missing checked-path requirements should fail");

    let ConfigError::DeclaredValidation { errors } = error else {
        panic!("expected declared validation error");
    };

    assert!(
        errors
            .iter()
            .any(|error| error.rule.as_deref() == Some("at_least_one_of"))
    );
    assert!(errors.iter().any(|error| {
        error.rule.as_deref() == Some("required_if")
            && error.path.is_empty()
            && error.related_paths
                == vec![
                    "tls.enabled".to_owned(),
                    "tls.cert".to_owned(),
                    "tls.key".to_owned(),
                ]
    }));
}

#[test]
fn derive_metadata_supports_source_policies() {
    let metadata = DerivedSourcePolicyConfig::metadata();
    let token = metadata.field("token").expect("token metadata");

    assert_eq!(
        token.allowed_sources.as_ref().expect("source policy"),
        &std::collections::BTreeSet::from([SourceKind::Environment, SourceKind::Arguments,])
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
fn derive_metadata_supports_internal_tagged_enums() {
    let metadata = InternalEnumConfig::metadata();

    assert!(metadata.field("cache.kind").is_some());
    assert!(metadata.field("cache.max_items").is_some());
    assert!(metadata.field("cache.path_root").is_some());

    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"cache.kind="memory""#,
        "--set",
        "cache.max_items=128",
    ]);
    let loaded = ConfigLoader::new(InternalEnumConfig::default())
        .derive_metadata()
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(loaded.cache, InternalCacheLayer::Memory { max_items: 128 });
}

#[test]
fn derive_metadata_supports_untagged_enums() {
    let metadata = UntaggedEnumConfig::metadata();

    assert!(metadata.field("endpoint.port").is_some());
    assert!(metadata.field("endpoint.path").is_some());

    let args = ArgsSource::from_args(["tier", "--set", "endpoint.port=7001"]);
    let loaded = ConfigLoader::new(UntaggedEnumConfig::default())
        .derive_metadata()
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(loaded.endpoint, UntaggedEndpoint::Tcp { port: 7001 });
}

#[test]
fn derive_metadata_matches_serde_variant_rename_rules_for_acronyms() {
    let serialized = serde_json::to_value(AcronymEnumConfig::default()).expect("serialize config");
    let variant = serialized
        .get("backend")
        .and_then(serde_json::Value::as_object)
        .and_then(|object| object.keys().next())
        .cloned()
        .expect("variant key");

    assert_eq!(variant, "h_t_t_p_server");

    let metadata = AcronymEnumConfig::metadata();
    assert!(metadata.field("backend.h_t_t_p_server.bind_port").is_some());
}

#[test]
fn internal_tagged_enums_keep_variant_specific_validation_rules_isolated() {
    let metadata = InternalOverlapConfig::metadata();
    assert!(metadata.field("mode.kind").is_some());
    assert!(metadata.field("mode.value").is_none());

    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"mode.kind="Text""#,
        "--set",
        r#"mode.value="hello""#,
    ]);
    let loaded = ConfigLoader::new(InternalOverlapConfig::default())
        .derive_metadata()
        .args(args)
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.mode,
        InternalOverlapMode::Text {
            value: "hello".to_owned(),
        }
    );
}

#[test]
fn internal_tagged_enum_conflicting_aliases_are_dropped_instead_of_erroring() {
    let metadata = InternalAliasConflictConfig::metadata();
    assert!(metadata.field("mode.first").is_some());
    assert!(metadata.field("mode.second").is_some());
    assert!(metadata.field("mode.legacy").is_none());

    let loaded = ConfigLoader::new(InternalAliasConflictConfig::default())
        .derive_metadata()
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.mode,
        InternalAliasConflictMode::A {
            first: "hello".to_owned(),
        }
    );
}

#[test]
fn internal_tagged_enum_conflicting_env_names_are_dropped_instead_of_erroring() {
    let metadata = InternalEnvConflictConfig::metadata();
    let first = metadata.field("mode.first").expect("mode.first metadata");
    let second = metadata.field("mode.second").expect("mode.second metadata");
    assert!(first.env.is_none());
    assert!(second.env.is_none());

    let loaded = ConfigLoader::new(InternalEnvConflictConfig::default())
        .derive_metadata()
        .env(EnvSource::from_pairs([("APP_VALUE", "\"override\"")]).prefix("TIER"))
        .load()
        .expect("config loads");

    assert_eq!(
        loaded.mode,
        InternalEnvConflictMode::A {
            first: "hello".to_owned(),
        }
    );
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
