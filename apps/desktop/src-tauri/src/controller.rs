//! The connection controller: drives [`kvn_core::state::StateMachine`] and
//! executes its [`Action`]s.
//!
//! The state machine itself is pure and fully unit-tested in `kvn-core`;
//! this module supplies the I/O it asks for:
//!
//! - `SpawnEngine`: locate + probe the engine binary, generate and validate
//!   the client config, start the process, wait for the local port,
//!   start the stats poller;
//! - `RunHealthCheck`: a real end-to-end request *through the tunnel*;
//! - `KillEngine`: graceful stop of the process;
//! - retries: bounded backoff timers.
//!
//! Every event produced by these tasks is tagged with an *epoch*; events
//! from an older epoch (after a cancel/switch/stop) are dropped, so a slow
//! spawn can never resurrect a cancelled connection.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kvn_core::state::{Action, ConnectionState, ErrorKind, Event, RetryPolicy, StateMachine};
use kvn_core::xray::{build_client_config, BuildOptions, FragmentOptions};
use kvn_engine::{
    health::default_targets, probe_version, query_stats, spawn_engine, validate_config,
    wait_for_port, EngineProcess, SpawnOptions, TrafficSnapshot,
};
use serde::Serialize;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::eventlog::{now_ms, EventLog};
use crate::paths::AppPaths;
use crate::store::{Settings, Store};
use crate::sysproxy;

#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub path: String,
    pub version: String,
    pub source: String,
}

/// State shared between the controller, the watchdog and the commands.
pub struct Shared {
    pub state: RwLock<ConnectionState>,
    pub traffic: RwLock<TrafficSnapshot>,
    pub latency_ms: RwLock<Option<u32>>,
    pub connected_since_ms: RwLock<Option<u64>>,
    pub engine_info: RwLock<Option<EngineInfo>>,
    pub api_port: RwLock<u16>,
    pub slot: Mutex<Option<EngineProcess>>,
    /// Bumped whenever pending scoped events must be invalidated.
    pub epoch: AtomicU64,
    /// Bumped whenever a new engine instance starts (stats pollers follow).
    pub instance: AtomicU64,
    /// Test mode: simulated engine, no real process.
    pub mock: AtomicBool,
    /// Whether the system proxy currently points at our tunnel port.
    pub sysproxy_on: AtomicBool,
}

impl Shared {
    pub fn new() -> Self {
        Shared {
            state: RwLock::new(ConnectionState::Disconnected),
            traffic: RwLock::new(TrafficSnapshot::default()),
            latency_ms: RwLock::new(None),
            connected_since_ms: RwLock::new(None),
            engine_info: RwLock::new(None),
            api_port: RwLock::new(0),
            slot: Mutex::new(None),
            epoch: AtomicU64::new(0),
            instance: AtomicU64::new(0),
            mock: AtomicBool::new(false),
            sysproxy_on: AtomicBool::new(false),
        }
    }

    pub async fn is_mock(&self) -> bool {
        self.mock.load(Ordering::SeqCst)
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

struct Msg {
    ev: Event,
    /// `Some(epoch)` — only accepted while that epoch is current.
    epoch: Option<u64>,
}

/// What the controller tasks need; shared, cheap to clone around.
pub struct Ctx {
    pub store: Arc<Store>,
    pub log: Arc<EventLog>,
    pub shared: Arc<Shared>,
    pub paths: AppPaths,
    tx: mpsc::UnboundedSender<Msg>,
}

impl Ctx {
    fn send_scoped(&self, ev: Event, epoch: u64) {
        let _ = self.tx.send(Msg {
            ev,
            epoch: Some(epoch),
        });
    }

    /// Facts about the real process (engine stopped / exited) are not tied
    /// to a request generation: they must always reach the state machine.
    /// Sent as scoped, an `EngineStopped` after the first epoch bump would
    /// be dropped as stale and the machine would hang in `Stopping`.
    fn send_fact(&self, ev: Event) {
        let _ = self.tx.send(Msg { ev, epoch: None });
    }
}

#[derive(Clone)]
pub struct Controller {
    tx: mpsc::UnboundedSender<Msg>,
    shared: Arc<Shared>,
}

impl Controller {
    pub fn start(
        store: Arc<Store>,
        log: Arc<EventLog>,
        shared: Arc<Shared>,
        paths: AppPaths,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let ctx = Arc::new(Ctx {
            store,
            log,
            shared: Arc::clone(&shared),
            paths,
            tx: tx.clone(),
        });
        tauri::async_runtime::spawn(run_loop(Arc::clone(&ctx), rx));
        tauri::async_runtime::spawn(watchdog(Arc::clone(&ctx)));
        Controller { tx, shared }
    }

    /// User/command-level event (never epoch-scoped).
    pub fn send(&self, ev: Event) {
        let _ = self.tx.send(Msg { ev, epoch: None });
    }

    pub fn stop(&self) {
        self.send(Event::StopRequested);
    }

    pub async fn state(&self) -> ConnectionState {
        self.shared.state.read().await.clone()
    }

    /// Force the engine process down (used at app shutdown).
    pub async fn shutdown_engine(&self) {
        self.shared.instance.fetch_add(1, Ordering::SeqCst);
        if let Some(process) = self.shared.slot.lock().await.take() {
            let _ = process.stop(Duration::from_millis(800)).await;
        }
    }
}

/// Scoped events are dropped once their generation is no longer current;
/// unscoped events (facts, user commands) always pass.
fn event_is_current(msg_epoch: Option<u64>, current: u64) -> bool {
    match msg_epoch {
        Some(epoch) => epoch == current,
        None => true,
    }
}

async fn run_loop(ctx: Arc<Ctx>, mut rx: mpsc::UnboundedReceiver<Msg>) {
    let mut sm = StateMachine::new(RetryPolicy::default());
    let mut last_line = String::new();
    while let Some(msg) = rx.recv().await {
        if let Some(epoch) = msg.epoch {
            if !event_is_current(Some(epoch), ctx.shared.epoch.load(Ordering::SeqCst)) {
                ctx.log.push(
                    "debug",
                    "state",
                    format!(
                        "dropped a stale event ({}): {}",
                        describe_event(&msg.ev),
                        epoch
                    ),
                );
                continue;
            }
        }
        let actions = sm.handle(msg.ev);
        let state = sm.state().clone();
        apply_state(&ctx, &state).await;

        let line = transition_line(&state);
        if line != last_line {
            last_line = line;
            let level = match &state {
                ConnectionState::Error { .. } => "error",
                ConnectionState::Reconnecting { .. } => "warn",
                _ => "info",
            };
            ctx.log.push(level, "state", describe_state(&ctx, &state));
        }

        for action in actions {
            execute(&ctx, action, &state);
        }
    }
}

fn execute(ctx: &Arc<Ctx>, action: Action, state: &ConnectionState) {
    match action {
        Action::SpawnEngine => {
            let epoch = ctx.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let profile_id = match state {
                ConnectionState::Connecting { profile_id, .. } => profile_id.clone(),
                other => {
                    ctx.log.push(
                        "warn",
                        "state",
                        format!(
                            "SpawnEngine without a target profile in state `{}`",
                            other.label()
                        ),
                    );
                    return;
                }
            };
            let ctx = Arc::clone(ctx);
            tauri::async_runtime::spawn(async move {
                let event = match do_spawn(&ctx, &profile_id, epoch).await {
                    Ok(()) => Event::EngineReady,
                    Err((kind, message)) => Event::EngineSpawnFailed { kind, message },
                };
                ctx.send_scoped(event, epoch);
            });
        }
        Action::KillEngine => {
            let ctx = Arc::clone(ctx);
            tauri::async_runtime::spawn(async move {
                do_kill(&ctx).await;
                ctx.send_fact(Event::EngineStopped);
                // A fact: the process is actually gone. It must never be
                // dropped by the epoch guard, or `Stopping` never resolves.
            });
        }
        Action::RunHealthCheck => {
            let epoch = ctx.shared.epoch.load(Ordering::SeqCst);
            let ctx = Arc::clone(ctx);
            tauri::async_runtime::spawn(async move {
                let event = match do_health(&ctx).await {
                    Ok(latency_ms) => Event::HealthCheckPassed { latency_ms },
                    Err((kind, message)) => Event::HealthCheckFailed { kind, message },
                };
                ctx.send_scoped(event, epoch);
            });
        }
        Action::ScheduleRetry { delay_ms, attempt } => {
            let epoch = ctx.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let ctx = Arc::clone(ctx);
            ctx.log.push(
                "info",
                "retry",
                format!("retrying in {delay_ms} ms (attempt {attempt})"),
            );
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                ctx.send_scoped(Event::RetryTimerFired, epoch);
            });
        }
        Action::CancelRetry => {
            ctx.shared.epoch.fetch_add(1, Ordering::SeqCst);
        }
    }
}

async fn apply_state(ctx: &Arc<Ctx>, next: &ConnectionState) {
    {
        let mut state = ctx.shared.state.write().await;
        *state = next.clone();
    }

    let engine_involved = next.engine_involved();
    match next {
        ConnectionState::Connected { latency_ms, .. } => {
            *ctx.shared.latency_ms.write().await = *latency_ms;
            let mut since = ctx.shared.connected_since_ms.write().await;
            if since.is_none() {
                *since = Some(now_ms());
            }
        }
        _ => {
            *ctx.shared.connected_since_ms.write().await = None;
            *ctx.shared.latency_ms.write().await = None;
        }
    }
    if !engine_involved {
        *ctx.shared.traffic.write().await = TrafficSnapshot::default();
    }

    // System proxy follows the connection when the user asked for it.
    let settings = ctx.store.snapshot().settings;
    if matches!(next, ConnectionState::Connected { .. }) {
        if settings.system_proxy_desired
            && sysproxy::available()
            && !ctx.shared.sysproxy_on.load(Ordering::SeqCst)
        {
            let paths = ctx.paths.clone();
            let log = Arc::clone(&ctx.log);
            let shared = Arc::clone(&ctx.shared);
            tauri::async_runtime::spawn(async move {
                match enable_sysproxy(&paths, &log, &shared, settings.http_port).await {
                    Ok(()) => log.push("info", "sysproxy", "system proxy now points at the tunnel"),
                    Err(e) => log.push("error", "sysproxy", e),
                }
            });
        }
    } else if !engine_involved && ctx.shared.sysproxy_on.load(Ordering::SeqCst) {
        let paths = ctx.paths.clone();
        let log = Arc::clone(&ctx.log);
        let shared = Arc::clone(&ctx.shared);
        tauri::async_runtime::spawn(async move {
            match disable_sysproxy(&paths, &log).await {
                Ok(()) => {
                    shared.sysproxy_on.store(false, Ordering::SeqCst);
                    log.push("info", "sysproxy", "system proxy restored");
                }
                Err(e) => {
                    log.push("error", "sysproxy", e);
                }
            }
        });
    }
}

// ------------------------------------------------------------ sysproxy flow --

pub async fn enable_sysproxy(
    paths: &AppPaths,
    log: &EventLog,
    shared: &Shared,
    http_port: u16,
) -> Result<(), String> {
    let paths = paths.clone();
    let result = tokio::task::spawn_blocking(move || {
        let backup = sysproxy::set_proxy(http_port)?;
        sysproxy::save_backup(&paths, &backup)?;
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| format!("sysproxy task failed: {e}"))?;
    match result {
        Ok(()) => {
            shared.sysproxy_on.store(true, Ordering::SeqCst);
            let _ = log;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub async fn disable_sysproxy(paths: &AppPaths, log: &EventLog) -> Result<(), String> {
    let paths = paths.clone();
    let result = tokio::task::spawn_blocking(move || {
        let path = paths.sysproxy_backup();
        let Some(text) = std::fs::read_to_string(&path).ok() else {
            return Ok(());
        };
        let backup: sysproxy::ProxyBackup =
            serde_json::from_str(&text).map_err(|e| format!("proxy backup unreadable: {e}"))?;
        sysproxy::restore(&backup)?;
        sysproxy::clear_backup(&paths);
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| format!("sysproxy task failed: {e}"))?;
    if let Err(e) = &result {
        log.push("warn", "sysproxy", e.clone());
    }
    result
}

// ------------------------------------------------------------- spawn / kill --

async fn do_spawn(ctx: &Arc<Ctx>, profile_id: &str, epoch: u64) -> Result<(), (ErrorKind, String)> {
    // Never leave a previous process behind, even if teardown was missed.
    if let Some(previous) = ctx.shared.slot.lock().await.take() {
        let pid = previous.pid();
        let _ = previous.stop(Duration::from_millis(600)).await;
        ctx.log.push(
            "debug",
            "engine",
            format!("replaced a leftover engine process (pid {pid})"),
        );
    }
    *ctx.shared.traffic.write().await = TrafficSnapshot::default();

    let data = ctx.store.snapshot();
    let settings = data.settings.clone();
    let profile = data.profile(profile_id).cloned().ok_or_else(|| {
        (
            ErrorKind::ConfigInvalid,
            "the selected profile no longer exists — import or pick another one".to_owned(),
        )
    })?;

    if settings.test_mode {
        ctx.shared.mock.store(true, Ordering::SeqCst);
        ctx.log.push(
            "warn",
            "engine",
            format!(
                "test mode: simulating an engine for `{}` (no real traffic)",
                profile.name
            ),
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
        return Ok(());
    }
    ctx.shared.mock.store(false, Ordering::SeqCst);

    let (binary, source) = locate_engine(&settings, &ctx.paths).await?;
    let version = probe_version(&binary).await.map_err(|e| {
        (
            ErrorKind::EngineMissing,
            format!(
                "`xray version` did not answer for {}: {e}",
                binary.display()
            ),
        )
    })?;
    *ctx.shared.engine_info.write().await = Some(EngineInfo {
        path: binary.display().to_string(),
        version: version.version.clone(),
        source: source.to_owned(),
    });
    ctx.log.push(
        "info",
        "engine",
        format!("using xray {} ({source})", version.version),
    );

    let api_port = free_port().ok_or_else(|| {
        (
            ErrorKind::Internal,
            "could not reserve a local port for the stats API".to_owned(),
        )
    })?;
    *ctx.shared.api_port.write().await = api_port;

    let build_opts = BuildOptions {
        socks_port: settings.socks_port,
        http_port: settings.http_port,
        api_port,
        dns_remote: settings.dns_remote.clone(),
        sniffing: settings.sniffing,
        bypass_lan: settings.bypass_lan,
        bypass_domains: settings.bypass_domains.clone(),
        bypass_ips: settings.bypass_ips.clone(),
        log_level: settings.log_level.clone(),
        error_log: Some(ctx.paths.logs_dir.join("engine.log").display().to_string()),
        fragment: settings.fragment_enabled.then(|| FragmentOptions {
            packets: settings.fragment_packets.clone(),
            length: settings.fragment_length.clone(),
            interval: settings.fragment_interval.clone(),
        }),
    };
    let outcome = build_client_config(&profile, &build_opts)
        .map_err(|e| (ErrorKind::ConfigInvalid, e.to_string()))?;
    for note in &outcome.notes {
        ctx.log.push("warn", "config", note.clone());
    }

    let config_path = ctx.paths.runtime_dir.join("client.json");
    crate::store::write_private_json(&config_path, &outcome.config).map_err(|e| {
        (
            ErrorKind::Internal,
            format!("could not write the engine config: {e}"),
        )
    })?;

    validate_config(&binary, &config_path).await.map_err(|e| {
        (
            ErrorKind::ConfigInvalid,
            format!("the engine rejected the generated config: {e}"),
        )
    })?;

    let stderr_log = ctx.paths.logs_dir.join("engine-stderr.log");
    let process = spawn_engine(&SpawnOptions {
        binary: binary.clone(),
        config_path,
        stderr_log: Some(stderr_log.clone()),
        work_dir: None,
    })
    .await
    .map_err(|e| {
        (
            ErrorKind::EngineFailed,
            format!("could not start the engine: {e}"),
        )
    })?;

    let instance = ctx.shared.instance.fetch_add(1, Ordering::SeqCst) + 1;
    *ctx.shared.slot.lock().await = Some(process);

    // If the user cancelled (or switched) while we were starting, discard
    // this instance instead of leaving an orphan process running.
    if epoch != ctx.shared.epoch.load(Ordering::SeqCst) {
        ctx.shared.instance.fetch_add(1, Ordering::SeqCst);
        if let Some(process) = ctx.shared.slot.lock().await.take() {
            let _ = process.stop(Duration::from_millis(600)).await;
        }
        return Err((
            ErrorKind::Internal,
            "spawn was superseded by a newer request".to_owned(),
        ));
    }

    if let Err(e) = wait_for_port(settings.socks_port, Duration::from_secs(8)).await {
        let tail = log_tail(
            &[ctx.paths.logs_dir.join("engine.log"), stderr_log.clone()],
            10,
        );
        if let Some(process) = ctx.shared.slot.lock().await.take() {
            let _ = process.stop(Duration::from_secs(2)).await;
        }
        return Err((
            ErrorKind::EngineFailed,
            format!("the engine started but its proxy port never opened: {e}{tail}"),
        ));
    }

    ctx.log.push(
        "info",
        "engine",
        format!(
            "engine is up (socks 127.0.0.1:{}, stats api 127.0.0.1:{api_port})",
            settings.socks_port
        ),
    );
    start_stats_poller(ctx, binary, api_port, instance);
    Ok(())
}

fn start_stats_poller(ctx: &Arc<Ctx>, binary: PathBuf, api_port: u16, instance: u64) {
    let ctx = Arc::clone(ctx);
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if ctx.shared.instance.load(Ordering::SeqCst) != instance {
                break;
            }
            let involved = ctx.shared.state.read().await.engine_involved();
            if !involved {
                break;
            }
            match query_stats(&binary, api_port, Duration::from_secs(5)).await {
                Ok(snapshot) => {
                    *ctx.shared.traffic.write().await = snapshot;
                }
                Err(e) => {
                    ctx.log.push("debug", "stats", e.to_string());
                }
            }
        }
    });
}

async fn do_kill(ctx: &Arc<Ctx>) {
    ctx.shared.instance.fetch_add(1, Ordering::SeqCst);
    *ctx.shared.api_port.write().await = 0;
    let taken = ctx.shared.slot.lock().await.take();
    match taken {
        Some(process) => {
            let pid = process.pid();
            match process.stop(Duration::from_secs(3)).await {
                Ok(()) => {
                    ctx.log
                        .push("info", "engine", format!("engine stopped (pid {pid})"));
                }
                Err(e) => {
                    ctx.log
                        .push("warn", "engine", format!("engine stop reported: {e}"));
                }
            }
        }
        None => {
            ctx.log.push("debug", "engine", "no engine process to stop");
        }
    }
}

async fn do_health(ctx: &Arc<Ctx>) -> Result<u32, (ErrorKind, String)> {
    if ctx.shared.is_mock().await {
        tokio::time::sleep(Duration::from_millis(150)).await;
        return Ok(35);
    }
    let settings = ctx.store.snapshot().settings;
    let targets = default_targets();
    match kvn_engine::health::check_through_socks_any(
        settings.socks_port,
        &targets,
        Duration::from_secs(12),
    )
    .await
    {
        Ok(latency_ms) => {
            ctx.log.push(
                "info",
                "health",
                format!("real traffic through the tunnel confirmed ({latency_ms} ms)"),
            );
            Ok(latency_ms)
        }
        Err(e) => {
            let message = e.to_string();
            let kind = if message.contains("local proxy port") {
                ErrorKind::EngineFailed
            } else if message.contains("no response through the tunnel") {
                ErrorKind::NetworkUnreachable
            } else {
                ErrorKind::HealthCheckFailed
            };
            Err((kind, message))
        }
    }
}

/// A supervised process that exits on its own sends `EngineExited`.
async fn watchdog(ctx: Arc<Ctx>) {
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let exited = {
            let mut slot = ctx.shared.slot.lock().await;
            match slot.as_mut() {
                Some(process) => {
                    let pid = process.pid();
                    process.try_exit().await.map(|status| (pid, status.code()))
                }
                None => None,
            }
        };
        if let Some((pid, code)) = exited {
            if let Some(process) = ctx.shared.slot.lock().await.take() {
                // Reap the zombie (harmless SIGTERM + wait inside).
                let _ = process.stop(Duration::from_millis(200)).await;
            }
            ctx.log.push(
                "warn",
                "engine",
                match code {
                    Some(code) => format!("engine process {pid} exited with code {code}"),
                    None => format!("engine process {pid} was terminated by a signal"),
                },
            );
            ctx.send_fact(Event::EngineExited { code });
            // A fact about the actual process, not a request generation:
            // the state machine ignores it in states where it makes no
            // sense (e.g. during an intentional teardown).
        }
    }
}

// ----------------------------------------------------------------- helpers ---

pub fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    listener.local_addr().ok().map(|addr| addr.port())
}

/// Locate a usable engine: settings override → app-installed → PATH.
async fn locate_engine(
    settings: &Settings,
    paths: &AppPaths,
) -> Result<(PathBuf, &'static str), (ErrorKind, String)> {
    if let Some(custom) = settings
        .engine_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = PathBuf::from(custom);
        if !path.is_file() {
            return Err((
                ErrorKind::EngineMissing,
                format!(
                    "the configured engine path does not exist: {}",
                    path.display()
                ),
            ));
        }
        if let Err(e) = probe_version(&path).await {
            return Err((
                ErrorKind::EngineMissing,
                format!(
                    "the binary at {} is not a working Xray engine: {e}",
                    path.display()
                ),
            ));
        }
        return Ok((path, "custom path"));
    }

    let installed = paths.engine_binary();
    if installed.is_file() && probe_version(&installed).await.is_ok() {
        return Ok((installed, "installed by KVN"));
    }

    if let Some(bundled) = kvn_engine::bundled_engine_path() {
        if probe_version(&bundled).await.is_ok() {
            return Ok((bundled, "bundled with app"));
        }
    }

    if let Some(path) = which_xray() {
        if probe_version(&path).await.is_ok() {
            return Ok((path, "PATH"));
        }
    }

    Err((
        ErrorKind::EngineMissing,
        "no Xray engine found: install it in Settings, point KVN at your own binary, or put `xray` on PATH"
            .to_owned(),
    ))
}

pub fn which_xray() -> Option<PathBuf> {
    let name = if cfg!(windows) { "xray.exe" } else { "xray" };
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Best-effort tail of engine log files for error messages.
pub fn log_tail(paths: &[PathBuf], lines: usize) -> String {
    let mut collected: Vec<String> = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let tail: Vec<&str> = text.lines().rev().take(lines).collect();
        if !tail.is_empty() {
            collected.push(format!(
                "\n--- {} (last {} lines) ---\n{}\n---",
                path.display(),
                tail.len(),
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            ));
        }
    }
    collected.join("")
}

fn describe_event(ev: &Event) -> &'static str {
    match ev {
        Event::ConnectRequested { .. } => "ConnectRequested",
        Event::EngineReady => "EngineReady",
        Event::EngineSpawnFailed { .. } => "EngineSpawnFailed",
        Event::HealthCheckPassed { .. } => "HealthCheckPassed",
        Event::HealthCheckFailed { .. } => "HealthCheckFailed",
        Event::EngineExited { .. } => "EngineExited",
        Event::StopRequested => "StopRequested",
        Event::EngineStopped => "EngineStopped",
        Event::RetryTimerFired => "RetryTimerFired",
    }
}

fn describe_state(ctx: &Ctx, state: &ConnectionState) -> String {
    let name_of = |id: &str| -> String {
        ctx.store
            .snapshot()
            .profile(id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "?".to_owned())
    };
    match state {
        ConnectionState::Disconnected => "disconnected".into(),
        ConnectionState::Connecting {
            profile_id,
            attempt,
            engine_ready,
        } => {
            let phase = if *engine_ready {
                "checking the tunnel"
            } else {
                "starting the engine"
            };
            format!(
                "connecting to `{}` — {phase} (attempt {attempt})",
                name_of(profile_id)
            )
        }
        ConnectionState::Connected {
            profile_id,
            latency_ms,
        } => {
            let latency = latency_ms
                .map(|l| format!("{l} ms"))
                .unwrap_or_else(|| "?".into());
            format!("connected to `{}` (latency {latency})", name_of(profile_id))
        }
        ConnectionState::Reconnecting {
            profile_id,
            attempt,
        } => {
            format!(
                "connection to `{}` dropped; retry {attempt} scheduled",
                name_of(profile_id)
            )
        }
        ConnectionState::Stopping { pending_profile } => match pending_profile {
            Some(id) => format!("stopping to switch to `{}`…", name_of(id)),
            None => "stopping…".into(),
        },
        ConnectionState::Error { kind, message, .. } => {
            format!("error [{kind:?}]: {message}")
        }
    }
}

/// Short one-liner used to suppress duplicate state log lines.
fn transition_line(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Connecting {
            profile_id,
            attempt,
            engine_ready,
        } => {
            format!("Connecting:{profile_id}:{attempt}:{engine_ready}")
        }
        ConnectionState::Connected { profile_id, .. } => format!("Connected:{profile_id}"),
        ConnectionState::Reconnecting {
            profile_id,
            attempt,
        } => {
            format!("Reconnecting:{profile_id}:{attempt}")
        }
        ConnectionState::Stopping { pending_profile } => {
            format!("Stopping:{pending_profile:?}")
        }
        ConnectionState::Error { kind, .. } => format!("Error:{kind:?}"),
        ConnectionState::Disconnected => "Disconnected".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_returns_usable_port() {
        let port = free_port().expect("a port must be available");
        assert!(port > 0);
    }

    #[test]
    fn which_xray_finds_nothing_with_empty_path() {
        // Can't safely mutate env in parallel tests; just probe the helper's
        // shape: an empty PATH yields None.
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let found = which_xray();
        match saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        assert!(found.is_none());
    }

    #[test]
    fn log_tail_reports_last_lines_only() {
        let dir = std::env::temp_dir().join(format!("kvn-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("engine.log");
        std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
        let tail = log_tail(&[file], 2);
        assert!(tail.contains("three"));
        assert!(tail.contains("four"));
        assert!(!tail.contains("one"));
        let missing = log_tail(&[dir.join("nope.log")], 2);
        assert!(missing.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unscoped_events_always_pass_but_stale_scopes_do_not() {
        // Facts (EngineStopped / EngineExited) go unscoped and must pass.
        assert!(event_is_current(None, 7));
        assert!(event_is_current(Some(7), 7));
        // The regression that hung `Stopping`: a scoped send with epoch 0
        // while the connection was already on epoch 1.
        assert!(!event_is_current(Some(0), 1));
        assert!(!event_is_current(Some(6), 7));
    }

    #[test]
    fn transition_line_is_stable_for_dedupe() {
        let s = |state| transition_line(&state);
        assert_eq!(s(ConnectionState::Disconnected), "Disconnected");
        assert_ne!(
            s(ConnectionState::Connecting {
                profile_id: "a".into(),
                attempt: 1,
                engine_ready: false
            }),
            s(ConnectionState::Connecting {
                profile_id: "a".into(),
                attempt: 2,
                engine_ready: false
            })
        );
    }
}
