#![cfg(feature = "schema")]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "toml")]
use tier::config_example_toml;
use tier::{
    ConfigLoader, ConfigMetadata, EnvDocOptions, FieldMetadata, MergeStrategy, Secret,
    TierMetadata, ValidationRule, annotated_json_schema_for, config_example_for, env_docs_for,
    env_docs_json, env_docs_markdown, env_docs_report_json, json_schema_for,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SchemaConfig {
    server: SchemaServer,
    secrets: SchemaSecrets,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SchemaServer {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SchemaSecrets {
    password: Secret<String>,
}

impl TierMetadata for SchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("server.host")
                .alias("server.hostname")
                .env("APP_SERVER_HOSTNAME")
                .doc("Address exposed by the service")
                .example("0.0.0.0")
                .non_empty()
                .min_length(3)
                .defaulted(),
            FieldMetadata::new("server.port")
                .example("8080")
                .deprecated("use server.bind_port instead")
                .merge_strategy(MergeStrategy::Replace)
                .min(1)
                .max(65_535),
            FieldMetadata::new("secrets.password").secret(),
        ])
        .required_if("server.port", 8080, ["server.host"])
    }
}

#[test]
fn exports_json_schema() {
    let schema = json_schema_for::<SchemaConfig>();
    let rendered = serde_json::to_string(&schema).expect("schema json");

    assert_eq!(schema["type"].as_str(), Some("object"));
    assert!(schema["properties"]["server"].is_object());
    assert!(rendered.contains("\"writeOnly\":true"));
    assert!(rendered.contains("\"x-tier-secret\":true"));
}

#[test]
fn annotated_schema_includes_tier_metadata_extensions() {
    let schema = annotated_json_schema_for::<SchemaConfig>();
    let rendered = serde_json::to_string(&schema).expect("annotated schema json");

    assert!(rendered.contains("\"x-tier-env\":\"APP_SERVER_HOSTNAME\""));
    assert!(rendered.contains("\"x-tier-aliases\":[\"server.hostname\"]"));
    assert!(rendered.contains("\"x-tier-has-default\":true"));
    assert!(rendered.contains("\"x-tier-merge\":\"replace\""));
    assert!(rendered.contains("\"x-tier-validate\""));
    assert!(rendered.contains("\"x-tier-checks\""));
    assert!(rendered.contains("\"x-tier-deprecated-note\":\"use server.bind_port instead\""));
}

#[test]
fn discovers_secret_paths_from_schema() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AutoSecretConfig {
        db: AutoSecretDb,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AutoSecretDb {
        password: Secret<String>,
    }

    impl Default for AutoSecretConfig {
        fn default() -> Self {
            Self {
                db: AutoSecretDb {
                    password: Secret::new("default-secret".to_owned()),
                },
            }
        }
    }

    let loaded = ConfigLoader::new(AutoSecretConfig::default())
        .discover_secret_paths_from_schema()
        .load()
        .expect("config loads");

    let rendered = loaded.report().redacted_pretty_json();
    assert!(rendered.contains("***redacted***"));
    assert!(!rendered.contains("default-secret"));
}

#[test]
fn generates_environment_docs_from_schema() {
    let docs = env_docs_for::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| entry.env == "APP_SERVER_HOSTNAME"
        && entry.description.as_deref() == Some("Address exposed by the service")
        && entry.example.as_deref() == Some("0.0.0.0")));
    assert!(docs.iter().any(|entry| {
        entry.path == "server.host"
            && entry.aliases == vec!["server.hostname".to_owned()]
            && entry.validations == vec![ValidationRule::NonEmpty, ValidationRule::MinLength(3)]
            && entry.has_default
    }));
    assert!(docs.iter().any(|entry| entry.env == "APP__SERVER__PORT"));
    assert!(
        docs.iter()
            .any(|entry| entry.env == "APP__SECRETS__PASSWORD" && entry.secret)
    );
    assert!(
        docs.iter()
            .any(|entry| { entry.path == "server.port" && entry.merge == MergeStrategy::Replace })
    );
    assert!(docs.iter().any(|entry| {
        entry.path == "server.port"
            && entry.deprecated.as_deref() == Some("use server.bind_port instead")
    }));

    let markdown = env_docs_markdown::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(markdown.contains("APP_SERVER_HOSTNAME"));
    assert!(markdown.contains("APP__SECRETS__PASSWORD"));
    assert!(markdown.contains("use server.bind_port instead"));
    assert!(markdown.contains("0.0.0.0"));
    assert!(markdown.contains("server.hostname"));
    assert!(markdown.contains("replace"));
    assert!(markdown.contains("non_empty"));
    assert!(markdown.contains("min=1"));

    let docs_json = env_docs_json::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let docs_array = docs_json.as_array().expect("env docs json array");
    assert!(docs_array.iter().any(|entry| {
        entry["path"].as_str() == Some("server.host")
            && entry["env"].as_str() == Some("APP_SERVER_HOSTNAME")
            && entry["has_default"].as_bool() == Some(true)
            && entry["validations"].as_array().map(Vec::len) == Some(2)
    }));

    let docs_report = env_docs_report_json::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert_eq!(docs_report["format_version"].as_u64(), Some(1));
    assert_eq!(
        docs_report["entries"].as_array().map(Vec::len),
        Some(docs.len())
    );
}

#[test]
fn generates_example_configuration_from_schema() {
    let example = config_example_for::<SchemaConfig>();

    assert_eq!(example["server"]["host"].as_str(), Some("0.0.0.0"));
    assert_eq!(example["server"]["port"].as_i64(), Some(8080));
    assert_eq!(example["secrets"]["password"].as_str(), Some("<secret>"));
}

#[cfg(feature = "toml")]
#[test]
fn generates_commented_toml_example_configuration() {
    let example = config_example_toml::<SchemaConfig>();

    assert!(example.contains("[server]"));
    assert!(example.contains("host = \"0.0.0.0\""));
    assert!(example.contains("# env: APP_SERVER_HOSTNAME"));
    assert!(example.contains("# aliases: server.hostname"));
    assert!(example.contains("# default: provided by serde"));
    assert!(example.contains("# validate: non_empty, min_length=3"));
    assert!(example.contains("# validate: required_if(server.port == 8080 -> server.host)"));
    assert!(example.contains("# merge: replace"));
    assert!(example.contains("# validate: min=1, max=65535"));
    assert!(example.contains("# deprecated: use server.bind_port instead"));
    assert!(example.contains("[secrets]"));
    assert!(example.contains("password = \"<secret>\""));
    assert!(example.contains("# secret: true"));
}
