use super::*;

/// Prefixes child metadata paths with a parent field name.
#[must_use]
pub fn prefixed_metadata(
    prefix: &str,
    prefix_aliases: Vec<String>,
    metadata: ConfigMetadata,
) -> ConfigMetadata {
    let prefix = if prefix.is_empty() {
        String::new()
    } else {
        try_normalize_metadata_path(prefix)
            .ok()
            .filter(|normalized| !normalized.is_empty())
            .unwrap_or_else(|| prefix.to_owned())
    };
    if prefix.is_empty() {
        return metadata;
    }
    let prefix_aliases = prefix_aliases
        .into_iter()
        .map(|alias| {
            if alias.is_empty() {
                alias
            } else {
                try_normalize_metadata_path(&alias)
                    .ok()
                    .filter(|normalized| !normalized.is_empty())
                    .unwrap_or(alias)
            }
        })
        .collect::<Vec<_>>();

    let mut prefixed = ConfigMetadata::from_fields(metadata.fields.into_iter().map(|field| {
        let canonical_suffix = field.path.clone();
        let alias_suffixes = if field.aliases.is_empty() {
            vec![canonical_suffix.clone()]
        } else {
            let mut suffixes = vec![canonical_suffix.clone()];
            suffixes.extend(field.aliases.iter().cloned());
            suffixes
        };

        let path = if canonical_suffix.is_empty() {
            prefix.clone()
        } else {
            format!("{prefix}.{}", canonical_suffix)
        };

        let mut aliases = field
            .aliases
            .into_iter()
            .map(|alias| {
                if alias.is_empty() {
                    prefix.clone()
                } else {
                    format!("{prefix}.{}", alias)
                }
            })
            .collect::<Vec<_>>();

        for prefix_alias in &prefix_aliases {
            if canonical_suffix.is_empty() {
                aliases.push(prefix_alias.clone());
                continue;
            }
            for suffix in &alias_suffixes {
                if prefix_alias.is_empty() {
                    aliases.push(suffix.clone());
                } else {
                    aliases.push(format!("{prefix_alias}.{suffix}"));
                }
            }
        }

        FieldMetadata {
            path,
            aliases,
            ..field
        }
    }));
    prefixed.extend_checks(
        metadata
            .checks
            .into_iter()
            .filter_map(|check| check.prefixed(&prefix)),
    );
    prefixed
}
