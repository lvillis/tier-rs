use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tier::{
    ArgsSource, ConfigLoader, ConfigMetadata, EnvSource, FieldMetadata, ReloadHandle, Secret,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchConfig {
    server: BenchServer,
    db: BenchDb,
    tls: BenchTls,
    features: Vec<String>,
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchServer {
    host: String,
    port: u16,
    mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchDb {
    url: String,
    password: Secret<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchTls {
    enabled: bool,
    cert: Option<String>,
    key: Option<String>,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            server: BenchServer {
                host: "127.0.0.1".to_owned(),
                port: 3000,
                mode: "memory".to_owned(),
            },
            db: BenchDb {
                url: "postgres://localhost/app".to_owned(),
                password: Secret::new("default-secret".to_owned()),
            },
            tls: BenchTls {
                enabled: false,
                cert: None,
                key: None,
            },
            features: vec!["core".to_owned()],
            labels: BTreeMap::from([("service".to_owned(), "tier".to_owned())]),
        }
    }
}

fn bench_metadata() -> ConfigMetadata {
    ConfigMetadata::from_fields([
        FieldMetadata::new("server.mode").one_of(["memory", "redis"]),
        FieldMetadata::new("db.url").env("DATABASE_URL"),
        FieldMetadata::new("db.password").secret().non_empty(),
        FieldMetadata::new("tls.cert").absolute_path(),
        FieldMetadata::new("tls.key").absolute_path(),
        FieldMetadata::new("features").min_length(1),
    ])
    .required_if("tls.enabled", true, ["tls.cert", "tls.key"])
}

fn render_toml(port: u16, mode: &str) -> String {
    format!(
        r#"
features = ["core", "api"]

[server]
host = "0.0.0.0"
port = {port}
mode = "{mode}"

[db]
url = "postgres://file/app"
password = "file-secret"

[tls]
enabled = false

[labels]
service = "tier"
region = "test"
"#
    )
}

fn bench_file_load(c: &mut Criterion) {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(&path, render_toml(4100, "memory")).expect("config file");
    let metadata = bench_metadata();

    c.bench_function("config/load/file_toml", |b| {
        b.iter(|| {
            let loaded = ConfigLoader::new(BenchConfig::default())
                .file(path.clone())
                .metadata(metadata.clone())
                .load()
                .expect("config loads");
            black_box(loaded.server.port)
        });
    });
}

fn bench_layered_load(c: &mut Criterion) {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("layered.toml");
    fs::write(&path, render_toml(4200, "memory")).expect("config file");
    let metadata = bench_metadata();
    let env = EnvSource::from_pairs([
        ("DATABASE_URL", "\"postgres://env/app\""),
        ("APP__SERVER__PORT", "4300"),
    ])
    .prefix("APP");
    let args = ArgsSource::from_args([
        "tier",
        "--set",
        r#"server.host="127.0.0.2""#,
        "--set",
        r#"db.password="cli-secret""#,
        "--set",
        r#"labels={"service":"tier","region":"bench"}"#,
    ]);

    c.bench_function("config/load/layered", |b| {
        b.iter(|| {
            let loaded = ConfigLoader::new(BenchConfig::default())
                .file(path.clone())
                .env(env.clone())
                .args(args.clone())
                .metadata(metadata.clone())
                .load()
                .expect("layered config loads");
            black_box((loaded.server.host.len(), loaded.server.port))
        });
    });
}

fn bench_report_diagnostics(c: &mut Criterion) {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("report.toml");
    fs::write(&path, render_toml(4400, "redis")).expect("config file");

    let loaded = ConfigLoader::new(BenchConfig::default())
        .file(path)
        .metadata(bench_metadata())
        .load()
        .expect("config loads");

    c.bench_function("config/report/doctor_and_explain", |b| {
        b.iter(|| {
            let doctor = loaded.report().doctor_json();
            let explanation = loaded
                .report()
                .explain("db.password")
                .expect("db.password explanation");
            black_box((
                doctor["summary"]["trace_count"].as_u64(),
                explanation.redacted,
            ))
        });
    });
}

fn bench_reload(c: &mut Criterion) {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("reload.toml");
    fs::write(&path, render_toml(4500, "memory")).expect("initial config");

    let path_for_loader = path.clone();
    let metadata = bench_metadata();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(BenchConfig::default())
            .file(path_for_loader.clone())
            .metadata(metadata.clone())
            .load()
    })
    .expect("initial load");
    let next_port = Cell::new(4600_u16);

    c.bench_function("config/reload/detailed", |b| {
        b.iter_batched(
            || {
                let port = next_port.get();
                next_port.set(port + 1);
                port
            },
            |port| {
                fs::write(&path, render_toml(port, "redis")).expect("updated config");
                let summary = handle.reload_detailed().expect("reload succeeds");
                black_box(summary.changed_paths.len())
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_file_load,
    bench_layered_load,
    bench_report_diagnostics,
    bench_reload
);
criterion_main!(benches);
