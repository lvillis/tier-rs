use super::*;

pub(super) fn collect_known_paths<T>(config: &T) -> Result<BTreeSet<String>, ConfigError>
where
    T: Serialize,
{
    let value = serde_json::to_value(config)?;
    Ok(collect_known_paths_from_value(&value))
}

pub(super) fn collect_known_paths_from_value(value: &Value) -> BTreeSet<String> {
    let mut paths = Vec::new();
    collect_paths(value, "", &mut paths);
    paths.into_iter().collect()
}

pub(super) fn collect_suggestion_paths(
    metadata: &ConfigMetadata,
    known_paths: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut candidates = BTreeMap::new();

    for field in metadata.fields() {
        candidates.insert(field.path.clone(), field.path.clone());
        for alias in &field.aliases {
            candidates.insert(alias.clone(), field.path.clone());
        }
    }

    if candidates.is_empty() {
        for path in known_paths {
            candidates.insert(path.clone(), path.clone());
        }
    } else {
        for path in known_paths {
            candidates
                .entry(path.clone())
                .or_insert_with(|| path.clone());
        }
    }

    candidates
}

pub(super) fn collect_unknown_fields_from_metadata_scope(
    value: &Value,
    metadata: &ConfigMetadata,
    suggestion_paths: &BTreeMap<String, String>,
    report: &ConfigReport,
    scope: Option<String>,
) -> Vec<UnknownField> {
    let patterns = metadata_pattern_paths(metadata);
    let mut paths = Vec::new();
    collect_paths(value, "", &mut paths);
    paths.sort();
    paths.dedup();

    paths
        .into_iter()
        .filter(|path| {
            scope
                .as_ref()
                .is_none_or(|scope| path == scope || path.starts_with(&format!("{scope}.")))
        })
        .filter(|path| !path_is_covered_by_patterns(path, &patterns))
        .map(|path| {
            let source = find_source_for_unknown_path(report, &path);
            let suggestion = best_path_suggestion(&path, suggestion_paths);
            UnknownField::new(path)
                .with_source(source)
                .with_suggestion(suggestion)
        })
        .collect()
}

pub(super) fn error_path_for_scope(error: &ConfigError) -> Option<&str> {
    match error {
        ConfigError::Deserialize { path, .. } => Some(path.as_str()),
        _ => None,
    }
}

pub(super) fn deserialize_error_scope(path: Option<&str>) -> Option<String> {
    let normalized = path.map(normalize_external_path)?;
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(super) fn metadata_pattern_paths(metadata: &ConfigMetadata) -> Vec<String> {
    let mut patterns = Vec::new();
    for field in metadata.fields() {
        patterns.push(field.path.clone());
        patterns.extend(field.aliases.iter().cloned());
    }
    patterns.sort();
    patterns.dedup();
    patterns
}

pub(super) fn path_is_covered_by_patterns(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        path_matches_pattern(path, pattern) || path_is_prefix_of_pattern(path, pattern)
    })
}

pub(super) fn path_is_prefix_of_pattern(prefix: &str, pattern: &str) -> bool {
    let prefix_segments = prefix
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let pattern_segments = pattern
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    prefix_segments.len() <= pattern_segments.len()
        && prefix_segments
            .iter()
            .zip(pattern_segments.iter())
            .all(|(actual, expected)| *expected == "*" || actual == expected)
}

pub(super) fn collect_unknown_fields<T>(
    value: &Value,
    suggestion_paths: &BTreeMap<String, String>,
    report: &ConfigReport,
    string_coercion_paths: &BTreeSet<String>,
) -> Result<Vec<UnknownField>, ConfigError>
where
    T: DeserializeOwned,
{
    let scan = scan_unknown_field_paths_with_retry::<T>(value, string_coercion_paths);
    scan.result.map_err(|error| ConfigError::Deserialize {
        path: "<unknown>".to_owned(),
        provenance: None,
        message: error.to_string(),
    })?;

    Ok(unknown_fields_from_paths(
        scan.ignored,
        &merge_suggestion_paths(suggestion_paths, &scan.known_paths),
        report,
    ))
}

pub(super) fn collect_unknown_fields_best_effort<T>(
    value: &Value,
    suggestion_paths: &BTreeMap<String, String>,
    report: &ConfigReport,
    string_coercion_paths: &BTreeSet<String>,
) -> Vec<UnknownField>
where
    T: DeserializeOwned,
{
    let scan = scan_unknown_field_paths_with_retry::<T>(value, string_coercion_paths);
    unknown_fields_from_paths(
        scan.ignored,
        &merge_suggestion_paths(suggestion_paths, &scan.known_paths),
        report,
    )
}

pub(super) struct UnknownFieldScan<T> {
    pub(super) ignored: Vec<String>,
    pub(super) known_paths: BTreeSet<String>,
    pub(super) result: Result<T, ValueDeError>,
}

pub(super) fn scan_unknown_field_paths<T>(
    value: &Value,
    string_coercion_paths: &BTreeSet<String>,
) -> UnknownFieldScan<T>
where
    T: DeserializeOwned,
{
    let ignored = RefCell::new(Vec::new());
    let known_paths = RefCell::new(BTreeSet::new());
    let deserializer = CoercingDeserializer::new(
        value,
        "",
        string_coercion_paths,
        Some(&known_paths),
        Some(&ignored),
    );
    let result = serde_ignored::deserialize(deserializer, |path| {
        ignored
            .borrow_mut()
            .push(normalize_external_path(&path.to_string()))
    });
    let mut ignored = ignored.into_inner();
    ignored.sort();
    ignored.dedup();
    UnknownFieldScan {
        ignored,
        known_paths: known_paths.into_inner(),
        result,
    }
}

pub(super) fn scan_unknown_field_paths_with_retry<T>(
    value: &Value,
    string_coercion_paths: &BTreeSet<String>,
) -> UnknownFieldScan<T>
where
    T: DeserializeOwned,
{
    let scan = scan_unknown_field_paths::<T>(value, string_coercion_paths);
    if scan.result.is_ok() {
        return scan;
    }

    let retry_value = coerce_retry_scalars(value, "", string_coercion_paths);
    if retry_value == *value {
        return scan;
    }

    let retry_scan = scan_unknown_field_paths::<T>(&retry_value, string_coercion_paths);
    if retry_scan.result.is_ok() {
        retry_scan
    } else {
        scan
    }
}

pub(super) fn merge_suggestion_paths(
    base: &BTreeMap<String, String>,
    known_paths: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut merged = base.clone();
    for path in known_paths {
        merged.entry(path.clone()).or_insert_with(|| path.clone());
    }
    merged
}

pub(super) fn unknown_fields_from_paths(
    paths: Vec<String>,
    suggestion_paths: &BTreeMap<String, String>,
    report: &ConfigReport,
) -> Vec<UnknownField> {
    paths
        .into_iter()
        .map(|path| normalize_external_path(&path))
        .filter(|path| !suggestion_paths.contains_key(path))
        .map(|path| {
            let source = find_source_for_unknown_path(report, &path);
            let suggestion = best_path_suggestion(&path, suggestion_paths);
            UnknownField::new(path)
                .with_source(source)
                .with_suggestion(suggestion)
        })
        .collect()
}

pub(super) fn find_source_for_unknown_path(
    report: &ConfigReport,
    path: &str,
) -> Option<SourceTrace> {
    let mut current = Some(normalize_external_path(path));
    while let Some(candidate) = current {
        if let Some(source) = report.latest_source_for(&candidate) {
            return Some(source);
        }
        current = candidate
            .rsplit_once('.')
            .map(|(parent, _)| parent.to_owned())
            .filter(|parent| !parent.is_empty());
    }
    None
}

pub(super) fn best_path_suggestion(
    path: &str,
    suggestion_paths: &BTreeMap<String, String>,
) -> Option<String> {
    if suggestion_paths.is_empty() {
        return None;
    }

    let normalized = normalize_external_path(path);
    let (parent, leaf) = normalized
        .rsplit_once('.')
        .map_or(("", normalized.as_str()), |(parent, leaf)| (parent, leaf));

    let mut sibling_best: Option<(usize, String)> = None;
    for (candidate, canonical) in suggestion_paths {
        let display_candidate = materialize_pattern_for_path(candidate, &normalized);
        let display_canonical = materialize_pattern_for_path(canonical, &normalized);
        let (candidate_parent, candidate_leaf) = display_candidate
            .rsplit_once('.')
            .map_or(("", display_candidate.as_str()), |(parent, leaf)| {
                (parent, leaf)
            });
        if candidate_parent != parent {
            continue;
        }

        let distance = levenshtein(leaf, candidate_leaf);
        match &mut sibling_best {
            Some((best_distance, best_candidate)) if distance < *best_distance => {
                *best_distance = distance;
                *best_candidate = display_canonical.clone();
            }
            None => sibling_best = Some((distance, display_canonical.clone())),
            _ => {}
        }
    }

    if let Some((distance, suggestion)) = sibling_best
        && distance <= 3
    {
        return Some(suggestion);
    }

    let mut best: Option<(usize, String)> = None;
    for (candidate, canonical) in suggestion_paths {
        let display_candidate = materialize_pattern_for_path(candidate, &normalized);
        let display_canonical = materialize_pattern_for_path(canonical, &normalized);
        let distance = levenshtein(&normalized, &display_candidate);
        match &mut best {
            Some((best_distance, best_candidate)) if distance < *best_distance => {
                *best_distance = distance;
                *best_candidate = display_canonical.clone();
            }
            None => best = Some((distance, display_canonical.clone())),
            _ => {}
        }
    }

    best.and_then(|(distance, suggestion)| {
        let max_len = normalized.len().max(suggestion.len());
        (distance <= (max_len / 3).max(2)).then_some(suggestion)
    })
}

pub(super) fn materialize_pattern_for_path(pattern: &str, actual_path: &str) -> String {
    let pattern_segments = pattern
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let actual_segments = actual_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    pattern_segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if *segment == "*" {
                actual_segments
                    .get(index)
                    .copied()
                    .unwrap_or("<item>")
                    .to_owned()
            } else {
                (*segment).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }

    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + cost);
        }
        previous.clone_from_slice(&current);
    }

    previous[right_chars.len()]
}
