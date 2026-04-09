use super::*;

pub(super) fn normalize_checks<I>(checks: I) -> Vec<ValidationCheck>
where
    I: IntoIterator<Item = ValidationCheck>,
{
    let mut normalized = Vec::new();
    for check in checks {
        let Some(check) = check.normalize() else {
            continue;
        };
        if !normalized.contains(&check) {
            normalized.push(check);
        }
    }
    normalized
}

pub(super) fn normalize_check_path_group<I>(paths: I) -> Option<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let mut normalized = Vec::new();
    for path in paths {
        let path = normalize_metadata_path(&path);
        if normalized.contains(&path) {
            continue;
        }
        normalized.push(path);
    }
    (!normalized.is_empty()).then_some(normalized)
}

pub(super) fn normalize_metadata_path(path: &str) -> String {
    try_normalize_metadata_path(path).unwrap_or_else(|_| path.to_owned())
}

pub(super) fn validate_metadata_path(path: &str) -> Result<(), ConfigError> {
    try_normalize_metadata_path(path)
        .map(|_| ())
        .map_err(|message| ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: format!("invalid metadata path: {message}"),
        })
}

pub(super) fn validate_check_path(path: &str) -> Result<(), ConfigError> {
    validate_metadata_path(path)?;
    if normalize_metadata_path(path).is_empty() {
        return Err(ConfigError::MetadataInvalid {
            path: path.to_owned(),
            message: "invalid metadata path: cross-field checks cannot use the root path"
                .to_owned(),
        });
    }
    Ok(())
}

pub(super) fn try_normalize_metadata_path(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Ok(String::new());
    }
    if path == "." {
        return Ok(String::new());
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
                    let index = parse_metadata_array_index(&mut chars)?;
                    segments.push(index);
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
                expecting_segment = true;
            }
            '[' => {
                if current.is_empty() {
                    return Err("array indices must follow a field name".to_owned());
                }
                segments.push(std::mem::take(&mut current));
                let index = parse_metadata_array_index(&mut chars)?;
                segments.push(index);
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

    for segment in &segments {
        if segment.contains('*') && segment != "*" {
            return Err("wildcard path segments must be exactly `*`".to_owned());
        }
    }

    Ok(segments.join("."))
}

fn parse_metadata_array_index<I>(chars: &mut std::iter::Peekable<I>) -> Result<String, String>
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

    Ok(index
        .parse::<usize>()
        .expect("checked numeric array indices")
        .to_string())
}

pub(super) fn metadata_match_score(path: &str, candidate: &str) -> Option<MetadataMatchScore> {
    if candidate != path && !path_matches_pattern(path, candidate) {
        return None;
    }

    let segments = candidate
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let positional_specificity = segments
        .iter()
        .map(|segment| *segment != "*")
        .collect::<Vec<_>>();
    let specificity = positional_specificity
        .iter()
        .filter(|segment| **segment)
        .count();
    Some(MetadataMatchScore {
        segment_count: segments.len(),
        specificity,
        positional_specificity,
    })
}

pub(super) fn alias_mapping_is_lossless(alias: &str, canonical: &str) -> bool {
    let alias_segments = path_segments(alias);
    let canonical_segments = path_segments(canonical);
    if canonical_segments.len() < alias_segments.len() {
        return false;
    }

    for index in 0..alias_segments.len() {
        let alias_wildcard = alias_segments[index] == "*";
        let canonical_wildcard = canonical_segments[index] == "*";
        if alias_wildcard != canonical_wildcard {
            return false;
        }
    }

    !canonical_segments[alias_segments.len()..].contains(&"*")
}

pub(super) fn alias_patterns_are_ambiguous(
    left_alias: &str,
    left_canonical: &str,
    right_alias: &str,
    right_canonical: &str,
) -> bool {
    if alias_rank(left_alias) != alias_rank(right_alias) {
        return false;
    }

    let left_segments = path_segments(left_alias);
    let right_segments = path_segments(right_alias);
    if left_segments.len() != right_segments.len() {
        return false;
    }

    if !left_segments
        .iter()
        .zip(right_segments.iter())
        .all(|(left, right)| *left == "*" || *right == "*" || left == right)
    {
        return false;
    }

    let sample_path = alias_overlap_sample_path(left_alias, right_alias);
    rewrite_alias_sample(&sample_path, left_alias, left_canonical)
        != rewrite_alias_sample(&sample_path, right_alias, right_canonical)
}

fn alias_rank(alias: &str) -> (usize, usize) {
    let segments = path_segments(alias);
    let specificity = segments.iter().filter(|segment| **segment != "*").count();
    (segments.len(), specificity)
}

pub(super) fn alias_overlap_sample_path(left: &str, right: &str) -> String {
    path_segments(left)
        .into_iter()
        .zip(path_segments(right))
        .map(|(left, right)| {
            if left == "*" && right == "*" {
                "item".to_owned()
            } else if left == "*" {
                right.to_owned()
            } else {
                left.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn rewrite_alias_sample(path: &str, alias: &str, canonical: &str) -> String {
    let concrete_segments = path_segments(path);
    let alias_segments = path_segments(alias);
    let canonical_segments = path_segments(canonical);

    let mut rewritten = canonical_segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if *segment == "*" && alias_segments.get(index) == Some(&"*") {
                concrete_segments[index].to_owned()
            } else {
                (*segment).to_owned()
            }
        })
        .collect::<Vec<_>>();
    rewritten.extend(
        concrete_segments[alias_segments.len()..]
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
