use clap::Parser;
use serde::{Deserialize, Serialize};
use tier::{ConfigLoader, Secret, TierCli};

#[derive(Debug, Parser)]
struct AppCli {
    #[command(flatten)]
    config: TierCli,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    server: ServerConfig,
    db: DbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerConfig {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbConfig {
    password: Secret<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_owned(),
                port: 3000,
            },
            db: DbConfig {
                password: Secret::new("default-secret".to_owned()),
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = AppCli::parse_from(["tier-app", "--set", "server.port=9000", "--print-config"]);

    let loaded = cli
        .config
        .apply(ConfigLoader::new(AppConfig::default()).secret_path("db.password"))
        .load()?;

    if let Some(output) = cli.config.render(&loaded)? {
        println!("{output}");
    }

    Ok(())
}
