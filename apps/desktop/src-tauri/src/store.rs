//! Persistent state: profiles, subscriptions and settings.
//!
//! Everything lives in a single `state.json` written atomically
//! (tmp + rename) with `0600` permissions on unix. A corrupt file is moved
//! aside instead of being deleted, and the app starts from defaults.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kvn_core::{Profile, ProfileSource, Secret};
use serde::{Deserialize, Serialize};

use crate::eventlog::EventLog;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
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
    /// Interface language: "ru" (default) or "en".
    pub language: String,
    /// User-provided engine binary path; empty means "auto".
    pub engine_path: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            socks_port: kvn_core::xray::DEFAULT_SOCKS_PORT,
            http_port: kvn_core::xray::DEFAULT_HTTP_PORT,
            dns_remote: "1.1.1.1".into(),
            bypass_lan: true,
            bypass_domains: Vec::new(),
            bypass_ips: Vec::new(),
            fragment_enabled: false,
            fragment_packets: "tlshello".into(),
            fragment_length: "100-200".into(),
            fragment_interval: "10-20".into(),
            sniffing: true,
            log_level: "warning".into(),
            test_mode: false,
            system_proxy_desired: false,
            language: "ru".into(),
            engine_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSubscription {
    pub id: String,
    pub name: String,
    /// The subscription URL can embed a token — treated as a secret
    /// (transparent for persistence, `***` in Debug).
    pub url: Secret,
    pub last_updated_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreData {
    pub profiles: Vec<Profile>,
    pub subscriptions: Vec<StoredSubscription>,
    pub settings: Settings,
    pub active_profile: Option<String>,
}

impl StoreData {
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }
}

pub struct Store {
    path: PathBuf,
    inner: Mutex<StoreData>,
}

impl Store {
    /// Load from disk; a missing file means defaults, a corrupt file is
    /// renamed aside and defaults are used (never silently deleted).
    pub fn load(path: PathBuf, log: &EventLog) -> Self {
        let data = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<StoreData>(&text) {
                Ok(data) => data,
                Err(e) => {
                    let backup =
                        path.with_extension(format!("json.bad-{}", crate::eventlog::now_ms()));
                    let _ = std::fs::rename(&path, &backup);
                    log.push(
                        "error",
                        "store",
                        format!(
                            "state file was corrupt ({e}); it was moved to {} and defaults are used",
                            backup.display()
                        ),
                    );
                    StoreData::default()
                }
            },
            Err(_) => StoreData::default(),
        };
        Store {
            path,
            inner: Mutex::new(data),
        }
    }

    pub fn snapshot(&self) -> StoreData {
        self.inner.lock().unwrap().clone()
    }

    /// Run `f` under the lock and persist the result before releasing.
    pub fn transaction<R>(&self, log: &EventLog, f: impl FnOnce(&mut StoreData) -> R) -> R {
        let mut guard = self.inner.lock().unwrap();
        let result = f(&mut guard);
        if let Err(e) = save_to(&self.path, &guard) {
            log.push("error", "store", format!("could not persist state: {e}"));
        }
        result
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn save_to(path: &Path, data: &StoreData) -> Result<(), String> {
    let text = serde_json::to_string_pretty(data).map_err(|e| format!("serialize state: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    // Pre-create with restrictive permissions so the secret-bearing file
    // never exists world-readable.
    write_private(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path).map_err(|e| format!("replace {}: {e}", path.display()))?;
    Ok(())
}

/// Write a file readable only by the current user (0600 on unix).
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

pub fn write_private_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("serialize: {e}"))?;
    write_private(path, text.as_bytes())
}

/// Replace same-server profiles in place (keeping id/name/source), add the
/// rest. Returns (added, replaced, ids-of-touched-profiles).
pub fn merge_profiles(data: &mut StoreData, incoming: Vec<Profile>) -> (usize, usize, Vec<String>) {
    let mut added = 0;
    let mut replaced = 0;
    let mut ids = Vec::new();
    for profile in incoming {
        if let Some(existing) = data
            .profiles
            .iter_mut()
            .find(|e| e.same_server_as(&profile))
        {
            let keep_id = existing.id.clone();
            let keep_name = existing.name.clone();
            let keep_source = existing.source.clone();
            *existing = profile;
            existing.id = keep_id;
            existing.name = keep_name;
            existing.source = keep_source;
            replaced += 1;
            ids.push(existing.id.clone());
        } else {
            added += 1;
            ids.push(profile.id.clone());
            data.profiles.push(profile);
        }
    }
    (added, replaced, ids)
}

/// Remove every profile that came from the given subscription.
pub fn remove_subscription_profiles(data: &mut StoreData, subscription_id: &str) {
    let gone: Vec<String> = data
        .profiles
        .iter()
        .filter(|p| {
            matches!(
                &p.source,
                ProfileSource::Subscription { subscription_id: sid } if sid == subscription_id
            )
        })
        .map(|p| p.id.clone())
        .collect();
    data.profiles.retain(|p| !gone.contains(&p.id));
    if let Some(active) = &data.active_profile {
        if gone.contains(active) {
            data.active_profile = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kvn_core::{Endpoint, ProtocolSpec, Security, Transport};

    fn profile(name: &str, port: u16) -> Profile {
        Profile::new(
            name,
            Endpoint {
                address: "vpn.example.com".into(),
                port,
            },
            ProtocolSpec::Vless {
                uuid: Secret::new("11111111-2222-3333-4444-555555555555"),
                flow: None,
                encryption: "none".into(),
            },
            Transport::Tcp {
                header_type: Default::default(),
                path: None,
                host: None,
            },
            Security::None,
        )
    }

    #[test]
    fn merge_replaces_same_server_keeping_identity() {
        let mut data = StoreData::default();
        let first = profile("Alpha", 443);
        data.profiles.push(first.clone());
        let mut second = profile("Renamed", 443);
        second.id = "other".into();
        let (added, replaced, ids) = merge_profiles(&mut data, vec![second, profile("Beta", 8443)]);
        assert_eq!((added, replaced), (1, 1));
        assert_eq!(ids[0], first.id);
        assert_eq!(ids[1], data.profiles[1].id);
        assert_eq!(data.profiles[0].name, "Alpha");
        assert_eq!(data.profiles.len(), 2);
    }

    #[test]
    fn subscription_removal_clears_active_profile() {
        let mut data = StoreData::default();
        let mut p = profile("Sub", 443);
        p.source = ProfileSource::Subscription {
            subscription_id: "s1".into(),
        };
        let id = p.id.clone();
        data.profiles.push(p);
        data.active_profile = Some(id.clone());
        remove_subscription_profiles(&mut data, "s1");
        assert!(data.profiles.is_empty());
        assert_eq!(data.active_profile, None);
    }

    #[test]
    fn state_file_roundtrip_with_0600_permissions() {
        let dir = std::env::temp_dir().join(format!("kvn-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        let mut data = StoreData::default();
        data.profiles.push(profile("P", 443));
        data.settings.socks_port = 10810;
        save_to(&path, &data).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "state file must be private");
        }

        let log = EventLog::new(8);
        let store = Store::load(path.clone(), &log);
        let loaded = store.snapshot();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.settings.socks_port, 10810);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_state_is_moved_aside_not_deleted() {
        let dir = std::env::temp_dir().join(format!("kvn-store-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, "{ not json").unwrap();
        let log = EventLog::new(8);
        let store = Store::load(path.clone(), &log);
        assert!(store.snapshot().profiles.is_empty());
        assert!(!path.exists());
        let kept = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("state.json.bad-"));
        assert!(kept, "corrupt file must be preserved for inspection");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
