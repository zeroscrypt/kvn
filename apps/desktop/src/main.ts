// Application bootstrap: state, polling, actions, keyboard.

import { api, ApiError } from "./api";
import { listen } from "@tauri-apps/api/event";
import { fmtBytes, modSymbol } from "./dom";
import { phaseLabel, profileWord, setLang, t } from "./i18n";
import { bindSubscriptionLogging, closeOverlay, initOverlay, overlayOpen, showImportModal, showSettingsModal } from "./modals";
import { showPalette } from "./palette";
import type { AppModel, LogEntryDto, UiContext } from "./types";
import { initViews, resetEventCache, resetViewCaches, render } from "./views";
import type { ViewHandlers } from "./views";

/** Early UI diagnostics: the probe must have registered this before us. */
declare global {
  interface Window {
    __kvnReport?: (level: string, message: string) => void;
    __kvnBooted?: boolean;
  }
}

const MOD = modSymbol();

/** Polling cadence (ms): status is hot, engine info is cold. */
const POLL_MS = {
  status: 700,
  events: 900,
  profiles: 6000,
  engine: 15000,
} as const;

/** Small delays before re-reading status after a control action. */
const REFRESH_DELAY_MS = {
  afterControl: 120,
  afterSysproxy: 150,
} as const;

document.title = "KVN";
window.__kvnReport?.("info", "ui module loaded");

const model: AppModel = {
  status: null,
  profiles: [],
  selectedProfileId: null,
  events: [],
  settings: null,
  engine: null,
  rates: { up: 0, down: 0 },
  busy: false,
};

let localEventId = -1;
let lastBackendEventId = 0;
let statusInFlight = false;
let eventsInFlight = false;
let lastTraffic: { t: number; up: number; down: number } | null = null;
let statusErrorShown = false;

function pushLocal(
  level: "debug" | "info" | "warn" | "error",
  source: string,
  message: string,
): void {
  const entry: LogEntryDto = {
    id: localEventId--,
    ts_ms: Date.now(),
    level,
    source,
    message,
  };
  model.events.push(entry);
  if (model.events.length > 1000) model.events.splice(0, model.events.length - 1000);
  renderAll();
}

const ctx: UiContext = {
  refreshProfiles,
  refreshStatus: pollStatus,
  pushLocal,
  setLanguage: async (lang) => {
    const updated = await api.settingsSet({ language: lang });
    model.settings = updated;
    setLang(updated.language);
    rebuildUI();
    pushLocal("info", "settings", t("settings.languageChanged", {
      lang: updated.language === "en" ? "English" : "Русский",
    }));
  },
};

// ------------------------------------------------------------- data loads ----

async function refreshProfiles(): Promise<void> {
  try {
    const profiles = await api.listProfiles();
    model.profiles = profiles;
    const stillThere =
      model.selectedProfileId != null &&
      profiles.some((p) => p.id === model.selectedProfileId);
    if (!stillThere) {
      model.selectedProfileId =
        model.status?.profile_id ?? profiles[0]?.id ?? null;
    }
    renderAll();
  } catch (e) {
    pushLocal("error", "profiles", (e as ApiError).message);
  }
}

async function pollStatus(): Promise<void> {
  if (statusInFlight) return;
  statusInFlight = true;
  try {
    const status = await api.getStatus();
    statusErrorShown = false;
    updateRates(status.traffic);
    model.status = status;
    if (model.selectedProfileId == null && status.profile_id) {
      model.selectedProfileId = status.profile_id;
    }
    document.title = `KVN — ${phaseLabel(status.phase)}`;
    renderAll();
  } catch (e) {
    if (!statusErrorShown) {
      statusErrorShown = true;
      pushLocal("error", "ui", t("log.statusFailed", { message: (e as ApiError).message }));
    }
  } finally {
    statusInFlight = false;
  }
}

async function pollEvents(): Promise<void> {
  if (eventsInFlight) return;
  eventsInFlight = true;
  try {
    const fresh = await api.getEvents(lastBackendEventId);
    if (fresh.length > 0) {
      lastBackendEventId = fresh[fresh.length - 1]!.id;
      model.events.push(...fresh);
      if (model.events.length > 1000) {
        model.events.splice(0, model.events.length - 1000);
      }
      renderAll();
    }
  } catch {
    // status poll will surface backend problems; stay quiet here
  } finally {
    eventsInFlight = false;
  }
}

async function pollEngine(): Promise<void> {
  try {
    model.engine = await api.engineStatus();
  } catch {
    /* keep previous */
  }
}

function updateRates(traffic: { uplink_bytes: number; downlink_bytes: number }): void {
  const now = performance.now();
  if (lastTraffic) {
    const dt = (now - lastTraffic.t) / 1000;
    if (dt >= 0.3) {
      const up = Math.max(0, traffic.uplink_bytes - lastTraffic.up) / dt;
      const down = Math.max(0, traffic.downlink_bytes - lastTraffic.down) / dt;
      if (traffic.uplink_bytes < lastTraffic.up || traffic.downlink_bytes < lastTraffic.down) {
        // counters reset (new engine); show fresh numbers directly
        model.rates = { up, down };
      } else {
        // light smoothing so the display does not flicker
        model.rates = {
          up: model.rates.up * 0.5 + up * 0.5,
          down: model.rates.down * 0.5 + down * 0.5,
        };
      }
      lastTraffic = { t: now, up: traffic.uplink_bytes, down: traffic.downlink_bytes };
    }
  } else {
    lastTraffic = { t: now, up: traffic.uplink_bytes, down: traffic.downlink_bytes };
  }
}

// ---------------------------------------------------------------- actions ----

const handlers: ViewHandlers = {
  connectOrCancel: () => {
    const phase = model.status?.phase ?? "disconnected";
    if (["connecting", "checking", "connected", "reconnecting"].includes(phase)) {
      void api.disconnect().catch((e) => pushLocal("error", "control", (e as ApiError).message));
    } else if (phase !== "stopping") {
      void api
        .connect(model.selectedProfileId ?? undefined)
        .catch((e) => pushLocal("error", "control", (e as ApiError).message));
    }
    setTimeout(pollStatus, REFRESH_DELAY_MS.afterControl);
  },
  ping: () => {
    const id = model.selectedProfileId;
    const profile = model.profiles.find((p) => p.id === id);
    if (!id) return;
    void api
      .pingProfile(id)
      .then((ms) => pushLocal("info", "ping", t("log.rtt", { name: profile?.name ?? id, ms })))
      .catch((e) => pushLocal("error", "ping", (e as ApiError).message));
  },
  toggleSysproxy: () => {
    const enable = !(model.status?.system_proxy ?? false);
    void api
      .systemProxySet(enable)
      .then((state) =>
        pushLocal("info", "sysproxy", state ? t("log.sysproxyOn") : t("log.sysproxyOff")),
      )
      .catch((e) => pushLocal("error", "sysproxy", (e as ApiError).message))
      .finally(() => setTimeout(pollStatus, REFRESH_DELAY_MS.afterSysproxy));
  },
  openPalette: () => showPalette(buildPaletteActions()),
  openImport: () => showImportModal(ctx),
  openSettings: () => showSettingsModal(ctx),
  selectProfile: (id) => {
    model.selectedProfileId = id;
    renderAll();
  },
  connectProfile: (id) => {
    model.selectedProfileId = id;
    void api.connect(id).catch((e) => pushLocal("error", "control", (e as ApiError).message));
    setTimeout(pollStatus, REFRESH_DELAY_MS.afterControl);
  },
  renameProfile: (id, name) => {
    void api
      .renameProfile(id, name)
      .then(() => pushLocal("info", "profiles", t("log.renamed", { name })))
      .then(refreshProfiles)
      .catch((e) => pushLocal("error", "profiles", (e as ApiError).message));
  },
  deleteProfile: (id) => {
    void api
      .deleteProfile(id)
      .then(() => {
        if (model.selectedProfileId === id) model.selectedProfileId = null;
        pushLocal("info", "profiles", t("log.profileDeleted"));
      })
      .then(refreshProfiles)
      .catch((e) => pushLocal("error", "profiles", (e as ApiError).message));
  },
  clearEvents: () => {
    void api.clearEvents().catch(() => {});
    model.events = [];
    resetEventCache();
    renderAll();
  },
};

function buildPaletteActions() {
  const connected = model.status?.phase === "connected";
  const selected = model.profiles.find((p) => p.id === model.selectedProfileId);
  const actions = [
    {
      id: "connect",
      label: connected ? t("cmd.disconnect") : t("cmd.connect"),
      hint: selected?.name ?? t("cmd.hintNoProfile"),
      run: () => handlers.connectOrCancel(),
    },
    {
      id: "import",
      label: t("cmd.import"),
      hint: `${MOD}I`,
      run: () => handlers.openImport(),
    },
    {
      id: "settings",
      label: t("cmd.settings"),
      hint: `${MOD},`,
      run: () => handlers.openSettings(),
    },
    {
      id: "ping",
      label: t("cmd.ping"),
      hint: selected?.endpoint ?? "",
      run: () => handlers.ping(),
    },
    {
      id: "sysproxy",
      label: model.status?.system_proxy ? t("cmd.sysproxyDisable") : t("cmd.sysproxyEnable"),
      hint: model.status?.system_proxy_available ? "" : t("cmd.hintNotAvailable"),
      run: () => handlers.toggleSysproxy(),
    },
    {
      id: "diagnostics",
      label: t("cmd.diagnostics"),
      hint: t("cmd.hintRedacted"),
      run: async () => {
        try {
          const path = await api.diagnosticsExport();
          pushLocal("info", "diagnostics", t("engine.diagWritten", { path }));
        } catch (e) {
          pushLocal("error", "diagnostics", (e as ApiError).message);
        }
      },
    },
  ];
  for (const profile of model.profiles.slice(0, 9)) {
    actions.push({
      id: `profile:${profile.id}`,
      label: t("cmd.connectProfile", { name: profile.name }),
      hint: profile.endpoint,
      run: () => handlers.connectProfile(profile.id),
    });
  }
  return actions;
}

// ------------------------------------------------------------------ boot ----

function renderAll(): void {
  render(model, handlers);
}

/** Rebuild the whole interface in the active language. */
function rebuildUI(): void {
  resetViewCaches();
  initViews(handlers);
  resetEventCache();
  renderAll();
}

function bootKeyboard(): void {
  window.addEventListener("keydown", (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "k") {
      e.preventDefault();
      handlers.openPalette();
    } else if (mod && e.key.toLowerCase() === "i") {
      e.preventDefault();
      handlers.openImport();
    } else if (mod && e.key === ",") {
      e.preventDefault();
      handlers.openSettings();
    } else if (e.key === "Escape" && overlayOpen()) {
      closeOverlay();
    } else if (e.key === "/" && !overlayOpen()) {
      const target = e.target as HTMLElement;
      const typing =
        target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
      if (!typing) {
        e.preventDefault();
        handlers.openPalette();
      }
    }
  });
}

async function boot(): Promise<void> {
  // Settings first: the interface must adopt the saved language before its
  // first paint (the default is Russian).
  try {
    model.settings = await api.settingsGet();
  } catch {
    /* defaults */
  }
  setLang(model.settings?.language);

  initOverlay();
  initViews(handlers);
  bindSubscriptionLogging(
    (msg) => pushLocal("info", "subs", msg),
    (msg) => pushLocal("error", "subs", msg),
  );
  bootKeyboard();

  // Menu-bar (tray) events: the backend owns the icon, the frontend owns
  // the connect/disconnect and settings flows (same handlers as the UI).
  void listen("tray-toggle", () => handlers.connectOrCancel());
  void listen("open-settings", () => handlers.openSettings());

  await Promise.all([pollStatus(), refreshProfiles(), pollEngine()]);
  if (model.settings?.test_mode) {
    pushLocal("warn", "ui", t("log.testModeOn"));
  }
  pushLocal("info", "ui", t("log.ready", {
    version: __APP_VERSION__,
    profiles: `${model.profiles.length} ${profileWord(model.profiles.length)}`,
  }));
  renderAll();
  document.getElementById("app")?.setAttribute("aria-busy", "false");

  setInterval(pollStatus, POLL_MS.status);
  setInterval(pollEvents, POLL_MS.events);
  setInterval(refreshProfiles, POLL_MS.profiles);
  setInterval(pollEngine, POLL_MS.engine);
}

void boot()
  .then(() => {
    window.__kvnBooted = true;
    window.__kvnReport?.("info", "ui boot finished");
  })
  .catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    window.__kvnReport?.("error", `ui boot failed: ${message}`);
  });

// expose for debugging in the webview devtools
declare global {
  interface Window {
    __kvn?: { model: AppModel; refresh: () => Promise<void>; bytes: (n: number) => string };
  }
}
window.__kvn = { model, refresh: refreshProfiles, bytes: fmtBytes };
