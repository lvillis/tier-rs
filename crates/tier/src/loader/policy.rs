use crate::{ConfigError, ConfigMetadata};

use super::Layer;

pub(super) fn enforce_source_policies(
    layer: &Layer,
    metadata: &ConfigMetadata,
) -> Result<(), ConfigError> {
    for (path, trace) in &layer.entries {
        let Some(policy) = metadata.effective_source_policy_for(path) else {
            continue;
        };

        if !policy.source_kind_allowed(trace.kind) {
            return Err(ConfigError::SourcePolicyViolation {
                path: path.clone(),
                trace: trace.clone(),
                allowed_sources: policy.allowed_sources_vec().into_boxed_slice(),
                denied_sources: Vec::new().into_boxed_slice(),
            });
        }

        if policy.source_kind_denied(trace.kind) {
            return Err(ConfigError::SourcePolicyViolation {
                path: path.clone(),
                trace: trace.clone(),
                allowed_sources: policy.allowed_sources_vec().into_boxed_slice(),
                denied_sources: policy.denied_sources_vec().into_boxed_slice(),
            });
        }
    }

    Ok(())
}
