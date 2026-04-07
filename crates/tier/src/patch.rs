use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::loader::record_direct_array_state;
use crate::report::normalize_path;
use crate::{ConfigError, Layer, SourceKind, SourceTrace};

/// Sparse patch field wrapper used by typed override structs.
///
/// `Patch<T>` makes intent explicit:
///
/// - `Patch::Unset` means "do not touch this field"
/// - `Patch::Set(value)` means "override this field with `value`"
///
/// For optional config fields, use `Patch<Option<T>>` when the patch needs to
/// distinguish "unset" from "set this field to null".
///
/// # Examples
///
/// ```
/// use tier::Patch;
///
/// let port = Patch::set(8080);
/// assert!(port.is_set());
///
/// let untouched: Patch<u16> = Patch::Unset;
/// assert_eq!(untouched.into_option(), None);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Patch<T> {
    /// Do not override this field.
    #[default]
    Unset,
    /// Override this field with the contained value.
    Set(T),
}

impl<T> Patch<T> {
    /// Creates a patch that overrides a field with `value`.
    #[must_use]
    pub fn set(value: T) -> Self {
        Self::Set(value)
    }

    /// Returns the contained value by reference when the patch is set.
    #[must_use]
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }

    /// Returns `true` when this patch carries an override value.
    #[must_use]
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    /// Consumes the patch and returns the override value, when present.
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Unset => None,
            Self::Set(value) => Some(value),
        }
    }
}

impl<T> From<T> for Patch<T> {
    fn from(value: T) -> Self {
        Self::Set(value)
    }
}

/// Trait implemented by typed sparse override structures.
///
/// The easiest way to implement this trait is `#[derive(TierPatch)]` with the
/// `derive` feature enabled. A `TierPatch` value can then be turned into a
/// [`Layer`] or applied directly to a [`crate::ConfigLoader`].
///
/// Fields are sparse by default:
///
/// - `Option<T>` fields only write when they are `Some(...)`
/// - nested patch structs can be connected with `#[tier(nested)]`
/// - use [`Patch<Option<T>>`] when the patch must distinguish "unset" from
///   "explicitly clear this optional config field"
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "derive")] {
/// # fn main() -> Result<(), tier::ConfigError> {
/// use serde::{Deserialize, Serialize};
/// use tier::{Layer, Patch, TierPatch};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct ServerConfig {
///     port: u16,
///     tls: TlsConfig,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct TlsConfig {
///     cert: Option<String>,
/// }
///
/// #[derive(Debug, TierPatch, Default)]
/// struct ServerPatch {
///     port: Option<u16>,
///     #[tier(path_expr = tier::path!(ServerConfig.tls.cert))]
///     cert_path: Patch<Option<String>>,
/// }
///
/// let patch = ServerPatch {
///     port: Some(8443),
///     cert_path: Patch::set(None),
/// };
///
/// let _layer = Layer::from_patch("typed-cli", &patch)?;
/// # Ok(())
/// # }
/// # }
/// ```
pub trait TierPatch {
    /// Writes sparse overrides into the provided layer builder.
    fn write_layer(&self, builder: &mut PatchLayerBuilder, prefix: &str)
    -> Result<(), ConfigError>;

    /// Converts the patch into a custom configuration layer.
    ///
    /// This is useful when the patch is built separately from the loader and
    /// should be applied with [`crate::ConfigLoader::layer`].
    fn to_layer(&self, name: impl Into<String>) -> Result<Layer, ConfigError>
    where
        Self: Sized,
    {
        Layer::from_patch(name, self)
    }
}

pub(crate) struct DeferredPatchLayer {
    trace: SourceTrace,
    writes: Vec<(String, Value)>,
}

impl DeferredPatchLayer {
    pub(crate) fn into_layer_with_shape(self, shape: Value) -> Result<Layer, ConfigError> {
        let mut builder = PatchLayerBuilder::from_trace_with_shape(self.trace, shape);
        for (path, value) in self.writes {
            builder.insert_value(&path, value)?;
        }
        Ok(builder.finish())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }
}

/// Hidden helper used by the `TierPatch` derive macro.
#[doc(hidden)]
pub struct PatchLayerBuilder {
    trace: SourceTrace,
    value: Value,
    shape: Value,
    entries: BTreeMap<String, SourceTrace>,
    claimed_paths: BTreeSet<String>,
    indexed_array_paths: BTreeSet<String>,
    indexed_array_base_lengths: BTreeMap<String, usize>,
    current_array_lengths: BTreeMap<String, usize>,
    direct_array_paths: BTreeSet<String>,
    deferred_writes: Option<Vec<(String, Value)>>,
}

impl PatchLayerBuilder {
    /// Creates a builder for a synthetic patch layer.
    #[must_use]
    pub fn new(kind: SourceKind, name: impl Into<String>) -> Self {
        Self::from_trace(SourceTrace {
            kind,
            name: name.into(),
            location: None,
        })
    }

    /// Creates a builder from an explicit source trace.
    #[must_use]
    pub fn from_trace(trace: SourceTrace) -> Self {
        Self::from_trace_with_shape(trace, Value::Object(Map::new()))
    }

    #[must_use]
    pub(crate) fn from_trace_deferred(trace: SourceTrace) -> Self {
        Self {
            trace,
            value: Value::Object(Map::new()),
            shape: Value::Object(Map::new()),
            entries: BTreeMap::new(),
            claimed_paths: BTreeSet::new(),
            indexed_array_paths: BTreeSet::new(),
            indexed_array_base_lengths: BTreeMap::new(),
            current_array_lengths: BTreeMap::new(),
            direct_array_paths: BTreeSet::new(),
            deferred_writes: Some(Vec::new()),
        }
    }

    /// Creates a builder from an explicit source trace and an existing shape.
    #[must_use]
    pub fn from_trace_with_shape(trace: SourceTrace, shape: Value) -> Self {
        Self {
            trace,
            value: Value::Object(Map::new()),
            shape,
            entries: BTreeMap::new(),
            claimed_paths: BTreeSet::new(),
            indexed_array_paths: BTreeSet::new(),
            indexed_array_base_lengths: BTreeMap::new(),
            current_array_lengths: BTreeMap::new(),
            direct_array_paths: BTreeSet::new(),
            deferred_writes: None,
        }
    }

    /// Inserts a serializable leaf override.
    pub fn insert_serialized<T>(&mut self, path: &str, value: &T) -> Result<(), ConfigError>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value)?;
        self.insert_value(path, value)
    }

    /// Inserts a pre-built JSON value override.
    pub fn insert_value(&mut self, path: &str, value: Value) -> Result<(), ConfigError> {
        let (segments, explicit_array_segments) =
            parse_patch_path(path).map_err(|message| ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path: path.to_owned(),
                message,
            })?;
        if segments.is_empty() {
            return Err(ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path: String::new(),
                message: "configuration path cannot be empty".to_owned(),
            });
        }
        if let Some(writes) = &mut self.deferred_writes {
            let shape_snapshot = self.shape.clone();
            let (segments, array_segments) =
                canonicalize_patch_path(&shape_snapshot, &segments, &explicit_array_segments);
            let canonical_path = normalize_path(&segments.join("."));
            claim_patch_path(&self.trace.name, &canonical_path, &mut self.claimed_paths)?;

            let indexed_array_container_paths =
                patch_indexed_array_container_paths(&segments, &array_segments);
            record_patch_indexed_array_state(
                &mut self.current_array_lengths,
                &mut self.indexed_array_base_lengths,
                &canonical_path,
                &indexed_array_container_paths,
            );
            if value.is_array() {
                record_direct_array_state(
                    &mut self.current_array_lengths,
                    &mut self.indexed_array_base_lengths,
                    &canonical_path,
                    &value,
                );
            }
            insert_path_with_shape(
                &mut self.shape,
                Some(&shape_snapshot),
                &segments,
                &array_segments,
                0,
                value.clone(),
            )
            .map_err(|message| ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path: canonical_path,
                message,
            })?;
            writes.push((path.to_owned(), value));
            return Ok(());
        }
        let shape_snapshot = self.shape.clone();
        let (segments, array_segments) =
            canonicalize_patch_path(&shape_snapshot, &segments, &explicit_array_segments);
        let path = normalize_path(&segments.join("."));
        claim_patch_path(&self.trace.name, &path, &mut self.claimed_paths)?;

        let indexed_array_container_paths =
            patch_indexed_array_container_paths(&segments, &array_segments);
        record_patch_indexed_array_state(
            &mut self.current_array_lengths,
            &mut self.indexed_array_base_lengths,
            &path,
            &indexed_array_container_paths,
        );
        if value.is_array() {
            record_direct_array_state(
                &mut self.current_array_lengths,
                &mut self.indexed_array_base_lengths,
                &path,
                &value,
            );
            self.direct_array_paths.insert(path.clone());
        }

        insert_path_with_shape(
            &mut self.value,
            Some(&shape_snapshot),
            &segments,
            &array_segments,
            0,
            value.clone(),
        )
        .map_err(|message| ConfigError::InvalidPatch {
            name: self.trace.name.clone(),
            path: path.clone(),
            message,
        })?;
        insert_path_with_shape(
            &mut self.shape,
            Some(&shape_snapshot),
            &segments,
            &array_segments,
            0,
            value,
        )
        .map_err(|message| ConfigError::InvalidPatch {
            name: self.trace.name.clone(),
            path: path.clone(),
            message,
        })?;
        self.indexed_array_paths
            .extend(indexed_array_container_paths);

        self.entries.insert(path.clone(), self.trace.clone());
        let mut prefix = String::new();
        for segment in &segments {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(segment);
            self.entries
                .entry(prefix.clone())
                .or_insert_with(|| self.trace.clone());
        }

        Ok(())
    }

    /// Finalizes the builder into a [`Layer`].
    #[must_use]
    pub fn finish(self) -> Layer {
        Layer::from_parts(
            self.trace,
            self.value,
            self.entries,
            BTreeSet::new(),
            self.indexed_array_paths,
            self.indexed_array_base_lengths,
            self.direct_array_paths,
        )
    }

    pub(crate) fn finish_deferred(self) -> DeferredPatchLayer {
        DeferredPatchLayer {
            trace: self.trace,
            writes: self.deferred_writes.unwrap_or_default(),
        }
    }
}

/// Hidden helper used by the `TierPatch` derive macro.
#[doc(hidden)]
#[must_use]
pub fn join_patch_prefix(prefix: &str, path: impl AsRef<str>) -> String {
    let path = path.as_ref();
    match (prefix.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (true, false) => path.to_owned(),
        (false, true) => prefix.to_owned(),
        (false, false) => format!("{prefix}.{path}"),
    }
}

fn parse_patch_path(path: &str) -> Result<(Vec<String>, BTreeSet<usize>), String> {
    if path == "." {
        return Ok((Vec::new(), BTreeSet::new()));
    }

    let mut segments = Vec::new();
    let mut explicit_array_segments = BTreeSet::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    let mut after_index = false;
    let mut expecting_segment = true;
    let mut segment_index = 0usize;

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
                    let index = parse_patch_array_index(&mut chars)?;
                    explicit_array_segments.insert(segment_index);
                    segments.push(index);
                    segment_index += 1;
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
                segments.push(std::mem::take(&mut current));
                segment_index += 1;
                expecting_segment = true;
            }
            '[' => {
                if current.is_empty() {
                    return Err("array indices must follow a field name".to_owned());
                }
                segments.push(std::mem::take(&mut current));
                segment_index += 1;
                let index = parse_patch_array_index(&mut chars)?;
                explicit_array_segments.insert(segment_index);
                segments.push(index);
                segment_index += 1;
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
        segments.push(current);
    }

    Ok((segments, explicit_array_segments))
}

fn parse_patch_array_index<I>(chars: &mut std::iter::Peekable<I>) -> Result<String, String>
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

    let normalized = index
        .parse::<usize>()
        .expect("checked numeric array indices")
        .to_string();
    Ok(normalized)
}

fn canonicalize_patch_path(
    root: &Value,
    segments: &[String],
    explicit_array_segments: &BTreeSet<usize>,
) -> (Vec<String>, BTreeSet<usize>) {
    enum PatchShape<'a> {
        Value(&'a Value),
        Object,
        Array,
    }

    let mut canonical = Vec::with_capacity(segments.len());
    let mut array_segments = BTreeSet::new();
    let mut current = PatchShape::Value(root);
    let mut index = 0;

    while index < segments.len() {
        let segment = &segments[index];
        let is_last = index + 1 == segments.len();
        let next_is_explicit_array = !is_last && explicit_array_segments.contains(&(index + 1));

        match current {
            PatchShape::Value(Value::Object(map)) => {
                canonical.push(segment.clone());
                current = if is_last {
                    PatchShape::Object
                } else if let Some(next) = map.get(segment) {
                    PatchShape::Value(next)
                } else if next_is_explicit_array {
                    PatchShape::Array
                } else {
                    PatchShape::Object
                };
            }
            PatchShape::Value(Value::Array(values)) => {
                let Ok(array_index) = segment.parse::<usize>() else {
                    canonical.extend(segments[index..].iter().cloned());
                    break;
                };
                canonical.push(array_index.to_string());
                array_segments.insert(index);
                current = if is_last {
                    PatchShape::Array
                } else if let Some(next) = values.get(array_index) {
                    PatchShape::Value(next)
                } else if next_is_explicit_array {
                    PatchShape::Array
                } else {
                    PatchShape::Object
                };
            }
            PatchShape::Value(_) => {
                canonical.extend(segments[index..].iter().cloned());
                break;
            }
            PatchShape::Object => {
                canonical.push(segment.clone());
                current = if is_last {
                    PatchShape::Object
                } else if next_is_explicit_array {
                    PatchShape::Array
                } else {
                    PatchShape::Object
                };
            }
            PatchShape::Array => {
                let Ok(array_index) = segment.parse::<usize>() else {
                    canonical.extend(segments[index..].iter().cloned());
                    break;
                };
                canonical.push(array_index.to_string());
                array_segments.insert(index);
                current = if is_last || next_is_explicit_array {
                    PatchShape::Array
                } else {
                    PatchShape::Object
                };
            }
        }

        index += 1;
    }

    (canonical, array_segments)
}

fn patch_indexed_array_container_paths(
    segments: &[String],
    array_segments: &BTreeSet<usize>,
) -> BTreeSet<String> {
    array_segments
        .iter()
        .map(|index| normalize_path(&segments[..*index].join(".")))
        .collect()
}

fn record_patch_indexed_array_state(
    current_array_lengths: &mut BTreeMap<String, usize>,
    indexed_array_base_lengths: &mut BTreeMap<String, usize>,
    path: &str,
    indexed_array_container_paths: &BTreeSet<String>,
) {
    for container_path in indexed_array_container_paths {
        let Some(index) = direct_patch_child_array_index(container_path, path) else {
            continue;
        };
        let Some(current_length) = current_array_lengths.get_mut(container_path) else {
            continue;
        };

        indexed_array_base_lengths
            .entry(container_path.clone())
            .or_insert(*current_length);
        if index >= *current_length {
            *current_length = index + 1;
        }
    }
}

fn direct_patch_child_array_index(container_path: &str, entry_path: &str) -> Option<usize> {
    let remainder = if container_path.is_empty() {
        entry_path
    } else {
        entry_path.strip_prefix(container_path)?.strip_prefix('.')?
    };
    remainder.split('.').next()?.parse::<usize>().ok()
}

fn insert_path_with_shape(
    current: &mut Value,
    shape: Option<&Value>,
    segments: &[String],
    array_segments: &BTreeSet<usize>,
    depth: usize,
    value: Value,
) -> Result<(), String> {
    let segment = &segments[depth];
    if segment.is_empty() {
        return Err("configuration path contains an empty segment".to_owned());
    }

    let is_last = depth + 1 == segments.len();
    match current {
        Value::Object(map) => {
            if is_last {
                map.insert(segment.clone(), value);
                return Ok(());
            }

            let shape_child = match shape {
                Some(Value::Object(shape_map)) => shape_map.get(segment),
                _ => None,
            };
            let next_is_array = array_segments.contains(&(depth + 1));
            let child = map.entry(segment.clone()).or_insert(patch_next_container(
                shape_child,
                next_is_array,
                &segments[depth + 1],
            )?);
            ensure_patch_container(child, shape_child, next_is_array, segment)?;
            insert_path_with_shape(
                child,
                shape_child,
                segments,
                array_segments,
                depth + 1,
                value,
            )
        }
        Value::Array(values) => {
            let index = segment.parse::<usize>().map_err(|_| {
                format!("path segment {segment} must be an array index at this position")
            })?;

            if values.len() <= index {
                values.resize(index + 1, Value::Null);
            }

            if is_last {
                values[index] = value;
                return Ok(());
            }

            let shape_child = match shape {
                Some(Value::Array(shape_values)) => shape_values.get(index),
                _ => None,
            };
            if values[index].is_null() {
                let next_is_array = array_segments.contains(&(depth + 1));
                values[index] =
                    patch_next_container(shape_child, next_is_array, &segments[depth + 1])?;
            }
            let next_is_array = array_segments.contains(&(depth + 1));
            ensure_patch_container(&values[index], shape_child, next_is_array, segment)?;
            insert_path_with_shape(
                &mut values[index],
                shape_child,
                segments,
                array_segments,
                depth + 1,
                value,
            )
        }
        _ => Err(format!(
            "path segment {segment} conflicts with an existing non-container value"
        )),
    }
}

fn patch_next_container(
    shape_child: Option<&Value>,
    next_is_array: bool,
    next_segment: &str,
) -> Result<Value, String> {
    match shape_child {
        Some(Value::Object(_)) => Ok(Value::Object(Map::new())),
        Some(Value::Array(_)) => Ok(Value::Array(Vec::new())),
        Some(_) => Err(format!(
            "path segment {next_segment} conflicts with an existing non-container value"
        )),
        None => Ok(if next_is_array {
            Value::Array(Vec::new())
        } else {
            Value::Object(Map::new())
        }),
    }
}

fn ensure_patch_container(
    child: &Value,
    shape_child: Option<&Value>,
    next_is_array: bool,
    segment: &str,
) -> Result<(), String> {
    let expected_array =
        matches!(shape_child, Some(Value::Array(_))) || (shape_child.is_none() && next_is_array);
    let expected_object =
        matches!(shape_child, Some(Value::Object(_))) || (shape_child.is_none() && !next_is_array);

    match child {
        Value::Object(_) if expected_object => Ok(()),
        Value::Array(_) if expected_array => Ok(()),
        _ => Err(format!(
            "path segment {segment} conflicts with an existing non-container value"
        )),
    }
}

fn claim_patch_path(
    layer_name: &str,
    path: &str,
    claimed_paths: &mut BTreeSet<String>,
) -> Result<(), ConfigError> {
    for existing_path in claimed_paths.iter() {
        if existing_path == path {
            return Err(ConfigError::InvalidPatch {
                name: layer_name.to_owned(),
                path: path.to_owned(),
                message: format!("duplicate patch path `{path}`"),
            });
        }

        if existing_path
            .strip_prefix(path)
            .is_some_and(|suffix| suffix.starts_with('.'))
            || path
                .strip_prefix(existing_path)
                .is_some_and(|suffix| suffix.starts_with('.'))
        {
            return Err(ConfigError::InvalidPatch {
                name: layer_name.to_owned(),
                path: path.to_owned(),
                message: format!("conflicting patch paths `{existing_path}` and `{path}` overlap"),
            });
        }
    }

    claimed_paths.insert(path.to_owned());
    Ok(())
}
