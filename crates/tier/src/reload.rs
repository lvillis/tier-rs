use std::collections::BTreeMap;
#[cfg(feature = "watch")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "watch")]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
#[cfg(feature = "watch")]
use std::time::Instant;
use std::time::{Duration, SystemTime};

#[cfg(feature = "watch")]
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

use crate::report::{collect_diff_paths, get_value_at_path};
use crate::{ConfigError, ConfigReport, LoadedConfig};

type LoaderFn<T> = dyn Fn() -> Result<LoadedConfig<T>, ConfigError> + Send + Sync + 'static;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Policy applied when a background watcher encounters a reload failure.
pub enum ReloadFailurePolicy {
    /// Keep the last good configuration and continue watching for future changes.
    #[default]
    KeepLastGood,
    /// Keep the last good configuration and stop the background watcher.
    StopWatcher,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Options controlling watcher-side reload behavior.
pub struct ReloadOptions {
    /// Behavior applied after a failed reload.
    pub on_error: ReloadFailurePolicy,
    /// Whether to emit success events even when the effective configuration did not change.
    pub emit_unchanged: bool,
}

impl Default for ReloadOptions {
    fn default() -> Self {
        Self {
            on_error: ReloadFailurePolicy::KeepLastGood,
            emit_unchanged: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
/// A single redacted configuration change observed during reload.
pub struct ConfigChange {
    /// Dot-delimited path that changed.
    pub path: String,
    /// Previous redacted value, when present.
    pub before: Option<Value>,
    /// New redacted value, when present.
    pub after: Option<Value>,
    /// Whether either side of the change was redacted.
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
/// Structured summary of a successful reload attempt.
pub struct ReloadSummary {
    /// Whether the effective redacted configuration changed.
    pub had_changes: bool,
    /// Changed paths in normalized order.
    pub changed_paths: Vec<String>,
    /// Structured per-path change details.
    pub changes: Vec<ConfigChange>,
}

impl ReloadSummary {
    /// Returns `true` when the reload was a no-op.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        !self.had_changes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Structured details about a rejected reload attempt.
pub struct ReloadFailure {
    /// Human-readable error message.
    pub error: String,
    /// Whether the previous configuration snapshot was preserved.
    pub last_good_retained: bool,
    /// Whether the watcher that observed the error stopped after the failure.
    pub watcher_stopped: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
/// Structured event emitted after each successful or rejected reload attempt.
pub enum ReloadEvent {
    /// A reload was applied successfully.
    Applied(ReloadSummary),
    /// A reload failed and the previous configuration was kept.
    Rejected(ReloadFailure),
}

/// Thread-safe holder for the active configuration and reload logic.
///
/// `ReloadHandle<T>` keeps the most recent successful [`LoadedConfig`] in
/// memory and reuses the same loader closure for subsequent reloads. Failed
/// reloads never replace the active configuration, which makes it suitable for
/// long-running services.
///
/// # Examples
///
/// ```no_run
/// use serde::{Deserialize, Serialize};
/// use tier::{ConfigLoader, ReloadHandle};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct AppConfig {
///     port: u16,
/// }
///
/// impl Default for AppConfig {
///     fn default() -> Self {
///         Self { port: 3000 }
///     }
/// }
///
/// let handle = ReloadHandle::new(|| ConfigLoader::new(AppConfig::default()).load())?;
/// let snapshot = handle.snapshot();
/// assert_eq!(snapshot.port, 3000);
/// # Ok::<(), tier::ConfigError>(())
/// ```
pub struct ReloadHandle<T> {
    state: Arc<RwLock<LoadedConfig<T>>>,
    loader: Arc<LoaderFn<T>>,
    last_error: Arc<Mutex<Option<String>>>,
    subscribers: Arc<Mutex<Vec<Sender<ReloadEvent>>>>,
}

impl<T> Clone for ReloadHandle<T> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            loader: Arc::clone(&self.loader),
            last_error: Arc::clone(&self.last_error),
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

impl<T> ReloadHandle<T>
where
    T: Send + Sync + 'static,
{
    /// Creates a reload handle from a loader closure and performs the initial load.
    pub fn new<F>(loader: F) -> Result<Self, ConfigError>
    where
        F: Fn() -> Result<LoadedConfig<T>, ConfigError> + Send + Sync + 'static,
    {
        let initial = loader()?;
        Ok(Self {
            state: Arc::new(RwLock::new(initial)),
            loader: Arc::new(loader),
            last_error: Arc::new(Mutex::new(None)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Attempts to reload configuration, preserving the previous state on failure.
    pub fn reload(&self) -> Result<(), ConfigError> {
        self.reload_with_options(&ReloadOptions {
            emit_unchanged: true,
            ..ReloadOptions::default()
        })
        .map(|_| ())
    }

    /// Attempts to reload configuration and returns a structured diff summary.
    pub fn reload_detailed(&self) -> Result<ReloadSummary, ConfigError> {
        self.reload_with_options(&ReloadOptions {
            emit_unchanged: true,
            ..ReloadOptions::default()
        })
    }

    /// Returns the most recent reload error, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        mutex_lock(&self.last_error).clone()
    }

    /// Subscribes to future reload events.
    pub fn subscribe(&self) -> Receiver<ReloadEvent> {
        let (tx, rx) = mpsc::channel();
        mutex_lock(&self.subscribers).push(tx);
        rx
    }

    /// Starts a polling watcher that reloads when any watched file changes.
    pub fn start_polling<I, P>(&self, paths: I, interval: Duration) -> PollingWatcher
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.start_polling_with_options(paths, interval, ReloadOptions::default())
    }

    /// Starts a polling watcher with explicit reload behavior options.
    pub fn start_polling_with_options<I, P>(
        &self,
        paths: I,
        interval: Duration,
        options: ReloadOptions,
    ) -> PollingWatcher
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        PollingWatcher::spawn(
            self.clone(),
            paths.into_iter().map(Into::into).collect(),
            interval,
            options,
        )
    }

    #[cfg(feature = "watch")]
    /// Starts a native filesystem watcher with event debouncing.
    ///
    /// File paths are watched through their parent directories so atomic
    /// replace-and-rename writes still trigger a reload.
    pub fn start_native<I, P>(
        &self,
        paths: I,
        debounce: Duration,
    ) -> Result<NativeWatcher, ConfigError>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.start_native_with_options(paths, debounce, ReloadOptions::default())
    }

    /// Starts a native filesystem watcher with explicit reload behavior options.
    #[cfg(feature = "watch")]
    pub fn start_native_with_options<I, P>(
        &self,
        paths: I,
        debounce: Duration,
        options: ReloadOptions,
    ) -> Result<NativeWatcher, ConfigError>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        NativeWatcher::spawn(
            self.clone(),
            paths.into_iter().map(Into::into).collect(),
            debounce,
            options,
        )
    }

    fn reload_with_options(&self, options: &ReloadOptions) -> Result<ReloadSummary, ConfigError> {
        let before_report = read_lock(&self.state).report().clone();
        let before_raw = before_report.final_value().clone();
        let before_redacted = before_report.redacted_value();

        match (self.loader)() {
            Ok(next) => {
                let after_raw = next.report().final_value().clone();
                let after_redacted = next.report().redacted_value();
                let summary = build_reload_summary(
                    &before_raw,
                    &after_raw,
                    &before_redacted,
                    &after_redacted,
                );
                *write_lock(&self.state) = next;
                *mutex_lock(&self.last_error) = None;
                if options.emit_unchanged || summary.had_changes {
                    self.emit_event(ReloadEvent::Applied(summary.clone()));
                }
                Ok(summary)
            }
            Err(error) => {
                self.record_error_message(error.to_string());
                self.emit_event(ReloadEvent::Rejected(ReloadFailure {
                    error: error.to_string(),
                    last_good_retained: true,
                    watcher_stopped: matches!(options.on_error, ReloadFailurePolicy::StopWatcher),
                }));
                Err(error)
            }
        }
    }
}

impl<T> ReloadHandle<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Returns a full snapshot of the current loaded configuration and report.
    #[must_use]
    pub fn snapshot(&self) -> LoadedConfig<T> {
        read_lock(&self.state).clone()
    }

    /// Returns a cloned copy of the current configuration value.
    #[must_use]
    pub fn config(&self) -> T {
        read_lock(&self.state).config().clone()
    }

    /// Returns a cloned copy of the current configuration report.
    #[must_use]
    pub fn report(&self) -> ConfigReport {
        read_lock(&self.state).report().clone()
    }
}

impl<T> ReloadHandle<T> {
    fn record_error_message(&self, message: String) {
        *mutex_lock(&self.last_error) = Some(message);
    }

    fn emit_event(&self, event: ReloadEvent) {
        let mut subscribers = mutex_lock(&self.subscribers);
        subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }
}

/// Handle for a background polling watcher.
pub struct PollingWatcher {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl PollingWatcher {
    fn spawn<T>(
        handle: ReloadHandle<T>,
        paths: Vec<PathBuf>,
        interval: Duration,
        options: ReloadOptions,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let join_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            let mut seen = collect_mtimes(&paths);
            while !join_stop.load(Ordering::Relaxed) {
                thread::sleep(interval);
                let current = collect_mtimes(&paths);
                if current != seen {
                    let reload_result = handle.reload_with_options(&options);
                    seen = current;
                    if reload_result.is_err()
                        && matches!(options.on_error, ReloadFailurePolicy::StopWatcher)
                    {
                        return;
                    }
                }
            }
        });

        Self {
            stop,
            join: Some(join),
        }
    }

    /// Stops the watcher and joins the background thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for PollingWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(feature = "watch")]
enum WatchMessage {
    Event(notify::Result<Event>),
    Stop,
}

#[cfg(feature = "watch")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchTargetKind {
    File,
    Directory,
}

#[cfg(feature = "watch")]
#[derive(Debug, Clone)]
struct WatchTarget {
    path: PathBuf,
    kind: WatchTargetKind,
    watch_root: PathBuf,
    recursive: bool,
}

#[cfg(feature = "watch")]
impl WatchTarget {
    fn matches_event_path(&self, path: &Path) -> bool {
        match self.kind {
            WatchTargetKind::File => path == self.path,
            WatchTargetKind::Directory => path == self.path || path.starts_with(&self.path),
        }
    }
}

#[cfg(feature = "watch")]
#[derive(Debug, Clone)]
struct WatchRegistration {
    root: PathBuf,
    recursive: bool,
}

#[cfg(feature = "watch")]
/// Handle for a background native filesystem watcher.
pub struct NativeWatcher {
    watcher: Option<RecommendedWatcher>,
    stop: Option<Sender<WatchMessage>>,
    join: Option<JoinHandle<()>>,
}

#[cfg(feature = "watch")]
impl NativeWatcher {
    fn spawn<T>(
        handle: ReloadHandle<T>,
        paths: Vec<PathBuf>,
        debounce: Duration,
        options: ReloadOptions,
    ) -> Result<Self, ConfigError>
    where
        T: Send + Sync + 'static,
    {
        let targets = prepare_watch_targets(paths)?;
        if targets.is_empty() {
            return Err(ConfigError::Watch {
                message: "at least one path must be watched".to_owned(),
            });
        }

        let registrations = collect_watch_registrations(&targets);
        let (tx, rx) = mpsc::channel();
        let callback_tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = callback_tx.send(WatchMessage::Event(event));
        })
        .map_err(map_watch_error)?;

        for registration in &registrations {
            let mode = if registration.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            watcher
                .watch(&registration.root, mode)
                .map_err(map_watch_error)?;
        }

        let join =
            thread::spawn(move || run_native_watch_loop(handle, targets, rx, debounce, options));

        Ok(Self {
            watcher: Some(watcher),
            stop: Some(tx),
            join: Some(join),
        })
    }

    /// Stops the watcher and joins the background thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.watcher.take();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(WatchMessage::Stop);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(feature = "watch")]
impl Drop for NativeWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(feature = "watch")]
fn run_native_watch_loop<T>(
    handle: ReloadHandle<T>,
    targets: Vec<WatchTarget>,
    rx: Receiver<WatchMessage>,
    debounce: Duration,
    options: ReloadOptions,
) where
    T: Send + Sync + 'static,
{
    loop {
        let deadline = match rx.recv() {
            Ok(WatchMessage::Stop) | Err(_) => return,
            Ok(WatchMessage::Event(event)) => {
                match handle_watch_message(&handle, &targets, event, debounce) {
                    Some(deadline) => deadline,
                    None => continue,
                }
            }
        };

        let mut deadline = deadline;
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(timeout) {
                Ok(WatchMessage::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => break,
                Ok(WatchMessage::Event(event)) => {
                    if let Some(next_deadline) =
                        handle_watch_message(&handle, &targets, event, debounce)
                    {
                        deadline = next_deadline;
                    }
                }
            }
        }

        if handle.reload_with_options(&options).is_err()
            && matches!(options.on_error, ReloadFailurePolicy::StopWatcher)
        {
            return;
        }
    }
}

#[cfg(feature = "watch")]
fn handle_watch_message<T>(
    handle: &ReloadHandle<T>,
    targets: &[WatchTarget],
    event: notify::Result<Event>,
    debounce: Duration,
) -> Option<Instant> {
    match event {
        Ok(event) if event_requires_reload(&event, targets) => Some(Instant::now() + debounce),
        Ok(_) => None,
        Err(error) => {
            handle.record_error_message(format!("watch error: {error}"));
            None
        }
    }
}

#[cfg(feature = "watch")]
fn event_requires_reload(event: &Event, targets: &[WatchTarget]) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }

    if event.paths.is_empty() {
        return true;
    }

    event
        .paths
        .iter()
        .filter_map(|path| absolutize_event_path(path))
        .any(|path| {
            targets
                .iter()
                .any(|target| target.matches_event_path(&path))
        })
}

#[cfg(feature = "watch")]
fn prepare_watch_targets(paths: Vec<PathBuf>) -> Result<Vec<WatchTarget>, ConfigError> {
    let mut targets = Vec::new();
    for path in paths {
        let path = absolutize_path(&path)?;
        if path.exists() && path.is_dir() {
            targets.push(WatchTarget {
                watch_root: path.clone(),
                path,
                kind: WatchTargetKind::Directory,
                recursive: true,
            });
            continue;
        }

        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.clone());
        let (watch_root, recursive) = if parent.exists() {
            (parent, false)
        } else if let Some(root) = nearest_existing_ancestor(&parent) {
            (root, true)
        } else {
            (std::env::current_dir().map_err(map_watch_io_error)?, true)
        };

        targets.push(WatchTarget {
            path,
            kind: WatchTargetKind::File,
            watch_root,
            recursive,
        });
    }

    Ok(targets)
}

#[cfg(feature = "watch")]
fn collect_watch_registrations(targets: &[WatchTarget]) -> Vec<WatchRegistration> {
    let mut registrations = BTreeMap::<PathBuf, bool>::new();
    for target in targets {
        registrations
            .entry(target.watch_root.clone())
            .and_modify(|recursive| *recursive |= target.recursive)
            .or_insert(target.recursive);
    }

    registrations
        .into_iter()
        .map(|(root, recursive)| WatchRegistration { root, recursive })
        .collect()
}

#[cfg(feature = "watch")]
fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

#[cfg(feature = "watch")]
fn absolutize_path(path: &Path) -> Result<PathBuf, ConfigError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(map_watch_io_error)?
            .join(path))
    }
}

#[cfg(feature = "watch")]
fn absolutize_event_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        std::env::current_dir().ok().map(|cwd| cwd.join(path))
    }
}

#[cfg(feature = "watch")]
fn map_watch_error(error: notify::Error) -> ConfigError {
    ConfigError::Watch {
        message: error.to_string(),
    }
}

#[cfg(feature = "watch")]
fn map_watch_io_error(error: std::io::Error) -> ConfigError {
    ConfigError::Watch {
        message: error.to_string(),
    }
}

fn collect_mtimes(paths: &[PathBuf]) -> BTreeMap<PathBuf, Option<SystemTime>> {
    paths
        .iter()
        .cloned()
        .map(|path| {
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            (path, mtime)
        })
        .collect()
}

fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn mutex_lock<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn build_reload_summary(
    before_raw: &Value,
    after_raw: &Value,
    before_redacted: &Value,
    after_redacted: &Value,
) -> ReloadSummary {
    let mut changed_paths = Vec::new();
    collect_diff_paths(before_raw, after_raw, "", &mut changed_paths);
    changed_paths.sort();
    changed_paths.dedup();

    let changes = changed_paths
        .iter()
        .map(|path| {
            let before_value = get_value_at_path(before_redacted, path).cloned();
            let after_value = get_value_at_path(after_redacted, path).cloned();
            let redacted = before_value.as_ref().is_some_and(is_redacted_value)
                || after_value.as_ref().is_some_and(is_redacted_value);
            ConfigChange {
                path: path.clone(),
                before: before_value,
                after: after_value,
                redacted,
            }
        })
        .collect::<Vec<_>>();

    ReloadSummary {
        had_changes: before_raw != after_raw,
        changed_paths,
        changes,
    }
}

fn is_redacted_value(value: &Value) -> bool {
    matches!(value, Value::String(text) if text == "***redacted***")
}
