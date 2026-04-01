use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fmt::{self, Display, Formatter};

use serde_json::Value;

use crate::error::UnknownField;
use crate::loader::SourceTrace;

/// Stable version tag for machine-readable doctor and audit reports.
pub const REPORT_FORMAT_VERSION: u32 = 1;

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
}

impl Display for ConfigWarning {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField(field) => Display::fmt(field, f),
            Self::DeprecatedField(field) => Display::fmt(field, f),
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
    traces: BTreeMap<String, Vec<ResolutionStep>>,
    applied_sources: Vec<SourceTrace>,
    validations: Vec<String>,
    warnings: Vec<ConfigWarning>,
}

impl ConfigReport {
    pub(crate) fn new(final_value: Value, secret_paths: BTreeSet<String>) -> Self {
        Self {
            final_value,
            secret_paths,
            traces: BTreeMap::new(),
            applied_sources: Vec::new(),
            validations: Vec::new(),
            warnings: Vec::new(),
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

    pub(crate) fn record_validation(&mut self, name: String) {
        self.validations.push(name);
    }

    pub(crate) fn record_warning(&mut self, warning: ConfigWarning) {
        self.warnings.push(warning);
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
        let normalized = normalize_path(path);
        let steps = self.traces.get(&normalized)?.clone();
        let redacted = self.is_secret_path(&normalized);
        let final_value = get_value_at_path(&self.final_value, &normalized)
            .cloned()
            .map(|value| {
                if redacted {
                    redact_value(&value, &normalized, &self.secret_paths)
                } else {
                    value
                }
            });

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
        self.traces
            .get(path)
            .and_then(|steps| steps.last())
            .map(|step| step.source.clone())
    }

    fn is_secret_path(&self, path: &str) -> bool {
        self.secret_paths
            .iter()
            .any(|secret| path == secret || path.starts_with(&format!("{secret}.")))
    }
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

pub(crate) fn redact_value(value: &Value, path: &str, secret_paths: &BTreeSet<String>) -> Value {
    if secret_paths.iter().any(|secret| {
        path == secret || (!path.is_empty() && path.starts_with(&format!("{secret}.")))
    }) {
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
        Value::Array(values) => Value::Array(values.clone()),
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
    }
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(inner) => inner.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_owned()),
    }
}
