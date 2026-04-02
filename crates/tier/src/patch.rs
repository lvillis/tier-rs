use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::loader::{
    indexed_array_container_paths, insert_path, record_direct_array_state,
    record_indexed_array_state, try_normalize_external_path,
};
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

/// Hidden helper used by the `TierPatch` derive macro.
#[doc(hidden)]
pub struct PatchLayerBuilder {
    trace: SourceTrace,
    value: Value,
    entries: BTreeMap<String, SourceTrace>,
    indexed_array_paths: BTreeSet<String>,
    indexed_array_base_lengths: BTreeMap<String, usize>,
    current_array_lengths: BTreeMap<String, usize>,
    direct_array_paths: BTreeSet<String>,
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
        Self {
            trace,
            value: Value::Object(Map::new()),
            entries: BTreeMap::new(),
            indexed_array_paths: BTreeSet::new(),
            indexed_array_base_lengths: BTreeMap::new(),
            current_array_lengths: BTreeMap::new(),
            direct_array_paths: BTreeSet::new(),
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
        let path =
            try_normalize_external_path(path).map_err(|message| ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path: path.to_owned(),
                message,
            })?;
        if path.is_empty() {
            return Err(ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path,
                message: "configuration path cannot be empty".to_owned(),
            });
        }

        let segments = path.split('.').collect::<Vec<_>>();
        record_indexed_array_state(
            &mut self.current_array_lengths,
            &mut self.indexed_array_base_lengths,
            &path,
            &segments,
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

        insert_path(&mut self.value, &segments, value).map_err(|message| {
            ConfigError::InvalidPatch {
                name: self.trace.name.clone(),
                path: path.clone(),
                message,
            }
        })?;
        self.indexed_array_paths
            .extend(indexed_array_container_paths(&segments));

        self.entries.insert(path.clone(), self.trace.clone());
        let mut prefix = String::new();
        for segment in segments {
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
