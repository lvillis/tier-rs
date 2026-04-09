use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fmt::{self, Display, Formatter};

use serde_json::Value;

use crate::error::{UnknownField, ValidationError};
use crate::loader::SourceTrace;

/// Stable version tag for machine-readable doctor and audit reports.
pub const REPORT_FORMAT_VERSION: u32 = 2;

#[cfg(feature = "schema")]
/// Stable version tag for machine-readable export bundles.
pub const EXPORT_BUNDLE_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Aggregate counts for a machine-readable configuration report.
pub struct ReportSummary {
    /// Number of applied sources.
    pub source_count: usize,
    /// Number of executed validations.
    pub validation_count: usize,
    /// Number of warnings recorded during loading.
    pub warning_count: usize,
    /// Number of traced configuration paths.
    pub trace_count: usize,
    /// Number of configured secret paths.
    pub secret_path_count: usize,
    /// Number of applied configuration migrations.
    pub migration_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Machine-readable summary of a loaded configuration report.
pub struct DoctorReport {
    /// Stable schema version for external consumers.
    pub format_version: u32,
    /// Aggregate counts for this report.
    pub summary: ReportSummary,
    /// Sources applied in order.
    pub sources: Vec<SourceTrace>,
    /// Validation names executed during loading.
    pub validations: Vec<String>,
    /// Structured warnings recorded during loading.
    pub warnings: Vec<ConfigWarning>,
    /// Applied migration steps recorded during loading.
    pub migrations: Vec<AppliedMigration>,
    /// Final configuration value with redaction applied.
    pub redacted_final: Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Structured audit details for a single resolved path.
pub struct TraceAudit {
    /// Full explanation for the path.
    pub explanation: Explanation,
    /// Most recent source that wrote the path, when known.
    pub last_source: Option<SourceTrace>,
    /// Number of recorded resolution steps for the path.
    pub step_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Machine-readable audit payload including traces for every resolved path.
pub struct AuditReport {
    /// Stable schema version for external consumers.
    pub format_version: u32,
    /// Aggregate counts for this report.
    pub summary: ReportSummary,
    /// Summary doctor payload.
    pub doctor: DoctorReport,
    /// Structured path explanations keyed by normalized path.
    pub traces: BTreeMap<String, TraceAudit>,
}

#[cfg(feature = "schema")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Versioned machine-readable export bundle for downstream integrations.
pub struct ExportBundleReport {
    /// Stable bundle version for external consumers.
    pub format_version: u32,
    /// Operational doctor summary for the loaded configuration.
    pub doctor: DoctorReport,
    /// Full audit payload for the loaded configuration.
    pub audit: AuditReport,
    /// Versioned env docs export.
    pub env_docs: crate::EnvDocsReport,
    /// Versioned plain JSON Schema export.
    pub json_schema: crate::JsonSchemaReport,
    /// Versioned annotated JSON Schema export.
    pub annotated_json_schema: crate::JsonSchemaReport,
    /// Versioned example export.
    pub example: crate::ConfigExampleReport,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// One source contribution recorded for a configuration path.
pub struct ResolutionStep {
    /// Source that wrote the value.
    pub source: SourceTrace,
    /// Value contributed by the source.
    pub value: Value,
    /// Whether the recorded value was redacted.
    pub redacted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Full resolution trace for a single configuration path.
pub struct Explanation {
    /// Dot-delimited configuration path.
    pub path: String,
    /// Final value for the path after all layers and normalization.
    pub final_value: Option<Value>,
    /// Ordered source contributions for the path.
    pub steps: Vec<ResolutionStep>,
    /// Whether the path is considered sensitive.
    pub redacted: bool,
}

impl Display for Explanation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let final_value = self
            .final_value
            .as_ref()
            .map_or_else(|| "null".to_owned(), render_value);
        writeln!(f, "{} = {}", self.path, final_value)?;

        for step in &self.steps {
            write!(f, "- {}", step.source)?;
            if !step.source.name.is_empty() {
                write!(f, ": ")?;
            } else {
                write!(f, " ")?;
            }
            writeln!(f, "{}", render_value(&step.value))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Information about a deprecated configuration path used during loading.
pub struct DeprecatedField {
    /// Dot-delimited deprecated path.
    pub path: String,
    /// Most recent source that contributed the deprecated path, when known.
    pub source: Option<SourceTrace>,
    /// Optional migration note or replacement guidance.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// A migration step applied while upgrading configuration input.
pub struct AppliedMigration {
    /// Stable migration kind.
    pub kind: String,
    /// Source version that triggered the migration.
    pub from_version: u32,
    /// Target version this migration belongs to.
    pub to_version: u32,
    /// Original path affected by the migration.
    pub from_path: String,
    /// Replacement path when the migration renames a field.
    pub to_path: Option<String>,
    /// Optional operator-facing note.
    pub note: Option<String>,
}

impl Display for AppliedMigration {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.to_path {
            Some(to_path) => {
                write!(
                    f,
                    "{} {} -> {} (v{} -> v{})",
                    self.kind, self.from_path, to_path, self.from_version, self.to_version
                )?;
            }
            None => {
                write!(
                    f,
                    "{} {} (v{} -> v{})",
                    self.kind, self.from_path, self.from_version, self.to_version
                )?;
            }
        }
        if let Some(note) = &self.note {
            write!(f, "; {note}")?;
        }
        Ok(())
    }
}

impl DeprecatedField {
    /// Creates a deprecated field diagnostic for a path.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: None,
            note: None,
        }
    }

    /// Attaches source information.
    #[must_use]
    pub fn with_source(mut self, source: Option<SourceTrace>) -> Self {
        self.source = source;
        self
    }

    /// Attaches an optional migration note.
    #[must_use]
    pub fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }
}

impl Display for DeprecatedField {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "deprecated field `{}`", self.path)?;
        if let Some(source) = &self.source {
            write!(f, " from {source}")?;
        }
        if let Some(note) = &self.note {
            write!(f, "; {note}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Non-fatal issues surfaced while loading configuration.
pub enum ConfigWarning {
    /// A path was present in input but not recognized by the target type.
    UnknownField(UnknownField),
    /// A deprecated path was used by one of the configured sources.
    DeprecatedField(DeprecatedField),
    /// A declarative validation was configured as warning-level.
    Validation(ValidationError),
}

impl Display for ConfigWarning {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField(field) => Display::fmt(field, f),
            Self::DeprecatedField(field) => Display::fmt(field, f),
            Self::Validation(error) => Display::fmt(error, f),
        }
    }
}

#[derive(Debug, Clone)]
/// Post-load diagnostics including source traces, warnings, and redacted output helpers.
///
/// `ConfigReport` is returned alongside the final typed configuration and is
/// designed for both humans and tooling:
///
/// - `doctor()` and `doctor_json()` summarize a load at a high level
/// - `explain()` shows how one path was resolved
/// - `audit_report()` and `audit_json()` provide a machine-readable trace for
///   every resolved path
///
/// # Examples
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use tier::ConfigLoader;
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self { port: 3000 }
///     }
/// }
///
/// let loaded = ConfigLoader::new(AppConfig::default()).load()?;
/// let doctor = loaded.report().doctor();
/// let explanation = loaded.report().explain("port").expect("port explanation");
///
/// assert!(doctor.contains("Config Doctor"));
/// assert_eq!(explanation.path, "port");
/// # Ok::<(), tier::ConfigError>(())
/// ```
pub struct ConfigReport {
    final_value: Value,
    secret_paths: BTreeSet<String>,
    alias_overrides: BTreeMap<String, String>,
    traces: BTreeMap<String, Vec<ResolutionStep>>,
    applied_sources: Vec<SourceTrace>,
    validations: Vec<String>,
    warnings: Vec<ConfigWarning>,
    migrations: Vec<AppliedMigration>,
}

impl ConfigReport {
    pub(crate) fn new(
        final_value: Value,
        secret_paths: BTreeSet<String>,
        alias_overrides: BTreeMap<String, String>,
    ) -> Self {
        Self {
            final_value,
            secret_paths,
            alias_overrides,
            traces: BTreeMap::new(),
            applied_sources: Vec::new(),
            validations: Vec::new(),
            warnings: Vec::new(),
            migrations: Vec::new(),
        }
    }

    pub(crate) fn record_source(&mut self, source: SourceTrace) {
        self.applied_sources.push(source);
    }

    pub(crate) fn record_step(&mut self, path: String, step: ResolutionStep) {
        self.traces.entry(path).or_default().push(step);
    }

    pub(crate) fn replace_final_value(&mut self, final_value: Value) {
        self.final_value = final_value;
    }

    pub(crate) fn replace_runtime_metadata(
        &mut self,
        secret_paths: BTreeSet<String>,
        alias_overrides: BTreeMap<String, String>,
    ) {
        self.secret_paths = secret_paths;
        self.alias_overrides = alias_overrides;
    }

    pub(crate) fn record_validation(&mut self, name: String) {
        self.validations.push(name);
    }

    pub(crate) fn record_warning(&mut self, warning: ConfigWarning) {
        self.warnings.push(warning);
    }

    pub(crate) fn record_migration(&mut self, migration: AppliedMigration) {
        self.migrations.push(migration);
    }

    /// Returns aggregate counts for machine-readable report consumers.
    #[must_use]
    pub fn summary(&self) -> ReportSummary {
        ReportSummary {
            source_count: self.applied_sources.len(),
            validation_count: self.validations.len(),
            warning_count: self.warnings.len(),
            trace_count: self.traces.len(),
            secret_path_count: self.secret_paths.len(),
            migration_count: self.migrations.len(),
        }
    }

    /// Returns the final merged configuration value before redaction.
    #[must_use]
    pub fn final_value(&self) -> &Value {
        &self.final_value
    }

    /// Returns sources that were applied in order.
    #[must_use]
    pub fn applied_sources(&self) -> &[SourceTrace] {
        &self.applied_sources
    }

    /// Returns successfully executed validator names.
    #[must_use]
    pub fn validations(&self) -> &[String] {
        &self.validations
    }

    /// Returns non-fatal warnings recorded during loading.
    #[must_use]
    pub fn warnings(&self) -> &[ConfigWarning] {
        &self.warnings
    }

    /// Returns applied migration steps recorded during loading.
    #[must_use]
    pub fn migrations(&self) -> &[AppliedMigration] {
        &self.migrations
    }

    /// Returns `true` when the report contains warnings.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Returns the final configuration value with secret paths redacted.
    #[must_use]
    pub fn redacted_value(&self) -> Value {
        redact_value(&self.final_value, "", &self.secret_paths)
    }

    /// Returns the final redacted configuration rendered as pretty JSON.
    #[must_use]
    pub fn redacted_pretty_json(&self) -> String {
        serde_json::to_string_pretty(&self.redacted_value())
            .unwrap_or_else(|_| "{\"error\":\"failed to render report\"}".to_owned())
    }

    /// Explains how a configuration path was resolved.
    #[must_use]
    pub fn explain(&self, path: &str) -> Option<Explanation> {
        let normalized = self.normalize_lookup_path(path)?;
        let redacted = self.path_overlaps_secret(&normalized);
        let steps = self
            .traces
            .get(&normalized)?
            .iter()
            .cloned()
            .map(|mut step| {
                if redacted {
                    step.value = redact_value(&step.value, &normalized, &self.secret_paths);
                    step.redacted = true;
                }
                step
            })
            .collect();
        let final_value = get_value_at_path(&self.final_value, &normalized)
            .cloned()
            .map(|value| redact_value(&value, &normalized, &self.secret_paths));

        Some(Explanation {
            path: normalized,
            final_value,
            steps,
            redacted,
        })
    }

    /// Returns all recorded path traces keyed by normalized path.
    #[must_use]
    pub fn traces(&self) -> &BTreeMap<String, Vec<ResolutionStep>> {
        &self.traces
    }

    /// Builds a machine-readable operational summary of the loaded configuration.
    #[must_use]
    pub fn doctor_report(&self) -> DoctorReport {
        DoctorReport {
            format_version: REPORT_FORMAT_VERSION,
            summary: self.summary(),
            sources: self.applied_sources.clone(),
            validations: self.validations.clone(),
            warnings: self.warnings.clone(),
            migrations: self.migrations.clone(),
            redacted_final: self.redacted_value(),
        }
    }

    /// Builds a machine-readable audit payload including all path traces.
    #[must_use]
    pub fn audit_report(&self) -> AuditReport {
        let traces = self
            .traces
            .keys()
            .filter_map(|path| {
                self.explain(path).map(|explanation| {
                    (
                        path.clone(),
                        TraceAudit {
                            last_source: explanation.steps.last().map(|step| step.source.clone()),
                            step_count: explanation.steps.len(),
                            explanation,
                        },
                    )
                })
            })
            .collect();

        AuditReport {
            format_version: REPORT_FORMAT_VERSION,
            summary: self.summary(),
            doctor: self.doctor_report(),
            traces,
        }
    }

    /// Renders a human-readable operational summary of the loaded configuration.
    #[must_use]
    pub fn doctor(&self) -> String {
        let doctor = self.doctor_report();
        let mut output = String::new();
        let _ = writeln!(&mut output, "Config Doctor");
        let _ = writeln!(&mut output, "Format: v{}", doctor.format_version);
        let _ = writeln!(&mut output, "Sources: {}", doctor.summary.source_count);
        for source in &doctor.sources {
            let _ = writeln!(&mut output, "- {source}");
        }

        let _ = writeln!(
            &mut output,
            "Validations: {}",
            doctor.summary.validation_count
        );
        for validation in &doctor.validations {
            let _ = writeln!(&mut output, "- {validation}");
        }

        let _ = writeln!(&mut output, "Traces: {}", doctor.summary.trace_count);
        let _ = writeln!(&mut output, "Secrets: {}", doctor.summary.secret_path_count);
        let _ = writeln!(
            &mut output,
            "Migrations: {}",
            doctor.summary.migration_count
        );
        for migration in &doctor.migrations {
            let _ = writeln!(&mut output, "- {migration}");
        }

        if doctor.warnings.is_empty() {
            let _ = writeln!(&mut output, "Warnings: 0");
        } else {
            let _ = writeln!(&mut output, "Warnings: {}", doctor.summary.warning_count);
            for warning in &doctor.warnings {
                let _ = writeln!(&mut output, "- {warning}");
            }
        }

        output
    }

    /// Renders a machine-readable operational summary of the loaded configuration.
    #[must_use]
    pub fn doctor_json(&self) -> Value {
        serde_json::to_value(self.doctor_report())
            .unwrap_or_else(|_| Value::Object(Default::default()))
    }

    /// Renders the machine-readable doctor output as pretty JSON.
    #[must_use]
    pub fn doctor_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.doctor_json())
            .unwrap_or_else(|_| "{\"error\":\"failed to render doctor report\"}".to_owned())
    }

    /// Renders a machine-readable audit payload including path traces.
    #[must_use]
    pub fn audit_json(&self) -> Value {
        serde_json::to_value(self.audit_report())
            .unwrap_or_else(|_| Value::Object(Default::default()))
    }

    /// Renders the machine-readable audit output as pretty JSON.
    #[must_use]
    pub fn audit_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.audit_json())
            .unwrap_or_else(|_| "{\"error\":\"failed to render audit report\"}".to_owned())
    }

    pub(crate) fn latest_source_for(&self, path: &str) -> Option<SourceTrace> {
        let path = self.normalize_lookup_path(path)?;
        self.traces
            .get(&path)
            .and_then(|steps| steps.last())
            .map(|step| step.source.clone())
    }

    fn normalize_lookup_path(&self, path: &str) -> Option<String> {
        let segments = parse_external_lookup_path(path).ok()?;
        let normalized = render_lookup_segments(&segments);
        let runtime = canonicalize_runtime_lookup_path(&self.final_value, &segments)?;
        let aliased_runtime = canonicalize_path_with_aliases(&runtime, &self.alias_overrides);
        if self.traces.contains_key(&aliased_runtime)
            || get_value_at_path(&self.final_value, &aliased_runtime).is_some()
        {
            return Some(aliased_runtime);
        }

        let aliased_normalized = canonicalize_path_with_aliases(&normalized, &self.alias_overrides);
        if self.traces.contains_key(&aliased_normalized)
            || get_value_at_path(&self.final_value, &aliased_normalized).is_some()
        {
            return Some(aliased_normalized);
        }

        Some(aliased_runtime)
    }

    fn path_overlaps_secret(&self, path: &str) -> bool {
        self.secret_paths
            .iter()
            .any(|secret| path_overlaps_pattern(path, secret))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LookupSegment {
    Field(String),
    Index(String),
}

fn parse_external_lookup_path(path: &str) -> Result<Vec<LookupSegment>, String> {
    if path == "." {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    let mut after_index = false;
    let mut expecting_segment = true;

    while let Some(ch) = chars.next() {
        if after_index {
            match ch {
                '.' => {
                    if chars.peek().is_none() {
                        return Err("configuration path cannot end with `.`".to_owned());
                    }
                    after_index = false;
                    expecting_segment = true;
                }
                '[' => {
                    let index = parse_lookup_index(&mut chars)?;
                    segments.push(LookupSegment::Index(index));
                    after_index = true;
                    expecting_segment = false;
                }
                _ => {
                    return Err(
                        "expected `.` or `[` after an array index in configuration path".to_owned(),
                    );
                }
            }
            continue;
        }

        match ch {
            '.' => {
                if current.is_empty() {
                    return Err("empty path segment in configuration path".to_owned());
                }
                segments.push(LookupSegment::Field(std::mem::take(&mut current)));
                expecting_segment = true;
            }
            '[' => {
                if current.is_empty() {
                    return Err("array indices must follow a field name".to_owned());
                }
                segments.push(LookupSegment::Field(std::mem::take(&mut current)));
                let index = parse_lookup_index(&mut chars)?;
                segments.push(LookupSegment::Index(index));
                after_index = true;
                expecting_segment = false;
            }
            ']' => return Err("unexpected `]` in configuration path".to_owned()),
            _ => {
                current.push(ch);
                expecting_segment = false;
            }
        }
    }

    if expecting_segment && !segments.is_empty() && current.is_empty() && !after_index {
        return Err("configuration path cannot end with `.`".to_owned());
    }

    if !current.is_empty() {
        segments.push(LookupSegment::Field(current));
    }

    Ok(segments)
}

fn parse_lookup_index<I>(chars: &mut std::iter::Peekable<I>) -> Result<String, String>
where
    I: Iterator<Item = char>,
{
    let mut index = String::new();
    let mut closed = false;
    for next in chars.by_ref() {
        if next == ']' {
            closed = true;
            break;
        }
        index.push(next);
    }
    if !closed {
        return Err("unclosed `[` in configuration path".to_owned());
    }
    if index.is_empty() {
        return Err("empty array index in configuration path".to_owned());
    }
    if !index.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("array indices in configuration paths must be numeric".to_owned());
    }
    index
        .parse::<usize>()
        .map(|value| value.to_string())
        .map_err(|_| "array indices in configuration paths must fit in usize".to_owned())
}

fn render_lookup_segments(segments: &[LookupSegment]) -> String {
    segments
        .iter()
        .map(|segment| match segment {
            LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn canonicalize_runtime_lookup_path(value: &Value, segments: &[LookupSegment]) -> Option<String> {
    let mut current = value;
    let mut canonical = Vec::new();

    for (index, segment) in segments.iter().enumerate() {
        match current {
            Value::Object(map) => {
                let LookupSegment::Field(field) = segment else {
                    return None;
                };
                canonical.push(field.clone());
                let Some(next) = map.get(field) else {
                    canonical.extend(segments[index + 1..].iter().map(|segment| match segment {
                        LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
                    }));
                    break;
                };
                current = next;
            }
            Value::Array(values) => {
                let array_index = match segment {
                    LookupSegment::Index(array_index) => array_index.clone(),
                    LookupSegment::Field(field) if field.parse::<usize>().is_ok() => field.clone(),
                    LookupSegment::Field(_) => return None,
                };
                let Ok(array_index) = array_index.parse::<usize>() else {
                    canonical.push(array_index);
                    canonical.extend(segments[index + 1..].iter().map(|segment| match segment {
                        LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
                    }));
                    break;
                };
                canonical.push(array_index.to_string());
                let Some(next) = values.get(array_index) else {
                    canonical.extend(segments[index + 1..].iter().map(|segment| match segment {
                        LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
                    }));
                    break;
                };
                current = next;
            }
            _ => {
                canonical.push(match segment {
                    LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
                });
                canonical.extend(segments[index + 1..].iter().map(|segment| match segment {
                    LookupSegment::Field(field) | LookupSegment::Index(field) => field.clone(),
                }));
                break;
            }
        }
    }

    Some(normalize_path(&canonical.join(".")))
}

pub(crate) fn normalize_path(path: &str) -> String {
    path.trim_matches('.').to_owned()
}

pub(crate) fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}.{child}")
    }
}

pub(crate) fn path_matches_pattern(path: &str, pattern: &str) -> bool {
    let actual_segments = path_segments(path);
    let pattern_segments = path_segments(pattern);
    actual_segments.len() == pattern_segments.len()
        && actual_segments
            .iter()
            .zip(pattern_segments.iter())
            .all(|(actual, expected)| *expected == "*" || actual == expected)
}

pub(crate) fn path_starts_with_pattern(path: &str, pattern: &str) -> bool {
    let actual_segments = path_segments(path);
    let pattern_segments = path_segments(pattern);
    actual_segments.len() >= pattern_segments.len()
        && actual_segments
            .iter()
            .zip(pattern_segments.iter())
            .all(|(actual, expected)| *expected == "*" || actual == expected)
}

pub(crate) fn path_overlaps_pattern(path: &str, pattern: &str) -> bool {
    let actual_segments = path_segments(path);
    let pattern_segments = path_segments(pattern);
    let shared = actual_segments.len().min(pattern_segments.len());
    actual_segments
        .iter()
        .take(shared)
        .zip(pattern_segments.iter().take(shared))
        .all(|(actual, expected)| *expected == "*" || *actual == "*" || actual == expected)
}

pub(crate) fn canonicalize_path_with_aliases(
    path: &str,
    aliases: &BTreeMap<String, String>,
) -> String {
    let normalized = normalize_path(path);
    if normalized.is_empty() || aliases.is_empty() {
        return normalized;
    }

    let path_segments = normalized.split('.').collect::<Vec<_>>();
    let mut best = None::<(usize, usize, String)>;

    for (alias, canonical) in aliases {
        let alias_segments = alias.split('.').collect::<Vec<_>>();
        if alias_segments.len() > path_segments.len() {
            continue;
        }

        let matched = alias_segments
            .iter()
            .zip(path_segments.iter())
            .all(|(expected, actual)| *expected == "*" || expected == actual);
        if !matched {
            continue;
        }

        let specificity = alias_segments
            .iter()
            .filter(|segment| **segment != "*")
            .count();
        let candidate = rewrite_alias_path(&path_segments, &alias_segments, canonical);
        match &mut best {
            Some((best_len, best_specificity, best_candidate))
                if alias_segments.len() > *best_len
                    || (alias_segments.len() == *best_len && specificity > *best_specificity) =>
            {
                *best_len = alias_segments.len();
                *best_specificity = specificity;
                *best_candidate = candidate;
            }
            None => best = Some((alias_segments.len(), specificity, candidate)),
            _ => {}
        }
    }

    best.map_or(normalized, |(_, _, candidate)| candidate)
}

fn rewrite_alias_path(path_segments: &[&str], alias_segments: &[&str], canonical: &str) -> String {
    let canonical_segments = canonical
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut rewritten = canonical_segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if *segment == "*" && alias_segments.get(index) == Some(&"*") {
                path_segments[index].to_owned()
            } else {
                (*segment).to_owned()
            }
        })
        .collect::<Vec<_>>();
    rewritten.extend(
        path_segments[alias_segments.len()..]
            .iter()
            .map(|segment| (*segment).to_owned()),
    );
    normalize_path(&rewritten.join("."))
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .collect()
}

pub(crate) fn redact_value(value: &Value, path: &str, secret_paths: &BTreeSet<String>) -> Value {
    if secret_paths
        .iter()
        .any(|secret| path_starts_with_pattern(path, secret))
    {
        return Value::String("***redacted***".to_owned());
    }

    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let next = join_path(path, key);
                    (key.clone(), redact_value(value, &next, secret_paths))
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let next = join_path(path, &index.to_string());
                    redact_value(value, &next, secret_paths)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn collect_paths(value: &Value, current: &str, paths: &mut Vec<String>) {
    if !current.is_empty() {
        paths.push(current.to_owned());
    }

    if let Value::Object(map) = value {
        for (key, child) in map {
            let next = join_path(current, key);
            collect_paths(child, &next, paths);
        }
    } else if let Value::Array(values) = value {
        for (index, child) in values.iter().enumerate() {
            let next = join_path(current, &index.to_string());
            collect_paths(child, &next, paths);
        }
    }
}

pub(crate) fn get_value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(segment)?;
            }
            Value::Array(values) => {
                let index = segment.parse::<usize>().ok()?;
                current = values.get(index)?;
            }
            _ => return None,
        }
    }

    Some(current)
}

pub(crate) fn collect_diff_paths(
    before: &Value,
    after: &Value,
    current: &str,
    paths: &mut Vec<String>,
) {
    if before == after {
        return;
    }

    if !current.is_empty() {
        paths.push(current.to_owned());
    }

    if let (Value::Object(before_map), Value::Object(after_map)) = (before, after) {
        let keys = before_map
            .keys()
            .chain(after_map.keys())
            .collect::<BTreeSet<_>>();
        for key in keys {
            let before_child = before_map.get(key).unwrap_or(&Value::Null);
            let after_child = after_map.get(key).unwrap_or(&Value::Null);
            let next = join_path(current, key);
            collect_diff_paths(before_child, after_child, &next, paths);
        }
    } else if let (Value::Array(before_values), Value::Array(after_values)) = (before, after) {
        let len = before_values.len().max(after_values.len());
        for index in 0..len {
            let before_child = before_values.get(index).unwrap_or(&Value::Null);
            let after_child = after_values.get(index).unwrap_or(&Value::Null);
            let next = join_path(current, &index.to_string());
            collect_diff_paths(before_child, after_child, &next, paths);
        }
    } else {
        if matches!(before, Value::Object(_) | Value::Array(_)) {
            collect_paths(before, current, paths);
        }
        if matches!(after, Value::Object(_) | Value::Array(_)) {
            collect_paths(after, current, paths);
        }
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(inner) => inner.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_owned()),
    }
}
