//! Tauri commands — the app's whole API surface for the UI.
//!
//! Every command is small and honest: validation errors come back as
//! structured [`ApiError`]s, secrets never appear in messages (URLs are
//! redacted via `kvn_core::secret`), and long-running work (HTTP, process
//! management) is handed to the controller or a blocking task.

use std::sync::Arc;
use std::time::{Duration, Instant};

use kvn_core::secret::redact_url;
use kvn_core::state::{ConnectionState, ErrorKind, Event};
use kvn_core::{Profile, ProfileSource, Security, Transport};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::controller::{self, Controller, Shared};
use crate::eventlog::{now_ms, EventLog, LogEntry};
use crate::paths::AppPaths;
use crate::store::{
    merge_profiles, remove_subscription_profiles, Settings, Store, StoreData, StoredSubscription,
};
use crate::{engine_install, sysproxy};

pub struct AppState {
    pub paths: AppPaths,
    pub store: Arc<Store>,
    pub log: Arc<EventLog>,
    pub shared: Arc<Shared>,
    pub controller: Controller,
    pub installing: std::sync::atomic::AtomicBool,
    /// Set by the first `get_status` call: proves the UI attached.
    pub ui_seen: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub message: String,
    pub detail: Option<String>,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        ApiError {
            message: message.into(),
            detail: None,
        }
    }
    pub fn with_detail(message: impl Into<String>, detail: impl Into<String>) -> Self {
        ApiError {
            message: message.into(),
            detail: Some(detail.into()),
        }
    }
}

impl From<String> for ApiError {
    fn from(value: String) -> Self {
        ApiError::new(value)
    }
}

impl From<&str> for ApiError {
    fn from(value: &str) -> Self {
        ApiError::new(value)
    }
}

// ------------------------------------------------------------------- DTOs ----

#[derive(Debug, Serialize)]
pub struct TrafficDto {
    pub uplink_bytes: u64,
    pub downlink_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct ErrorDto {
    pub kind: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileDto {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub endpoint: String,
    pub transport: String,
    pub security: String,
    pub flow: Option<String>,
    pub source: String,
    pub subscription_name: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusDto {
    pub phase: String,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub profile_summary: Option<String>,
    pub endpoint: Option<String>,
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
    pub latency_ms: Option<u32>,
    pub connected_since_ms: Option<u64>,
    pub error: Option<ErrorDto>,
    pub traffic: TrafficDto,
    pub socks_port: u16,
    pub http_port: u16,
    pub system_proxy: bool,
    pub system_proxy_available: bool,
    pub test_mode: bool,
    pub engine: Option<EngineStatusDto>,
}

#[derive(Debug, Serialize)]
pub struct EngineStatusDto {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub pinned_version: String,
    pub platform_asset: Option<String>,
    pub install_supported: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportOutcomeDto {
    pub added: usize,
    pub replaced: usize,
    pub failed: usize,
    pub issues: Vec<String>,
    pub profile_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionDto {
    pub id: String,
    pub name: String,
    pub url_redacted: String,
    pub last_updated_ms: Option<u64>,
    pub profile_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SettingsDto {
    pub socks_port: u16,
    pub http_port: u16,
    pub dns_remote: String,
    pub bypass_lan: bool,
    pub bypass_domains: Vec<String>,
    pub bypass_ips: Vec<String>,
    pub fragment_enabled: bool,
    pub fragment_packets: String,
    pub fragment_length: String,
    pub fragment_interval: String,
    pub sniffing: bool,
    pub log_level: String,
    pub test_mode: bool,
    pub system_proxy_desired: bool,
    pub language: String,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        SettingsDto {
            socks_port: s.socks_port,
            http_port: s.http_port,
            dns_remote: s.dns_remote.clone(),
            bypass_lan: s.bypass_lan,
            bypass_domains: s.bypass_domains.clone(),
            bypass_ips: s.bypass_ips.clone(),
            fragment_enabled: s.fragment_enabled,
            fragment_packets: s.fragment_packets.clone(),
            fragment_length: s.fragment_length.clone(),
            fragment_interval: s.fragment_interval.clone(),
            sniffing: s.sniffing,
            log_level: s.log_level.clone(),
            test_mode: s.test_mode,
            system_proxy_desired: s.system_proxy_desired,
            language: s.language.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub socks_port: Option<u16>,
    pub http_port: Option<u16>,
    pub dns_remote: Option<String>,
    pub bypass_lan: Option<bool>,
    pub bypass_domains: Option<Vec<String>>,
    pub bypass_ips: Option<Vec<String>>,
    pub fragment_enabled: Option<bool>,
    pub fragment_packets: Option<String>,
    pub fragment_length: Option<String>,
    pub fragment_interval: Option<String>,
    pub sniffing: Option<bool>,
    pub log_level: Option<String>,
    pub test_mode: Option<bool>,
    pub system_proxy_desired: Option<bool>,
    pub language: Option<String>,
}

// ---------------------------------------------------------------- helpers ----

fn kind_str(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::EngineMissing => "engine_missing",
        ErrorKind::EngineFailed => "engine_failed",
        ErrorKind::ConfigInvalid => "config_invalid",
        ErrorKind::HealthCheckFailed => "health_check_failed",
        ErrorKind::NetworkUnreachable => "network_unreachable",
        ErrorKind::AuthFailed => "auth_failed",
        ErrorKind::Internal => "internal",
    }
}

fn profile_warnings(profile: &Profile) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Security::Tls(tls) = &profile.security {
        if tls.allow_insecure {
            warnings.push("TLS verification disabled (allowInsecure)".into());
        }
    }
    if matches!(
        profile.transport,
        Transport::Kcp { .. } | Transport::Quic { .. }
    ) {
        warnings.push("transport is not buildable yet".into());
    }
    warnings
}

fn profile_dto(data: &StoreData, profile: &Profile) -> ProfileDto {
    let subscription_name = match &profile.source {
        ProfileSource::Subscription { subscription_id } => data
            .subscriptions
            .iter()
            .find(|s| &s.id == subscription_id)
            .map(|s| s.name.clone()),
        _ => None,
    };
    let source = match &profile.source {
        ProfileSource::Manual => "manual",
        ProfileSource::UriImport => "import",
        ProfileSource::Subscription { .. } => "subscription",
    };
    let flow = match &profile.protocol {
        kvn_core::ProtocolSpec::Vless { flow, .. }
        | kvn_core::ProtocolSpec::Trojan { flow, .. } => flow.clone(),
        _ => None,
    };
    ProfileDto {
        id: profile.id.clone(),
        name: profile.name.clone(),
        protocol: profile.protocol().as_str().to_owned(),
        endpoint: profile.endpoint.label(),
        transport: profile.transport.label().to_owned(),
        security: profile.security.label().to_owned(),
        flow,
        source: source.to_owned(),
        subscription_name,
        warnings: profile_warnings(profile),
    }
}

fn state_profile_id(connection: &ConnectionState, data: &StoreData) -> Option<String> {
    match connection {
        ConnectionState::Connecting { profile_id, .. }
        | ConnectionState::Connected { profile_id, .. }
        | ConnectionState::Reconnecting { profile_id, .. } => Some(profile_id.clone()),
        ConnectionState::Stopping { pending_profile } => pending_profile
            .clone()
            .or_else(|| data.active_profile.clone()),
        ConnectionState::Disconnected | ConnectionState::Error { .. } => {
            data.active_profile.clone()
        }
    }
}

// ---------------------------------------------------------------- commands ---

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<StatusDto, ApiError> {
    if !state
        .ui_seen
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        state.log.push(
            "info",
            "ui",
            "user interface attached (first status request)",
        );
    }
    let data = state.store.snapshot();
    let connection = state.shared.state.read().await.clone();
    let traffic = {
        let t = state.shared.traffic.read().await;
        TrafficDto {
            uplink_bytes: t.uplink_bytes,
            downlink_bytes: t.downlink_bytes,
        }
    };
    let latency_ms = *state.shared.latency_ms.read().await;
    let connected_since_ms = *state.shared.connected_since_ms.read().await;

    let profile_id = state_profile_id(&connection, &data);
    let active_profile = profile_id.as_deref().and_then(|id| data.profile(id));

    let (attempt, max_attempts) = match &connection {
        ConnectionState::Connecting { attempt, .. } => (
            Some(*attempt),
            Some(kvn_core::state::RetryPolicy::default().max_attempts),
        ),
        ConnectionState::Reconnecting { attempt, .. } => (
            Some(*attempt),
            Some(kvn_core::state::RetryPolicy::default().max_attempts),
        ),
        _ => (None, None),
    };

    let error = match &connection {
        ConnectionState::Error {
            kind,
            message,
            retryable,
        } => Some(ErrorDto {
            kind: kind_str(*kind).to_owned(),
            message: message.clone(),
            retryable: *retryable,
        }),
        _ => None,
    };

    Ok(StatusDto {
        phase: match &connection {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Connecting {
                engine_ready: false,
                ..
            } => "connecting",
            ConnectionState::Connecting {
                engine_ready: true, ..
            } => "checking",
            ConnectionState::Connected { .. } => "connected",
            ConnectionState::Reconnecting { .. } => "reconnecting",
            ConnectionState::Stopping { .. } => "stopping",
            ConnectionState::Error { .. } => "error",
        }
        .to_owned(),
        profile_id,
        profile_name: active_profile.map(|p| p.name.clone()),
        profile_summary: active_profile.map(|p| p.summary()),
        endpoint: active_profile.map(|p| p.endpoint.label()),
        attempt,
        max_attempts,
        latency_ms,
        connected_since_ms,
        error,
        traffic,
        socks_port: data.settings.socks_port,
        http_port: data.settings.http_port,
        system_proxy: state
            .shared
            .sysproxy_on
            .load(std::sync::atomic::Ordering::SeqCst),
        system_proxy_available: sysproxy::available(),
        test_mode: data.settings.test_mode,
        engine: None,
    })
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Vec<ProfileDto> {
    let data = state.store.snapshot();
    data.profiles
        .iter()
        .map(|p| profile_dto(&data, p))
        .collect()
}

#[tauri::command]
pub async fn import_from_text(
    state: State<'_, AppState>,
    payload: String,
    subscription_name: Option<String>,
) -> Result<ImportOutcomeDto, ApiError> {
    if payload.trim().is_empty() {
        return Err(ApiError::new("nothing to import"));
    }
    let source = match &subscription_name {
        Some(id) => ProfileSource::Subscription {
            subscription_id: id.clone(),
        },
        None => ProfileSource::UriImport,
    };
    let report = kvn_core::uri::parse_import_payload(&payload, source);
    if report.profiles.is_empty() && report.issues.is_empty() {
        return Err(ApiError::new(
            "the payload contained no links and no parseable issues",
        ));
    }
    let issues: Vec<String> = report
        .issues
        .iter()
        .map(|i| {
            if i.input.is_empty() {
                i.message.clone()
            } else {
                format!("{} — {}", i.input, i.message)
            }
        })
        .collect();
    let failed = issues.len();

    let (added, replaced, profile_ids) = state.store.transaction(&state.log, |data| {
        let (added, replaced, ids) = merge_profiles(data, report.profiles);
        if data.active_profile.is_none() {
            data.active_profile = ids.first().cloned();
        }
        (added, replaced, ids)
    });

    state.log.push(
        "info",
        "import",
        format!("import finished: {added} added, {replaced} already known, {failed} failed"),
    );
    Ok(ImportOutcomeDto {
        added,
        replaced,
        failed,
        issues,
        profile_ids,
    })
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), ApiError> {
    let connection = state.shared.state.read().await.clone();
    if let Some(busy) = state_profile_id(&connection, &state.store.snapshot()) {
        if busy == id && connection.engine_involved() {
            return Err(ApiError::new(
                "disconnect before deleting the profile in use",
            ));
        }
    }
    let found = state.store.transaction(&state.log, |data| {
        let before = data.profiles.len();
        data.profiles.retain(|p| p.id != id);
        if data.active_profile.as_deref() == Some(id.as_str()) {
            data.active_profile = None;
        }
        before != data.profiles.len()
    });
    if found {
        state.log.push("info", "profiles", "profile deleted");
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_profile(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), ApiError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.len() > 80 {
        return Err(ApiError::new("the name must be 1–80 characters"));
    }
    let found = state.store.transaction(&state.log, |data| {
        match data.profiles.iter_mut().find(|p| p.id == id) {
            Some(profile) => {
                profile.name = name.clone();
                true
            }
            None => false,
        }
    });
    if !found {
        return Err(ApiError::new("profile not found"));
    }
    Ok(())
}

#[tauri::command]
pub async fn set_active_profile(state: State<'_, AppState>, id: String) -> Result<(), ApiError> {
    let known = state.store.transaction(&state.log, |data| {
        if data.profile(&id).is_some() {
            data.active_profile = Some(id.clone());
            true
        } else {
            false
        }
    });
    if !known {
        return Err(ApiError::new("profile not found"));
    }
    Ok(())
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<(), ApiError> {
    let data = state.store.snapshot();
    let id = profile_id
        .or_else(|| data.active_profile.clone())
        .ok_or_else(|| ApiError::new("no profile selected — import one first"))?;
    if data.profile(&id).is_none() {
        return Err(ApiError::new("profile not found"));
    }
    let connection = state.shared.state.read().await.clone();
    if matches!(connection, ConnectionState::Stopping { .. }) {
        return Err(ApiError::new(
            "the previous connection is still stopping — try again in a moment",
        ));
    }
    let id_clone = id.clone();
    state.store.transaction(&state.log, |data| {
        data.active_profile = Some(id_clone);
    });
    state
        .controller
        .send(Event::ConnectRequested { profile_id: id });
    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), ApiError> {
    state.controller.stop();
    Ok(())
}

#[tauri::command]
pub async fn ping_profile(state: State<'_, AppState>, id: String) -> Result<u32, ApiError> {
    let data = state.store.snapshot();
    let profile = data
        .profile(&id)
        .ok_or_else(|| ApiError::new("profile not found"))?;
    let address = profile.endpoint.address.clone();
    let port = profile.endpoint.port;
    let started = Instant::now();
    let mut addrs = tokio::net::lookup_host((address.as_str(), port))
        .await
        .map_err(|e| ApiError::with_detail("cannot resolve the server address", e.to_string()))?;
    let target = addrs
        .next()
        .ok_or_else(|| ApiError::new("the address resolved to no IP"))?;
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(target),
    )
    .await
    .map_err(|_| ApiError::new("the server did not answer within 5 s"))?
    .map_err(|e| ApiError::with_detail("connection failed", e.to_string()))?;
    Ok(started.elapsed().as_millis() as u32)
}

#[tauri::command]
pub fn get_events(state: State<'_, AppState>, after_id: u64) -> Vec<LogEntry> {
    state.log.since(after_id)
}

#[tauri::command]
pub fn clear_events(state: State<'_, AppState>) {
    state.log.clear();
}

// ------------------------------------------------------------ subscriptions --

const USER_AGENT: &str = concat!(
    "kvn/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/zeroscrypt/kvn)"
);

async fn fetch_text(url: &str, log: &EventLog) -> Result<String, ApiError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ApiError::new(
            "the subscription URL must start with http:// or https://",
        ));
    }
    let url_owned = url.to_owned();
    let fetched = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut response = ureq::get(&url_owned)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "*/*")
            .call()
            .map_err(|e| e.to_string())?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| ApiError::new(format!("fetch task failed: {e}")))?;

    match fetched {
        Ok(text) => {
            log.push(
                "info",
                "subs",
                format!("fetched {} ({} bytes)", redact_url(url), text.len()),
            );
            Ok(text)
        }
        Err(e) => Err(ApiError::with_detail(
            format!("could not fetch {}", redact_url(url)),
            e,
        )),
    }
}

#[tauri::command]
pub fn subscription_list(state: State<'_, AppState>) -> Vec<SubscriptionDto> {
    let data = state.store.snapshot();
    data.subscriptions
        .iter()
        .map(|sub| SubscriptionDto {
            id: sub.id.clone(),
            name: sub.name.clone(),
            url_redacted: redact_url(sub.url.expose()),
            last_updated_ms: sub.last_updated_ms,
            profile_count: data
                .profiles
                .iter()
                .filter(|p| {
                    matches!(&p.source, ProfileSource::Subscription { subscription_id } if subscription_id == &sub.id)
                })
                .count(),
        })
        .collect()
}

#[tauri::command]
pub async fn subscription_add(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> Result<(), ApiError> {
    let url = url.trim().to_owned();
    let text = fetch_text(&url, &state.log).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let name = if name.trim().is_empty() {
        "subscription".to_owned()
    } else {
        name.trim().to_owned()
    };
    let report = kvn_core::uri::parse_import_payload(
        &text,
        ProfileSource::Subscription {
            subscription_id: id.clone(),
        },
    );
    if report.profiles.is_empty() {
        let first = report
            .issues
            .first()
            .map(|i| format!("{} — {}", i.input, i.message))
            .unwrap_or_else(|| "the body contained no recognizable links".to_owned());
        return Err(ApiError::with_detail(
            "the subscription returned no usable profiles",
            first,
        ));
    }
    let (added, replaced) = state.store.transaction(&state.log, |data| {
        let (added, replaced, ids) = merge_profiles(data, report.profiles);
        if data.active_profile.is_none() {
            data.active_profile = ids.first().cloned();
        }
        data.subscriptions.push(StoredSubscription {
            id: id.clone(),
            name: name.clone(),
            url: kvn_core::Secret::new(url.clone()),
            last_updated_ms: Some(now_ms()),
        });
        (added, replaced)
    });
    state.log.push(
        "info",
        "subs",
        format!("subscription `{name}` added: {added} new, {replaced} updated"),
    );
    Ok(())
}

#[tauri::command]
pub async fn subscription_update(state: State<'_, AppState>, id: String) -> Result<(), ApiError> {
    let data = state.store.snapshot();
    let sub = data
        .subscriptions
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| ApiError::new("subscription not found"))?;
    let text = fetch_text(sub.url.expose(), &state.log).await?;
    let source = ProfileSource::Subscription {
        subscription_id: id.clone(),
    };
    let report = kvn_core::uri::parse_import_payload(&text, source);
    if report.profiles.is_empty() {
        return Err(ApiError::new(
            "the subscription returned no usable profiles; existing ones were kept",
        ));
    }

    let counts = state.store.transaction(&state.log, |data| {
        let keep_active = data.active_profile.clone();
        let old: Vec<Profile> = data
            .profiles
            .iter()
            .filter(|p| {
                matches!(&p.source, ProfileSource::Subscription { subscription_id } if subscription_id == &id)
            })
            .cloned()
            .collect();
        remove_subscription_profiles(data, &id);
        for mut incoming in report.profiles {
            if let Some(previous) = old.iter().find(|o| o.same_server_as(&incoming)) {
                incoming.id = previous.id.clone();
                incoming.name = previous.name.clone();
            }
            if let Some(existing) = data
                .profiles
                .iter_mut()
                .find(|e| e.same_server_as(&incoming))
            {
                let keep_id = existing.id.clone();
                let keep_name = existing.name.clone();
                let keep_source = existing.source.clone();
                *existing = incoming;
                existing.id = keep_id;
                existing.name = keep_name;
                existing.source = keep_source;
            } else {
                data.profiles.push(incoming);
            }
        }
        if let Some(active) = keep_active {
            if data.profile(&active).is_some() {
                data.active_profile = Some(active);
            }
        }
        if let Some(sub) = data.subscriptions.iter_mut().find(|s| s.id == id) {
            sub.last_updated_ms = Some(now_ms());
        }
        data.profiles
            .iter()
            .filter(|p| {
                matches!(&p.source, ProfileSource::Subscription { subscription_id } if subscription_id == &id)
            })
            .count()
    });
    state.log.push(
        "info",
        "subs",
        format!("subscription `{}` updated: {counts} profiles", sub.name),
    );
    Ok(())
}

#[tauri::command]
pub async fn subscription_delete(state: State<'_, AppState>, id: String) -> Result<(), ApiError> {
    let name = state.store.transaction(&state.log, |data| {
        let name = data
            .subscriptions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone());
        data.subscriptions.retain(|s| s.id != id);
        remove_subscription_profiles(data, &id);
        name
    });
    if let Some(name) = name {
        state
            .log
            .push("info", "subs", format!("subscription `{name}` deleted"));
    }
    Ok(())
}

// ---------------------------------------------------------------- settings ---

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> SettingsDto {
    SettingsDto::from(&state.store.snapshot().settings)
}

#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<SettingsDto, ApiError> {
    let engine_involved = state.shared.state.read().await.clone().engine_involved();
    // Only patch fields that actually feed the engine config justify the
    // "applies after the next connection" warning (language, test mode and
    // system-proxy desire do not).
    let touches_engine = patch.socks_port.is_some()
        || patch.http_port.is_some()
        || patch.dns_remote.is_some()
        || patch.bypass_lan.is_some()
        || patch.bypass_domains.is_some()
        || patch.bypass_ips.is_some()
        || patch.fragment_enabled.is_some()
        || patch.fragment_packets.is_some()
        || patch.fragment_length.is_some()
        || patch.fragment_interval.is_some()
        || patch.sniffing.is_some()
        || patch.log_level.is_some();
    let result = state.store.transaction(
        &state.log,
        |data| -> Result<(SettingsDto, bool), ApiError> {
            let settings = &mut data.settings;
            let old_ports = (settings.socks_port, settings.http_port);

            if let Some(port) = patch.socks_port {
                if port < 1024 {
                    return Err(ApiError::new("the SOCKS port must be >= 1024"));
                }
                settings.socks_port = port;
            }
            if let Some(port) = patch.http_port {
                if port < 1024 {
                    return Err(ApiError::new("the HTTP port must be >= 1024"));
                }
                settings.http_port = port;
            }
            if settings.socks_port == settings.http_port {
                return Err(ApiError::new("the SOCKS and HTTP ports must differ"));
            }
            if let Some(dns) = patch.dns_remote {
                let dns = dns.trim().to_owned();
                if dns.is_empty() {
                    return Err(ApiError::new("the DNS field must not be empty"));
                }
                settings.dns_remote = dns;
            }
            if let Some(v) = patch.bypass_lan {
                settings.bypass_lan = v;
            }
            if let Some(v) = patch.bypass_domains {
                let cleaned: Vec<String> = v
                    .into_iter()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                for entry in &cleaned {
                    if entry.chars().any(|c| c.is_whitespace() || c == '"') {
                        return Err(ApiError::new(format!(
                            "invalid bypass domain entry: {entry}"
                        )));
                    }
                }
                settings.bypass_domains = cleaned;
            }
            if let Some(v) = patch.bypass_ips {
                let cleaned: Vec<String> = v
                    .into_iter()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                for entry in &cleaned {
                    if entry.chars().any(|c| c.is_whitespace() || c == '"') {
                        return Err(ApiError::new(format!("invalid bypass IP entry: {entry}")));
                    }
                }
                settings.bypass_ips = cleaned;
            }
            if let Some(v) = patch.fragment_enabled {
                settings.fragment_enabled = v;
            }
            if let Some(v) = patch.fragment_packets {
                settings.fragment_packets = v.trim().to_owned();
            }
            if let Some(v) = patch.fragment_length {
                settings.fragment_length = v.trim().to_owned();
            }
            if let Some(v) = patch.fragment_interval {
                settings.fragment_interval = v.trim().to_owned();
            }
            if settings.fragment_enabled
                && (settings.fragment_packets.is_empty()
                    || settings.fragment_length.is_empty()
                    || settings.fragment_interval.is_empty())
            {
                return Err(ApiError::new(
                    "fragmentation needs packets, length and interval values",
                ));
            }
            if let Some(v) = patch.sniffing {
                settings.sniffing = v;
            }
            if let Some(level) = patch.log_level {
                if !["debug", "info", "warning", "error"].contains(&level.as_str()) {
                    return Err(ApiError::new(
                        "the log level must be debug, info, warning or error",
                    ));
                }
                settings.log_level = level;
            }
            if let Some(v) = patch.test_mode {
                settings.test_mode = v;
            }
            if let Some(v) = patch.system_proxy_desired {
                settings.system_proxy_desired = v;
            }
            if let Some(lang) = patch.language {
                let lang = lang.trim().to_ascii_lowercase();
                if lang != "ru" && lang != "en" {
                    return Err(ApiError::new(
                        "the interface language must be \"ru\" or \"en\"",
                    ));
                }
                settings.language = lang;
            }
            let new_ports = (settings.socks_port, settings.http_port);
            let ports_changed = old_ports != new_ports;
            Ok((SettingsDto::from(&*settings), ports_changed))
        },
    );

    let (dto, ports_changed) = result?;
    state.log.push("info", "settings", "settings saved");
    if engine_involved && touches_engine {
        state.log.push(
            "warn",
            "settings",
            if ports_changed {
                "port changes apply after the next connection"
            } else {
                "some settings apply after the next connection (routing, fragmentation)"
            },
        );
    }
    if !dto.system_proxy_desired
        && state
            .shared
            .sysproxy_on
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        if let Err(e) = controller::disable_sysproxy(&state.paths, &state.log).await {
            state.log.push("warn", "sysproxy", e);
        } else {
            state
                .shared
                .sysproxy_on
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    Ok(dto)
}

// ------------------------------------------------------------------ engine ---

pub async fn engine_status_dto(state: &AppState) -> EngineStatusDto {
    let settings = state.store.snapshot().settings;
    let asset = kvn_engine::asset_for_current_platform();
    let mut notes = Vec::new();
    let mut found: Option<(String, String, String)> = None; // path, version, source

    if let Some(custom) = settings
        .engine_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = std::path::PathBuf::from(custom);
        if path.is_file() {
            match kvn_engine::probe_version(&path).await {
                Ok(version) => {
                    found = Some((
                        path.display().to_string(),
                        version.version,
                        "custom path".into(),
                    ))
                }
                Err(e) => notes.push(format!(
                    "the configured binary failed the version probe: {e}"
                )),
            }
        } else {
            notes.push(format!(
                "the configured engine path does not exist: {}",
                path.display()
            ));
        }
    }

    if found.is_none() {
        let installed = state.paths.engine_binary();
        if installed.is_file() {
            match kvn_engine::probe_version(&installed).await {
                Ok(version) => {
                    found = Some((
                        installed.display().to_string(),
                        version.version,
                        "installed by KVN".into(),
                    ))
                }
                Err(e) => notes.push(format!(
                    "the installed engine failed the version probe: {e}"
                )),
            }
        }
    }

    if found.is_none() {
        if let Some(bundled) = kvn_engine::bundled_engine_path() {
            match kvn_engine::probe_version(&bundled).await {
                Ok(version) => {
                    found = Some((
                        bundled.display().to_string(),
                        version.version,
                        "bundled with app".into(),
                    ))
                }
                Err(e) => notes.push(format!("the bundled engine failed the version probe: {e}")),
            }
        }
    }

    if found.is_none() {
        if let Some(path) = controller::which_xray() {
            if let Ok(version) = kvn_engine::probe_version(&path).await {
                found = Some((path.display().to_string(), version.version, "PATH".into()));
            }
        }
    }

    if found.is_none() {
        notes.push(
            "no engine found: this build carries no bundled engine — install the pinned build below, point KVN at your own binary, or put `xray` on PATH"
                .to_owned(),
        );
    }
    if asset.is_none() {
        notes.push(format!(
            "no pinned engine release for this platform ({}/{}) — provide your own binary",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }

    if let Some((path, version, source)) = &found {
        let mut info = state.shared.engine_info.write().await;
        *info = Some(controller::EngineInfo {
            path: path.clone(),
            version: version.clone(),
            source: source.clone(),
        });
    }

    EngineStatusDto {
        found: found.is_some(),
        path: found.as_ref().map(|f| f.0.clone()),
        version: found.as_ref().map(|f| f.1.clone()),
        source: found.as_ref().map(|f| f.2.clone()),
        pinned_version: kvn_engine::release().to_owned(),
        platform_asset: asset.map(|a| a.file.clone()),
        install_supported: asset.is_some(),
        notes,
    }
}

#[tauri::command]
pub async fn engine_status(state: State<'_, AppState>) -> Result<EngineStatusDto, ApiError> {
    Ok(engine_status_dto(&state).await)
}

#[tauri::command]
pub async fn engine_install(state: State<'_, AppState>) -> Result<EngineStatusDto, ApiError> {
    use std::sync::atomic::Ordering;
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err(ApiError::new("an engine install is already running"));
    }
    let result = engine_install::install_pinned(&state.paths, &state.log).await;
    state.installing.store(false, Ordering::SeqCst);
    match result {
        Ok(info) => {
            *state.shared.engine_info.write().await = Some(info);
            Ok(engine_status_dto(&state).await)
        }
        Err(e) => Err(ApiError::new(e)),
    }
}

#[tauri::command]
pub async fn engine_set_path(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<EngineStatusDto, ApiError> {
    let cleaned = path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
    if let Some(candidate) = &cleaned {
        let candidate_path = std::path::PathBuf::from(candidate);
        if !candidate_path.is_file() {
            return Err(ApiError::new(format!("no file at {candidate}")));
        }
        kvn_engine::probe_version(&candidate_path)
            .await
            .map_err(|e| {
                ApiError::with_detail("that binary is not a working Xray engine", e.to_string())
            })?;
    }
    state.store.transaction(&state.log, |data| {
        data.settings.engine_path = cleaned.clone();
    });
    state.log.push(
        "info",
        "engine",
        match &cleaned {
            Some(p) => format!("custom engine path set: {p}"),
            None => "custom engine path cleared; auto-detection is back".to_owned(),
        },
    );
    Ok(engine_status_dto(&state).await)
}

/// Result of the informational engine-update check (ADR 0005).
#[derive(Debug, Serialize)]
pub struct EngineUpdateDto {
    /// The release this KVN build pins (core/xray.json).
    pub current: String,
    /// Latest Xray-core release tag on GitHub, `None` when the check failed.
    pub latest: Option<String>,
    pub update_available: bool,
    /// Release page URL, for reference.
    pub release_url: Option<String>,
}

/// Informational check: compare the pinned engine release with the latest
/// Xray-core release on GitHub. This never changes the pin — engine updates
/// ship with a new KVN release (the pin bump is reviewed in CI).
#[tauri::command]
pub async fn engine_check_update() -> Result<EngineUpdateDto, ApiError> {
    let current = kvn_engine::release().to_owned();
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        kvn_engine::manifest::repository()
    );
    let fetched =
        tokio::task::spawn_blocking(move || -> Result<(String, Option<String>), String> {
            let mut response = ureq::get(&url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.github+json")
                .call()
                .map_err(|e| e.to_string())?;
            let text = response
                .body_mut()
                .read_to_string()
                .map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let tag = value
                .get("tag_name")
                .and_then(|v| v.as_str())
                .ok_or("GitHub returned no tag_name")?
                .to_owned();
            let release_url = value
                .get("html_url")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            Ok((tag, release_url))
        })
        .await
        .map_err(|e| ApiError::new(format!("update check task failed: {e}")))?;

    match fetched {
        Ok((latest, release_url)) => Ok(EngineUpdateDto {
            update_available: kvn_engine::is_version_newer(&current, &latest),
            current,
            latest: Some(latest),
            release_url,
        }),
        Err(e) => Err(ApiError::with_detail(
            "could not check the latest Xray-core release",
            e,
        )),
    }
}

// ------------------------------------------------------------- system proxy --

#[tauri::command]
pub async fn system_proxy_set(state: State<'_, AppState>, enabled: bool) -> Result<bool, ApiError> {
    use std::sync::atomic::Ordering;
    if enabled {
        if !sysproxy::available() {
            return Err(ApiError::new(
                sysproxy::available_note().unwrap_or("system proxy is not available here"),
            ));
        }
        let involved = state.shared.state.read().await.clone().engine_involved();
        if !involved {
            return Err(ApiError::new(
                "connect first — the system proxy points at the local tunnel port",
            ));
        }
        let http_port = state.store.snapshot().settings.http_port;
        controller::enable_sysproxy(&state.paths, &state.log, &state.shared, http_port)
            .await
            .map_err(ApiError::new)?;
        state.log.push("info", "sysproxy", "system proxy enabled");
        Ok(true)
    } else {
        controller::disable_sysproxy(&state.paths, &state.log)
            .await
            .map_err(ApiError::new)?;
        state.shared.sysproxy_on.store(false, Ordering::SeqCst);
        state.log.push("info", "sysproxy", "system proxy disabled");
        Ok(false)
    }
}

// -------------------------------------------------------------- diagnostics --

/// UI-side diagnostics (boot probe, uncaught JS errors). Levels are
/// restricted and the message is capped by the event log itself, so a
/// misbehaving page cannot flood or fake unrelated entries.
#[tauri::command]
pub async fn ui_report(
    state: State<'_, AppState>,
    level: String,
    message: String,
) -> Result<(), ApiError> {
    let level = match level.as_str() {
        "error" | "warn" | "info" | "debug" => level,
        _ => "info".to_owned(),
    };
    state.log.push(&level, "ui", message);
    Ok(())
}

#[tauri::command]
pub async fn diagnostics_export(state: State<'_, AppState>) -> Result<String, ApiError> {
    let data = state.store.snapshot();
    let connection = state.shared.state.read().await.clone();
    let engine = engine_status_dto(&state).await;
    let mut out = String::new();
    out.push_str("KVN diagnostics\n");
    out.push_str(&format!(
        "generated: {}\n",
        crate::eventlog::iso_utc(now_ms())
    ));
    out.push_str(&format!(
        "app: kvn {} on {}/{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str(&format!(
        "state: {} (attempt {:?})\n",
        connection.label(),
        match &connection {
            ConnectionState::Connecting { attempt, .. }
            | ConnectionState::Reconnecting { attempt, .. } => Some(*attempt),
            _ => None,
        }
    ));
    out.push_str(&format!(
        "engine: found={} version={:?} source={:?} path={:?} (pinned {})\n",
        engine.found, engine.version, engine.source, engine.path, engine.pinned_version
    ));
    for note in &engine.notes {
        out.push_str(&format!("engine note: {note}\n"));
    }
    out.push_str(&format!(
        "settings: {}\n",
        serde_json::to_string_pretty(&SettingsDto::from(&data.settings))
            .unwrap_or_else(|_| "{}".into())
    ));
    out.push_str(&format!("test mode: {}\n", data.settings.test_mode));
    out.push_str("subscriptions (redacted):\n");
    for sub in &data.subscriptions {
        out.push_str(&format!(
            "  - {} [{}] updated={:?}\n",
            sub.name,
            redact_url(sub.url.expose()),
            sub.last_updated_ms
        ));
    }
    out.push_str(&format!(
        "profiles (no credentials): {}\n",
        data.profiles.len()
    ));
    for profile in &data.profiles {
        out.push_str(&format!(
            "  - {} [{}] {} {} warnings={:?}\n",
            profile.name,
            profile.id,
            profile.summary(),
            profile.endpoint.label(),
            profile_warnings(profile)
        ));
    }
    out.push_str("recent events:\n");
    for entry in state.log.tail(200) {
        out.push_str(&format!(
            "  {} {:5} {:<12} {}\n",
            crate::eventlog::iso_utc(entry.ts_ms),
            entry.level,
            entry.source,
            entry.message
        ));
    }

    let path = state
        .paths
        .logs_dir
        .join(format!("diagnostics-{}.txt", now_ms() / 1000));
    crate::store::write_private(&path, out.as_bytes())
        .map_err(|e| ApiError::new(format!("could not write diagnostics: {e}")))?;
    state.log.push(
        "info",
        "diagnostics",
        format!("diagnostics written to {}", path.display()),
    );
    Ok(path.display().to_string())
}
