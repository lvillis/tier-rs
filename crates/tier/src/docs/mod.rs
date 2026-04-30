use std::collections::BTreeSet;

use serde_json::Value;

use crate::{
    ConfigMetadata, JsonSchema, MergeStrategy, SourceKind, TierMetadata, ValidationRule,
    export::{json_pretty, json_value},
    json_schema_for,
    schema::{
        allows_additional_array_items_for_schema as allows_additional_array_items,
        dynamic_object_placeholder_for_schema, legacy_additional_items_for_schema,
        required_contains_additional_items_for_docs,
    },
};

mod collect;

use self::collect::*;

/// Stable version tag for machine-readable environment documentation payloads.
pub const ENV_DOCS_FORMAT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// A single schema-derived environment variable documentation row.
pub struct EnvDocEntry {
    /// Dot-delimited configuration path.
    pub path: String,
    /// Environment variable name corresponding to the path.
    ///
    /// Collection segments are rendered as placeholder tokens such as `{item}`.
    /// Replace them with a concrete index or key when setting an actual
    /// environment variable.
    pub env: String,
    /// Human-readable schema type summary.
    pub ty: String,
    /// Whether the field is required by the schema.
    pub required: bool,
    /// Whether the field should be treated as sensitive.
    pub secret: bool,
    /// Optional field description pulled from the schema.
    pub description: Option<String>,
    /// Optional example value provided by metadata.
    pub example: Option<String>,
    /// Optional deprecation note provided by metadata.
    pub deprecated: Option<String>,
    /// Alternate deserialize aliases accepted for this field.
    pub aliases: Vec<String>,
    /// Whether the field can be omitted because deserialization supplies a default.
    pub has_default: bool,
    /// Merge policy applied when multiple layers target this field.
    pub merge: MergeStrategy,
    /// Source kinds allowed to override this field.
    ///
    /// An empty list means the field does not restrict its allowed sources.
    pub allowed_sources: Vec<SourceKind>,
    /// Source kinds explicitly denied from overriding this field.
    pub denied_sources: Vec<SourceKind>,
    /// Declarative validation rules applied to this field.
    pub validations: Vec<ValidationRule>,
    /// Per-rule validation levels keyed by rule code.
    pub validation_levels: std::collections::BTreeMap<String, crate::ValidationLevel>,
    /// Per-rule custom messages keyed by rule code.
    pub validation_messages: std::collections::BTreeMap<String, String>,
    /// Per-rule machine-readable tags keyed by rule code.
    pub validation_tags: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Versioned machine-readable environment documentation payload.
pub struct EnvDocsReport {
    /// Stable schema version for external consumers.
    pub format_version: u32,
    /// Rendered environment variable documentation entries.
    pub entries: Vec<EnvDocEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Options controlling schema-derived environment variable documentation.
///
/// Use `EnvDocOptions` to keep generated env docs aligned with the same prefix
/// and separator conventions you use at runtime.
///
/// # Examples
///
/// ```
/// use tier::EnvDocOptions;
///
/// let env = EnvDocOptions::prefixed("APP").env_name("server.port");
/// assert_eq!(env, "APP__SERVER__PORT");
/// ```
pub struct EnvDocOptions {
    prefix: Option<String>,
    separator: String,
    uppercase: bool,
}

impl Default for EnvDocOptions {
    fn default() -> Self {
        Self {
            prefix: None,
            separator: "__".to_owned(),
            uppercase: true,
        }
    }
}

impl EnvDocOptions {
    /// Creates options with no prefix, `__` separator, and uppercase names.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates options with a fixed environment variable prefix.
    #[must_use]
    pub fn prefixed(prefix: impl Into<String>) -> Self {
        Self::default().prefix(prefix)
    }

    /// Sets the environment variable prefix.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the separator placed between path segments.
    #[must_use]
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        let separator = separator.into();
        if !separator.is_empty() {
            self.separator = separator;
        }
        self
    }

    /// Preserves field case instead of uppercasing environment names.
    #[must_use]
    pub fn preserve_case(mut self) -> Self {
        self.uppercase = false;
        self
    }

    /// Converts a dot-delimited configuration path into an environment variable name.
    ///
    /// Collection segments are rendered as placeholder tokens so generated docs
    /// describe a template rather than the internal wildcard syntax.
    #[must_use]
    pub fn env_name(&self, path: &str) -> String {
        let segments = path
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                if segment == "*" {
                    "{item}".to_owned()
                } else if segment.starts_with('{') && segment.ends_with('}') {
                    segment.to_owned()
                } else if self.uppercase {
                    segment.to_ascii_uppercase()
                } else {
                    segment.to_owned()
                }
            })
            .collect::<Vec<_>>();
        let body = segments.join(&self.separator);
        match &self.prefix {
            Some(prefix) if !prefix.is_empty() => {
                let prefix = normalize_env_prefix(prefix, &self.separator);
                if prefix.is_empty() {
                    body
                } else if body.is_empty() {
                    prefix
                } else {
                    format!("{prefix}{}{}", self.separator, body)
                }
            }
            _ => body,
        }
    }
}

fn normalize_env_prefix(prefix: &str, separator: &str) -> String {
    if prefix.is_empty() {
        return String::new();
    }

    let mut normalized = prefix.to_owned();
    if !separator.is_empty() {
        while normalized.ends_with(separator) {
            normalized.truncate(normalized.len() - separator.len());
        }
    }
    if separator != "_" {
        normalized = normalized.trim_end_matches('_').to_owned();
    }
    normalized
}

/// Generates environment variable documentation rows from a configuration schema.
///
/// # Examples
///
/// ```
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigMetadata, EnvDocOptions, FieldMetadata, TierMetadata, env_docs_for};
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct AppConfig {
///     server: ServerConfig,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
/// struct ServerConfig {
///     port: u16,
/// }
///
/// impl TierMetadata for AppConfig {
///     fn metadata() -> ConfigMetadata {
///         ConfigMetadata::from_fields([
///             FieldMetadata::new("server.port")
///                 .env("APP_SERVER_PORT")
///                 .doc("Port used for incoming traffic"),
///         ])
///     }
/// }
///
/// let docs = env_docs_for::<AppConfig>(&EnvDocOptions::prefixed("APP"));
/// assert_eq!(docs[0].path, "server.port");
/// assert_eq!(docs[0].env, "APP_SERVER_PORT");
/// ```
#[must_use]
pub fn env_docs_for<T>(options: &EnvDocOptions) -> Vec<EnvDocEntry>
where
    T: JsonSchema + TierMetadata,
{
    let schema = json_schema_for::<T>();
    let mut docs = Vec::new();
    collect_env_docs(
        &schema,
        &schema,
        "",
        true,
        &mut docs,
        &mut BTreeSet::new(),
        None,
    );
    let metadata = T::metadata();
    docs.sort_by(|left, right| left.path.cmp(&right.path));
    docs = merge_duplicate_env_docs(docs);

    for entry in &mut docs {
        apply_field_metadata(entry, &metadata, options);
    }

    docs
}

/// Renders schema-derived environment variable documentation as Markdown.
#[must_use]
pub fn env_docs_markdown<T>(options: &EnvDocOptions) -> String
where
    T: JsonSchema + TierMetadata,
{
    let docs = env_docs_for::<T>(options);
    let mut output = String::from(
        "| Path | Env | Type | Required | Default | Merge | Secret | Validation | Aliases | Deprecated | Example | Description |\n",
    );
    output.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");

    for entry in docs {
        let description = entry
            .description
            .unwrap_or_default()
            .replace('\n', " ")
            .replace('|', "\\|");
        let example = entry
            .example
            .unwrap_or_default()
            .replace('\n', " ")
            .replace('|', "\\|");
        let deprecated = entry
            .deprecated
            .unwrap_or_default()
            .replace('\n', " ")
            .replace('|', "\\|");
        let required = if entry.required { "yes" } else { "no" };
        let defaulted = if entry.has_default { "yes" } else { "no" };
        let secret = if entry.secret { "yes" } else { "no" };
        let validations = entry
            .validations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
            .replace('|', "\\|");
        let aliases = entry.aliases.join(", ").replace('|', "\\|");
        output.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | {} | `{}` | {} | {} | {} | {} | {} | {} |\n",
            entry.path,
            entry.env,
            entry.ty,
            required,
            defaulted,
            entry.merge,
            secret,
            validations,
            aliases,
            deprecated,
            example,
            description
        ));
    }

    output
}

/// Renders schema-derived environment variable documentation as machine-readable JSON.
#[must_use]
pub fn env_docs_json<T>(options: &EnvDocOptions) -> Value
where
    T: JsonSchema + TierMetadata,
{
    json_value(&env_docs_for::<T>(options), Value::Array(Vec::new()))
}

/// Renders machine-readable environment variable documentation as pretty JSON.
#[must_use]
pub fn env_docs_json_pretty<T>(options: &EnvDocOptions) -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(&env_docs_json::<T>(options), "[]")
}

/// Renders versioned machine-readable environment variable documentation.
#[must_use]
pub fn env_docs_report<T>(options: &EnvDocOptions) -> EnvDocsReport
where
    T: JsonSchema + TierMetadata,
{
    EnvDocsReport {
        format_version: ENV_DOCS_FORMAT_VERSION,
        entries: env_docs_for::<T>(options),
    }
}

/// Renders versioned environment variable documentation as JSON.
#[must_use]
pub fn env_docs_report_json<T>(options: &EnvDocOptions) -> Value
where
    T: JsonSchema + TierMetadata,
{
    json_value(
        &env_docs_report::<T>(options),
        Value::Object(Default::default()),
    )
}

/// Renders versioned environment variable documentation as pretty JSON.
#[must_use]
pub fn env_docs_report_json_pretty<T>(options: &EnvDocOptions) -> String
where
    T: JsonSchema + TierMetadata,
{
    json_pretty(
        &env_docs_report_json::<T>(options),
        "{\"error\":\"failed to render env docs report\"}",
    )
}

fn apply_field_metadata(
    entry: &mut EnvDocEntry,
    metadata: &ConfigMetadata,
    options: &EnvDocOptions,
) {
    let fields = metadata.matching_fields_for_path(&entry.path);
    if fields.is_empty() {
        entry.env = options.env_name(&entry.path);
        return;
    }

    entry.env = options.env_name(&entry.path);

    for field in fields {
        if let Some(env) = &field.env {
            entry.env = env.clone();
        }
        entry.secret |= field.secret;
        if let Some(doc) = &field.doc {
            entry.description = Some(doc.clone());
        }
        if let Some(example) = &field.example {
            entry.example = Some(example.clone());
        }
        if let Some(deprecated) = &field.deprecated {
            entry.deprecated = Some(deprecated.clone());
        }
        for alias in &field.aliases {
            if !entry.aliases.contains(alias) {
                entry.aliases.push(alias.clone());
            }
        }
        entry.has_default |= field.has_default;
        if field.has_default {
            entry.required = false;
        }
    }

    if let Some(effective) = metadata.effective_field_for(&entry.path) {
        entry.merge = effective.merge;
        entry.validations = effective.validations.clone();
        let validation_export = effective.validation_export();
        entry.validation_levels = validation_export.levels;
        entry.validation_messages = validation_export.messages;
        entry.validation_tags = validation_export.tags;
    }

    if let Some(policy) = metadata.effective_source_policy_for(&entry.path) {
        entry.allowed_sources = policy.allowed_sources_vec();
        entry.denied_sources = policy.denied_sources_vec();
    }

    if entry.secret && entry.example.is_some() {
        entry.example = Some("<secret>".to_owned());
    }
}

fn merge_duplicate_env_docs(entries: Vec<EnvDocEntry>) -> Vec<EnvDocEntry> {
    let mut merged = Vec::<EnvDocEntry>::new();

    for entry in entries {
        if let Some(existing) = merged.last_mut()
            && existing.path == entry.path
        {
            merge_env_doc_entry(existing, entry);
        } else {
            merged.push(entry);
        }
    }

    merged
}

fn merge_env_doc_entry(existing: &mut EnvDocEntry, incoming: EnvDocEntry) {
    existing.required |= incoming.required;
    existing.secret |= incoming.secret;
    existing.has_default |= incoming.has_default;

    existing.ty = merge_env_doc_types(&existing.ty, &incoming.ty);
    if existing.description.is_none() {
        existing.description = incoming.description;
    }
    if existing.example.is_none() {
        existing.example = incoming.example;
    }
    if existing.deprecated.is_none() {
        existing.deprecated = incoming.deprecated;
    }
    if existing.aliases.is_empty() {
        existing.aliases = incoming.aliases;
    } else {
        for alias in incoming.aliases {
            if !existing.aliases.contains(&alias) {
                existing.aliases.push(alias);
            }
        }
    }
    if existing.merge == MergeStrategy::Merge && incoming.merge != MergeStrategy::Merge {
        existing.merge = incoming.merge;
    }
    if !incoming.allowed_sources.is_empty() {
        existing.allowed_sources = incoming.allowed_sources;
    }
    if !incoming.denied_sources.is_empty() {
        existing.denied_sources = incoming.denied_sources;
    }
    for rule in incoming.validations {
        if !existing.validations.contains(&rule) {
            existing.validations.push(rule);
        }
    }
    existing
        .validation_levels
        .extend(incoming.validation_levels);
    existing
        .validation_messages
        .extend(incoming.validation_messages);
    existing.validation_tags.extend(incoming.validation_tags);
}

fn merge_env_doc_types(existing: &str, incoming: &str) -> String {
    if existing == incoming {
        return existing.to_owned();
    }

    let mut merged = Vec::<String>::new();
    for ty in [existing, incoming]
        .into_iter()
        .flat_map(|value| value.split(" | "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !merged.iter().any(|existing| existing == ty) {
            merged.push(ty.to_owned());
        }
    }

    if merged.is_empty() {
        "unknown".to_owned()
    } else {
        merged.join(" | ")
    }
}

fn apply_local_schema_entry_overrides(
    path: &str,
    required: bool,
    object: &serde_json::Map<String, Value>,
    docs: &mut [EnvDocEntry],
) {
    if path.is_empty() {
        return;
    }

    let description = object
        .get("description")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let secret = object
        .get("writeOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || object
            .get("x-tier-secret")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    if !required && !secret && description.is_none() {
        return;
    }

    for entry in docs.iter_mut().filter(|entry| entry.path == path) {
        entry.required |= required;
        entry.secret |= secret;
        if let Some(description) = &description {
            entry.description = Some(description.clone());
        }
    }
}

fn schema_type(object: &serde_json::Map<String, Value>) -> String {
    match object.get("type") {
        Some(Value::String(ty)) => ty.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" | "),
        _ if object.contains_key("const") => object
            .get("const")
            .map_or_else(|| "object".to_owned(), infer_type_from_value),
        _ if object.contains_key("enum") => "enum".to_owned(),
        _ if object.contains_key("items") || object.contains_key("prefixItems") => {
            "array".to_owned()
        }
        _ => "object".to_owned(),
    }
}

fn infer_type_from_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "boolean".to_owned(),
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "integer".to_owned()
            } else {
                "number".to_owned()
            }
        }
        Value::String(_) => "string".to_owned(),
        Value::Array(_) => "array".to_owned(),
        Value::Object(_) => "object".to_owned(),
    }
}

fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

fn inlined_schema_ref(schema: &Value, root: &Value) -> Option<Value> {
    let reference = schema.get("$ref").and_then(Value::as_str)?;
    let target = resolve_schema_ref(root, reference)?;
    let mut inlined = target.clone();
    if let (Some(inlined_object), Some(reference_object)) =
        (inlined.as_object_mut(), schema.as_object())
    {
        for (key, value) in reference_object {
            if key != "$ref" {
                merge_schema_keyword(inlined_object, key, value);
            }
        }
    }
    Some(inlined)
}

fn merge_schema_keyword(target: &mut serde_json::Map<String, Value>, key: &str, overlay: &Value) {
    match key {
        "required" => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                for value in values {
                    if !merged.contains(value) {
                        merged.push(value.clone());
                    }
                }
            } else {
                target.insert(key.to_owned(), overlay.clone());
                return;
            }
            target.insert(key.to_owned(), Value::Array(merged));
        }
        "prefixItems" | "items" if overlay.is_array() => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                merge_schema_arrays(&mut merged, values);
                target.insert(key.to_owned(), Value::Array(merged));
            } else {
                target.insert(key.to_owned(), overlay.clone());
            }
        }
        "allOf" | "anyOf" | "oneOf" => {
            let mut merged = target
                .get(key)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(values) = overlay.as_array() {
                merged.extend(values.iter().cloned());
                target.insert(key.to_owned(), Value::Array(merged));
            } else {
                target.insert(key.to_owned(), overlay.clone());
            }
        }
        _ => match (target.get_mut(key), overlay) {
            (Some(Value::Object(existing)), Value::Object(overlay_map)) => {
                merge_schema_objects(existing, overlay_map);
            }
            _ => {
                target.insert(key.to_owned(), overlay.clone());
            }
        },
    }
}

fn merge_schema_objects(
    target: &mut serde_json::Map<String, Value>,
    overlay: &serde_json::Map<String, Value>,
) {
    for (key, value) in overlay {
        merge_schema_keyword(target, key, value);
    }
}

fn merge_schema_arrays(target: &mut Vec<Value>, overlay: &[Value]) {
    for (index, value) in overlay.iter().enumerate() {
        if value.is_null() {
            continue;
        }
        if let Some(existing) = target.get_mut(index) {
            merge_schema_value(existing, value);
        } else {
            target.push(value.clone());
        }
    }
}

fn merge_schema_value(target: &mut Value, overlay: &Value) {
    match overlay {
        Value::Object(overlay_map) if target.is_object() => {
            let Value::Object(existing) = target else {
                unreachable!("checked object target")
            };
            merge_schema_objects(existing, overlay_map);
        }
        Value::Array(overlay_items) if target.is_array() => {
            let Value::Array(existing) = target else {
                unreachable!("checked array target")
            };
            merge_schema_arrays(existing, overlay_items);
        }
        _ => *target = overlay.clone(),
    }
}
