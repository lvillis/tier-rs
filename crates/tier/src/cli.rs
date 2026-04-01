use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

#[cfg(all(feature = "schema", not(feature = "toml")))]
use crate::config_example_pretty;
#[cfg(all(feature = "schema", feature = "toml"))]
use crate::config_example_toml;
use crate::{ArgsSource, ConfigError, ConfigLoader, LoadedConfig};
#[cfg(feature = "schema")]
use crate::{
    EnvDocOptions, JsonSchema, TierMetadata, annotated_json_schema_pretty, env_docs_markdown,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// High-level action requested by [`TierCli`].
pub enum TierCliCommand {
    /// Run the application normally.
    Run,
    /// Print the final redacted configuration and exit.
    PrintConfig,
    /// Validate configuration and exit.
    ValidateConfig,
    /// Explain how a single configuration path was resolved and exit.
    ExplainConfig {
        /// Dot-delimited configuration path to explain.
        path: String,
    },
    #[cfg(feature = "schema")]
    /// Print the JSON Schema for the configuration type and exit.
    PrintConfigSchema,
    #[cfg(feature = "schema")]
    /// Print environment variable documentation derived from the schema and exit.
    PrintEnvDocs,
    #[cfg(feature = "schema")]
    /// Print an example configuration document derived from schema and metadata.
    PrintConfigExample,
}

impl TierCliCommand {
    /// Returns `true` when the selected command should terminate after rendering output.
    #[must_use]
    pub fn should_exit(&self) -> bool {
        !matches!(self, Self::Run)
    }
}

#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Configuration")]
/// Reusable `clap` flag group for `tier` configuration loading and diagnostics.
///
/// Embed `TierCli` into an existing application CLI with `#[command(flatten)]`
/// to reuse `tier`'s config-related flags without rebuilding that surface
/// yourself.
///
/// # Examples
///
/// ```ignore
/// use clap::Parser;
/// use tier::TierCli;
///
/// #[derive(Debug, Parser)]
/// struct AppCli {
///     #[command(flatten)]
///     config: TierCli,
/// }
/// ```
pub struct TierCli {
    /// Additional configuration files to load.
    #[arg(long = "config", value_name = "PATH", value_hint = clap::ValueHint::FilePath)]
    pub config: Vec<PathBuf>,

    /// Active profile used for `{profile}` path templates.
    #[arg(long = "profile", value_name = "PROFILE")]
    pub profile: Option<String>,

    /// Inline configuration overrides in `key=value` form.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    pub set: Vec<String>,

    /// Prints the final redacted configuration and exits.
    #[arg(long = "print-config", group = "tier_action")]
    pub print_config: bool,

    /// Validates the configuration and exits.
    #[arg(long = "validate-config", group = "tier_action")]
    pub validate_config: bool,

    /// Explains how a single configuration path was resolved and exits.
    #[arg(long = "explain-config", value_name = "PATH", group = "tier_action")]
    pub explain_config: Option<String>,

    #[cfg(feature = "schema")]
    /// Prints the JSON Schema for the configuration type and exits.
    #[arg(long = "print-config-schema", group = "tier_action")]
    pub print_config_schema: bool,

    #[cfg(feature = "schema")]
    /// Prints schema-derived environment variable documentation and exits.
    #[arg(long = "print-env-docs", group = "tier_action")]
    pub print_env_docs: bool,

    #[cfg(feature = "schema")]
    /// Prints a generated example configuration and exits.
    #[arg(long = "print-config-example", group = "tier_action")]
    pub print_config_example: bool,

    #[cfg(feature = "schema")]
    /// Prefix to apply when generating schema-derived environment variable docs.
    #[arg(long = "env-prefix", value_name = "PREFIX")]
    pub env_prefix: Option<String>,

    #[cfg(feature = "schema")]
    /// Separator to use when generating schema-derived environment variable docs.
    #[arg(long = "env-separator", value_name = "SEP", default_value = "__")]
    pub env_separator: String,
}

impl TierCli {
    /// Resolves the requested CLI action.
    #[must_use]
    pub fn command(&self) -> TierCliCommand {
        #[cfg(feature = "schema")]
        if self.print_config_schema {
            return TierCliCommand::PrintConfigSchema;
        }
        #[cfg(feature = "schema")]
        if self.print_env_docs {
            return TierCliCommand::PrintEnvDocs;
        }
        #[cfg(feature = "schema")]
        if self.print_config_example {
            return TierCliCommand::PrintConfigExample;
        }
        if let Some(path) = &self.explain_config {
            return TierCliCommand::ExplainConfig { path: path.clone() };
        }
        if self.print_config {
            return TierCliCommand::PrintConfig;
        }
        if self.validate_config {
            return TierCliCommand::ValidateConfig;
        }
        TierCliCommand::Run
    }

    /// Converts parsed CLI flags into an [`ArgsSource`] suitable for [`ConfigLoader`].
    #[must_use]
    pub fn to_args_source(&self) -> ArgsSource {
        let mut args = vec!["tier".to_owned()];
        for path in &self.config {
            args.push("--config".to_owned());
            args.push(path.display().to_string());
        }
        if let Some(profile) = &self.profile {
            args.push("--profile".to_owned());
            args.push(profile.clone());
        }
        for assignment in &self.set {
            args.push("--set".to_owned());
            args.push(assignment.clone());
        }
        ArgsSource::from_args(args)
    }

    /// Applies CLI-derived overrides onto an existing [`ConfigLoader`].
    #[must_use]
    pub fn apply<T>(&self, loader: ConfigLoader<T>) -> ConfigLoader<T>
    where
        T: Serialize + DeserializeOwned,
    {
        loader.args(self.to_args_source())
    }

    /// Renders output for runtime, validation, print, and explain commands.
    pub fn render<T>(&self, loaded: &LoadedConfig<T>) -> Result<Option<String>, ConfigError> {
        match self.command() {
            TierCliCommand::Run => Ok(None),
            TierCliCommand::PrintConfig => Ok(Some(loaded.report().redacted_pretty_json())),
            TierCliCommand::ValidateConfig => Ok(Some("configuration is valid".to_owned())),
            TierCliCommand::ExplainConfig { path } => loaded
                .report()
                .explain(&path)
                .map(|explanation| explanation.to_string())
                .map(Some)
                .ok_or(ConfigError::ExplainPathNotFound { path }),
            #[cfg(feature = "schema")]
            TierCliCommand::PrintConfigSchema
            | TierCliCommand::PrintEnvDocs
            | TierCliCommand::PrintConfigExample => Ok(None),
        }
    }

    #[cfg(feature = "schema")]
    /// Builds schema documentation options from CLI flags.
    #[must_use]
    pub fn env_doc_options(&self) -> EnvDocOptions {
        let options = self
            .env_prefix
            .clone()
            .map_or_else(EnvDocOptions::new, EnvDocOptions::prefixed);
        options.separator(self.env_separator.clone())
    }

    #[cfg(feature = "schema")]
    /// Renders output for commands that may require schema support.
    pub fn render_with_schema<T>(
        &self,
        loaded: &LoadedConfig<T>,
    ) -> Result<Option<String>, ConfigError>
    where
        T: Serialize + DeserializeOwned + JsonSchema + TierMetadata,
    {
        match self.command() {
            TierCliCommand::PrintConfigSchema => Ok(Some(annotated_json_schema_pretty::<T>())),
            TierCliCommand::PrintEnvDocs => {
                Ok(Some(env_docs_markdown::<T>(&self.env_doc_options())))
            }
            TierCliCommand::PrintConfigExample => {
                #[cfg(feature = "toml")]
                {
                    Ok(Some(config_example_toml::<T>()))
                }
                #[cfg(not(feature = "toml"))]
                {
                    Ok(Some(config_example_pretty::<T>()))
                }
            }
            _ => self.render(loaded),
        }
    }
}
