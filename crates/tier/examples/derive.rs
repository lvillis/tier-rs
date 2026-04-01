use serde::{Deserialize, Serialize};
use tier::{ArgsSource, ConfigLoader, EnvSource, Secret, TierConfig};

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
#[tier(exactly_one_of("listener.port", "listener.unix_socket"))]
struct AppConfig {
    #[tier(doc = "Logical service name", non_empty, min_length = 3)]
    service_name: String,
    listener: ListenerConfig,
    db: DbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct ListenerConfig {
    #[tier(min = 1, max = 65535)]
    port: Option<u16>,
    unix_socket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TierConfig)]
struct DbConfig {
    #[tier(env = "DATABASE_URL")]
    url: String,
    password: Secret<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            service_name: "tier-api".to_owned(),
            listener: ListenerConfig {
                port: Some(3000),
                unix_socket: None,
            },
            db: DbConfig {
                url: "postgres://localhost/app".to_owned(),
                password: Secret::new("default-secret".to_owned()),
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = EnvSource::from_pairs([("DATABASE_URL", "\"postgres://env/app\"")]);
    let args = ArgsSource::from_args(["app", "--set", r#"db.password="rotated-secret""#]);

    let loaded = ConfigLoader::new(AppConfig::default())
        .derive_metadata()
        .env(env)
        .args(args)
        .load()?;

    println!("{}", loaded.report().doctor());
    println!("{}", loaded.report().redacted_pretty_json());
    Ok(())
}
