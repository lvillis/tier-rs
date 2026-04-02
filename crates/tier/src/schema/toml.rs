use super::*;

pub(super) fn render_example_toml(value: &Value, metadata: &ConfigMetadata) -> String {
    let mut output = String::new();
    render_toml_root_comments(metadata, &mut output);
    match value {
        Value::Object(root) => render_toml_table("", "", root, metadata, &mut output),
        other => render_toml_root_value(other, metadata, &mut output),
    }

    if !output.ends_with('\n') {
        output.push('\n');
    }

    output
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_root_value(
    value: &Value,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    if matches!(value, Value::Array(_)) {
        render_toml_inline_array_item_comments("", metadata, output);
    }

    output.push_str(
        "# root values are rendered as comments because TOML documents require a table at the top level\n",
    );
    output.push_str("# ");
    output.push_str(&toml_inline_value(value));
    output.push('\n');
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_root_comments(metadata: &ConfigMetadata, output: &mut String) {
    if metadata.checks().is_empty() {
        return;
    }

    for check in metadata.checks() {
        output.push_str("# validate: ");
        output.push_str(&check.to_string());
        output.push('\n');
    }

    output.push('\n');
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_table(
    display_path: &str,
    metadata_path: &str,
    table: &serde_json::Map<String, Value>,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    let mut nested = Vec::new();

    for (key, value) in table {
        if value.is_null() {
            continue;
        }

        if is_nested_toml_value(value) {
            nested.push((key.as_str(), value));
            continue;
        }

        let metadata_child_path =
            resolve_toml_object_child_metadata_path(metadata_path, key, metadata);
        render_toml_comments(&metadata_child_path, metadata, output);
        if matches!(value, Value::Array(_)) {
            render_toml_inline_array_item_comments(&metadata_child_path, metadata, output);
        }
        output.push_str(&toml_key(key));
        output.push_str(" = ");
        output.push_str(&toml_inline_value(value));
        output.push('\n');
    }

    let nested_count = nested.len();
    for (index, (key, value)) in nested.into_iter().enumerate() {
        let display_child_path = join_metadata_path(display_path, key);
        let metadata_child_path =
            resolve_toml_object_child_metadata_path(metadata_path, key, metadata);
        match value {
            Value::Object(child) => {
                start_toml_section(output);
                render_toml_comments(&metadata_child_path, metadata, output);
                output.push('[');
                output.push_str(&toml_table_name(&display_child_path));
                output.push_str("]\n");
                render_toml_table(
                    &display_child_path,
                    &metadata_child_path,
                    child,
                    metadata,
                    output,
                );
            }
            Value::Array(items) => {
                for (item_index, item) in items.iter().enumerate() {
                    let Some(child) = item.as_object() else {
                        continue;
                    };
                    let item_metadata_path = resolve_toml_array_item_metadata_path(
                        &metadata_child_path,
                        item_index,
                        metadata,
                    );
                    start_toml_section(output);
                    if item_index == 0 {
                        render_toml_comments(&metadata_child_path, metadata, output);
                    }
                    render_toml_array_table_item_comments(&item_metadata_path, metadata, output);
                    output.push_str("[[");
                    output.push_str(&toml_table_name(&display_child_path));
                    output.push_str("]]\n");
                    render_toml_table(
                        &display_child_path,
                        &item_metadata_path,
                        child,
                        metadata,
                        output,
                    );
                }
            }
            _ => {}
        }

        if index + 1 < nested_count && !output.ends_with("\n\n") {
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_comments(path: &str, metadata: &ConfigMetadata, output: &mut String) {
    for field in toml_comment_fields(path, metadata) {
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_array_table_item_comments(
    path: &str,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    for field in toml_comment_fields(path, metadata) {
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
pub(super) fn render_toml_inline_array_item_comments(
    path: &str,
    metadata: &ConfigMetadata,
    output: &mut String,
) {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}.")
    };
    let mut wildcard = None::<&FieldMetadata>;
    let mut indexed = BTreeMap::<Vec<TomlArrayCommentSegment>, &FieldMetadata>::new();
    for field in metadata.fields() {
        let Some(rest) = field.path.strip_prefix(&prefix) else {
            continue;
        };
        if rest == "*" {
            wildcard = Some(field);
            continue;
        }
        let segments = rest
            .split('.')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            continue;
        }
        if !segments
            .iter()
            .all(|segment| *segment == "*" || segment.parse::<usize>().is_ok())
        {
            continue;
        }
        indexed.insert(parse_toml_comment_segments(&segments), field);
    }

    if let Some(field) = wildcard {
        for line in toml_comment_lines(field) {
            output.push_str("# [*] ");
            output.push_str(&line);
            output.push('\n');
        }
    }

    for (segments, field) in indexed {
        let marker = segments
            .iter()
            .map(|segment| match segment {
                TomlArrayCommentSegment::Wildcard => "[*]".to_owned(),
                TomlArrayCommentSegment::Index(index) => format!("[{index}]"),
            })
            .collect::<String>();
        for line in toml_comment_lines(field) {
            output.push_str("# ");
            output.push_str(&marker);
            output.push(' ');
            output.push_str(&line);
            output.push('\n');
        }
    }
}

#[cfg(feature = "toml")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TomlArrayCommentSegment {
    Wildcard,
    Index(usize),
}

#[cfg(feature = "toml")]
pub(super) fn parse_toml_comment_segments(segments: &[&str]) -> Vec<TomlArrayCommentSegment> {
    segments
        .iter()
        .map(|segment| {
            if *segment == "*" {
                TomlArrayCommentSegment::Wildcard
            } else {
                TomlArrayCommentSegment::Index(
                    segment
                        .parse::<usize>()
                        .expect("validated numeric array comment index"),
                )
            }
        })
        .collect()
}

#[cfg(feature = "toml")]
pub(super) fn toml_comment_sort_key(path: &str) -> (usize, usize) {
    let segments = path.split('.').filter(|segment| !segment.is_empty());
    let total = segments.clone().count();
    let specificity = segments.filter(|segment| *segment != "*").count();
    (specificity, total)
}

#[cfg(feature = "toml")]
pub(super) fn toml_comment_fields<'a>(
    path: &str,
    metadata: &'a ConfigMetadata,
) -> Vec<&'a FieldMetadata> {
    let mut fields = metadata
        .fields()
        .iter()
        .filter(|field| field.path == path || path_matches_pattern(path, &field.path))
        .collect::<Vec<_>>();
    fields.sort_by(|left, right| {
        toml_comment_sort_key(&left.path)
            .cmp(&toml_comment_sort_key(&right.path))
            .then_with(|| left.path.cmp(&right.path))
    });
    fields
}

#[cfg(feature = "toml")]
pub(super) fn toml_comment_lines(field: &FieldMetadata) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(doc) = &field.doc {
        lines.extend(
            doc.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned),
        );
    }
    if let Some(env) = &field.env {
        lines.push(format!("env: {env}"));
    }
    if !field.aliases.is_empty() {
        lines.push(format!("aliases: {}", field.aliases.join(", ")));
    }
    if field.has_default {
        lines.push("default: provided by serde".to_owned());
    }
    if field.merge != crate::MergeStrategy::Merge {
        lines.push(format!("merge: {}", field.merge));
    }
    if !field.validations.is_empty() {
        lines.push(format!(
            "validate: {}",
            field
                .validations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if field.secret {
        lines.push("secret: true".to_owned());
    }
    if let Some(note) = &field.deprecated {
        lines.push(format!("deprecated: {note}"));
    }
    lines
}

#[cfg(feature = "toml")]
pub(super) fn resolve_toml_array_item_metadata_path(
    parent_path: &str,
    index: usize,
    metadata: &ConfigMetadata,
) -> String {
    let concrete = join_metadata_path(parent_path, &index.to_string());
    if metadata_has_path_or_descendant(metadata, &concrete) {
        concrete
    } else {
        join_metadata_path(parent_path, "*")
    }
}

#[cfg(feature = "toml")]
pub(super) fn metadata_has_path_or_descendant(metadata: &ConfigMetadata, path: &str) -> bool {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}.")
    };
    metadata
        .fields()
        .iter()
        .any(|field| field.path == path || field.path.starts_with(&prefix))
}

#[cfg(feature = "toml")]
pub(super) fn start_toml_section(output: &mut String) {
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
}

#[cfg(feature = "toml")]
pub(super) fn is_nested_toml_value(value: &Value) -> bool {
    match value {
        Value::Object(_) => true,
        Value::Array(items) => !items.is_empty() && items.iter().all(Value::is_object),
        _ => false,
    }
}

#[cfg(feature = "toml")]
pub(super) fn toml_inline_value(value: &Value) -> String {
    match value {
        Value::Null => toml_string("<unset>"),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => toml_string(string),
        Value::Array(items) => {
            let rendered = items
                .iter()
                .map(toml_inline_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{rendered}]")
        }
        Value::Object(map) => {
            let rendered = map
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| format!("{} = {}", toml_key(key), toml_inline_value(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {rendered} }}")
        }
    }
}

#[cfg(feature = "toml")]
pub(super) fn toml_table_name(path: &str) -> String {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .map(toml_key)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(feature = "toml")]
pub(super) fn toml_key(segment: &str) -> String {
    if segment
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        segment.to_owned()
    } else {
        toml_string(segment)
    }
}

#[cfg(feature = "toml")]
pub(super) fn toml_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            control if control.is_control() => {
                let code = control as u32;
                output.push_str(&format!("\\u{:04X}", code));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

#[cfg(feature = "toml")]
pub(super) fn join_metadata_path(parent: &str, segment: &str) -> String {
    if parent.is_empty() {
        segment.to_owned()
    } else {
        format!("{parent}.{segment}")
    }
}

#[cfg(feature = "toml")]
pub(super) fn resolve_toml_object_child_metadata_path(
    parent_path: &str,
    key: &str,
    metadata: &ConfigMetadata,
) -> String {
    let literal = join_metadata_path(parent_path, key);
    if key != "{item}" || metadata_has_path_or_descendant(metadata, &literal) {
        return literal;
    }

    join_metadata_path(parent_path, "*")
}
