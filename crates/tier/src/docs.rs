use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{JsonSchema, MergeStrategy, TierMetadata, ValidationRule, json_schema_for};

/// Stable version tag for machine-readable environment documentation payloads.
pub const ENV_DOCS_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// A single schema-derived environment variable documentation row.
pub struct EnvDocEntry {
    /// Dot-delimited configuration path.
    pub path: String,
    /// Environment variable name corresponding to the path.
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
    /// Declarative validation rules applied to this field.
    pub validations: Vec<ValidationRule>,
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
        self.separator = separator.into();
        self
    }

    /// Preserves field case instead of uppercasing environment names.
    #[must_use]
    pub fn preserve_case(mut self) -> Self {
        self.uppercase = false;
        self
    }

    /// Converts a dot-delimited configuration path into an environment variable name.
    #[must_use]
    pub fn env_name(&self, path: &str) -> String {
        let segments = path
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                if self.uppercase {
                    segment.to_ascii_uppercase()
                } else {
                    segment.to_owned()
                }
            })
            .collect::<Vec<_>>();
        let body = segments.join(&self.separator);
        match &self.prefix {
            Some(prefix) if !prefix.is_empty() => {
                format!("{}{}{}", prefix.trim_end_matches('_'), self.separator, body)
            }
            _ => body,
        }
    }
}

/// Generates environment variable documentation rows from a configuration schema.
#[must_use]
pub fn env_docs_for<T>(options: &EnvDocOptions) -> Vec<EnvDocEntry>
where
    T: JsonSchema + TierMetadata,
{
    let schema = json_schema_for::<T>();
    let mut docs = Vec::new();
    collect_env_docs(&schema, &schema, "", true, &mut docs, &mut BTreeSet::new());
    let metadata = T::metadata().fields_by_path();
    docs.sort_by(|left, right| left.path.cmp(&right.path));
    docs.dedup_by(|left, right| left.path == right.path);

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
    serde_json::to_value(env_docs_for::<T>(options)).unwrap_or_else(|_| Value::Array(Vec::new()))
}

/// Renders machine-readable environment variable documentation as pretty JSON.
#[must_use]
pub fn env_docs_json_pretty<T>(options: &EnvDocOptions) -> String
where
    T: JsonSchema + TierMetadata,
{
    serde_json::to_string_pretty(&env_docs_json::<T>(options)).unwrap_or_else(|_| "[]".to_owned())
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
    serde_json::to_value(env_docs_report::<T>(options))
        .unwrap_or_else(|_| Value::Object(Default::default()))
}

/// Renders versioned environment variable documentation as pretty JSON.
#[must_use]
pub fn env_docs_report_json_pretty<T>(options: &EnvDocOptions) -> String
where
    T: JsonSchema + TierMetadata,
{
    serde_json::to_string_pretty(&env_docs_report_json::<T>(options))
        .unwrap_or_else(|_| "{\"error\":\"failed to render env docs report\"}".to_owned())
}

fn apply_field_metadata(
    entry: &mut EnvDocEntry,
    metadata: &BTreeMap<String, crate::FieldMetadata>,
    options: &EnvDocOptions,
) {
    if let Some(field) = metadata.get(&entry.path) {
        if let Some(env) = &field.env {
            entry.env = env.clone();
        } else {
            entry.env = options.env_name(&entry.path);
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
        entry.aliases = field.aliases.clone();
        entry.has_default = field.has_default;
        entry.merge = field.merge;
        entry.validations = field.validations.clone();
        if field.has_default {
            entry.required = false;
        }
    } else {
        entry.env = options.env_name(&entry.path);
    }
}

fn collect_env_docs(
    schema: &Value,
    root: &Value,
    path: &str,
    required: bool,
    docs: &mut Vec<EnvDocEntry>,
    visited_refs: &mut BTreeSet<String>,
) {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if visited_refs.insert(reference.to_owned()) {
            if let Some(target) = resolve_schema_ref(root, reference) {
                collect_env_docs(target, root, path, required, docs, visited_refs);
            }
            visited_refs.remove(reference);
        }
        return;
    }

    let Some(object) = schema.as_object() else {
        if !path.is_empty() {
            docs.push(EnvDocEntry {
                path: path.to_owned(),
                env: String::new(),
                ty: "unknown".to_owned(),
                required,
                secret: false,
                description: None,
                example: None,
                deprecated: None,
                aliases: Vec::new(),
                has_default: false,
                merge: MergeStrategy::Merge,
                validations: Vec::new(),
            });
        }
        return;
    };

    let properties = object.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        let required_properties = object
            .get("required")
            .and_then(Value::as_array)
            .map(|required| {
                required
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();

        for (key, child_schema) in properties {
            let next = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            collect_env_docs(
                child_schema,
                root,
                &next,
                required_properties.contains(key),
                docs,
                visited_refs,
            );
        }
        return;
    }

    if !path.is_empty() {
        docs.push(EnvDocEntry {
            path: path.to_owned(),
            env: String::new(),
            ty: schema_type(object),
            required,
            secret: object
                .get("writeOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || object
                    .get("x-tier-secret")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            description: object
                .get("description")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            example: None,
            deprecated: None,
            aliases: Vec::new(),
            has_default: false,
            merge: MergeStrategy::Merge,
            validations: Vec::new(),
        });
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
        _ if object.contains_key("enum") => "enum".to_owned(),
        _ if object.contains_key("items") => "array".to_owned(),
        _ => "object".to_owned(),
    }
}

fn resolve_schema_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}
