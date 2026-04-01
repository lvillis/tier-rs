#![cfg(feature = "toml")]

use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::tempdir;

use tier::{
    ConfigLoader, ReloadEvent, ReloadFailurePolicy, ReloadHandle, ReloadOptions, Secret,
    ValidationErrors,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReloadConfig {
    server: ReloadServer,
    db: ReloadDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReloadServer {
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ReloadDb {
    password: Secret<String>,
}

impl Default for ReloadConfig {
    fn default() -> Self {
        Self {
            server: ReloadServer { port: 3000 },
            db: ReloadDb {
                password: Secret::new("default-secret".to_owned()),
            },
        }
    }
}

#[test]
fn reload_keeps_previous_config_on_failure() {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(
        &path,
        r#"
            [server]
            port = 4000

            [db]
            password = "first-secret"
        "#,
    )
    .expect("initial config");

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(ReloadConfig::default())
            .file(path_for_loader.clone())
            .secret_path("db.password")
            .validator("port-range", |config| {
                if config.server.port < 1024 {
                    return Err(ValidationErrors::from_message(
                        "server.port",
                        "port must be >= 1024",
                    ));
                }
                Ok(())
            })
            .load()
    })
    .expect("initial load");

    assert_eq!(handle.config().server.port, 4000);

    fs::write(
        &path,
        r#"
            [server]
            port = 10
        "#,
    )
    .expect("broken config");

    let error = handle.reload().expect_err("reload should fail");
    assert!(error.to_string().contains("port must be >= 1024"));
    assert_eq!(handle.config().server.port, 4000);
    assert!(handle.last_error().is_some());

    fs::write(
        &path,
        r#"
            [server]
            port = 5000

            [db]
            password = "second-secret"
        "#,
    )
    .expect("fixed config");

    handle.reload().expect("reload should succeed");
    assert_eq!(handle.config().server.port, 5000);
    assert!(handle.last_error().is_none());
}

#[test]
fn reload_detailed_reports_redacted_changes_and_emits_events() {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(
        &path,
        r#"
            [server]
            port = 4000

            [db]
            password = "first-secret"
        "#,
    )
    .expect("initial config");

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(ReloadConfig::default())
            .file(path_for_loader.clone())
            .secret_path("db.password")
            .load()
    })
    .expect("initial load");

    let events = handle.subscribe();

    fs::write(
        &path,
        r#"
            [server]
            port = 5000

            [db]
            password = "second-secret"
        "#,
    )
    .expect("updated config");

    let summary = handle.reload_detailed().expect("reload succeeds");
    assert!(summary.had_changes);
    assert!(summary.changed_paths.contains(&"server.port".to_owned()));
    assert!(summary.changed_paths.contains(&"db.password".to_owned()));

    let password_change = summary
        .changes
        .iter()
        .find(|change| change.path == "db.password")
        .expect("password change");
    assert_eq!(
        password_change
            .before
            .as_ref()
            .and_then(|value| value.as_str()),
        Some("***redacted***")
    );
    assert_eq!(
        password_change
            .after
            .as_ref()
            .and_then(|value| value.as_str()),
        Some("***redacted***")
    );
    assert!(password_change.redacted);

    match events
        .recv_timeout(Duration::from_secs(1))
        .expect("reload event")
    {
        ReloadEvent::Applied(event_summary) => {
            assert_eq!(event_summary.changed_paths, summary.changed_paths);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn polling_watcher_can_start_and_stop() {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(
        &path,
        r#"
            [server]
            port = 3001

            [db]
            password = "secret"
        "#,
    )
    .expect("config");

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(ReloadConfig::default())
            .file(path_for_loader.clone())
            .secret_path("db.password")
            .load()
    })
    .expect("initial load");

    let watcher = handle.start_polling([path], Duration::from_millis(25));
    assert_eq!(handle.config().server.port, 3001);
    watcher.stop();
}

#[test]
fn polling_watcher_can_stop_after_reload_failure() {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(
        &path,
        r#"
            [server]
            port = 3001

            [db]
            password = "secret"
        "#,
    )
    .expect("config");

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(ReloadConfig::default())
            .file(path_for_loader.clone())
            .secret_path("db.password")
            .validator("port-range", |config| {
                if config.server.port < 1024 {
                    return Err(ValidationErrors::from_message(
                        "server.port",
                        "port must be >= 1024",
                    ));
                }
                Ok(())
            })
            .load()
    })
    .expect("initial load");

    let events = handle.subscribe();
    let watcher = handle.start_polling_with_options(
        [path.clone()],
        Duration::from_millis(25),
        ReloadOptions {
            on_error: ReloadFailurePolicy::StopWatcher,
            emit_unchanged: false,
        },
    );
    std::thread::sleep(Duration::from_millis(75));

    fs::write(
        &path,
        r#"
            [server]
            port = 10

            [db]
            password = "broken-secret"
        "#,
    )
    .expect("broken config");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline && handle.last_error().is_none() {
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(handle.last_error().is_some());
    assert_eq!(handle.config().server.port, 3001);

    match events
        .recv_timeout(Duration::from_secs(1))
        .expect("reload failure event")
    {
        ReloadEvent::Rejected(failure) => {
            assert!(failure.last_good_retained);
            assert!(failure.watcher_stopped);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    fs::write(
        &path,
        r#"
            [server]
            port = 4500

            [db]
            password = "recovered-secret"
        "#,
    )
    .expect("fixed config");

    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(handle.config().server.port, 3001);
    watcher.stop();
}

#[cfg(feature = "watch")]
#[test]
fn native_watcher_reloads_on_file_change() {
    let dir = tempdir().expect("temporary directory");
    let path = dir.path().join("app.toml");
    fs::write(
        &path,
        r#"
            [server]
            port = 3100

            [db]
            password = "first-secret"
        "#,
    )
    .expect("config");

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(ReloadConfig::default())
            .file(path_for_loader.clone())
            .secret_path("db.password")
            .load()
    })
    .expect("initial load");

    let watcher = handle
        .start_native([path.clone()], Duration::from_millis(75))
        .expect("native watcher starts");

    fs::write(
        &path,
        r#"
            [server]
            port = 3200

            [db]
            password = "second-secret"
        "#,
    )
    .expect("updated config");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if handle.config().server.port == 3200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert_eq!(handle.config().server.port, 3200);
    assert!(handle.last_error().is_none());
    watcher.stop();
}
