use serde::{Deserialize, Serialize};
use tier::{ArgsSource, ConfigLoader, EnvSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    host: String,
    port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 3000,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = EnvSource::from_pairs([("APP__PORT", "8080")]).prefix("APP");
    let args = ArgsSource::from_args(["app", "--set", r#"host="0.0.0.0""#]);

    let loaded = ConfigLoader::new(AppConfig::default())
        .env(env)
        .args(args)
        .load()?;

    println!("listening on {}:{}", loaded.host, loaded.port);
    println!("{}", loaded.report().doctor());
    Ok(())
}
