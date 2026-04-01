use serde::{Deserialize, Serialize};
use tier::{ConfigLoader, ConfigMetadata, EnvSource, FieldMetadata, Secret};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    db: DbConfig,
    tls: TlsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbConfig {
    url: String,
    password: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TlsConfig {
    enabled: bool,
    cert: Option<String>,
    key: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            db: DbConfig {
                url: "postgres://localhost/app".to_owned(),
                password: Secret::new("default-secret".to_owned()),
            },
            tls: TlsConfig {
                enabled: true,
                cert: Some("/etc/tier/tls.crt".to_owned()),
                key: Some("/etc/tier/tls.key".to_owned()),
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = ConfigMetadata::from_fields([
        FieldMetadata::new("db.url")
            .env("DATABASE_URL")
            .doc("Primary database connection string"),
        FieldMetadata::new("db.password").secret().non_empty(),
        FieldMetadata::new("tls.cert").absolute_path(),
        FieldMetadata::new("tls.key").absolute_path(),
    ])
    .required_if("tls.enabled", true, ["tls.cert", "tls.key"]);

    let env = EnvSource::from_pairs([
        ("DATABASE_URL", "\"postgres://env/app\""),
        ("APP__DB__PASSWORD", "\"rotated-secret\""),
    ])
    .prefix("APP");

    let loaded = ConfigLoader::new(AppConfig::default())
        .metadata(metadata)
        .env(env)
        .load()?;

    println!("{}", loaded.report().redacted_pretty_json());
    println!(
        "{}",
        loaded
            .report()
            .explain("db.password")
            .expect("db.password explanation")
    );
    Ok(())
}
