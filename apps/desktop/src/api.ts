// Thin typed wrapper over the Tauri IPC commands (src-tauri/src/commands.rs).

import { invoke } from "@tauri-apps/api/core";
import type {
  EngineStatusDto,
  EngineUpdateDto,
  ImportOutcomeDto,
  LogEntryDto,
  ProfileDto,
  SettingsDto,
  SettingsPatch,
  StatusDto,
  SubscriptionDto,
} from "./types";

/** Normalize a Tauri command error into a readable message. */
export class ApiError extends Error {
  readonly detail: string | null;

  constructor(message: string, detail: string | null) {
    super(message);
    this.detail = detail;
  }
}

function normalize(e: unknown): ApiError {
  if (e && typeof e === "object" && "message" in e) {
    const msg = String((e as { message: unknown }).message);
    const detail =
      "detail" in e && (e as { detail: unknown }).detail != null
        ? String((e as { detail: unknown }).detail)
        : null;
    return new ApiError(msg, detail);
  }
  return new ApiError(String(e), null);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalize(e);
  }
}

export const api = {
  getStatus: () => call<StatusDto>("get_status"),
  listProfiles: () => call<ProfileDto[]>("list_profiles"),
  importFromText: (payload: string, subscriptionName?: string) =>
    call<ImportOutcomeDto>("import_from_text", {
      payload,
      subscriptionName: subscriptionName ?? null,
    }),
  deleteProfile: (id: string) => call<void>("delete_profile", { id }),
  renameProfile: (id: string, name: string) =>
    call<void>("rename_profile", { id, name }),
  setActiveProfile: (id: string) =>
    call<void>("set_active_profile", { id }),
  connect: (profileId?: string) =>
    call<void>("connect", { profileId: profileId ?? null }),
  disconnect: () => call<void>("disconnect"),
  pingProfile: (id: string) => call<number>("ping_profile", { id }),
  getEvents: (afterId: number) => call<LogEntryDto[]>("get_events", { afterId }),
  clearEvents: () => call<void>("clear_events"),
  subscriptions: () => call<SubscriptionDto[]>("subscription_list"),
  subscriptionAdd: (name: string, url: string) =>
    call<void>("subscription_add", { name, url }),
  subscriptionUpdate: (id: string) => call<void>("subscription_update", { id }),
  subscriptionDelete: (id: string) => call<void>("subscription_delete", { id }),
  settingsGet: () => call<SettingsDto>("settings_get"),
  settingsSet: (patch: SettingsPatch) =>
    call<SettingsDto>("settings_set", { patch }),
  engineStatus: () => call<EngineStatusDto>("engine_status"),
  engineInstall: () => call<EngineStatusDto>("engine_install"),
  engineSetPath: (path: string | null) =>
    call<EngineStatusDto>("engine_set_path", { path }),
  engineCheckUpdate: () => call<EngineUpdateDto>("engine_check_update"),
  systemProxySet: (enabled: boolean) =>
    call<boolean>("system_proxy_set", { enabled }),
  diagnosticsExport: () => call<string>("diagnostics_export"),
  /** Best-effort UI diagnostics (boot probe, uncaught errors). */
  uiReport: (level: string, message: string) =>
    call<void>("ui_report", { level, message }),
};
