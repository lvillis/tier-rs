use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tier::{
    EnvDocOptions, Secret, TierConfig, annotated_json_schema_pretty, config_example_toml,
    env_docs_report_json_pretty,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TierConfig)]
struct AppConfig {
    server: ServerConfig,
    db: DbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TierConfig)]
struct ServerConfig {
    #[tier(
        env = "APP_SERVER_HOST",
        doc = "Address exposed by the service",
        example = "0.0.0.0",
        hostname
    )]
    host: String,
    #[tier(min = 1, max = 65535)]
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TierConfig)]
struct DbConfig {
    #[tier(env = "DATABASE_URL")]
    url: String,
    password: Secret<String>,
}

fn main() {
    println!("{}", annotated_json_schema_pretty::<AppConfig>());
    println!(
        "{}",
        env_docs_report_json_pretty::<AppConfig>(&EnvDocOptions::prefixed("APP"))
    );
    println!("{}", config_example_toml::<AppConfig>());
}
