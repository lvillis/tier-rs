use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::BuildHasher;

/// Hidden helper trait used by [`path_pattern!`](macro@crate::path_pattern).
#[doc(hidden)]
pub trait PathPatternItem {
    /// Item type yielded by the collection-like path segment.
    type Item;
}

impl<T> PathPatternItem for Vec<T> {
    type Item = T;
}

impl<T, const N: usize> PathPatternItem for [T; N] {
    type Item = T;
}

impl<K, V> PathPatternItem for BTreeMap<K, V> {
    type Item = V;
}

impl<K, V, S> PathPatternItem for HashMap<K, V, S>
where
    S: BuildHasher,
{
    type Item = V;
}

impl<T> PathPatternItem for BTreeSet<T> {
    type Item = T;
}

impl<T, S> PathPatternItem for HashSet<T, S>
where
    S: BuildHasher,
{
    type Item = T;
}

/// Hidden helper used by [`path_pattern!`](macro@crate::path_pattern).
#[doc(hidden)]
#[must_use]
pub fn pattern_item_ref<C>(_: &C) -> Option<&C::Item>
where
    C: PathPatternItem,
{
    None
}

/// Hidden helper used by path macros to normalize `stringify!` output.
#[doc(hidden)]
#[must_use]
pub fn normalize_macro_path(path: &str) -> String {
    path.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Builds a compile-time checked dot path from a config type.
///
/// This macro keeps field names refactor-safe for runtime APIs that still
/// accept string paths, such as manual metadata, validators, `env_decoder`,
/// and report lookup helpers.
///
/// ```rust
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct DbConfig {
///     token: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     db: DbConfig,
/// }
///
/// assert_eq!(tier::path!(AppConfig.db.token), "db.token");
/// ```
#[macro_export]
macro_rules! path {
    ($root:ident $(:: $root_tail:ident)* . $segment:tt $(. $rest:tt)*) => {{
        let _ = |__tier_value: &$root $(:: $root_tail)*| {
            let _ = &$crate::__tier_path_check!((__tier_value).$segment $(. $rest)*);
        };
        $crate::path::normalize_macro_path(stringify!($segment $(. $rest)*))
    }};
}

/// Builds a compile-time checked wildcard path from a config type.
///
/// Use this macro for collection item paths such as `services.*.token`.
/// Wildcard segments are type-checked against common collection containers
/// like `Vec<T>`, arrays, maps, and sets.
///
/// ```rust
/// use std::collections::BTreeMap;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct ServiceConfig {
///     token: String,
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     services: BTreeMap<String, ServiceConfig>,
/// }
///
/// assert_eq!(tier::path_pattern!(AppConfig.services.*.token), "services.*.token");
/// ```
#[macro_export]
macro_rules! path_pattern {
    ($root:ident $(:: $root_tail:ident)* . $segment:tt $(. $rest:tt)*) => {{
        let _ = |__tier_value: &$root $(:: $root_tail)*| {
            $crate::__tier_path_pattern_check!((__tier_value).$segment $(. $rest)*);
        };
        $crate::path::normalize_macro_path(stringify!($segment $(. $rest)*))
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tier_path_check {
    (($value:expr) . $segment:tt) => {
        $value.$segment
    };
    (($value:expr) . $segment:tt $(. $rest:tt)+) => {
        $crate::__tier_path_check!(($value.$segment) $(. $rest)+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tier_path_pattern_check {
    (($value:expr) . *) => {
        if false {
            let _ = $crate::path::pattern_item_ref($value);
        }
    };
    (($value:expr) . * $(. $rest:tt)+) => {
        if false {
            if let Some(__tier_item) = $crate::path::pattern_item_ref($value) {
                $crate::__tier_path_pattern_check!((__tier_item) $(. $rest)+);
            }
        }
    };
    (($value:expr) . $segment:tt) => {
        if false {
            let _ = &$value.$segment;
        }
    };
    (($value:expr) . $segment:tt $(. $rest:tt)+) => {
        $crate::__tier_path_pattern_check!((&$value.$segment) $(. $rest)+);
    };
}
