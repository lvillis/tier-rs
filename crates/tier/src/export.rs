use serde::Serialize;
use serde_json::Value;

pub(crate) fn json_value<T>(value: &T, fallback: Value) -> Value
where
    T: Serialize,
{
    serde_json::to_value(value).unwrap_or(fallback)
}

pub(crate) fn json_pretty<T>(value: &T, fallback: &str) -> String
where
    T: Serialize,
{
    serde_json::to_string_pretty(value).unwrap_or_else(|_| fallback.to_owned())
}
