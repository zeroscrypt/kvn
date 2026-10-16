// Shared API types — mirror the Rust DTOs in src-tauri/src/commands.rs.

export type ConnectionPhase =
  | "disconnected"
  | "connecting"
  | "checking"
  | "connected"
  | "reconnecting"
  | "stopping"
  | "error";

export interface ErrorDto {
  kind: string;
  message: string;
  retryable: boolean;
}

export interface TrafficSnapshot {
  uplink_bytes: number;
  downlink_bytes: number;
}

export interface ProfileDto {
  id: string;
  name: string;
  protocol: string;
  endpoint: string;
  transport: string;
  security: string;
  flow: string | null;
  source: string;
  subscription_name: string | null;
  warnings: string[];
}

export interface StatusDto {
  phase: ConnectionPhase;
  profile_id: string | null;
  profile_name: string | null;
  profile_summary: string | null;
  endpoint: string | null;
  attempt: number | null;
  max_attempts: number | null;
  latency_ms: number | null;
  connected_since_ms: number | null;
  error: ErrorDto | null;
  traffic: TrafficSnapshot;
  socks_port: number;
  http_port: number;
  system_proxy: boolean;
  system_proxy_available: boolean;
  test_mode: boolean;
  engine: EngineStatusDto | null;
}

export interface EngineStatusDto {
  found: boolean;
  path: string | null;
  version: string | null;
  source: string | null;
  pinned_version: string;
  platform_asset: string | null;
  install_supported: boolean;
  notes: string[];
}

export interface EngineUpdateDto {
  /** The release this KVN build pins (core/xray.json). */
  current: string;
  /** Latest Xray-core release tag on GitHub, null when the check failed. */
  latest: string | null;
  update_available: boolean;
  release_url: string | null;
}

export interface ImportOutcomeDto {
  added: number;
  replaced: number;
  failed: number;
  issues: string[];
  profile_ids: string[];
}

export interface LogEntryDto {
  id: number;
  ts_ms: number;
  level: "debug" | "info" | "warn" | "error";
  source: string;
  message: string;
}

export interface SubscriptionDto {
  id: string;
  name: string;
  url_redacted: string;
  last_updated_ms: number | null;
  profile_count: number;
}

export interface SettingsDto {
  socks_port: number;
  http_port: number;
  dns_remote: string;
  bypass_lan: boolean;
  bypass_domains: string[];
  bypass_ips: string[];
  fragment_enabled: boolean;
  fragment_packets: string;
  fragment_length: string;
  fragment_interval: string;
  sniffing: boolean;
  log_level: string;
  test_mode: boolean;
  system_proxy_desired: boolean;
  language: string;
}

/** UI-side state used by the render layer. */
export interface AppModel {
  status: StatusDto | null;
  profiles: ProfileDto[];
  selectedProfileId: string | null;
  events: LogEntryDto[];
  settings: SettingsDto | null;
  engine: EngineStatusDto | null;
  rates: { up: number; down: number };
  busy: boolean;
}

/** Shared helpers handed to views and modals. */
export interface UiContext {
  refreshProfiles(): Promise<void>;
  refreshStatus(): Promise<void>;
  /** Persist the interface language and rebuild the UI in it. */
  setLanguage(lang: "ru" | "en"): Promise<void>;
  pushLocal(
    level: "debug" | "info" | "warn" | "error",
    source: string,
    message: string,
  ): void;
}

export interface SettingsPatch {
  socks_port?: number | null;
  http_port?: number | null;
  dns_remote?: string | null;
  bypass_lan?: boolean | null;
  bypass_domains?: string[] | null;
  bypass_ips?: string[] | null;
  fragment_enabled?: boolean | null;
  fragment_packets?: string | null;
  fragment_length?: string | null;
  fragment_interval?: string | null;
  sniffing?: boolean | null;
  log_level?: string | null;
  test_mode?: boolean | null;
  system_proxy_desired?: boolean | null;
  language?: string | null;
}
