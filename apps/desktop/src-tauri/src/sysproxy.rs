//! System-wide HTTP/HTTPS proxy management.
//!
//! Staged rollout, honest about platform state (see docs/PLATFORM_SUPPORT.md):
//! - macOS: `networksetup` (implemented, tested by hand);
//! - Linux: GNOME `gsettings` best-effort (works on GNOME desktops only);
//! - Windows: not implemented yet — the command reports that clearly.
//!
//! We point the system HTTP/HTTPS proxy at the local tunnel port and — just
//! as important — restore whatever the user had before. A backup file is
//! written while the proxy is ours so a crash cannot silently leave the
//! machine pointing at a dead port: the next start restores it.

use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::eventlog::EventLog;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyBackup {
    pub platform: String,
    pub detail: serde_json::Value,
}

pub fn available() -> bool {
    cfg!(target_os = "macos") || cfg!(target_os = "linux")
}

pub fn available_note() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        None
    } else if cfg!(target_os = "linux") {
        Some("system proxy on Linux is best-effort and requires a GNOME session with gsettings")
    } else {
        Some("system proxy management is not implemented on this platform yet — use the local SOCKS/HTTP ports")
    }
}

/// Apply the tunnel as the system proxy; returns the state to restore later.
pub fn set_proxy(http_port: u16) -> Result<ProxyBackup, String> {
    #[cfg(target_os = "macos")]
    {
        macos::set(http_port)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = http_port;
        linux::set(http_port)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = http_port;
        Err("system proxy management is not implemented on this platform yet".into())
    }
}

pub fn restore(backup: &ProxyBackup) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if backup.platform == "macos" {
        return macos::restore(&backup.detail);
    }
    #[cfg(target_os = "linux")]
    if backup.platform == "linux-gnome" {
        return linux::restore(&backup.detail);
    }
    Err(format!(
        "cannot restore a proxy state recorded for platform `{}`",
        backup.platform
    ))
}

pub fn save_backup(paths: &AppPaths, backup: &ProxyBackup) -> Result<(), String> {
    crate::store::write_private_json(
        &paths.sysproxy_backup(),
        &serde_json::to_value(backup).map_err(|e| e.to_string())?,
    )
}

pub fn clear_backup(paths: &AppPaths) {
    let _ = std::fs::remove_file(paths.sysproxy_backup());
}

/// Crash recovery: if a backup file exists, a previous run left the system
/// proxy pointing at our (now dead) local port — put the old settings back.
pub fn restore_backup_if_present(paths: &AppPaths, log: &EventLog) {
    let path = paths.sysproxy_backup();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    match serde_json::from_str::<ProxyBackup>(&text) {
        Ok(backup) => match restore(&backup) {
            Ok(()) => {
                log.push(
                    "warn",
                    "sysproxy",
                    "restored the system proxy left over from an earlier run",
                );
                clear_backup(paths);
            }
            Err(e) => {
                log.push(
                    "error",
                    "sysproxy",
                    format!("could not restore the system proxy from an earlier run: {e}"),
                );
            }
        },
        Err(e) => {
            log.push(
                "warn",
                "sysproxy",
                format!("proxy backup file was unreadable: {e}"),
            );
        }
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("could not run `{cmd}`: {e}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`{cmd} {}` failed: {}", args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ------------------------------------------------------------------ macOS ----

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ProxyGet {
        pub enabled: bool,
        pub server: String,
        pub port: u16,
    }

    /// Parse `networksetup -getwebproxy <service>` output.
    pub fn parse_proxy_get(text: &str) -> ProxyGet {
        let mut enabled = false;
        let mut server = String::new();
        let mut port = 0u16;
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "Enabled" => enabled = value.eq_ignore_ascii_case("yes"),
                "Server" => server = value.to_owned(),
                "Port" => port = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        ProxyGet {
            enabled,
            server,
            port,
        }
    }

    /// Map an interface name (e.g. `en0`) to a network service name from
    /// `networksetup -listnetworkserviceorder`.
    pub fn service_for_interface(text: &str, interface: &str) -> Option<String> {
        let needle = format!("Device: {interface}");
        let mut last_service: Option<String> = None;
        for line in text.lines() {
            if line.starts_with('(') && !line.starts_with("(Hardware") {
                // "(1) Wi-Fi"
                if let Some(idx) = line.find(") ") {
                    last_service = Some(line[idx + 2..].trim().to_owned());
                }
            } else if line.contains(&needle) {
                return last_service.clone();
            }
        }
        None
    }

    fn primary_service() -> Result<String, String> {
        let order = run("networksetup", &["-listnetworkserviceorder"])?;
        let interface = run("route", &["-n", "get", "default"])
            .ok()
            .and_then(|route| {
                route
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("interface: "))
                    .map(str::trim)
                    .map(str::to_owned)
            });
        if let Some(interface) = interface {
            if let Some(service) = service_for_interface(&order, &interface) {
                return Ok(service);
            }
        }
        // Fallback: first enabled service.
        let list = run("networksetup", &["-listallnetworkservices"])?;
        list.lines()
            .skip(1)
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('*'))
            .map(str::to_owned)
            .ok_or_else(|| "no network service found to configure".to_owned())
    }

    fn get(family: &str, service: &str) -> ProxyGet {
        // -getwebproxy / -getsecurewebproxy
        match run("networksetup", &[family, service]) {
            Ok(text) => parse_proxy_get(&text),
            Err(_) => ProxyGet {
                enabled: false,
                server: String::new(),
                port: 0,
            },
        }
    }

    fn apply(
        family: &str,
        state_cmd: &str,
        service: &str,
        host: &str,
        port: u16,
    ) -> Result<(), String> {
        run("networksetup", &[family, service, host, &port.to_string()])?;
        run("networksetup", &[state_cmd, service, "on"])?;
        Ok(())
    }

    pub fn set(http_port: u16) -> Result<ProxyBackup, String> {
        let service = primary_service()?;
        let web = get("-getwebproxy", &service);
        let secure = get("-getsecurewebproxy", &service);
        apply(
            "-setwebproxy",
            "-setwebproxystate",
            &service,
            "127.0.0.1",
            http_port,
        )?;
        apply(
            "-setsecurewebproxy",
            "-setsecurewebproxystate",
            &service,
            "127.0.0.1",
            http_port,
        )?;
        Ok(ProxyBackup {
            platform: "macos".into(),
            detail: json!({
                "service": service,
                "web": { "enabled": web.enabled, "server": web.server, "port": web.port },
                "secure": { "enabled": secure.enabled, "server": secure.server, "port": secure.port },
            }),
        })
    }

    pub fn restore(detail: &serde_json::Value) -> Result<(), String> {
        let service = detail
            .get("service")
            .and_then(|s| s.as_str())
            .ok_or_else(|| "backup does not name a network service".to_owned())?;
        for (key, family, state_cmd) in [
            ("web", "-setwebproxy", "-setwebproxystate"),
            ("secure", "-setsecurewebproxy", "-setsecurewebproxystate"),
        ] {
            let prev = detail.get(key).cloned().unwrap_or_default();
            let enabled = prev
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if enabled {
                let server = prev.get("server").and_then(|v| v.as_str()).unwrap_or("");
                let port = prev.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                if !server.is_empty() && port > 0 {
                    apply(family, state_cmd, service, server, port)?;
                    continue;
                }
            }
            run("networksetup", &[state_cmd, service, "off"])?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ Linux ----

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use serde_json::json;

    const SCHEMA: &str = "org.gnome.system.proxy";

    fn gsettings(args: &[&str]) -> Result<String, String> {
        run("gsettings", args)
    }

    fn has_gsettings() -> bool {
        Command::new("gsettings")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn get_string(schema_key: &str) -> Result<String, String> {
        let raw = gsettings(&["get", schema_key])?;
        Ok(raw.trim().trim_matches('\'').to_owned())
    }

    fn get_int(schema_key: &str) -> Result<i64, String> {
        let raw = gsettings(&["get", schema_key])?;
        raw.trim()
            .parse()
            .map_err(|_| format!("unexpected value from `{schema_key}`"))
    }

    pub fn set(http_port: u16) -> Result<ProxyBackup, String> {
        if !has_gsettings() {
            return Err(
                "gsettings is not available; system proxy is only supported on GNOME shells".into(),
            );
        }
        let prev = json!({
            "mode": get_string(&format!("{SCHEMA} mode"))?,
            "http_host": get_string(&format!("{SCHEMA}.http host"))?,
            "http_port": get_int(&format!("{SCHEMA}.http port"))?,
            "https_host": get_string(&format!("{SCHEMA}.https host"))?,
            "https_port": get_int(&format!("{SCHEMA}.https port"))?,
        });
        let port = http_port.to_string();
        gsettings(&["set", &format!("{SCHEMA}.http host"), "127.0.0.1"])?;
        gsettings(&["set", &format!("{SCHEMA}.http port"), &port])?;
        gsettings(&["set", &format!("{SCHEMA}.https host"), "127.0.0.1"])?;
        gsettings(&["set", &format!("{SCHEMA}.https port"), &port])?;
        gsettings(&["set", &format!("{SCHEMA} mode"), "manual"])?;
        Ok(ProxyBackup {
            platform: "linux-gnome".into(),
            detail: prev,
        })
    }

    pub fn restore(detail: &serde_json::Value) -> Result<(), String> {
        let str_of = |k: &str| {
            detail
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned()
        };
        let int_of = |k: &str| detail.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        gsettings(&["set", &format!("{SCHEMA}.http host"), &str_of("http_host")])?;
        gsettings(&[
            "set",
            &format!("{SCHEMA}.http port"),
            &int_of("http_port").to_string(),
        ])?;
        gsettings(&[
            "set",
            &format!("{SCHEMA}.https host"),
            &str_of("https_host"),
        ])?;
        gsettings(&[
            "set",
            &format!("{SCHEMA}.https port"),
            &int_of("https_port").to_string(),
        ])?;
        let mode = str_of("mode");
        let mode = if mode.is_empty() || mode == "none" {
            "none"
        } else {
            mode.as_str()
        };
        gsettings(&["set", &format!("{SCHEMA} mode"), mode])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn availability_matches_target() {
        assert_eq!(
            super::available(),
            cfg!(target_os = "macos") || cfg!(target_os = "linux")
        );
    }

    #[cfg(target_os = "macos")]
    mod macos_parsing {
        use super::super::macos::{parse_proxy_get, service_for_interface};

        #[test]
        fn parses_networksetup_get_output() {
            let text =
                "Enabled: Yes\nServer: 127.0.0.1\nPort: 8080\nAuthenticated Proxy Enabled: 0\n";
            let got = parse_proxy_get(text);
            assert!(got.enabled);
            assert_eq!(got.server, "127.0.0.1");
            assert_eq!(got.port, 8080);

            let off = parse_proxy_get("Enabled: No\nServer: \nPort: 0\n");
            assert!(!off.enabled);
            assert_eq!(off.port, 0);
        }

        #[test]
        fn maps_interface_to_service_name() {
            let order = "An asterisk (*) denotes that a network service is disabled.\n(1) Wi-Fi\n(Hardware Port: Wi-Fi, Device: en0)\n(2) Ethernet\n(Hardware Port: Ethernet, Device: en5)\n";
            assert_eq!(
                service_for_interface(order, "en0").as_deref(),
                Some("Wi-Fi")
            );
            assert_eq!(
                service_for_interface(order, "en5").as_deref(),
                Some("Ethernet")
            );
            assert_eq!(service_for_interface(order, "en9"), None);
        }
    }
}
