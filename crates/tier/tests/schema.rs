#![cfg(feature = "schema")]

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(feature = "toml")]
use tier::config_example_toml;
use tier::{
    ConfigLoader, ConfigMetadata, ENV_DOCS_FORMAT_VERSION, EnvDocOptions, FieldMetadata,
    MergeStrategy, SCHEMA_EXPORT_FORMAT_VERSION, Secret, SourceKind, TierMetadata, ValidationLevel,
    ValidationRule, annotated_json_schema_for, annotated_json_schema_report_json,
    config_example_for, config_example_report_json, env_docs_for, env_docs_json, env_docs_markdown,
    env_docs_report_json, json_schema_for, json_schema_report_json,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SchemaConfig {
    server: SchemaServer,
    secrets: SchemaSecrets,
    website: String,
    contact_email: String,
    worker_count: u16,
    tags: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct ArraySchemaConfig {
    users: Vec<ArraySchemaUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct ArraySchemaUser {
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct OptionalNestedSchemaConfig {
    db: Option<OptionalNestedDb>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct OptionalNestedDb {
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct EnumSchemaConfig {
    mode: Option<EnumSchemaMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct ReusedRefSchemaConfig {
    primary: ReusedRefDb,
    replica: ReusedRefDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct ReusedRefDb {
    password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct MapExampleSchemaConfig {
    services: BTreeMap<String, MapExampleService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct MapExampleService {
    host: String,
    token: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct LiteralPlaceholderKeySchemaConfig {
    settings: LiteralPlaceholderKeySettings,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct LiteralPlaceholderKeySettings {
    #[serde(rename = "{item}")]
    item_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct WildcardTemplateMetadataSchemaConfig {
    services: BTreeMap<String, WildcardTemplateMetadataService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct WildcardTemplateMetadataService {
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CollidingMapPlaceholderSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchCollidingMapPlaceholderSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SecretExampleSchemaConfig {
    token: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SecretSchemaProvidedExampleConfig {
    token: Secret<SchemaProvidedSecretValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct TupleSchemaConfig {
    pair: (String, u16),
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct RootTupleSchemaConfig(String, u16);

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct PrimitiveArraySchemaConfig {
    ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BooleanContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsWithItemsFalseSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsWithAdditionalItemsFalseSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HomogeneousItemsWithAdditionalItemsFalseSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PropertyNamesContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UniqueContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternPropertiesContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UniqueFixedArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UniqueShortStringArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UniqueFixedAndContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsSatisfiedAfterUniqueRepairSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CascadingUniqueArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternPropertiesDynamicMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternPropertiesPropertyNamesDynamicMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternPropertiesMinPropertiesDynamicMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PropertyNamesFalseAdditionalPropertiesSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PropertyNamesFalsePatternPropertiesSchemaConfig;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct NestedPrimitiveArraySchemaConfig {
    matrix: Vec<Vec<u16>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct IndexedPrimitiveArraySchemaConfig {
    ports: Vec<u16>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct IndexedArrayTableCommentSchemaConfig {
    users: Vec<IndexedArrayTableCommentUser>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct IndexedArrayTableCommentUser {
    name: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SpecificArrayTableFieldCommentSchemaConfig {
    users: Vec<SpecificArrayTableFieldCommentUser>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SpecificArrayTableFieldCommentUser {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllOfLocalSchemaConfig {
    enabled: bool,
    server: AllOfLocalServer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllOfLocalServer {
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OneOfLocalSchemaConfig {
    enabled: bool,
    server: OneOfLocalServer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OneOfLocalServer {
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefSiblingSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefSiblingTupleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PropertyAndMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MinPropertiesDynamicMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MinPropertiesSatisfiedByFixedPropertySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImplicitAdditionalPropertiesMinPropertiesSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PropertyNamesEnumDynamicMapSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllOfRequiredUnionSchemaConfig;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MixedArrayTomlSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OneOfUnionTypeSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OneOfDescribedUnionSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AllOfArrayExampleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NestedAllOfArrayExampleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TupleWithAdditionalItemsSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TupleWithoutAdditionalItemsSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartiallyRequiredTupleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TupleWithRequiredAdditionalItemsSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyAdditionalItemsSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainsSatisfiedByFixedTupleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConstrainedContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatternContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MultipleOfContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LargeIntegerExampleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FractionalMultipleOfIntegerExampleSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdditionalPropertiesFalseContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MinPropertiesContainsArraySchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverlaidFixedItemsContainsSchemaConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EnumSchemaMode {
    Tcp { port: u16 },
    Unix { path: String },
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
                .pattern("^[a-z0-9.-]+$")
                .min_length(3)
                .defaulted(),
            FieldMetadata::new("server.port")
                .example("8080")
                .deprecated("use server.bind_port instead")
                .merge_strategy(MergeStrategy::Replace)
                .min(1)
                .max(65_535),
            FieldMetadata::new("secrets.password").secret(),
            FieldMetadata::new("website")
                .doc("Public service URL")
                .example("https://api.example.com")
                .allow_sources([SourceKind::Environment, SourceKind::Arguments])
                .deny_sources([SourceKind::File])
                .validation_level("url", ValidationLevel::Warning)
                .validation_message("url", "website should be a valid public URL")
                .validation_tags("url", ["network", "public"])
                .url(),
            FieldMetadata::new("contact_email")
                .doc("Operations contact address")
                .example("ops@example.com")
                .email(),
            FieldMetadata::new("worker_count").multiple_of(4),
            FieldMetadata::new("tags").unique_items(),
        ])
        .required_if("server.port", 8080, ["server.host"])
    }
}

impl TierMetadata for ArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("users.*.password")
            .secret()
            .doc("Password for each user")
            .non_empty()])
    }
}

impl TierMetadata for OptionalNestedSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("db.password")
            .secret()
            .doc("Optional database password")])
    }
}

impl TierMetadata for EnumSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("mode.port").doc("TCP port"),
            FieldMetadata::new("mode.path").doc("Unix socket path"),
        ])
    }
}

impl TierMetadata for ReusedRefSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("primary.password")
                .doc("Primary database password")
                .env("APP_PRIMARY_PASSWORD")
                .example("primary-secret")
                .secret(),
            FieldMetadata::new("replica.password")
                .doc("Replica database password")
                .env("APP_REPLICA_PASSWORD")
                .example("replica-secret")
                .secret(),
        ])
    }
}

impl TierMetadata for SecretExampleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("token")
            .doc("Secret token")
            .example("raw-secret-example")])
    }
}

impl TierMetadata for MapExampleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("services.*.host")
                .doc("Service host")
                .example("api.internal"),
            FieldMetadata::new("services.*.token")
                .secret()
                .example("map-secret"),
        ])
    }
}

impl TierMetadata for LiteralPlaceholderKeySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("settings.{item}")
            .doc("Literal placeholder-shaped key")
            .example("literal-value")])
    }
}

impl TierMetadata for WildcardTemplateMetadataSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("services.*.*")
                .doc("Any service field")
                .deprecated("generic service field"),
            FieldMetadata::new("services.*.token")
                .secret()
                .example("template-secret"),
        ])
    }
}

impl TierMetadata for SecretSchemaProvidedExampleConfig {}
impl TierMetadata for TupleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("pair.0")
                .doc("Primary host")
                .example("edge"),
            FieldMetadata::new("pair.1")
                .doc("Primary port")
                .example("8080")
                .min(1)
                .max(65_535),
        ])
    }
}
impl TierMetadata for RootTupleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("0").doc("Primary host").example("edge"),
            FieldMetadata::new("1")
                .doc("Primary port")
                .example("8080")
                .min(1)
                .max(65_535),
        ])
    }
}
impl TierMetadata for PrimitiveArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Allowed port")
            .example("8080")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for ContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "contains": {
                        "type": "integer",
                        "example": 8080
                    },
                    "minContains": 2
                }
            },
            "required": ["ports"]
        }))
        .expect("valid contains array schema")
    }
}

impl TierMetadata for ContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Required matching port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for BooleanContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("BooleanContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::BooleanContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "contains": true,
                    "minContains": 3,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid boolean contains array schema")
    }
}

impl TierMetadata for BooleanContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*").doc("Any matching value")])
    }
}

impl JsonSchema for ContainsWithItemsFalseSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsWithItemsFalseSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ContainsWithItemsFalseSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 0
                        }
                    ],
                    "items": false,
                    "contains": {
                        "type": "integer",
                        "minimum": 1
                    },
                    "minContains": 1
                }
            },
            "required": ["ports"]
        }))
        .expect("valid items=false contains array schema")
    }
}

impl TierMetadata for ContainsWithItemsFalseSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*").doc("Forbidden extra port")])
    }
}

impl JsonSchema for ContainsWithAdditionalItemsFalseSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsWithAdditionalItemsFalseSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ContainsWithAdditionalItemsFalseSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "items": [
                        {
                            "type": "string",
                            "example": "edge"
                        }
                    ],
                    "additionalItems": false,
                    "contains": {
                        "type": "integer",
                        "example": 8080
                    },
                    "minContains": 1
                }
            },
            "required": ["pair"]
        }))
        .expect("valid additionalItems=false contains array schema")
    }
}

impl TierMetadata for ContainsWithAdditionalItemsFalseSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("pair.*").doc("Forbidden trailing item")])
    }
}

impl JsonSchema for HomogeneousItemsWithAdditionalItemsFalseSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("HomogeneousItemsWithAdditionalItemsFalseSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::HomogeneousItemsWithAdditionalItemsFalseSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "items": {
                        "type": "integer",
                        "example": 8080
                    },
                    "additionalItems": false,
                    "minItems": 2
                }
            },
            "required": ["ports"]
        }))
        .expect("valid homogeneous items schema with ignored additionalItems=false")
    }
}

impl TierMetadata for HomogeneousItemsWithAdditionalItemsFalseSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*").doc("Allowed port")])
    }
}

impl JsonSchema for ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed(
            "tier::tests::ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig",
        )
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "groups": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "array",
                            "items": {
                                "type": "integer",
                                "example": 8080
                            },
                            "additionalItems": false,
                            "minItems": 1
                        }
                    ],
                    "contains": {
                        "type": "array",
                        "items": {
                            "type": "integer",
                            "example": 8080
                        },
                        "additionalItems": false,
                        "minItems": 1
                    },
                    "minContains": 1
                }
            },
            "required": ["groups"]
        }))
        .expect("valid nested homogeneous additionalItems=false contains schema")
    }
}

impl TierMetadata for ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("groups.*").doc("Additional group")])
    }
}

impl JsonSchema for PropertyNamesContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PropertyNamesContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PropertyNamesContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "backends": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "object",
                            "example": {
                                "legacy": {
                                    "token": "stale"
                                }
                            }
                        }
                    ],
                    "contains": {
                        "type": "object",
                        "minProperties": 1,
                        "propertyNames": {
                            "enum": ["primary"]
                        },
                        "additionalProperties": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "fresh" }
                            },
                            "required": ["token"]
                        }
                    },
                    "minContains": 1
                }
            },
            "required": ["backends"]
        }))
        .expect("valid propertyNames contains array schema")
    }
}

impl TierMetadata for PropertyNamesContainsArraySchemaConfig {}

impl JsonSchema for UniqueContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("UniqueContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::UniqueContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 2_000,
                            "multipleOf": 2_000
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "example": 2_000,
                        "multipleOf": 2_000
                    },
                    "minContains": 2,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid unique contains array schema")
    }
}

impl TierMetadata for UniqueContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Unique required matching port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for PatternPropertiesContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PatternPropertiesContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PatternPropertiesContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "backends": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "object",
                            "example": {
                                "svc-primary": {
                                    "token": "fresh"
                                }
                            }
                        }
                    ],
                    "contains": {
                        "type": "object",
                        "minProperties": 1,
                        "patternProperties": {
                            "^svc-": {
                                "type": "object",
                                "properties": {
                                    "token": { "type": "string", "example": "fresh" }
                                },
                                "required": ["token"]
                            }
                        },
                        "additionalProperties": false
                    },
                    "minContains": 1
                }
            },
            "required": ["backends"]
        }))
        .expect("valid patternProperties contains array schema")
    }
}

impl TierMetadata for PatternPropertiesContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("backends.*").doc("Pattern matched backend item")
        ])
    }
}

impl JsonSchema for UniqueFixedArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("UniqueFixedArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::UniqueFixedArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 2_000,
                            "minimum": 0,
                            "maximum": 2_000,
                            "multipleOf": 2_000
                        },
                        {
                            "type": "integer",
                            "example": 2_000,
                            "minimum": 0,
                            "maximum": 2_000,
                            "multipleOf": 2_000
                        }
                    ],
                    "minItems": 2,
                    "maxItems": 2,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid unique fixed array schema")
    }
}

impl TierMetadata for UniqueFixedArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Unique fixed port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for UniqueShortStringArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("UniqueShortStringArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::UniqueShortStringArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "codes": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "a",
                            "minLength": 1,
                            "maxLength": 1
                        },
                        {
                            "type": "string",
                            "example": "a",
                            "minLength": 1,
                            "maxLength": 1
                        }
                    ],
                    "minItems": 2,
                    "maxItems": 2,
                    "uniqueItems": true
                }
            },
            "required": ["codes"]
        }))
        .expect("valid unique short string array schema")
    }
}

impl TierMetadata for UniqueShortStringArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for UniqueFixedAndContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("UniqueFixedAndContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::UniqueFixedAndContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 0,
                            "maximum": 0
                        },
                        {
                            "type": "integer",
                            "example": 0,
                            "maximum": 0
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "minimum": 1
                    },
                    "minContains": 1,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid unique fixed and contains array schema")
    }
}

impl TierMetadata for UniqueFixedAndContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for ContainsSatisfiedAfterUniqueRepairSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsSatisfiedAfterUniqueRepairSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ContainsSatisfiedAfterUniqueRepairSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "enum": [1]
                        },
                        {
                            "type": "integer",
                            "enum": [1, 2]
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "enum": [1, 2]
                    },
                    "minContains": 2,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid contains satisfied after unique repair schema")
    }
}

impl TierMetadata for ContainsSatisfiedAfterUniqueRepairSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*").doc("Additional port")])
    }
}

impl JsonSchema for CascadingUniqueArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CascadingUniqueArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::CascadingUniqueArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "enum": [1]
                        },
                        {
                            "type": "integer",
                            "enum": [1, 2]
                        },
                        {
                            "type": "integer",
                            "enum": [2, 3]
                        }
                    ],
                    "minItems": 3,
                    "maxItems": 3,
                    "uniqueItems": true
                }
            },
            "required": ["ports"]
        }))
        .expect("valid cascading unique array schema")
    }
}

impl TierMetadata for CascadingUniqueArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl TierMetadata for NestedPrimitiveArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("matrix.*.*")
            .doc("Allowed matrix value")
            .example("8080")
            .min(1)
            .max(65_535)])
    }
}
impl TierMetadata for IndexedPrimitiveArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("ports.10").doc("Tenth port"),
            FieldMetadata::new("ports.2").doc("Second port"),
        ])
    }
}
impl TierMetadata for IndexedArrayTableCommentSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("users.*").doc("Any user item"),
            FieldMetadata::new("users.0").doc("Primary user item"),
            FieldMetadata::new("users.*.name").doc("User name"),
        ])
    }
}
impl TierMetadata for SpecificArrayTableFieldCommentSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("users.*.name").doc("Any user name"),
            FieldMetadata::new("users.0.name").doc("Primary user name"),
        ])
    }
}
impl TierMetadata for AllOfLocalSchemaConfig {}
impl TierMetadata for OneOfLocalSchemaConfig {}
impl TierMetadata for RefSiblingSchemaConfig {}
impl TierMetadata for PropertyAndMapSchemaConfig {}
impl TierMetadata for MinPropertiesDynamicMapSchemaConfig {}
impl TierMetadata for MinPropertiesSatisfiedByFixedPropertySchemaConfig {}
impl TierMetadata for MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig {}
impl TierMetadata for MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig {}
impl TierMetadata for ImplicitAdditionalPropertiesMinPropertiesSchemaConfig {}
impl TierMetadata for ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig {}
impl TierMetadata for PropertyNamesEnumDynamicMapSchemaConfig {}
impl TierMetadata for AllOfRequiredUnionSchemaConfig {}
impl TierMetadata for MixedArrayTomlSchemaConfig {}
impl TierMetadata for OneOfUnionTypeSchemaConfig {}
impl TierMetadata for OneOfDescribedUnionSchemaConfig {}
impl TierMetadata for AllOfArrayExampleSchemaConfig {}
impl TierMetadata for NestedAllOfArrayExampleSchemaConfig {}
impl TierMetadata for TupleWithAdditionalItemsSchemaConfig {}
impl TierMetadata for CollidingMapPlaceholderSchemaConfig {}
impl TierMetadata for BranchCollidingMapPlaceholderSchemaConfig {}
impl TierMetadata for RefSiblingTupleSchemaConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaProvidedSecretValue(String);

impl JsonSchema for SchemaProvidedSecretValue {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("SchemaProvidedSecretValue")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::SchemaProvidedSecretValue")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "string",
            "example": "schema-secret-example"
        }))
        .expect("valid string schema")
    }
}

impl JsonSchema for AllOfLocalSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("AllOfLocalSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::AllOfLocalSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" }
            },
            "required": ["enabled"],
            "allOf": [
                {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer" }
                            },
                            "required": ["port"]
                        }
                    },
                    "required": ["server"]
                }
            ]
        }))
        .expect("valid composed schema")
    }
}

impl JsonSchema for OneOfLocalSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("OneOfLocalSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::OneOfLocalSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" }
            },
            "required": ["enabled"],
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer" }
                            },
                            "required": ["port"]
                        }
                    },
                    "required": ["server"]
                },
                {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer", "default": 8080 }
                            },
                            "required": ["port"]
                        }
                    },
                    "required": ["server"]
                }
            ]
        }))
        .expect("valid composed schema")
    }
}

impl JsonSchema for RefSiblingSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RefSiblingSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::RefSiblingSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "db": {
                    "$ref": "#/$defs/SharedDb",
                    "properties": {
                        "replica": { "type": "string", "default": "replica-a" }
                    },
                    "required": ["replica"]
                }
            },
            "required": ["db"],
            "$defs": {
                "SharedDb": {
                    "type": "object",
                    "properties": {
                        "password": { "type": "string", "default": "shared-secret" }
                    },
                    "required": ["password"]
                }
            }
        }))
        .expect("valid ref sibling schema")
    }
}

impl JsonSchema for RefSiblingTupleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("RefSiblingTupleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::RefSiblingTupleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "$ref": "#/$defs/SharedPair",
                    "prefixItems": [
                        null,
                        { "type": "integer", "default": 8080 }
                    ]
                }
            },
            "required": ["pair"],
            "$defs": {
                "SharedPair": {
                    "type": "array",
                    "prefixItems": [
                        { "type": "string", "default": "edge" }
                    ]
                }
            }
        }))
        .expect("valid ref sibling tuple schema")
    }
}

impl JsonSchema for PropertyAndMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PropertyAndMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PropertyAndMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "properties": {
                        "primary": {
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" }
                            },
                            "required": ["url"]
                        }
                    },
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string" }
                        },
                        "required": ["token"]
                    },
                    "required": ["primary"]
                }
            },
            "required": ["services"]
        }))
        .expect("valid property and map schema")
    }
}

impl JsonSchema for MinPropertiesDynamicMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MinPropertiesDynamicMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MinPropertiesDynamicMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 2,
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string" }
                        },
                        "required": ["token"]
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid minProperties dynamic map schema")
    }
}

impl JsonSchema for MinPropertiesSatisfiedByFixedPropertySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MinPropertiesSatisfiedByFixedPropertySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MinPropertiesSatisfiedByFixedPropertySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 1,
                    "properties": {
                        "primary": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string" }
                            },
                            "required": ["token"]
                        }
                    },
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string" }
                        },
                        "required": ["token"]
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid minProperties schema satisfied by fixed property")
    }
}

impl JsonSchema for MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "maxProperties": 1,
                    "properties": {
                        "primary": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string" }
                            },
                            "required": ["token"]
                        }
                    },
                    "required": ["primary"],
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string" }
                        },
                        "required": ["token"]
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid maxProperties schema with exhausted required fixed property")
    }
}

impl JsonSchema for MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "maxProperties": 1,
                    "properties": {
                        "primary": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "primary-token" }
                            },
                            "required": ["token"]
                        },
                        "secondary": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "secondary-token" }
                            },
                            "required": ["token"]
                        }
                    },
                    "required": ["primary"]
                }
            },
            "required": ["services"]
        }))
        .expect("valid maxProperties schema that trims optional fixed properties")
    }
}

impl JsonSchema for ImplicitAdditionalPropertiesMinPropertiesSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ImplicitAdditionalPropertiesMinPropertiesSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ImplicitAdditionalPropertiesMinPropertiesSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 2,
                    "properties": {
                        "primary": {
                            "type": "string",
                            "example": "primary-token"
                        }
                    },
                    "required": ["primary"]
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with implicit additionalProperties and minProperties")
    }
}

impl JsonSchema for ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed(
            "tier::tests::ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig",
        )
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 2,
                    "properties": {
                        "primary": {
                            "type": "string",
                            "example": "primary-token"
                        }
                    },
                    "required": ["primary"],
                    "additionalProperties": false
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with forbidden additionalProperties and oversized minProperties")
    }
}

impl JsonSchema for PropertyNamesEnumDynamicMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PropertyNamesEnumDynamicMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PropertyNamesEnumDynamicMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 2,
                    "propertyNames": {
                        "enum": ["primary", "secondary"]
                    },
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string", "example": "enum-token" }
                        },
                        "required": ["token"]
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with propertyNames-constrained dynamic map")
    }
}

impl JsonSchema for PatternPropertiesDynamicMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PatternPropertiesDynamicMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PatternPropertiesDynamicMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "patternProperties": {
                        "^svc-": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "pattern-token" }
                            },
                            "required": ["token"]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with patternProperties dynamic map")
    }
}

impl TierMetadata for PatternPropertiesDynamicMapSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("services.*.token").doc("Pattern service token")
        ])
    }
}

impl JsonSchema for PatternPropertiesMinPropertiesDynamicMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PatternPropertiesMinPropertiesDynamicMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PatternPropertiesMinPropertiesDynamicMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "minProperties": 2,
                    "patternProperties": {
                        "^svc-": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "pattern-token" }
                            },
                            "required": ["token"]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with minProperties-constrained patternProperties")
    }
}

impl TierMetadata for PatternPropertiesMinPropertiesDynamicMapSchemaConfig {}

impl JsonSchema for PatternPropertiesPropertyNamesDynamicMapSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PatternPropertiesPropertyNamesDynamicMapSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PatternPropertiesPropertyNamesDynamicMapSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "propertyNames": {
                        "enum": ["svc-primary"]
                    },
                    "patternProperties": {
                        "^svc-": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "pattern-token" }
                            },
                            "required": ["token"]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with patternProperties constrained by propertyNames")
    }
}

impl TierMetadata for PatternPropertiesPropertyNamesDynamicMapSchemaConfig {}

impl JsonSchema for PropertyNamesFalseAdditionalPropertiesSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PropertyNamesFalseAdditionalPropertiesSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PropertyNamesFalseAdditionalPropertiesSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "propertyNames": false,
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "token": { "type": "string", "example": "blocked-token" }
                        },
                        "required": ["token"]
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with propertyNames false and additionalProperties")
    }
}

impl TierMetadata for PropertyNamesFalseAdditionalPropertiesSchemaConfig {}

impl JsonSchema for PropertyNamesFalsePatternPropertiesSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PropertyNamesFalsePatternPropertiesSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PropertyNamesFalsePatternPropertiesSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "propertyNames": false,
                    "patternProperties": {
                        "^svc-": {
                            "type": "object",
                            "properties": {
                                "token": { "type": "string", "example": "blocked-token" }
                            },
                            "required": ["token"]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "required": ["services"]
        }))
        .expect("valid schema with propertyNames false and patternProperties")
    }
}

impl TierMetadata for PropertyNamesFalsePatternPropertiesSchemaConfig {}

impl JsonSchema for AllOfRequiredUnionSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("AllOfRequiredUnionSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::AllOfRequiredUnionSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "allOf": [
                {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer" }
                            }
                        }
                    }
                },
                {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "object",
                            "properties": {
                                "port": { "type": "integer" }
                            },
                            "required": ["port"]
                        }
                    },
                    "required": ["server"]
                }
            ]
        }))
        .expect("valid allOf required union schema")
    }
}

impl JsonSchema for MixedArrayTomlSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MixedArrayTomlSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MixedArrayTomlSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "backends": {
                    "type": "array",
                    "default": [
                        { "name": "api" },
                        "fallback"
                    ]
                }
            },
            "required": ["backends"]
        }))
        .expect("valid mixed array schema")
    }
}

impl JsonSchema for OneOfUnionTypeSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("OneOfUnionTypeSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::OneOfUnionTypeSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" }
                    }
                },
                {
                    "type": "object",
                    "properties": {
                        "value": { "type": "integer" }
                    }
                }
            ]
        }))
        .expect("valid oneOf union type schema")
    }
}

impl JsonSchema for OneOfDescribedUnionSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("OneOfDescribedUnionSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::OneOfDescribedUnionSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "value": {
                    "description": "Union value",
                    "oneOf": [
                        { "type": "string" },
                        { "type": "integer" }
                    ]
                }
            },
            "required": ["value"]
        }))
        .expect("valid described oneOf union type schema")
    }
}

impl JsonSchema for AllOfArrayExampleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("AllOfArrayExampleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::AllOfArrayExampleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "allOf": [
                        {
                            "type": "array",
                            "example": ["edge"]
                        },
                        {
                            "type": "array",
                            "example": [null, 8080]
                        }
                    ]
                }
            },
            "required": ["pair"]
        }))
        .expect("valid allOf array example schema")
    }
}

impl JsonSchema for NestedAllOfArrayExampleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("NestedAllOfArrayExampleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::NestedAllOfArrayExampleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "allOf": [
                        {
                            "type": "object",
                            "properties": {
                                "ports": {
                                    "type": "array",
                                    "example": ["edge"]
                                }
                            }
                        },
                        {
                            "type": "object",
                            "properties": {
                                "ports": {
                                    "type": "array",
                                    "example": [null, 8080]
                                }
                            }
                        }
                    ]
                }
            },
            "required": ["server"]
        }))
        .expect("valid nested allOf array example schema")
    }
}

impl JsonSchema for TupleWithAdditionalItemsSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TupleWithAdditionalItemsSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::TupleWithAdditionalItemsSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "edge"
                        }
                    ],
                    "items": {
                        "type": "integer",
                        "example": 8080
                    }
                }
            },
            "required": ["pair"]
        }))
        .expect("valid tuple schema with additional items")
    }
}

impl JsonSchema for TupleWithoutAdditionalItemsSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TupleWithoutAdditionalItemsSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::TupleWithoutAdditionalItemsSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "edge"
                        }
                    ],
                    "items": {
                        "type": "integer",
                        "example": 8080
                    },
                    "maxItems": 1
                }
            },
            "required": ["pair"]
        }))
        .expect("valid tuple schema without additional items")
    }
}

impl TierMetadata for TupleWithoutAdditionalItemsSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for PartiallyRequiredTupleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PartiallyRequiredTupleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PartiallyRequiredTupleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "edge"
                        },
                        {
                            "type": "integer",
                            "example": 8080
                        }
                    ],
                    "minItems": 1,
                    "maxItems": 2
                }
            },
            "required": ["pair"]
        }))
        .expect("valid partially required tuple schema")
    }
}

impl TierMetadata for PartiallyRequiredTupleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for TupleWithRequiredAdditionalItemsSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("TupleWithRequiredAdditionalItemsSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::TupleWithRequiredAdditionalItemsSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "edge"
                        }
                    ],
                    "items": {
                        "type": "integer",
                        "example": 8080
                    },
                    "minItems": 3,
                    "maxItems": 3
                }
            },
            "required": ["pair"]
        }))
        .expect("valid tuple schema with required additional items")
    }
}

impl TierMetadata for TupleWithRequiredAdditionalItemsSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for ContainsSatisfiedByFixedTupleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ContainsSatisfiedByFixedTupleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ContainsSatisfiedByFixedTupleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 8080
                        }
                    ],
                    "items": {
                        "type": "integer",
                        "example": 8080
                    },
                    "contains": {
                        "type": "integer"
                    },
                    "minContains": 1
                }
            },
            "required": ["pair"]
        }))
        .expect("valid contains tuple schema satisfied by fixed item")
    }
}

impl TierMetadata for ContainsSatisfiedByFixedTupleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("pair.*")
            .doc("Additional port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for ConstrainedContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ConstrainedContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::ConstrainedContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 0
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "minimum": 1
                    },
                    "minContains": 1
                }
            },
            "required": ["ports"]
        }))
        .expect("valid constrained contains array schema")
    }
}

impl TierMetadata for ConstrainedContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Positive port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for PatternContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PatternContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::PatternContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string",
                            "example": "dev-edge"
                        }
                    ],
                    "contains": {
                        "type": "string",
                        "pattern": "^prod-",
                        "example": "prod-edge"
                    },
                    "minContains": 1
                }
            },
            "required": ["ports"]
        }))
        .expect("valid pattern contains array schema")
    }
}

impl TierMetadata for PatternContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Production label")
            .min_length(5)])
    }
}

impl JsonSchema for MultipleOfContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MultipleOfContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MultipleOfContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "ports": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 1
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "minimum": 1,
                        "multipleOf": 2
                    },
                    "minContains": 1
                }
            },
            "required": ["ports"]
        }))
        .expect("valid multipleOf contains array schema")
    }
}

impl TierMetadata for MultipleOfContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("ports.*")
            .doc("Even port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for LargeIntegerExampleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("LargeIntegerExampleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::LargeIntegerExampleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "minimum": 9_007_199_254_740_993u64,
                    "multipleOf": 2
                },
                "sparse_multiple": {
                    "type": "integer",
                    "minimum": 1,
                    "multipleOf": 20_000
                }
            },
            "required": ["id", "sparse_multiple"]
        }))
        .expect("valid large integer schema")
    }
}

impl TierMetadata for LargeIntegerExampleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for FractionalMultipleOfIntegerExampleSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("FractionalMultipleOfIntegerExampleSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::FractionalMultipleOfIntegerExampleSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "batch_size": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50_000,
                    "multipleOf": 20_000.5
                }
            },
            "required": ["batch_size"]
        }))
        .expect("valid fractional multipleOf integer schema")
    }
}

impl TierMetadata for FractionalMultipleOfIntegerExampleSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::new()
    }
}

impl JsonSchema for AdditionalPropertiesFalseContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("AdditionalPropertiesFalseContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::AdditionalPropertiesFalseContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "backends": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "object",
                            "properties": {
                                "kind": {
                                    "type": "string",
                                    "example": "prod"
                                },
                                "extra": {
                                    "type": "boolean",
                                    "example": true
                                }
                            },
                            "required": ["kind", "extra"]
                        }
                    ],
                    "contains": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "const": "prod"
                            }
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    },
                    "minContains": 1
                }
            },
            "required": ["backends"]
        }))
        .expect("valid additionalProperties=false contains array schema")
    }
}

impl TierMetadata for AdditionalPropertiesFalseContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("backends.*").doc("Strict backend object")])
    }
}

impl JsonSchema for MinPropertiesContainsArraySchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("MinPropertiesContainsArraySchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::MinPropertiesContainsArraySchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "backends": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "object"
                        }
                    ],
                    "contains": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "example": "prod"
                            }
                        },
                        "minProperties": 1,
                        "additionalProperties": false
                    },
                    "minContains": 1
                }
            },
            "required": ["backends"]
        }))
        .expect("valid minProperties contains array schema")
    }
}

impl TierMetadata for MinPropertiesContainsArraySchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([
            FieldMetadata::new("backends.*").doc("Non-empty backend object")
        ])
    }
}

impl JsonSchema for OverlaidFixedItemsContainsSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("OverlaidFixedItemsContainsSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::OverlaidFixedItemsContainsSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "integer",
                            "example": 8080
                        }
                    ],
                    "items": [
                        {
                            "type": "integer",
                            "example": 8080
                        }
                    ],
                    "contains": {
                        "type": "integer",
                        "minimum": 1
                    },
                    "minContains": 2
                }
            },
            "required": ["pair"]
        }))
        .expect("valid overlaid fixed items contains schema")
    }
}

impl TierMetadata for OverlaidFixedItemsContainsSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("pair.*")
            .doc("Additional required port")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for LegacyAdditionalItemsSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("LegacyAdditionalItemsSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::LegacyAdditionalItemsSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "pair": {
                    "type": "array",
                    "items": [
                        {
                            "type": "string",
                            "example": "edge"
                        }
                    ],
                    "additionalItems": {
                        "type": "integer",
                        "example": 8080
                    }
                }
            },
            "required": ["pair"]
        }))
        .expect("valid legacy additionalItems tuple schema")
    }
}

impl TierMetadata for LegacyAdditionalItemsSchemaConfig {
    fn metadata() -> ConfigMetadata {
        ConfigMetadata::from_fields([FieldMetadata::new("pair.*")
            .doc("Trailing item")
            .min(1)
            .max(65_535)])
    }
}

impl JsonSchema for CollidingMapPlaceholderSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CollidingMapPlaceholderSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::CollidingMapPlaceholderSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "properties": {
                        "{item}": {
                            "type": "object",
                            "properties": {
                                "url": {
                                    "type": "string",
                                    "example": "literal.internal"
                                }
                            }
                        }
                    },
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "example": "dynamic.internal"
                            }
                        }
                    }
                }
            },
            "required": ["services"]
        }))
        .expect("valid colliding map placeholder schema")
    }
}

impl JsonSchema for BranchCollidingMapPlaceholderSchemaConfig {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("BranchCollidingMapPlaceholderSchemaConfig")
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Borrowed("tier::tests::BranchCollidingMapPlaceholderSchemaConfig")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "allOf": [
                        {
                            "type": "object",
                            "properties": {
                                "{item}": {
                                    "type": "object",
                                    "properties": {
                                        "url": {
                                            "type": "string",
                                            "example": "literal.internal"
                                        }
                                    }
                                }
                            }
                        },
                        {
                            "type": "object",
                            "additionalProperties": {
                                "type": "object",
                                "properties": {
                                    "url": {
                                        "type": "string",
                                        "example": "dynamic.internal"
                                    }
                                }
                            }
                        }
                    ]
                }
            },
            "required": ["services"]
        }))
        .expect("valid branch colliding map placeholder schema")
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
    let tag_rules = schema["properties"]["tags"]["x-tier-validate"]
        .as_array()
        .expect("tags validation rules");
    let website_rules = schema["properties"]["website"]["x-tier-validate"]
        .as_array()
        .expect("website validation rules");
    let contact_email_rules = schema["properties"]["contact_email"]["x-tier-validate"]
        .as_array()
        .expect("contact_email validation rules");
    let worker_count_rules = schema["properties"]["worker_count"]["x-tier-validate"]
        .as_array()
        .expect("worker_count validation rules");

    assert!(rendered.contains("\"x-tier-env\":\"APP_SERVER_HOSTNAME\""));
    assert!(rendered.contains("\"x-tier-aliases\":[\"server.hostname\"]"));
    assert!(rendered.contains("\"x-tier-has-default\":true"));
    assert!(rendered.contains("\"x-tier-merge\":\"replace\""));
    assert!(rendered.contains("\"x-tier-validate\""));
    assert!(rendered.contains("^[a-z0-9.-]+$"));
    assert!(
        tag_rules
            .iter()
            .any(|rule| rule.as_str() == Some("UniqueItems"))
    );
    assert!(
        website_rules
            .iter()
            .any(|rule| rule.as_str() == Some("Url"))
    );
    assert_eq!(
        schema["properties"]["website"]["x-tier-sources"],
        serde_json::json!(["env", "cli"])
    );
    assert_eq!(
        schema["properties"]["website"]["x-tier-denied-sources"],
        serde_json::json!(["file"])
    );
    assert_eq!(
        schema["properties"]["website"]["x-tier-validation-config"]["url"],
        serde_json::json!({
            "level": "warning",
            "message": "website should be a valid public URL",
            "tags": ["network", "public"]
        })
    );
    assert!(
        contact_email_rules
            .iter()
            .any(|rule| rule.as_str() == Some("Email"))
    );
    assert!(worker_count_rules.iter().any(|rule| {
        rule["MultipleOf"]["Finite"].as_u64() == Some(4) || rule["MultipleOf"].as_u64() == Some(4)
    }));
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
fn discovers_secret_paths_from_reused_schema_refs() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct MultiDbConfig {
        primary: SharedDb,
        replica: SharedDb,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct SharedDb {
        password: Secret<String>,
    }

    impl Default for MultiDbConfig {
        fn default() -> Self {
            Self {
                primary: SharedDb {
                    password: Secret::new("primary-secret".to_owned()),
                },
                replica: SharedDb {
                    password: Secret::new("replica-secret".to_owned()),
                },
            }
        }
    }

    let loaded = ConfigLoader::new(MultiDbConfig::default())
        .discover_secret_paths_from_schema()
        .load()
        .expect("config loads");

    let redacted = loaded.report().redacted_value();
    assert_eq!(
        redacted["primary"]["password"].as_str(),
        Some("***redacted***")
    );
    assert_eq!(
        redacted["replica"]["password"].as_str(),
        Some("***redacted***")
    );
    let rendered = loaded.report().redacted_pretty_json();
    assert!(!rendered.contains("primary-secret"));
    assert!(!rendered.contains("replica-secret"));
}

#[test]
fn discovers_secret_paths_from_tuple_items() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TupleSecretConfig {
        pair: (String, Secret<String>),
    }

    impl Default for TupleSecretConfig {
        fn default() -> Self {
            Self {
                pair: ("public".to_owned(), Secret::new("tuple-secret".to_owned())),
            }
        }
    }

    let loaded = ConfigLoader::new(TupleSecretConfig::default())
        .discover_secret_paths_from_schema()
        .load()
        .expect("config loads");

    let redacted = loaded.report().redacted_value();
    assert_eq!(redacted["pair"][0].as_str(), Some("public"));
    assert_eq!(redacted["pair"][1].as_str(), Some("***redacted***"));
}

#[test]
fn annotated_schema_supports_collection_item_paths() {
    let schema = annotated_json_schema_for::<ArraySchemaConfig>();
    assert_eq!(
        schema["properties"]["users"]["items"]["properties"]["password"]["x-tier-secret"].as_bool(),
        Some(true)
    );
}

#[test]
fn annotated_schema_prefers_exact_metadata_over_later_wildcard_matches() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AnnotatedSchemaOrderConfig {
        pair: (String, u16),
    }

    impl TierMetadata for AnnotatedSchemaOrderConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.1")
                    .doc("Specific pair item")
                    .example("8080"),
                FieldMetadata::new("pair.*")
                    .doc("Generic pair item")
                    .example("9000"),
            ])
        }
    }

    let schema = annotated_json_schema_for::<AnnotatedSchemaOrderConfig>();
    let item = &schema["properties"]["pair"]["prefixItems"][1];

    assert_eq!(item["description"].as_str(), Some("Specific pair item"));
    assert_eq!(item["example"].as_u64(), Some(8080));
}

#[test]
fn exact_validation_rules_override_generic_rule_kinds_in_exports() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct ValidationOverrideTupleSchemaConfig {
        pair: (u16, u16),
    }

    impl TierMetadata for ValidationOverrideTupleSchemaConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*")
                    .min(10)
                    .validation_message("min", "generic minimum rule"),
                FieldMetadata::new("pair.1").min(1),
            ])
        }
    }

    let schema = annotated_json_schema_for::<ValidationOverrideTupleSchemaConfig>();
    let item = &schema["properties"]["pair"]["prefixItems"][1];
    let rendered = serde_json::to_string(item).expect("annotated tuple item schema");
    let docs = env_docs_for::<ValidationOverrideTupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let entry = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert_eq!(item["x-tier-validate"].as_array().map(Vec::len), Some(1));
    assert!(rendered.contains("\"Min\":1"));
    assert!(!rendered.contains("\"Min\":10"));
    assert_eq!(
        item["x-tier-validation-config"]["min"]["message"].as_str(),
        Some("generic minimum rule")
    );

    assert_eq!(
        entry
            .validations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["min=1".to_owned()]
    );
    assert_eq!(
        entry.validation_messages.get("min").map(String::as_str),
        Some("generic minimum rule")
    );
}

#[test]
fn exact_validation_configs_override_inherited_wildcard_rules_in_exports() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct ValidationConfigOverrideTupleSchemaConfig {
        pair: (u16, u16),
    }

    impl TierMetadata for ValidationConfigOverrideTupleSchemaConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*").min(10),
                FieldMetadata::new("pair.1")
                    .validation_message("min", "exact inherited minimum rule"),
            ])
        }
    }

    let schema = annotated_json_schema_for::<ValidationConfigOverrideTupleSchemaConfig>();
    let item = &schema["properties"]["pair"]["prefixItems"][1];
    let docs =
        env_docs_for::<ValidationConfigOverrideTupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let entry = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert_eq!(
        item["x-tier-validation-config"]["min"]["message"].as_str(),
        Some("exact inherited minimum rule")
    );
    assert_eq!(
        entry.validation_messages.get("min").map(String::as_str),
        Some("exact inherited minimum rule")
    );
}

#[test]
fn annotated_schema_does_not_project_exact_indices_onto_homogeneous_array_items() {
    let schema = annotated_json_schema_for::<IndexedPrimitiveArraySchemaConfig>();
    let item_schema = &schema["properties"]["ports"]["items"];

    assert_eq!(item_schema["description"], serde_json::Value::Null);
    assert_eq!(item_schema["example"], serde_json::Value::Null);
}

#[test]
fn annotated_schema_supports_optional_nested_paths() {
    let schema = annotated_json_schema_for::<OptionalNestedSchemaConfig>();
    let password = schema["properties"]["db"]["anyOf"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|branch| branch["properties"]["password"].is_object())
        })
        .and_then(|branch| branch["properties"]["password"].as_object())
        .expect("optional db password schema");
    assert_eq!(
        password
            .get("x-tier-secret")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        password
            .get("description")
            .and_then(serde_json::Value::as_str),
        Some("Optional database password")
    );
}

#[test]
fn annotated_schema_keeps_reused_ref_metadata_path_specific() {
    let schema = annotated_json_schema_for::<ReusedRefSchemaConfig>();
    let primary_password = &schema["properties"]["primary"]["properties"]["password"];
    let replica_password = &schema["properties"]["replica"]["properties"]["password"];

    assert_eq!(
        primary_password["description"].as_str(),
        Some("Primary database password")
    );
    assert_eq!(
        primary_password["x-tier-env"].as_str(),
        Some("APP_PRIMARY_PASSWORD")
    );
    assert_eq!(primary_password["example"].as_str(), Some("<secret>"));
    assert_eq!(primary_password["x-tier-secret"].as_bool(), Some(true));

    assert_eq!(
        replica_password["description"].as_str(),
        Some("Replica database password")
    );
    assert_eq!(
        replica_password["x-tier-env"].as_str(),
        Some("APP_REPLICA_PASSWORD")
    );
    assert_eq!(replica_password["example"].as_str(), Some("<secret>"));
    assert_eq!(replica_password["x-tier-secret"].as_bool(), Some(true));
}

#[test]
fn schema_and_docs_redact_examples_for_schema_secret_fields() {
    let schema = annotated_json_schema_for::<SecretExampleSchemaConfig>();
    assert_eq!(
        schema["properties"]["token"]["writeOnly"].as_bool(),
        Some(true)
    );
    assert_eq!(
        schema["properties"]["token"]["example"].as_str(),
        Some("<secret>")
    );

    let docs = env_docs_for::<SecretExampleSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(docs.iter().any(|entry| {
        entry.path == "token"
            && entry.secret
            && entry.example.as_deref() == Some("<secret>")
            && entry.description.as_deref() == Some("Secret token")
    }));

    let example = config_example_for::<SecretExampleSchemaConfig>();
    assert_eq!(example["token"].as_str(), Some("<secret>"));
}

#[test]
fn schema_and_examples_redact_schema_provided_secret_examples() {
    let schema = annotated_json_schema_for::<SecretSchemaProvidedExampleConfig>();
    assert_eq!(
        schema["properties"]["token"]["writeOnly"].as_bool(),
        Some(true)
    );
    assert_eq!(
        schema["properties"]["token"]["example"].as_str(),
        Some("<secret>")
    );

    let example = config_example_for::<SecretSchemaProvidedExampleConfig>();
    assert_eq!(example["token"].as_str(), Some("<secret>"));
}

#[test]
fn generates_environment_docs_from_schema() {
    let docs = env_docs_for::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let array_docs = env_docs_for::<ArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let optional_docs = env_docs_for::<OptionalNestedSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let reused_docs = env_docs_for::<ReusedRefSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| entry.env == "APP_SERVER_HOSTNAME"
        && entry.description.as_deref() == Some("Address exposed by the service")
        && entry.example.as_deref() == Some("0.0.0.0")));
    assert!(docs.iter().any(|entry| {
        entry.path == "server.host"
            && entry.aliases == vec!["server.hostname".to_owned()]
            && entry.validations
                == vec![
                    ValidationRule::NonEmpty,
                    ValidationRule::Pattern("^[a-z0-9.-]+$".to_owned()),
                    ValidationRule::MinLength(3),
                ]
            && entry.has_default
    }));
    assert!(docs.iter().any(|entry| entry.env == "APP__SERVER__PORT"));
    assert!(
        docs.iter()
            .any(|entry| entry.env == "APP__SECRETS__PASSWORD" && entry.secret)
    );
    assert!(docs.iter().any(|entry| {
        entry.path == "website"
            && entry.env == "APP__WEBSITE"
            && entry.description.as_deref() == Some("Public service URL")
            && entry.example.as_deref() == Some("https://api.example.com")
            && entry.allowed_sources == vec![SourceKind::Environment, SourceKind::Arguments]
            && entry.denied_sources == vec![SourceKind::File]
            && entry.validations == vec![ValidationRule::Url]
            && entry.validation_levels.get("url") == Some(&ValidationLevel::Warning)
            && entry.validation_messages.get("url").map(String::as_str)
                == Some("website should be a valid public URL")
            && entry.validation_tags.get("url")
                == Some(&vec!["network".to_owned(), "public".to_owned()])
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "contact_email"
            && entry.env == "APP__CONTACT_EMAIL"
            && entry.description.as_deref() == Some("Operations contact address")
            && entry.example.as_deref() == Some("ops@example.com")
            && entry.validations == vec![ValidationRule::Email]
    }));
    assert!(
        docs.iter()
            .any(|entry| { entry.path == "server.port" && entry.merge == MergeStrategy::Replace })
    );
    assert!(docs.iter().any(|entry| {
        entry.path == "server.port"
            && entry.deprecated.as_deref() == Some("use server.bind_port instead")
    }));
    assert!(array_docs.iter().any(|entry| {
        entry.path == "users.*.password"
            && entry.env == "APP__USERS__{item}__PASSWORD"
            && entry.secret
            && entry.description.as_deref() == Some("Password for each user")
            && entry.validations == vec![ValidationRule::NonEmpty]
    }));
    assert!(optional_docs.iter().any(|entry| {
        entry.path == "db.password"
            && entry.env == "APP__DB__PASSWORD"
            && entry.secret
            && entry.description.as_deref() == Some("Optional database password")
            && !entry.required
    }));
    assert!(reused_docs.iter().any(|entry| {
        entry.path == "primary.password"
            && entry.secret
            && entry.example.as_deref() == Some("<secret>")
    }));
    assert!(reused_docs.iter().any(|entry| {
        entry.path == "replica.password"
            && entry.secret
            && entry.example.as_deref() == Some("<secret>")
    }));
    let indexed_docs =
        env_docs_for::<IndexedPrimitiveArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(indexed_docs.iter().any(|entry| {
        entry.path == "ports.*"
            && entry.env == "APP__PORTS__{item}"
            && entry.description.is_none()
            && entry.example.is_none()
    }));

    let enum_docs = env_docs_for::<EnumSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(enum_docs.iter().any(|entry| {
        entry.path == "mode.kind" && entry.env == "APP__MODE__KIND" && !entry.required
    }));
    assert!(enum_docs.iter().any(|entry| {
        entry.path == "mode.port"
            && entry.env == "APP__MODE__PORT"
            && entry.description.as_deref() == Some("TCP port")
            && !entry.required
    }));
    assert!(enum_docs.iter().any(|entry| {
        entry.path == "mode.path"
            && entry.env == "APP__MODE__PATH"
            && entry.description.as_deref() == Some("Unix socket path")
            && !entry.required
    }));

    let markdown = env_docs_markdown::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(markdown.contains("APP_SERVER_HOSTNAME"));
    assert!(markdown.contains("APP__SECRETS__PASSWORD"));
    assert!(markdown.contains("use server.bind_port instead"));
    assert!(markdown.contains("0.0.0.0"));
    assert!(markdown.contains("server.hostname"));
    assert!(markdown.contains("replace"));
    assert!(markdown.contains("non_empty"));
    assert!(markdown.contains("https://api.example.com"));
    assert!(markdown.contains("ops@example.com"));
    assert!(markdown.contains("url"));
    assert!(markdown.contains("email"));
    assert!(markdown.contains("min=1"));

    let docs_json = env_docs_json::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let docs_array = docs_json.as_array().expect("env docs json array");
    assert!(docs_array.iter().any(|entry| {
        entry["path"].as_str() == Some("server.host")
            && entry["env"].as_str() == Some("APP_SERVER_HOSTNAME")
            && entry["has_default"].as_bool() == Some(true)
            && entry["validations"].as_array().map(Vec::len) == Some(3)
    }));
    assert!(docs_array.iter().any(|entry| {
        entry["path"].as_str() == Some("website")
            && entry["denied_sources"] == serde_json::json!(["file"])
            && entry["validation_levels"]["url"] == serde_json::json!("warning")
            && entry["validation_messages"]["url"]
                == serde_json::json!("website should be a valid public URL")
            && entry["validation_tags"]["url"] == serde_json::json!(["network", "public"])
    }));

    let array_docs_json = env_docs_json::<ArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));
    let array_docs_array = array_docs_json
        .as_array()
        .expect("array env docs json array");
    assert!(array_docs_array.iter().any(|entry| {
        entry["path"].as_str() == Some("users.*.password")
            && entry["env"].as_str() == Some("APP__USERS__{item}__PASSWORD")
            && entry["secret"].as_bool() == Some(true)
    }));

    let docs_report = env_docs_report_json::<SchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert_eq!(
        docs_report["format_version"].as_u64(),
        Some(ENV_DOCS_FORMAT_VERSION as u64)
    );
    assert_eq!(
        docs_report["entries"].as_array().map(Vec::len),
        Some(docs.len())
    );
}

#[test]
fn versioned_schema_and_example_exports_are_structured() {
    let schema_report = json_schema_report_json::<SchemaConfig>();
    assert_eq!(
        schema_report["format_version"].as_u64(),
        Some(SCHEMA_EXPORT_FORMAT_VERSION as u64)
    );
    assert_eq!(schema_report["schema"]["type"].as_str(), Some("object"));

    let annotated_report = annotated_json_schema_report_json::<SchemaConfig>();
    assert_eq!(
        annotated_report["format_version"].as_u64(),
        Some(SCHEMA_EXPORT_FORMAT_VERSION as u64)
    );
    assert!(annotated_report["schema"]["properties"]["website"]["x-tier-validate"].is_array());

    let example_report = config_example_report_json::<SchemaConfig>();
    assert_eq!(
        example_report["format_version"].as_u64(),
        Some(SCHEMA_EXPORT_FORMAT_VERSION as u64)
    );
    assert!(example_report["example"].is_object());
}

#[test]
fn tuple_items_generate_examples_and_env_docs() {
    let docs = env_docs_for::<TupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(docs.iter().any(|entry| {
        entry.path == "pair.0"
            && entry.env == "APP__PAIR__0"
            && entry.description.as_deref() == Some("Primary host")
            && entry.example.as_deref() == Some("edge")
            && entry.ty == "string"
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "pair.1"
            && entry.env == "APP__PAIR__1"
            && entry.description.as_deref() == Some("Primary port")
            && entry.example.as_deref() == Some("8080")
            && entry.validations
                == vec![
                    ValidationRule::Min(1.into()),
                    ValidationRule::Max(65_535.into()),
                ]
            && entry.ty == "integer"
    }));

    let example = config_example_for::<TupleSchemaConfig>();
    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_u64(), Some(8080));
}

#[test]
fn env_docs_respect_min_items_for_tuple_positions() {
    let docs = env_docs_for::<PartiallyRequiredTupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let first = docs
        .iter()
        .find(|entry| entry.path == "pair.0")
        .expect("pair.0 env doc entry");
    let second = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert!(first.required);
    assert!(!second.required);
}

#[test]
fn env_docs_mark_required_additional_tuple_items() {
    let docs = env_docs_for::<TupleWithRequiredAdditionalItemsSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "pair.*")
        .expect("pair.* env doc entry");

    assert_eq!(wildcard.env, "APP__PAIR__{item}");
    assert!(wildcard.required);
}

#[test]
fn env_docs_include_legacy_additional_items() {
    let docs = env_docs_for::<LegacyAdditionalItemsSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "pair.0" && entry.env == "APP__PAIR__0" && entry.ty == "string"
    }));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "pair.*")
        .expect("pair.* env doc entry");
    assert_eq!(wildcard.env, "APP__PAIR__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert_eq!(wildcard.description.as_deref(), Some("Trailing item"));
}

#[test]
fn env_docs_include_contains_array_items() {
    let docs = env_docs_for::<ContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(wildcard.required);
    assert_eq!(
        wildcard.description.as_deref(),
        Some("Required matching port")
    );
}

#[test]
fn env_docs_include_boolean_contains_array_items() {
    let docs = env_docs_for::<BooleanContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "any");
    assert!(wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Any matching value"));
}

#[test]
fn env_docs_omit_contains_wildcards_when_items_false_forbids_extra_items() {
    let docs = env_docs_for::<ContainsWithItemsFalseSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(!docs.iter().any(|entry| entry.path == "ports.*"));
}

#[test]
fn env_docs_omit_contains_wildcards_when_additional_items_false_forbids_extra_items() {
    let docs = env_docs_for::<ContainsWithAdditionalItemsFalseSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(!docs.iter().any(|entry| entry.path == "pair.*"));
}

#[test]
fn env_docs_keep_homogeneous_items_when_additional_items_false_is_irrelevant() {
    let docs = env_docs_for::<HomogeneousItemsWithAdditionalItemsFalseSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Allowed port"));
}

#[test]
fn env_docs_do_not_require_contains_when_homogeneous_additional_items_false_already_matches() {
    let docs = env_docs_for::<
        ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig,
    >(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "groups.*.*")
        .expect("groups.*.* env doc entry");

    assert_eq!(wildcard.env, "APP__GROUPS__{item}__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(!wildcard.required);
}

#[test]
fn env_docs_mark_property_names_contains_wildcards_required_when_fixed_items_use_invalid_keys() {
    let docs =
        env_docs_for::<PropertyNamesContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "backends.*.*.token")
        .expect("backends.*.*.token env doc entry");

    assert_eq!(wildcard.env, "APP__BACKENDS__{item}__{item}__TOKEN");
    assert!(wildcard.required);
}

#[test]
fn env_docs_do_not_mark_pattern_properties_contains_wildcards_required_when_fixed_items_match() {
    let docs =
        env_docs_for::<PatternPropertiesContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "backends.*.*.token")
        .expect("backends.*.*.token env doc entry");

    assert_eq!(wildcard.env, "APP__BACKENDS__{item}__{item}__TOKEN");
    assert!(!wildcard.required);
}

#[test]
fn env_docs_do_not_mark_contains_wildcards_required_when_fixed_items_already_satisfy_it() {
    let docs =
        env_docs_for::<ContainsSatisfiedByFixedTupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "pair.*")
        .expect("pair.* env doc entry");

    assert_eq!(wildcard.env, "APP__PAIR__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(!wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Additional port"));
}

#[test]
fn env_docs_do_not_require_contains_wildcards_after_unique_fixed_item_repair() {
    let docs = env_docs_for::<ContainsSatisfiedAfterUniqueRepairSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(!wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Additional port"));
}

#[test]
fn env_docs_mark_constrained_contains_wildcards_required_when_fixed_items_do_not_satisfy_it() {
    let docs =
        env_docs_for::<ConstrainedContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Positive port"));
}

#[test]
fn env_docs_mark_pattern_contains_wildcards_required_when_fixed_items_do_not_match() {
    let docs = env_docs_for::<PatternContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "string");
    assert!(wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Production label"));
}

#[test]
fn env_docs_mark_multiple_of_contains_wildcards_required_when_fixed_items_do_not_match() {
    let docs = env_docs_for::<MultipleOfContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "ports.*")
        .expect("ports.* env doc entry");

    assert_eq!(wildcard.env, "APP__PORTS__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(wildcard.required);
    assert_eq!(wildcard.description.as_deref(), Some("Even port"));
}

#[test]
fn env_docs_mark_contains_wildcards_required_when_fixed_objects_violate_additional_properties() {
    let docs = env_docs_for::<AdditionalPropertiesFalseContainsArraySchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "backends.*.kind")
        .expect("backends.*.kind env doc entry");

    assert_eq!(wildcard.env, "APP__BACKENDS__{item}__KIND");
    assert_eq!(wildcard.ty, "string");
    assert!(wildcard.required);
}

#[test]
fn env_docs_mark_contains_wildcards_required_when_fixed_objects_violate_min_properties() {
    let docs =
        env_docs_for::<MinPropertiesContainsArraySchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "backends.*.kind")
        .expect("backends.*.kind env doc entry");

    assert_eq!(wildcard.env, "APP__BACKENDS__{item}__KIND");
    assert_eq!(wildcard.ty, "string");
    assert!(wildcard.required);
}

#[test]
fn env_docs_do_not_double_count_overlaid_fixed_items_for_contains() {
    let docs =
        env_docs_for::<OverlaidFixedItemsContainsSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let wildcard = docs
        .iter()
        .find(|entry| entry.path == "pair.*")
        .expect("pair.* env doc entry");

    assert_eq!(wildcard.env, "APP__PAIR__{item}");
    assert_eq!(wildcard.ty, "integer");
    assert!(wildcard.required);
    assert_eq!(
        wildcard.description.as_deref(),
        Some("Additional required port")
    );
}

#[test]
fn env_docs_merge_wildcard_and_exact_metadata_for_tuple_items() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TupleMetadataMergeConfig {
        pair: (String, u16),
    }

    impl TierMetadata for TupleMetadataMergeConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*")
                    .doc("Any pair item")
                    .deprecated("generic tuple item"),
                FieldMetadata::new("pair.1").example("8080"),
            ])
        }
    }

    let docs = env_docs_for::<TupleMetadataMergeConfig>(&EnvDocOptions::prefixed("APP"));
    let item = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert_eq!(item.env, "APP__PAIR__1");
    assert_eq!(item.description.as_deref(), Some("Any pair item"));
    assert_eq!(item.deprecated.as_deref(), Some("generic tuple item"));
    assert_eq!(item.example.as_deref(), Some("8080"));
}

#[test]
fn env_docs_and_examples_include_local_properties_alongside_all_of() {
    let docs = env_docs_for::<AllOfLocalSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(
        docs.iter().any(|entry| {
            entry.path == "enabled" && entry.env == "APP__ENABLED" && entry.required
        })
    );
    assert!(docs.iter().any(|entry| {
        entry.path == "server.port" && entry.env == "APP__SERVER__PORT" && entry.required
    }));

    let example = config_example_for::<AllOfLocalSchemaConfig>();
    assert_eq!(example["enabled"].as_bool(), Some(false));
    assert_eq!(example["server"]["port"].as_i64(), Some(0));
}

#[test]
fn env_docs_and_examples_include_local_properties_alongside_ref_targets() {
    let docs = env_docs_for::<RefSiblingSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(docs.iter().any(|entry| {
        entry.path == "db.password" && entry.env == "APP__DB__PASSWORD" && entry.required
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "db.replica" && entry.env == "APP__DB__REPLICA" && entry.required
    }));

    let example = config_example_for::<RefSiblingSchemaConfig>();
    assert_eq!(example["db"]["password"].as_str(), Some("shared-secret"));
    assert_eq!(example["db"]["replica"].as_str(), Some("replica-a"));
}

#[test]
fn examples_include_ref_target_tuple_items_alongside_local_siblings() {
    let docs = env_docs_for::<RefSiblingTupleSchemaConfig>(&EnvDocOptions::prefixed("APP"));
    assert!(docs.iter().any(|entry| {
        entry.path == "pair.0" && entry.env == "APP__PAIR__0" && entry.ty == "string"
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "pair.1" && entry.env == "APP__PAIR__1" && entry.ty == "integer"
    }));

    let example = config_example_for::<RefSiblingTupleSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_i64(), Some(8080));
}

#[test]
fn env_docs_include_additional_properties_alongside_fixed_properties() {
    let docs = env_docs_for::<PropertyAndMapSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "services.primary.url"
            && entry.env == "APP__SERVICES__PRIMARY__URL"
            && entry.required
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "services.*.token"
            && entry.env == "APP__SERVICES__{item}__TOKEN"
            && !entry.required
    }));
}

#[test]
fn env_docs_mark_dynamic_map_items_required_when_min_properties_requires_entries() {
    let docs = env_docs_for::<MinPropertiesDynamicMapSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "services.*.token"
            && entry.env == "APP__SERVICES__{item}__TOKEN"
            && entry.required
    }));
}

#[test]
fn env_docs_do_not_mark_dynamic_map_items_required_when_fixed_properties_can_satisfy_min_properties()
 {
    let docs = env_docs_for::<MinPropertiesSatisfiedByFixedPropertySchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(docs.iter().any(|entry| {
        entry.path == "services.*.token"
            && entry.env == "APP__SERVICES__{item}__TOKEN"
            && !entry.required
    }));
}

#[test]
fn env_docs_omit_dynamic_map_items_when_max_properties_is_exhausted_by_required_fixed_properties() {
    let docs = env_docs_for::<MaxPropertiesExhaustedByRequiredFixedPropertySchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(docs.iter().any(|entry| {
        entry.path == "services.primary.token"
            && entry.env == "APP__SERVICES__PRIMARY__TOKEN"
            && entry.required
    }));
    assert!(!docs.iter().any(|entry| entry.path == "services.*.token"));
}

#[test]
fn env_docs_omit_dynamic_entries_when_property_names_forbid_all_keys() {
    let additional_docs = env_docs_for::<PropertyNamesFalseAdditionalPropertiesSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );
    let pattern_docs = env_docs_for::<PropertyNamesFalsePatternPropertiesSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(
        !additional_docs
            .iter()
            .any(|entry| entry.path == "services.*.token")
    );
    assert!(
        !pattern_docs
            .iter()
            .any(|entry| entry.path == "services.*.token")
    );
}

#[test]
fn example_generation_satisfies_min_properties_for_dynamic_maps() {
    let example = config_example_for::<MinPropertiesDynamicMapSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 2);
    assert_eq!(services["{item}"]["token"].as_str(), Some("example"));
    assert_eq!(services["{key}"]["token"].as_str(), Some("example"));
}

#[test]
fn example_generation_omits_dynamic_entries_when_property_names_forbid_all_keys() {
    let additional = config_example_for::<PropertyNamesFalseAdditionalPropertiesSchemaConfig>();
    let pattern = config_example_for::<PropertyNamesFalsePatternPropertiesSchemaConfig>();

    assert_eq!(additional["services"], serde_json::json!({}));
    assert_eq!(pattern["services"], serde_json::json!({}));
}

#[test]
fn example_generation_includes_pattern_properties_dynamic_items() {
    let example = config_example_for::<PatternPropertiesDynamicMapSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 1);
    let (key, value) = services
        .iter()
        .next()
        .expect("patternProperties example entry");
    assert!(key.starts_with("svc-"));
    assert_eq!(value["token"].as_str(), Some("pattern-token"));
}

#[test]
fn example_generation_satisfies_min_properties_for_pattern_properties() {
    let example = config_example_for::<PatternPropertiesMinPropertiesDynamicMapSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 2);
    assert!(services.keys().all(|key| key.starts_with("svc-")));
    assert!(
        services
            .values()
            .all(|value| value["token"].as_str() == Some("pattern-token"))
    );
}

#[test]
fn example_generation_respects_property_names_for_pattern_properties() {
    let example = config_example_for::<PatternPropertiesPropertyNamesDynamicMapSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 1);
    assert_eq!(
        services["svc-primary"]["token"].as_str(),
        Some("pattern-token")
    );
    assert!(!services.contains_key("svc-"));
}

#[test]
fn example_generation_respects_max_properties_for_fixed_object_properties() {
    let example = config_example_for::<MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 1);
    assert_eq!(services["primary"]["token"].as_str(), Some("primary-token"));
    assert!(services.get("secondary").is_none());
}

#[test]
fn example_generation_satisfies_min_properties_with_implicit_additional_properties() {
    let example = config_example_for::<ImplicitAdditionalPropertiesMinPropertiesSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 2);
    assert_eq!(services["primary"].as_str(), Some("primary-token"));
    assert!(services.contains_key("{item}"));
    assert!(services["{item}"].is_null());
}

#[test]
fn example_generation_respects_property_names_for_dynamic_maps() {
    let example = config_example_for::<PropertyNamesEnumDynamicMapSchemaConfig>();
    let services = example["services"].as_object().expect("services object");

    assert_eq!(services.len(), 2);
    assert_eq!(services["primary"]["token"].as_str(), Some("enum-token"));
    assert_eq!(services["secondary"]["token"].as_str(), Some("enum-token"));
    assert!(!services.contains_key("{item}"));
    assert!(!services.contains_key("{key}"));
}

#[test]
fn env_docs_omit_optional_fixed_properties_when_max_properties_is_exhausted() {
    let docs = env_docs_for::<MaxPropertiesTrimsOptionalFixedPropertiesSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(docs.iter().any(|entry| {
        entry.path == "services.primary.token"
            && entry.env == "APP__SERVICES__PRIMARY__TOKEN"
            && entry.required
    }));
    assert!(
        !docs
            .iter()
            .any(|entry| entry.path == "services.secondary.token")
    );
}

#[test]
fn env_docs_include_required_dynamic_items_when_additional_properties_is_implicit() {
    let docs = env_docs_for::<ImplicitAdditionalPropertiesMinPropertiesSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(docs.iter().any(|entry| {
        entry.path == "services.primary" && entry.env == "APP__SERVICES__PRIMARY" && entry.required
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "services.*"
            && entry.env == "APP__SERVICES__{item}"
            && entry.ty == "any"
            && entry.required
    }));
}

#[test]
fn env_docs_do_not_reintroduce_wildcards_when_additional_properties_is_explicitly_forbidden() {
    let docs = env_docs_for::<ExplicitlyForbiddenAdditionalPropertiesMinPropertiesSchemaConfig>(
        &EnvDocOptions::prefixed("APP"),
    );

    assert!(docs.iter().any(|entry| {
        entry.path == "services.primary" && entry.env == "APP__SERVICES__PRIMARY" && entry.required
    }));
    assert!(!docs.iter().any(|entry| entry.path == "services.*"));
}

#[test]
fn env_docs_include_pattern_properties_dynamic_items() {
    let docs =
        env_docs_for::<PatternPropertiesDynamicMapSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "services.*.token"
            && entry.env == "APP__SERVICES__{item}__TOKEN"
            && entry.ty == "string"
            && !entry.required
            && entry.description.as_deref() == Some("Pattern service token")
    }));
}

#[test]
fn env_docs_avoid_colliding_with_literal_placeholder_keys() {
    let docs = env_docs_for::<CollidingMapPlaceholderSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "services.{item}.url" && entry.env == "APP__SERVICES__{item}__URL"
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "services.{key}.url" && entry.env == "APP__SERVICES__{key}__URL"
    }));
}

#[test]
fn env_docs_avoid_colliding_with_branch_defined_placeholder_keys() {
    let docs =
        env_docs_for::<BranchCollidingMapPlaceholderSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "services.{item}.url" && entry.env == "APP__SERVICES__{item}__URL"
    }));
    assert!(docs.iter().any(|entry| {
        entry.path == "services.{key}.url" && entry.env == "APP__SERVICES__{key}__URL"
    }));
}

#[test]
fn env_docs_merge_generic_wildcard_metadata_for_template_paths() {
    let docs =
        env_docs_for::<WildcardTemplateMetadataSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let token = docs
        .iter()
        .find(|entry| entry.path == "services.*.token")
        .expect("services.*.token env doc entry");

    assert_eq!(token.env, "APP__SERVICES__{item}__TOKEN");
    assert_eq!(token.description.as_deref(), Some("Any service field"));
    assert_eq!(token.deprecated.as_deref(), Some("generic service field"));
    assert_eq!(token.example.as_deref(), Some("<secret>"));
    assert!(token.secret);
}

#[test]
fn env_docs_preserve_generic_source_policies_when_exact_metadata_only_adds_docs() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct SourcePolicyDocsConfig {
        pair: (String, String),
    }

    impl TierMetadata for SourcePolicyDocsConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*")
                    .allow_sources([SourceKind::Environment])
                    .deny_sources([SourceKind::File]),
                FieldMetadata::new("pair.1").doc("Secondary value"),
            ])
        }
    }

    let docs = env_docs_for::<SourcePolicyDocsConfig>(&EnvDocOptions::prefixed("APP"));
    let pair = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert_eq!(pair.allowed_sources, vec![SourceKind::Environment]);
    assert_eq!(pair.denied_sources, vec![SourceKind::File]);
    assert_eq!(pair.description.as_deref(), Some("Secondary value"));
}

#[test]
fn annotated_schema_merges_generic_wildcard_metadata_for_template_paths() {
    let schema = annotated_json_schema_for::<WildcardTemplateMetadataSchemaConfig>();
    let token = &schema["properties"]["services"]["additionalProperties"]["properties"]["token"];

    assert_eq!(token["description"].as_str(), Some("Any service field"));
    assert_eq!(
        token["x-tier-deprecated-note"].as_str(),
        Some("generic service field")
    );
    assert_eq!(token["x-tier-secret"].as_bool(), Some(true));
    assert_eq!(token["example"].as_str(), Some("<secret>"));
}

#[test]
fn annotated_schema_preserves_generic_merge_strategies_when_exact_metadata_only_adds_docs() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct SchemaMergeConfig {
        pair: (String, String),
    }

    impl TierMetadata for SchemaMergeConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*").merge_strategy(MergeStrategy::Replace),
                FieldMetadata::new("pair.1").doc("Secondary value"),
            ])
        }
    }

    let schema = annotated_json_schema_for::<SchemaMergeConfig>();
    let pair = &schema["properties"]["pair"]["prefixItems"][1];

    assert_eq!(pair["description"].as_str(), Some("Secondary value"));
    assert_eq!(pair["x-tier-merge"].as_str(), Some("replace"));
}

#[test]
fn exact_explicit_default_merge_strategies_override_generic_merge_exports() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct SchemaMergeOverrideConfig {
        pair: (String, String),
    }

    impl TierMetadata for SchemaMergeOverrideConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*").merge_strategy(MergeStrategy::Replace),
                FieldMetadata::new("pair.1").merge_strategy(MergeStrategy::Merge),
            ])
        }
    }

    let schema = annotated_json_schema_for::<SchemaMergeOverrideConfig>();
    let pair = &schema["properties"]["pair"]["prefixItems"][1];
    let docs = env_docs_for::<SchemaMergeOverrideConfig>(&EnvDocOptions::prefixed("APP"));
    let entry = docs
        .iter()
        .find(|entry| entry.path == "pair.1")
        .expect("pair.1 env doc entry");

    assert_eq!(pair["x-tier-merge"].as_str(), Some("merge"));
    assert_eq!(entry.merge, MergeStrategy::Merge);
}

#[test]
fn env_docs_merge_duplicate_paths_from_all_of() {
    let docs = env_docs_for::<AllOfRequiredUnionSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let port = docs
        .iter()
        .find(|entry| entry.path == "server.port")
        .expect("server.port env doc entry");

    assert_eq!(port.env, "APP__SERVER__PORT");
    assert!(port.required);
}

#[test]
fn env_docs_merge_types_for_duplicate_paths_from_one_of() {
    let docs = env_docs_for::<OneOfUnionTypeSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let value = docs
        .iter()
        .find(|entry| entry.path == "value")
        .expect("value env doc entry");

    assert_eq!(value.env, "APP__VALUE");
    assert_eq!(value.ty, "string | integer");
}

#[test]
fn env_docs_preserve_local_description_for_one_of_unions() {
    let docs = env_docs_for::<OneOfDescribedUnionSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    let value = docs
        .iter()
        .find(|entry| entry.path == "value")
        .expect("value env doc entry");

    assert_eq!(value.env, "APP__VALUE");
    assert_eq!(value.ty, "string | integer");
    assert!(value.required);
    assert_eq!(value.description.as_deref(), Some("Union value"));
}

#[test]
fn example_generation_includes_local_properties_alongside_one_of() {
    let example = config_example_for::<OneOfLocalSchemaConfig>();

    assert_eq!(example["enabled"].as_bool(), Some(false));
    assert_eq!(example["server"]["port"].as_i64(), Some(0));
}

#[test]
fn example_generation_merges_array_examples_from_all_of() {
    let example = config_example_for::<AllOfArrayExampleSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_i64(), Some(8080));
}

#[test]
fn example_generation_merges_nested_array_examples_from_all_of() {
    let example = config_example_for::<NestedAllOfArrayExampleSchemaConfig>();

    assert_eq!(example["server"]["ports"][0].as_str(), Some("edge"));
    assert_eq!(example["server"]["ports"][1].as_i64(), Some(8080));
}

#[test]
fn example_generation_includes_additional_tuple_items() {
    let example = config_example_for::<TupleWithAdditionalItemsSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_i64(), Some(8080));
}

#[test]
fn env_docs_respect_max_items_for_tuples_without_additional_items() {
    let docs =
        env_docs_for::<TupleWithoutAdditionalItemsSchemaConfig>(&EnvDocOptions::prefixed("APP"));

    assert!(docs.iter().any(|entry| {
        entry.path == "pair.0" && entry.env == "APP__PAIR__0" && entry.ty == "string"
    }));
    assert!(!docs.iter().any(|entry| entry.path == "pair.*"));
}

#[test]
fn example_generation_respects_max_items_for_tuples_without_additional_items() {
    let example = config_example_for::<TupleWithoutAdditionalItemsSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert!(example["pair"].get(1).is_none());
}

#[test]
fn example_generation_satisfies_min_items_for_required_additional_tuple_items() {
    let example = config_example_for::<TupleWithRequiredAdditionalItemsSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_i64(), Some(8080));
    assert_eq!(example["pair"][2].as_i64(), Some(8080));
    assert!(example["pair"].get(3).is_none());
}

#[test]
fn example_generation_includes_legacy_additional_items() {
    let example = config_example_for::<LegacyAdditionalItemsSchemaConfig>();

    assert_eq!(example["pair"][0].as_str(), Some("edge"));
    assert_eq!(example["pair"][1].as_i64(), Some(8080));
}

#[test]
fn example_generation_satisfies_min_contains_for_arrays() {
    let example = config_example_for::<ContainsArraySchemaConfig>();

    assert_eq!(example["ports"][0].as_i64(), Some(8080));
    assert_eq!(example["ports"][1].as_i64(), Some(8080));
    assert!(example["ports"].get(2).is_none());
}

#[test]
fn example_generation_respects_contains_property_names_constraints() {
    let example = config_example_for::<PropertyNamesContainsArraySchemaConfig>();
    let backends = example["backends"].as_array().expect("backends array");

    assert_eq!(backends.len(), 2);
    assert_eq!(backends[0]["legacy"]["token"].as_str(), Some("stale"));
    assert_eq!(backends[1]["primary"]["token"].as_str(), Some("fresh"));
}

#[test]
fn example_generation_respects_contains_pattern_properties_constraints() {
    let example = config_example_for::<PatternPropertiesContainsArraySchemaConfig>();
    let backends = example["backends"].as_array().expect("backends array");

    assert_eq!(backends.len(), 1);
    assert_eq!(backends[0]["svc-primary"]["token"].as_str(), Some("fresh"));
}

#[test]
fn example_generation_respects_unique_items_for_contains_arrays() {
    let example = config_example_for::<UniqueContainsArraySchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 2);
    assert_eq!(ports[0].as_i64(), Some(2_000));
    assert_eq!(ports[1].as_i64(), Some(4_000));
}

#[test]
fn example_generation_respects_unique_items_for_fixed_arrays() {
    let example = config_example_for::<UniqueFixedArraySchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 2);
    assert_eq!(ports[0].as_i64(), Some(2_000));
    assert_eq!(ports[1].as_i64(), Some(0));
}

#[test]
fn example_generation_respects_unique_items_for_length_limited_strings() {
    let example = config_example_for::<UniqueShortStringArraySchemaConfig>();
    let codes = example["codes"].as_array().expect("codes array");

    assert_eq!(codes.len(), 2);
    assert_eq!(codes[0].as_str(), Some("a"));
    assert_eq!(codes[1].as_str(), Some("b"));
}

#[test]
fn example_generation_preserves_unique_items_after_contains_additions() {
    let example = config_example_for::<UniqueFixedAndContainsArraySchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 3);
    assert_eq!(ports[0].as_i64(), Some(0));
    assert_eq!(ports[1].as_i64(), Some(-1));
    assert_eq!(ports[2].as_i64(), Some(1));
}

#[test]
fn example_generation_counts_unique_repaired_fixed_items_before_contains_additions() {
    let example = config_example_for::<ContainsSatisfiedAfterUniqueRepairSchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 2);
    assert_eq!(ports[0].as_i64(), Some(1));
    assert_eq!(ports[1].as_i64(), Some(2));
}

#[test]
fn example_generation_repairs_cascading_unique_array_duplicates() {
    let example = config_example_for::<CascadingUniqueArraySchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 3);
    assert_eq!(ports[0].as_i64(), Some(1));
    assert_eq!(ports[1].as_i64(), Some(2));
    assert_eq!(ports[2].as_i64(), Some(3));
}

#[test]
fn example_generation_satisfies_boolean_contains_constraints() {
    let example = config_example_for::<BooleanContainsArraySchemaConfig>();

    assert_eq!(example["ports"].as_array().map(Vec::len), Some(3));
    assert!(example["ports"][0].is_null());
    assert_eq!(example["ports"][1].as_bool(), Some(false));
    assert_eq!(example["ports"][2].as_bool(), Some(true));
}

#[test]
fn example_generation_does_not_add_contains_items_when_items_false_forbids_them() {
    let example = config_example_for::<ContainsWithItemsFalseSchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0].as_i64(), Some(0));
}

#[test]
fn example_generation_does_not_add_contains_items_when_additional_items_false_forbids_them() {
    let example = config_example_for::<ContainsWithAdditionalItemsFalseSchemaConfig>();
    let pair = example["pair"].as_array().expect("pair array");

    assert_eq!(pair.len(), 1);
    assert_eq!(pair[0].as_str(), Some("edge"));
}

#[test]
fn example_generation_keeps_homogeneous_items_when_additional_items_false_is_irrelevant() {
    let example = config_example_for::<HomogeneousItemsWithAdditionalItemsFalseSchemaConfig>();
    let ports = example["ports"].as_array().expect("ports array");

    assert_eq!(ports.len(), 2);
    assert_eq!(ports[0].as_i64(), Some(8080));
    assert_eq!(ports[1].as_i64(), Some(8080));
}

#[test]
fn example_generation_counts_homogeneous_additional_items_false_contains_matches() {
    let example = config_example_for::<
        ContainsSatisfiedByHomogeneousArrayWithAdditionalItemsFalseSchemaConfig,
    >();
    let groups = example["groups"].as_array().expect("groups array");

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0][0].as_i64(), Some(8080));
}

#[test]
fn example_generation_respects_contains_numeric_constraints() {
    let example = config_example_for::<ConstrainedContainsArraySchemaConfig>();

    assert_eq!(example["ports"][0].as_i64(), Some(0));
    assert_eq!(example["ports"][1].as_i64(), Some(1));
    assert!(example["ports"].get(2).is_none());
}

#[test]
fn example_generation_respects_contains_pattern_constraints() {
    let example = config_example_for::<PatternContainsArraySchemaConfig>();

    assert_eq!(example["ports"][0].as_str(), Some("dev-edge"));
    assert_eq!(example["ports"][1].as_str(), Some("prod-edge"));
    assert!(example["ports"].get(2).is_none());
}

#[test]
fn example_generation_respects_contains_multiple_of_constraints() {
    let example = config_example_for::<MultipleOfContainsArraySchemaConfig>();

    assert_eq!(example["ports"][0].as_i64(), Some(1));
    assert_eq!(example["ports"][1].as_i64(), Some(2));
    assert!(example["ports"].get(2).is_none());
}

#[test]
fn example_generation_preserves_large_integer_numeric_precision() {
    let example = config_example_for::<LargeIntegerExampleSchemaConfig>();

    assert_eq!(example["id"].as_u64(), Some(9_007_199_254_740_994));
    assert_eq!(example["sparse_multiple"].as_u64(), Some(20_000));
}

#[test]
fn example_generation_respects_fractional_multiple_of_for_integer_values() {
    let example = config_example_for::<FractionalMultipleOfIntegerExampleSchemaConfig>();

    assert_eq!(example["batch_size"].as_u64(), Some(40_001));
}

#[test]
fn example_generation_respects_contains_additional_properties_constraints() {
    let example = config_example_for::<AdditionalPropertiesFalseContainsArraySchemaConfig>();

    assert_eq!(example["backends"][0]["kind"].as_str(), Some("prod"));
    assert_eq!(example["backends"][0]["extra"].as_bool(), Some(true));
    assert_eq!(example["backends"][1]["kind"].as_str(), Some("prod"));
    assert!(example["backends"][1].get("extra").is_none());
    assert!(example["backends"].get(2).is_none());
}

#[test]
fn example_generation_respects_contains_min_properties_constraints() {
    let example = config_example_for::<MinPropertiesContainsArraySchemaConfig>();

    assert_eq!(example["backends"][0], serde_json::json!({}));
    assert_eq!(example["backends"][1]["kind"].as_str(), Some("prod"));
    assert!(example["backends"].get(2).is_none());
}

#[test]
fn generates_example_configuration_from_schema() {
    let example = config_example_for::<SchemaConfig>();

    assert_eq!(example["server"]["host"].as_str(), Some("0.0.0.0"));
    assert_eq!(example["server"]["port"].as_i64(), Some(8080));
    assert_eq!(example["secrets"]["password"].as_str(), Some("<secret>"));
    assert_eq!(example["website"].as_str(), Some("https://api.example.com"));
    assert_eq!(example["contact_email"].as_str(), Some("ops@example.com"));
}

#[test]
fn generates_example_configuration_for_map_items() {
    let example = config_example_for::<MapExampleSchemaConfig>();

    assert_eq!(
        example["services"]["{item}"]["host"].as_str(),
        Some("api.internal")
    );
    assert_eq!(
        example["services"]["{item}"]["token"].as_str(),
        Some("<secret>")
    );
}

#[test]
fn map_examples_avoid_colliding_with_literal_placeholder_keys() {
    let example = config_example_for::<CollidingMapPlaceholderSchemaConfig>();

    assert_eq!(
        example["services"]["{item}"]["url"].as_str(),
        Some("literal.internal")
    );
    assert_eq!(
        example["services"]["{key}"]["url"].as_str(),
        Some("dynamic.internal")
    );
}

#[test]
fn map_examples_avoid_colliding_with_branch_defined_placeholder_keys() {
    let example = config_example_for::<BranchCollidingMapPlaceholderSchemaConfig>();

    assert_eq!(
        example["services"]["{item}"]["url"].as_str(),
        Some("literal.internal")
    );
    assert_eq!(
        example["services"]["{key}"]["url"].as_str(),
        Some("dynamic.internal")
    );
}

#[test]
fn generates_example_configuration_for_tagged_enums() {
    let example = config_example_for::<EnumSchemaConfig>();

    assert_eq!(example["mode"]["kind"].as_str(), Some("tcp"));
    assert_eq!(example["mode"]["port"].as_u64(), Some(0));
}

#[test]
fn example_generation_redacts_reused_ref_secret_examples() {
    let example = config_example_for::<ReusedRefSchemaConfig>();

    assert_eq!(example["primary"]["password"].as_str(), Some("<secret>"));
    assert_eq!(example["replica"]["password"].as_str(), Some("<secret>"));
}

#[test]
fn env_doc_options_support_custom_separators_with_prefixed_suffixes() {
    let default_separator = EnvDocOptions::new().prefix("APP__");
    let custom_separator = EnvDocOptions::new().prefix("APP--").separator("--");
    let ignored_empty_separator = EnvDocOptions::new().prefix("APP").separator("");
    let empty_prefix = EnvDocOptions::new().prefix("");
    let separator_only_prefix = EnvDocOptions::new().prefix("--").separator("--");

    assert_eq!(
        default_separator.env_name("server.port"),
        "APP__SERVER__PORT"
    );
    assert_eq!(
        custom_separator.env_name("server.port"),
        "APP--SERVER--PORT"
    );
    assert_eq!(
        ignored_empty_separator.env_name("server.port"),
        "APP__SERVER__PORT"
    );
    assert_eq!(empty_prefix.env_name("server.port"), "SERVER__PORT");
    assert_eq!(
        separator_only_prefix.env_name("server.port"),
        "SERVER--PORT"
    );
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
    assert!(example.contains("# validate: non_empty, pattern=\"^[a-z0-9.-]+$\", min_length=3"));
    assert!(example.contains("# validate: required_if(server.port == 8080 -> server.host)"));
    assert!(example.contains("# merge: replace"));
    assert!(example.contains("# validate: min=1, max=65535"));
    assert!(example.contains("# deprecated: use server.bind_port instead"));
    assert!(example.contains("[secrets]"));
    assert!(example.contains("password = \"<secret>\""));
    assert!(example.contains("# secret: true"));
    assert!(example.contains("website = \"https://api.example.com\""));
    assert!(example.contains("# Public service URL"));
    assert!(example.contains("# validate: url"));
    assert!(example.contains("contact_email = \"ops@example.com\""));
    assert!(example.contains("# Operations contact address"));
    assert!(example.contains("# validate: email"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_collection_item_metadata() {
    let example = config_example_toml::<ArraySchemaConfig>();

    assert!(example.contains("[[users]]"));
    assert!(example.contains("# Password for each user"));
    assert!(example.contains("# validate: non_empty"));
    assert!(example.contains("# secret: true"));
    assert!(example.contains("password = \"<secret>\""));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_wildcard_and_exact_array_table_item_comments() {
    let example = config_example_toml::<IndexedArrayTableCommentSchemaConfig>();

    assert!(example.contains("[[users]]"));
    assert!(example.contains("# Any user item"));
    assert!(example.contains("# Primary user item"));
    assert!(example.contains("# User name"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_wildcard_and_exact_array_table_field_comments() {
    let example = config_example_toml::<SpecificArrayTableFieldCommentSchemaConfig>();

    assert!(example.contains("[[users]]"));
    assert!(example.contains("# Any user name"));
    assert!(example.contains("# Primary user name"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_use_effective_validation_rules_for_concrete_field_comments() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct ValidationCommentOverrideSchemaConfig {
        users: Vec<SpecificArrayTableFieldCommentUser>,
    }

    impl TierMetadata for ValidationCommentOverrideSchemaConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("users.*.name")
                    .doc("Any user name")
                    .min_length(3),
                FieldMetadata::new("users.0.name")
                    .doc("Primary user name")
                    .min_length(1),
            ])
        }
    }

    let example = config_example_toml::<ValidationCommentOverrideSchemaConfig>();

    assert!(example.contains("[[users]]"));
    assert!(example.contains("# Any user name"));
    assert!(example.contains("# Primary user name"));
    assert!(example.contains("# validate: min_length=1"));
    assert!(!example.contains("# validate: min_length=3"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_use_effective_validation_rules_for_inline_array_item_comments() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct InlineValidationCommentOverrideSchemaConfig {
        pair: (String, String),
    }

    impl TierMetadata for InlineValidationCommentOverrideSchemaConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*")
                    .doc("Any pair item")
                    .min_length(3),
                FieldMetadata::new("pair.1").doc("Secondary value"),
            ])
        }
    }

    let example = config_example_toml::<InlineValidationCommentOverrideSchemaConfig>();

    assert!(example.contains("# [*] Any pair item"));
    assert!(example.contains("# [*] validate: min_length=3"));
    assert!(example.contains("# [1] Secondary value"));
    assert!(example.contains("# [1] validate: min_length=3"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_explicit_default_merge_overrides() {
    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct InlineMergeCommentOverrideSchemaConfig {
        pair: (String, String),
    }

    impl TierMetadata for InlineMergeCommentOverrideSchemaConfig {
        fn metadata() -> ConfigMetadata {
            ConfigMetadata::from_fields([
                FieldMetadata::new("pair.*").merge_strategy(MergeStrategy::Replace),
                FieldMetadata::new("pair.1").merge_strategy(MergeStrategy::Merge),
            ])
        }
    }

    let example = config_example_toml::<InlineMergeCommentOverrideSchemaConfig>();

    assert!(example.contains("# [*] merge: replace"));
    assert!(example.contains("# [1] merge: merge"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_map_item_metadata() {
    let example = config_example_toml::<MapExampleSchemaConfig>();

    assert!(example.contains("[services.\"{item}\"]"));
    assert!(example.contains("# Service host"));
    assert!(example.contains("host = \"api.internal\""));
    assert!(example.contains("# secret: true"));
    assert!(example.contains("token = \"<secret>\""));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_satisfy_min_properties_for_dynamic_maps() {
    let example = config_example_toml::<MinPropertiesDynamicMapSchemaConfig>();

    assert!(example.contains(r#"[services."{item}"]"#));
    assert!(example.contains(r#"[services."{key}"]"#));
    assert!(example.contains(r#"token = "example""#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_respect_property_names_for_dynamic_maps() {
    let example = config_example_toml::<PropertyNamesEnumDynamicMapSchemaConfig>();

    assert!(example.contains("[services.primary]"));
    assert!(example.contains("[services.secondary]"));
    assert!(example.contains(r#"token = "enum-token""#));
    assert!(!example.contains(r#""{item}""#));
    assert!(!example.contains(r#""{key}""#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_merge_generic_wildcard_metadata_for_template_paths() {
    let example = config_example_toml::<WildcardTemplateMetadataSchemaConfig>();

    assert!(example.contains("[services.\"{item}\"]"));
    assert!(example.contains("# Any service field"));
    assert!(example.contains("# deprecated: generic service field"));
    assert!(example.contains("# secret: true"));
    assert!(example.contains("token = \"<secret>\""));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_preserve_metadata_for_literal_placeholder_keys() {
    let example = config_example_toml::<LiteralPlaceholderKeySchemaConfig>();

    assert!(example.contains("[settings]"));
    assert!(example.contains("# Literal placeholder-shaped key"));
    assert!(example.contains(r#""{item}" = "literal-value""#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_avoid_colliding_with_literal_placeholder_keys() {
    let example = config_example_toml::<CollidingMapPlaceholderSchemaConfig>();

    assert!(example.contains(r#"[services."{item}"]"#));
    assert!(example.contains(r#"[services."{key}"]"#));
    assert!(example.contains(r#"url = "literal.internal""#));
    assert!(example.contains(r#"url = "dynamic.internal""#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_avoid_colliding_with_branch_defined_placeholder_keys() {
    let example = config_example_toml::<BranchCollidingMapPlaceholderSchemaConfig>();

    assert!(example.contains(r#"[services."{item}"]"#));
    assert!(example.contains(r#"[services."{key}"]"#));
    assert!(example.contains(r#"url = "literal.internal""#));
    assert!(example.contains(r#"url = "dynamic.internal""#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_keep_mixed_arrays_inline() {
    let example = config_example_toml::<MixedArrayTomlSchemaConfig>();

    assert!(example.contains(r#"backends = [{ name = "api" }, "fallback"]"#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_tuple_item_metadata() {
    let example = config_example_toml::<TupleSchemaConfig>();

    assert!(example.contains("# [0] Primary host"));
    assert!(example.contains("# [1] Primary port"));
    assert!(example.contains("# [1] validate: min=1, max=65535"));
    assert!(example.contains(r#"pair = ["edge", 8080]"#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_respect_max_items_for_tuples_without_additional_items() {
    let example = config_example_toml::<TupleWithoutAdditionalItemsSchemaConfig>();

    assert!(example.contains(r#"pair = ["edge"]"#));
    assert!(!example.contains("8080"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_satisfy_min_items_for_required_additional_tuple_items() {
    let example = config_example_toml::<TupleWithRequiredAdditionalItemsSchemaConfig>();

    assert!(example.contains(r#"pair = ["edge", 8080, 8080]"#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_legacy_additional_item_metadata() {
    let example = config_example_toml::<LegacyAdditionalItemsSchemaConfig>();

    assert!(example.contains("# [*] Trailing item"));
    assert!(example.contains("# [*] validate: min=1, max=65535"));
    assert!(example.contains(r#"pair = ["edge", 8080]"#));
}

#[test]
fn annotated_schema_projects_wildcard_metadata_to_legacy_additional_items() {
    let schema = annotated_json_schema_for::<LegacyAdditionalItemsSchemaConfig>();
    let item = &schema["properties"]["pair"]["additionalItems"];

    assert_eq!(item["description"].as_str(), Some("Trailing item"));
    assert_eq!(item["example"].as_i64(), Some(8080));
    assert_eq!(item["x-tier-validate"].as_array().map(Vec::len), Some(2));
}

#[test]
fn annotated_schema_projects_wildcard_metadata_to_contains_items() {
    let schema = annotated_json_schema_for::<ContainsArraySchemaConfig>();
    let item = &schema["properties"]["ports"]["contains"];

    assert_eq!(item["description"].as_str(), Some("Required matching port"));
    assert_eq!(item["example"].as_i64(), Some(8080));
    assert_eq!(item["x-tier-validate"].as_array().map(Vec::len), Some(2));
}

#[test]
fn annotated_schema_projects_wildcard_metadata_to_pattern_properties() {
    let schema = annotated_json_schema_for::<PatternPropertiesDynamicMapSchemaConfig>();
    let token =
        &schema["properties"]["services"]["patternProperties"]["^svc-"]["properties"]["token"];

    assert_eq!(token["description"].as_str(), Some("Pattern service token"));
    assert_eq!(token["x-tier-merge"].as_str(), Some("merge"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_root_tuple_item_metadata() {
    let example = config_example_toml::<RootTupleSchemaConfig>();

    assert!(example.contains("# [0] Primary host"));
    assert!(example.contains("# [1] Primary port"));
    assert!(example.contains("# [1] validate: min=1, max=65535"));
    assert!(example.contains(r#"# ["edge", 8080]"#));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_remain_valid_for_root_tuple_types() {
    let example = config_example_toml::<RootTupleSchemaConfig>();

    let parsed: Result<toml::Value, _> = toml::from_str(&example);
    assert!(parsed.is_ok(), "generated TOML should stay valid");
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_wildcard_array_item_metadata() {
    let example = config_example_toml::<PrimitiveArraySchemaConfig>();

    assert!(example.contains("# [*] Allowed port"));
    assert!(example.contains("# [*] validate: min=1, max=65535"));
    assert!(example.contains("ports = [8080]"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_contains_array_item_metadata() {
    let example = config_example_toml::<ContainsArraySchemaConfig>();

    assert!(example.contains("# [*] Required matching port"));
    assert!(example.contains("# [*] validate: min=1, max=65535"));
    assert!(example.contains("ports = [8080, 8080]"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_include_nested_array_item_metadata() {
    let example = config_example_toml::<NestedPrimitiveArraySchemaConfig>();

    assert!(example.contains("# [*][*] Allowed matrix value"));
    assert!(example.contains("# [*][*] validate: min=1, max=65535"));
    assert!(example.contains("matrix = [[8080]]"));
}

#[cfg(feature = "toml")]
#[test]
fn commented_toml_examples_sort_exact_array_item_comments_numerically() {
    let example = config_example_toml::<IndexedPrimitiveArraySchemaConfig>();

    let second = example
        .find("# [2] Second port")
        .expect("second port comment");
    let tenth = example
        .find("# [10] Tenth port")
        .expect("tenth port comment");
    assert!(second < tenth);
}
