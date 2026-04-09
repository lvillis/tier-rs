use super::*;
use serde::de::{DeserializeSeed, Deserializer, Error as _, Unexpected};

pub(super) fn deserialize_with_path<T>(
    value: &Value,
    report: &ConfigReport,
    string_coercion_paths: &BTreeSet<String>,
) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    let deserialize_attempt = |value: &Value| {
        let deserializer = CoercingDeserializer::new(value, "", string_coercion_paths, None, None);
        let result: Result<T, serde_path_to_error::Error<ValueDeError>> =
            serde_path_to_error::deserialize(deserializer);
        result
    };

    match deserialize_attempt(value) {
        Ok(config) => Ok(config),
        Err(error) => {
            let retry_value = coerce_retry_scalars(value, "", string_coercion_paths);
            if retry_value != *value
                && let Ok(config) = deserialize_attempt(&retry_value)
            {
                return Ok(config);
            }
            Err(deserialization_error(report, error))
        }
    }
}

fn deserialization_error(
    report: &ConfigReport,
    error: serde_path_to_error::Error<ValueDeError>,
) -> ConfigError {
    let path = error.path().to_string();
    let lookup_path = normalize_external_path(&path);
    let source = find_source_for_unknown_path(report, &lookup_path);
    ConfigError::Deserialize {
        path,
        provenance: source,
        message: error.inner().to_string(),
    }
}

fn unexpected_value(value: &Value) -> Unexpected<'_> {
    match value {
        Value::Null => Unexpected::Unit,
        Value::Bool(value) => Unexpected::Bool(*value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Unexpected::Signed(value)
            } else if let Some(value) = number.as_u64() {
                Unexpected::Unsigned(value)
            } else if let Some(value) = number.as_f64() {
                Unexpected::Float(value)
            } else {
                Unexpected::Other("number")
            }
        }
        Value::String(value) => Unexpected::Str(value),
        Value::Array(_) => Unexpected::Other("array"),
        Value::Object(_) => Unexpected::Other("object"),
    }
}

pub(super) struct CoercingDeserializer<'a> {
    value: &'a Value,
    path: String,
    string_coercion_paths: &'a BTreeSet<String>,
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a> CoercingDeserializer<'a> {
    pub(super) fn new(
        value: &'a Value,
        path: impl Into<String>,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            value,
            path: path.into(),
            string_coercion_paths,
            known_paths,
            ignored_paths,
        }
    }

    fn coercible_string(&self) -> Option<&'a str> {
        match self.value {
            Value::String(value) if self.string_coercion_paths.contains(&self.path) => Some(value),
            _ => None,
        }
    }

    fn invalid_type<'de, V>(&self, visitor: &V) -> ValueDeError
    where
        V: Visitor<'de>,
    {
        ValueDeError::invalid_type(unexpected_value(self.value), visitor)
    }

    fn invalid_string_type<'de, V>(&self, raw: &str, visitor: &V) -> ValueDeError
    where
        V: Visitor<'de>,
    {
        ValueDeError::invalid_type(Unexpected::Str(raw), visitor)
    }

    fn record_known_path(&self, path: &str) {
        if let Some(known_paths) = self.known_paths {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                known_paths.borrow_mut().insert(normalized);
            }
        }
    }

    fn record_ignored_path(&self, path: &str) {
        if let Some(ignored_paths) = self.ignored_paths {
            let normalized = normalize_path(path);
            if !normalized.is_empty() {
                ignored_paths.borrow_mut().push(normalized);
            }
        }
    }
}

macro_rules! deserialize_integer_from_value {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            if let Some(raw) = self.coercible_string() {
                return raw
                    .trim()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_string_type(raw, &visitor))
                    .and_then(|value| visitor.$visit(value));
            }

            match self.value {
                Value::Number(number) => number
                    .to_string()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_type(&visitor))
                    .and_then(|value| visitor.$visit(value)),
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

macro_rules! deserialize_float_from_value {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            if let Some(raw) = self.coercible_string() {
                return raw
                    .trim()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_string_type(raw, &visitor))
                    .and_then(|value| visitor.$visit(value));
            }

            match self.value {
                Value::Number(number) => number
                    .to_string()
                    .parse::<$ty>()
                    .map_err(|_| self.invalid_type(&visitor))
                    .and_then(|value| visitor.$visit(value)),
                _ => Err(self.invalid_type(&visitor)),
            }
        }
    };
}

impl<'de, 'a> Deserializer<'de> for CoercingDeserializer<'a>
where
    'a: 'de,
{
    type Error = ValueDeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(value) => visitor.visit_bool(*value),
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    visitor.visit_i64(value)
                } else if let Some(value) = number.as_u64() {
                    visitor.visit_u64(value)
                } else if let Some(value) = number.as_f64() {
                    visitor.visit_f64(value)
                } else {
                    Err(self.invalid_type(&visitor))
                }
            }
            Value::String(value) => visitor.visit_borrowed_str(value),
            Value::Array(values) => visitor.visit_seq(CoercingSeqAccess::new(
                values.iter().enumerate(),
                self.path,
                self.string_coercion_paths,
                self.known_paths,
                self.ignored_paths,
            )),
            Value::Object(map) => visitor.visit_map(CoercingMapAccess::new(
                map.iter(),
                self.path,
                self.string_coercion_paths,
                self.known_paths,
                self.ignored_paths,
            )),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if let Some(raw) = self.coercible_string() {
            return match raw.trim() {
                "true" => visitor.visit_bool(true),
                "false" => visitor.visit_bool(false),
                _ => Err(self.invalid_string_type(raw, &visitor)),
            };
        }

        match self.value {
            Value::Bool(value) => visitor.visit_bool(*value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    deserialize_integer_from_value!(deserialize_i8, visit_i8, i8);
    deserialize_integer_from_value!(deserialize_i16, visit_i16, i16);
    deserialize_integer_from_value!(deserialize_i32, visit_i32, i32);
    deserialize_integer_from_value!(deserialize_i64, visit_i64, i64);
    deserialize_integer_from_value!(deserialize_i128, visit_i128, i128);
    deserialize_integer_from_value!(deserialize_u8, visit_u8, u8);
    deserialize_integer_from_value!(deserialize_u16, visit_u16, u16);
    deserialize_integer_from_value!(deserialize_u32, visit_u32, u32);
    deserialize_integer_from_value!(deserialize_u64, visit_u64, u64);
    deserialize_integer_from_value!(deserialize_u128, visit_u128, u128);
    deserialize_float_from_value!(deserialize_f32, visit_f32, f32);
    deserialize_float_from_value!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let Value::String(value) = self.value else {
            return Err(self.invalid_type(&visitor));
        };
        let mut chars = value.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => visitor.visit_char(ch),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_borrowed_str(value),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_string(value.clone()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_borrowed_bytes(value.as_bytes()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_byte_buf(value.as_bytes().to_vec()),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if matches!(self.value, Value::Null) {
            return visitor.visit_none();
        }

        if let Some(raw) = self.coercible_string()
            && raw.trim() == "null"
        {
            return visitor.visit_none();
        }

        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if matches!(self.value, Value::Null) {
            return visitor.visit_unit();
        }

        if let Some(raw) = self.coercible_string()
            && raw.trim() == "null"
        {
            return visitor.visit_unit();
        }

        Err(self.invalid_type(&visitor))
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Array(values) => visitor.visit_seq(CoercingSeqAccess::new(
                values.iter().enumerate(),
                self.path,
                self.string_coercion_paths,
                self.known_paths,
                self.ignored_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if let Value::Array(values) = self.value {
            for index in 0.._len {
                self.record_known_path(&join_path(&self.path, &index.to_string()));
            }
            for index in _len..values.len() {
                self.record_ignored_path(&join_path(&self.path, &index.to_string()));
            }
        }
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(_len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::Object(map) => visitor.visit_map(CoercingMapAccess::new(
                map.iter(),
                self.path,
                self.string_coercion_paths,
                self.known_paths,
                self.ignored_paths,
            )),
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        for field in fields {
            self.record_known_path(&join_path(&self.path, field));
        }
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Value::String(value) => visitor.visit_enum(value.as_str().into_deserializer()),
            Value::Object(map) => {
                visitor.visit_enum(MapAccessDeserializer::new(CoercingMapAccess::new(
                    map.iter(),
                    self.path,
                    self.string_coercion_paths,
                    self.known_paths,
                    self.ignored_paths,
                )))
            }
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }
}

struct CoercingSeqAccess<'a, I> {
    iter: I,
    parent_path: String,
    string_coercion_paths: &'a BTreeSet<String>,
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a, I> CoercingSeqAccess<'a, I> {
    fn new(
        iter: I,
        parent_path: String,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            iter,
            parent_path,
            string_coercion_paths,
            known_paths,
            ignored_paths,
        }
    }
}

impl<'de, 'a, I> SeqAccess<'de> for CoercingSeqAccess<'a, I>
where
    'a: 'de,
    I: Iterator<Item = (usize, &'a Value)>,
{
    type Error = ValueDeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let Some((index, value)) = self.iter.next() else {
            return Ok(None);
        };
        let path = join_path(&self.parent_path, &index.to_string());
        seed.deserialize(CoercingDeserializer::new(
            value,
            path,
            self.string_coercion_paths,
            self.known_paths,
            self.ignored_paths,
        ))
        .map(Some)
    }
}

struct CoercingMapAccess<'a, I> {
    iter: I,
    current: Option<(&'a str, &'a Value)>,
    parent_path: String,
    string_coercion_paths: &'a BTreeSet<String>,
    known_paths: Option<&'a RefCell<BTreeSet<String>>>,
    ignored_paths: Option<&'a RefCell<Vec<String>>>,
}

impl<'a, I> CoercingMapAccess<'a, I> {
    fn new(
        iter: I,
        parent_path: String,
        string_coercion_paths: &'a BTreeSet<String>,
        known_paths: Option<&'a RefCell<BTreeSet<String>>>,
        ignored_paths: Option<&'a RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            iter,
            current: None,
            parent_path,
            string_coercion_paths,
            known_paths,
            ignored_paths,
        }
    }
}

impl<'de, 'a, I> MapAccess<'de> for CoercingMapAccess<'a, I>
where
    'a: 'de,
    I: Iterator<Item = (&'a String, &'a Value)>,
{
    type Error = ValueDeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.iter.next() else {
            return Ok(None);
        };
        self.current = Some((key.as_str(), value));
        seed.deserialize(key.as_str().into_deserializer()).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let (key, value) = self
            .current
            .take()
            .expect("map value requested before key was deserialized");
        let path = join_path(&self.parent_path, key);
        seed.deserialize(CoercingDeserializer::new(
            value,
            path,
            self.string_coercion_paths,
            self.known_paths,
            self.ignored_paths,
        ))
    }
}

pub(crate) fn insert_path(root: &mut Value, segments: &[&str], value: Value) -> Result<(), String> {
    if segments.is_empty() {
        return Err("configuration path cannot be empty".to_owned());
    }

    insert_path_recursive(root, segments, value)
}

fn insert_path_recursive(
    current: &mut Value,
    segments: &[&str],
    value: Value,
) -> Result<(), String> {
    let segment = segments[0];
    if segment.is_empty() {
        return Err("configuration path contains an empty segment".to_owned());
    }

    let is_last = segments.len() == 1;
    match current {
        Value::Object(map) => {
            if is_last {
                map.insert(segment.to_owned(), value);
                return Ok(());
            }

            let next_is_index = segments[1].parse::<usize>().is_ok();
            let child = map.entry(segment.to_owned()).or_insert_with(|| {
                if next_is_index {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                }
            });

            match child {
                Value::Object(_) if !next_is_index => {}
                Value::Array(_) if next_is_index => {}
                _ => {
                    return Err(format!(
                        "path segment {segment} conflicts with an existing non-container value"
                    ));
                }
            }

            insert_path_recursive(child, &segments[1..], value)
        }
        Value::Array(values) => {
            let index = segment.parse::<usize>().map_err(|_| {
                format!("path segment {segment} must be an array index at this position")
            })?;

            if is_last {
                if values.len() <= index {
                    values.resize(index + 1, Value::Null);
                }
                values[index] = value;
                return Ok(());
            }

            let next_is_index = segments[1].parse::<usize>().is_ok();
            if values.len() <= index {
                values.resize_with(index + 1, || {
                    if next_is_index {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(Map::new())
                    }
                });
            }

            let child = &mut values[index];
            if child.is_null() {
                *child = if next_is_index {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(Map::new())
                };
            }

            match child {
                Value::Object(_) if !next_is_index => {}
                Value::Array(_) if next_is_index => {}
                _ => {
                    return Err(format!(
                        "path segment {segment} conflicts with an existing non-container value"
                    ));
                }
            }

            insert_path_recursive(child, &segments[1..], value)
        }
        _ => Err(format!(
            "path segment {segment} conflicts with an existing non-container value"
        )),
    }
}
