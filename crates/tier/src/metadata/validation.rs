use super::*;

impl ValidationNumber {
    /// Returns the numeric value as `f64`, when representable.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Finite(number) => number.as_f64(),
            Self::Invalid(_) => None,
        }
    }

    /// Returns `true` when the bound is a finite JSON-compatible number.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        matches!(self, Self::Finite(_))
    }

    /// Returns the bound rendered as a JSON value.
    #[must_use]
    pub fn as_json_value(&self) -> serde_json::Value {
        match self {
            Self::Finite(number) => serde_json::Value::Number(number.clone()),
            Self::Invalid(value) => serde_json::Value::String(value.clone()),
        }
    }
}

impl Display for ValidationNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finite(number) => Display::fmt(number, f),
            Self::Invalid(value) => Display::fmt(value, f),
        }
    }
}

impl From<i8> for ValidationNumber {
    fn from(value: i8) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i16> for ValidationNumber {
    fn from(value: i16) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i32> for ValidationNumber {
    fn from(value: i32) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<i64> for ValidationNumber {
    fn from(value: i64) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<isize> for ValidationNumber {
    fn from(value: isize) -> Self {
        Self::Finite(serde_json::Number::from(value as i64))
    }
}

impl From<u8> for ValidationNumber {
    fn from(value: u8) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u16> for ValidationNumber {
    fn from(value: u16) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u32> for ValidationNumber {
    fn from(value: u32) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<u64> for ValidationNumber {
    fn from(value: u64) -> Self {
        Self::Finite(serde_json::Number::from(value))
    }
}

impl From<usize> for ValidationNumber {
    fn from(value: usize) -> Self {
        Self::Finite(serde_json::Number::from(value as u64))
    }
}

impl From<f32> for ValidationNumber {
    fn from(value: f32) -> Self {
        match serde_json::Number::from_f64(value as f64) {
            Some(number) => Self::Finite(number),
            None => Self::Invalid(value.to_string()),
        }
    }
}

impl From<f64> for ValidationNumber {
    fn from(value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(number) => Self::Finite(number),
            None => Self::Invalid(value.to_string()),
        }
    }
}

impl Display for ValidationValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.0 {
            serde_json::Value::String(value) => write!(f, "{value:?}"),
            value => Display::fmt(value, f),
        }
    }
}

impl From<bool> for ValidationValue {
    fn from(value: bool) -> Self {
        Self(serde_json::Value::Bool(value))
    }
}

impl From<String> for ValidationValue {
    fn from(value: String) -> Self {
        Self(serde_json::Value::String(value))
    }
}

impl From<&str> for ValidationValue {
    fn from(value: &str) -> Self {
        Self(serde_json::Value::String(value.to_owned()))
    }
}

impl From<f32> for ValidationValue {
    fn from(value: f32) -> Self {
        match serde_json::Number::from_f64(value as f64) {
            Some(number) => Self(serde_json::Value::Number(number)),
            None => Self(serde_json::Value::String(value.to_string())),
        }
    }
}

impl From<f64> for ValidationValue {
    fn from(value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(number) => Self(serde_json::Value::Number(number)),
            None => Self(serde_json::Value::String(value.to_string())),
        }
    }
}

macro_rules! impl_validation_value_from_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for ValidationValue {
                fn from(value: $ty) -> Self {
                    Self(serde_json::to_value(value).expect("validation values must serialize"))
                }
            }
        )*
    };
}

impl_validation_value_from_number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl ValidationRule {
    /// Returns a stable machine-readable rule identifier.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NonEmpty => "non_empty",
            Self::Min(_) => "min",
            Self::Max(_) => "max",
            Self::MinLength(_) => "min_length",
            Self::MaxLength(_) => "max_length",
            Self::MinItems(_) => "min_items",
            Self::MaxItems(_) => "max_items",
            Self::MinProperties(_) => "min_properties",
            Self::MaxProperties(_) => "max_properties",
            Self::MultipleOf(_) => "multiple_of",
            Self::Pattern(_) => "pattern",
            Self::UniqueItems => "unique_items",
            Self::OneOf(_) => "one_of",
            Self::Hostname => "hostname",
            Self::Url => "url",
            Self::Email => "email",
            Self::IpAddr => "ip_addr",
            Self::SocketAddr => "socket_addr",
            Self::AbsolutePath => "absolute_path",
        }
    }
}

impl Display for ValidationRule {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonEmpty => write!(f, "non_empty"),
            Self::Min(value) => write!(f, "min={value}"),
            Self::Max(value) => write!(f, "max={value}"),
            Self::MinLength(value) => write!(f, "min_length={value}"),
            Self::MaxLength(value) => write!(f, "max_length={value}"),
            Self::MinItems(value) => write!(f, "min_items={value}"),
            Self::MaxItems(value) => write!(f, "max_items={value}"),
            Self::MinProperties(value) => write!(f, "min_properties={value}"),
            Self::MaxProperties(value) => write!(f, "max_properties={value}"),
            Self::MultipleOf(value) => write!(f, "multiple_of={value}"),
            Self::Pattern(value) => write!(f, "pattern={value:?}"),
            Self::UniqueItems => write!(f, "unique_items"),
            Self::OneOf(values) => write!(
                f,
                "one_of=[{}]",
                values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Hostname => write!(f, "hostname"),
            Self::Url => write!(f, "url"),
            Self::Email => write!(f, "email"),
            Self::IpAddr => write!(f, "ip_addr"),
            Self::SocketAddr => write!(f, "socket_addr"),
            Self::AbsolutePath => write!(f, "absolute_path"),
        }
    }
}

impl ValidationCheck {
    /// Returns a stable machine-readable rule identifier.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::AtLeastOneOf { .. } => "at_least_one_of",
            Self::ExactlyOneOf { .. } => "exactly_one_of",
            Self::MutuallyExclusive { .. } => "mutually_exclusive",
            Self::RequiredWith { .. } => "required_with",
            Self::RequiredIf { .. } => "required_if",
        }
    }

    pub(super) fn normalize(self) -> Option<Self> {
        match self {
            Self::AtLeastOneOf { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::AtLeastOneOf { paths })
            }
            Self::ExactlyOneOf { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::ExactlyOneOf { paths })
            }
            Self::MutuallyExclusive { paths } => {
                normalize_check_path_group(paths).map(|paths| Self::MutuallyExclusive { paths })
            }
            Self::RequiredWith { path, requires } => {
                let path = normalize_metadata_path(&path);
                let requires = normalize_check_path_group(requires)?;
                Some(Self::RequiredWith { path, requires })
            }
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => {
                let path = normalize_metadata_path(&path);
                let requires = normalize_check_path_group(requires)?;
                Some(Self::RequiredIf {
                    path,
                    equals,
                    requires,
                })
            }
        }
    }

    pub(super) fn prefixed(self, prefix: &str) -> Option<Self> {
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            try_normalize_metadata_path(prefix)
                .ok()
                .filter(|normalized| !normalized.is_empty())
                .unwrap_or_else(|| prefix.to_owned())
        };
        if prefix.is_empty() {
            return self.normalize();
        }

        let join = |path: String| {
            if path.is_empty() {
                prefix.clone()
            } else {
                format!("{prefix}.{path}")
            }
        };

        match self {
            Self::AtLeastOneOf { paths } => Some(Self::AtLeastOneOf {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::ExactlyOneOf { paths } => Some(Self::ExactlyOneOf {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::MutuallyExclusive { paths } => Some(Self::MutuallyExclusive {
                paths: paths.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::RequiredWith { path, requires } => Some(Self::RequiredWith {
                path: join(path),
                requires: requires.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => Some(Self::RequiredIf {
                path: join(path),
                equals,
                requires: requires.into_iter().map(join).collect(),
            })
            .and_then(Self::normalize),
        }
    }
}

impl Display for ValidationCheck {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtLeastOneOf { paths } => {
                write!(f, "at_least_one_of({})", paths.join(", "))
            }
            Self::ExactlyOneOf { paths } => {
                write!(f, "exactly_one_of({})", paths.join(", "))
            }
            Self::MutuallyExclusive { paths } => {
                write!(f, "mutually_exclusive({})", paths.join(", "))
            }
            Self::RequiredWith { path, requires } => {
                write!(f, "required_with({path} -> {})", requires.join(", "))
            }
            Self::RequiredIf {
                path,
                equals,
                requires,
            } => write!(
                f,
                "required_if({path} == {equals} -> {})",
                requires.join(", ")
            ),
        }
    }
}
