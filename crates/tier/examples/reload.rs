use std::fs;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tier::{ConfigLoader, ReloadHandle};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { port: 3000 }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path =
        std::env::temp_dir().join(format!("tier-reload-example-{}.toml", std::process::id()));
    fs::write(&path, "port = 3000\n")?;

    let path_for_loader = path.clone();
    let handle = ReloadHandle::new(move || {
        ConfigLoader::new(AppConfig::default())
            .file(path_for_loader.clone())
            .load()
    })?;

    let watcher = handle.start_polling([path.clone()], Duration::from_millis(50));
    fs::write(&path, "port = 4000\n")?;

    thread::sleep(Duration::from_millis(150));
    println!("reloaded port = {}", handle.config().port);

    watcher.stop();
    let _ = fs::remove_file(path);
    Ok(())
}
