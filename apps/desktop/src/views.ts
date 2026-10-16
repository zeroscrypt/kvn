// Render layer: topbar, sidebar, main panels, statusbar.

import {
  clear,
  el,
  fmtBytes,
  fmtDuration,
  fmtRate,
  fmtTime,
  modSymbol,
  socksAddress,
} from "./dom";
import { errorHint, lineWord, phaseLabel, sourceLabel, t } from "./i18n";
import type {
  AppModel,
  LogEntryDto,
  ProfileDto,
  StatusDto,
} from "./types";

export interface ViewHandlers {
  connectOrCancel(): void;
  ping(): void;
  toggleSysproxy(): void;
  openPalette(): void;
  openImport(): void;
  openSettings(): void;
  selectProfile(id: string): void;
  connectProfile(id: string): void;
  renameProfile(id: string, name: string): void;
  deleteProfile(id: string): void;
  clearEvents(): void;
}

interface Refs {
  topbar: HTMLElement;
  sidebar: HTMLElement;
  main: HTMLElement;
  statusbar: HTMLElement;
  head: HTMLElement;
  headDot: HTMLElement;
  headLabel: HTMLElement;
  headAttempt: HTMLElement;
  profileLine: HTMLElement;
  chips: HTMLElement;
  metrics: Record<string, HTMLElement>;
  actions: HTMLElement;
  errorHost: HTMLElement;
  profileBody: HTMLElement;
  events: HTMLElement;
  btnConnect: HTMLButtonElement;
  btnPing: HTMLButtonElement;
  btnProxy: HTMLButtonElement;
  eventsCount: HTMLElement;
  sbEngine: HTMLElement;
  sbPorts: HTMLElement;
  sbPhase: HTMLElement;
}

let refs: Refs;
const MOD = modSymbol();

const cache = {
  topbar: "",
  sidebar: "",
  profilePanel: "",
  chips: "",
  error: "",
};
let renameState: { id: string; value: string } | null = null;
let pendingDelete: { id: string; until: number } | null = null;

/** Drop render caches (the UI is rebuilt when the language changes). */
export function resetViewCaches(): void {
  cache.topbar = "";
  cache.sidebar = "";
  cache.profilePanel = "";
  cache.chips = "";
  cache.error = "";
}

export function initViews(handlers: ViewHandlers): void {
  const topbar = document.getElementById("topbar")!;
  const sidebar = document.getElementById("sidebar")!;
  const main = document.getElementById("main")!;
  const statusbar = document.getElementById("statusbar")!;
  // A language switch rebuilds the whole skeleton: start from a clean slate.
  clear(topbar);
  clear(sidebar);
  clear(main);
  clear(statusbar);
  sidebar.setAttribute("aria-label", t("sidebar.profiles"));

  // ---- topbar skeleton
  const topChips = el("div", { class: "topbar-extra", id: "topbar-extra" });
  topbar.append(
    el("span", { class: "wordmark", text: "KVN" }),
    el("span", { class: "wordmark-suffix", text: t("topbar.suffix") }),
    el("span", { class: "topbar-spacer" }),
    topChips,
    el("button", {
      class: "btn small",
      text: t("topbar.palette", { mod: MOD }),
      onclick: handlers.openPalette,
      title: t("palette.label"),
    }),
    el("button", { class: "btn small", text: t("topbar.settings"), onclick: handlers.openSettings }),
  );

  // ---- sidebar skeleton
  const listHost = el("div", { id: "profile-list-host" });
  sidebar.append(
    el("div", { class: "profiles-head" }, [
      el("h3", { class: "panel-title", text: t("sidebar.profiles") }),
      el("div", { class: "btn-row" }, [
        el("button", { class: "btn small", text: t("sidebar.import"), onclick: handlers.openImport, title: t("import.title") }),
        el("button", { class: "btn small", text: t("sidebar.subs"), onclick: handlers.openImport, title: t("subs.label") }),
      ]),
    ]),
    listHost,
  );

  // ---- main skeleton
  const headDot = el("span", { class: "dot" });
  const headLabel = el("span", { class: "label", text: "…" });
  const headAttempt = el("span", { class: "faint" });
  const head = el("div", { class: "status-head" }, [headDot, headLabel, headAttempt]);
  const profileLine = el("div", { class: "status-profile" });
  const chips = el("div", { class: "status-chips" });
  const metric = (key: string, label: string) => {
    const v = el("div", { class: "v", text: "—" });
    const box = el("div", { class: "metric" }, [el("div", { class: "k", text: label }), v]);
    return { key, box, v };
  };
  const m1 = metric("latency", t("metrics.latency"));
  const m2 = metric("uptime", t("metrics.uptime"));
  const m3 = metric("down", t("metrics.down"));
  const m4 = metric("up", t("metrics.up"));
  const metricsRow = el("div", { class: "metrics" }, [m1.box, m2.box, m3.box, m4.box]);
  const btnConnect = el("button", { class: "btn primary", text: t("buttons.connect"), onclick: handlers.connectOrCancel });
  const btnPing = el("button", { class: "btn", text: t("buttons.testRtt"), onclick: handlers.ping });
  const btnProxy = el("button", { class: "btn", text: t("buttons.sysproxyOff"), onclick: handlers.toggleSysproxy });
  const actions = el("div", { class: "btn-row mt-12" }, [btnConnect, btnPing, btnProxy]);
  const errorHost = el("div");
  const statusPanel = el("section", { class: "panel", "aria-label": t("status.panelAria") }, [
    head,
    profileLine,
    chips,
    metricsRow,
    actions,
    errorHost,
  ]);

  const profileBody = el("div");
  const profilePanel = el("section", { class: "panel", "aria-label": t("profile.title") }, [
    el("h3", { class: "panel-title", text: t("profile.title") }),
    profileBody,
  ]);

  const events = el("div", { class: "events", role: "log", "aria-live": "polite" });
  const eventsCount = el("span", { class: "faint" });
  const eventsPanel = el("section", { class: "panel events-panel", "aria-label": t("events.title") }, [
    el("div", { class: "events-head" }, [
      el("h3", { class: "panel-title", text: t("events.title") }),
      el("div", { class: "btn-row" }, [
        eventsCount,
        el("button", { class: "btn small", text: t("common.clear"), onclick: handlers.clearEvents }),
      ]),
    ]),
    events,
  ]);

  main.append(statusPanel, profilePanel, eventsPanel);

  // ---- statusbar skeleton
  const sbEngine = el("span", { text: "—" });
  const sbPorts = el("span", { text: "—" });
  const sbPhase = el("span", { text: "" });
  statusbar.append(
    sbEngine,
    el("span", { class: "faint", text: "·" }),
    sbPorts,
    el("span", { class: "topbar-spacer" }),
    sbPhase,
    el("span", { class: "faint", text: "·" }),
    el("span", { class: "faint", text: t("statusbar.hint", { mod: MOD }) }),
  );

  refs = {
    topbar,
    sidebar,
    main,
    statusbar,
    head,
    headDot,
    headLabel,
    headAttempt,
    profileLine,
    chips,
    metrics: { latency: m1.v, uptime: m2.v, down: m3.v, up: m4.v },
    actions,
    errorHost,
    profileBody,
    events,
    btnConnect,
    btnPing,
    btnProxy,
    eventsCount,
    sbEngine,
    sbPorts,
    sbPhase,
  };
}

const DOT_CLASS: Record<string, string> = {
  disconnected: "",
  connecting: "connecting",
  checking: "connecting",
  connected: "connected",
  reconnecting: "connecting",
  stopping: "connecting",
  error: "error",
};

function setText(node: HTMLElement, text: string): void {
  if (node.textContent !== text) node.textContent = text;
}

export function render(model: AppModel, handlers: ViewHandlers): void {
  renderTopbar(model);
  renderSidebar(model, handlers);
  renderStatus(model, handlers);
  renderProfilePanel(model, handlers);
  renderEventsPanel(model);
  renderStatusbar(model);
}

function renderTopbar(model: AppModel): void {
  const status = model.status;
  const key = `${status?.test_mode}|${status?.system_proxy}|${model.engine?.version}|${model.engine?.found}`;
  if (key === cache.topbar) return;
  cache.topbar = key;
  const host = document.getElementById("topbar-extra");
  if (!host) return;
  clear(host);
  if (status?.test_mode) {
    host.append(el("span", { class: "chip purple", text: t("topbar.chipTestMode") }));
  }
  if (model.engine?.found) {
    host.append(el("span", { class: "chip accent", text: t("topbar.chipEngine", { version: model.engine.version ?? "?" }) }));
  } else {
    host.append(el("span", { class: "chip err", text: t("topbar.chipEngineMissing") }));
  }
  if (status?.system_proxy) {
    host.append(el("span", { class: "chip accent", text: t("topbar.chipSysproxy") }));
  }
}

function renderSidebar(model: AppModel, handlers: ViewHandlers): void {
  const connectedId =
    model.status && (model.status.phase === "connected" || model.status.phase === "checking" || model.status.phase === "connecting")
      ? model.status.profile_id
      : null;
  const key = JSON.stringify({
    ids: model.profiles.map((p) => [p.id, p.name, p.subscription_name, p.warnings.length]),
    selected: model.selectedProfileId,
    connected: connectedId,
  });
  if (key === cache.sidebar) return;
  cache.sidebar = key;

  const host = document.getElementById("profile-list-host");
  if (!host) return;
  clear(host);

  if (model.profiles.length === 0) {
    host.append(
      el("div", { class: "empty-note", text: t("sidebar.empty") }),
    );
    return;
  }

  const list = el("ul", { class: "profile-list" });
  list.addEventListener("keydown", (e) => {
    const items = Array.from(list.querySelectorAll<HTMLButtonElement>(".profile-item"));
    const idx = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown" && idx >= 0) {
      items[Math.min(idx + 1, items.length - 1)]?.focus();
      e.preventDefault();
    } else if (e.key === "ArrowUp" && idx >= 0) {
      items[Math.max(idx - 1, 0)]?.focus();
      e.preventDefault();
    }
  });

  for (const profile of model.profiles) {
    const selected = profile.id === model.selectedProfileId;
    const active = profile.id === connectedId;
    const btn = el("button", {
      class: `profile-item${selected ? " selected" : ""}${active ? " connected" : ""}`,
      type: "button",
      title: t("sidebar.itemTitle", { mod: MOD }),
    });
    btn.addEventListener("click", () => handlers.selectProfile(profile.id));
    btn.addEventListener("dblclick", () => handlers.connectProfile(profile.id));
    btn.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        handlers.connectProfile(profile.id);
        e.preventDefault();
      }
    });

    const nameRow = el("div", { class: "name" }, [el("span", { text: profile.name })]);
    if (profile.subscription_name) {
      nameRow.append(el("span", { class: "profile-source", text: `sub:${profile.subscription_name}` }));
    }
    if (profile.warnings.length > 0) {
      nameRow.append(el("span", { class: "chip warn", text: "!" }));
    }
    const subParts = [profile.protocol, profile.security, profile.transport].filter(Boolean);
    btn.append(
      nameRow,
      el("div", { class: "sub" }, [
        el("span", { text: subParts.join(" · ") }),
        el("span", { class: "faint", text: profile.endpoint }),
      ]),
    );
    list.append(el("li", {}, [btn]));
  }
  host.append(list);
}

function renderStatus(model: AppModel, handlers: ViewHandlers): void {
  const s = model.status;
  if (!s) {
    setText(refs.headLabel, t("status.starting"));
    refs.headDot.className = "dot connecting";
    return;
  }

  setText(refs.headLabel, phaseLabel(s.phase));
  refs.headDot.className = `dot ${DOT_CLASS[s.phase] ?? ""}`;
  setText(
    refs.headAttempt,
    s.phase === "reconnecting" && s.attempt != null
      ? t("status.attempt", { attempt: s.attempt, max: s.max_attempts ?? "?" })
      : "",
  );

  setText(
    refs.profileLine,
    s.profile_name
      ? `${s.profile_name} — ${s.profile_summary ?? ""} — ${s.endpoint ?? ""}`
      : t("status.noProfile"),
  );

  const chipKey = `${s.profile_id}|${s.phase}|${s.endpoint}`;
  if (chipKey !== cache.chips) {
    cache.chips = chipKey;
    clear(refs.chips);
    if (s.profile_summary) {
      for (const part of s.profile_summary.split("·").map((x) => x.trim()).filter(Boolean)) {
        refs.chips.append(el("span", { class: "chip", text: part }));
      }
    }
    if (s.endpoint) {
      refs.chips.append(el("span", { class: "faint", text: s.endpoint }));
    }
  }

  const connected = s.phase === "connected";
  setText(
    refs.metrics.latency,
    connected && s.latency_ms != null ? `${s.latency_ms} ms` : "—",
  );
  setText(
    refs.metrics.uptime,
    connected && s.connected_since_ms != null
      ? fmtDuration(Date.now() - s.connected_since_ms)
      : "—",
  );
  setText(
    refs.metrics.down,
    `${fmtBytes(s.traffic.downlink_bytes)} · ${fmtRate(model.rates.down)}`,
  );
  setText(
    refs.metrics.up,
    `${fmtBytes(s.traffic.uplink_bytes)} · ${fmtRate(model.rates.up)}`,
  );

  // connect button
  refs.btnConnect.disabled = false;
  if (s.phase === "stopping") {
    refs.btnConnect.textContent = t("buttons.stopping");
    refs.btnConnect.disabled = true;
  } else if (connected) {
    refs.btnConnect.textContent = t("buttons.disconnect");
  } else if (s.phase === "connecting" || s.phase === "checking") {
    refs.btnConnect.textContent = t("buttons.cancel");
  } else if (s.phase === "reconnecting") {
    refs.btnConnect.textContent = t("buttons.disconnect");
  } else {
    refs.btnConnect.textContent = t("buttons.connect");
    refs.btnConnect.disabled = model.profiles.length === 0;
  }
  refs.btnConnect.onclick = handlers.connectOrCancel;

  // ping button
  const pingAllowed =
    model.selectedProfileId != null &&
    !["connecting", "checking", "stopping", "reconnecting"].includes(s.phase);
  refs.btnPing.disabled = !pingAllowed;
  const selected = model.profiles.find((p) => p.id === model.selectedProfileId);
  refs.btnPing.onclick = handlers.ping;
  refs.btnPing.title = selected
    ? t("buttons.testRttTitle", { endpoint: selected.endpoint })
    : t("buttons.testRttNeedProfile");

  // system proxy button
  refs.btnProxy.disabled = !s.system_proxy_available || (!connected && !s.system_proxy);
  refs.btnProxy.textContent = s.system_proxy ? t("buttons.sysproxyOn") : t("buttons.sysproxyOff");
  refs.btnProxy.title = s.system_proxy_available
    ? t("buttons.sysproxyTitle")
    : t("buttons.sysproxyUnavailable");
  refs.btnProxy.onclick = handlers.toggleSysproxy;

  // error box
  const errKey = s.error ? `${s.error.kind}|${s.error.message}` : "";
  if (errKey !== cache.error) {
    cache.error = errKey;
    clear(refs.errorHost);
    if (s.error) {
      const box = el("div", { class: "error-box" }, [
        el("div", { text: `${s.error.kind}: ${s.error.message}` }),
      ]);
      const hint = errorHint(s.error.kind);
      if (hint) box.append(el("span", { class: "hint", text: hint }));
      refs.errorHost.append(box);
    }
  }
}

function renderProfilePanel(model: AppModel, handlers: ViewHandlers): void {
  const profile = model.profiles.find((p) => p.id === model.selectedProfileId) ?? null;
  const editing = renameState?.id === profile?.id;
  const key = JSON.stringify({ id: profile?.id, name: profile?.name, editing });
  // Re-render only on real changes: the panel holds a live rename input and
  // must not rebuild (and lose the caret) on every status poll.
  if (key === cache.profilePanel) return;
  cache.profilePanel = key;

  clear(refs.profileBody);
  if (!profile) {
    refs.profileBody.append(
      el("div", { class: "note", text: t("profile.selectNote") }),
    );
    return;
  }

  const dl = el("dl", { class: "kv" });
  const row = (k: string, v: string) => {
    dl.append(el("dt", { text: k }), el("dd", { text: v }));
  };
  row(t("profile.name"), profile.name);
  row(t("profile.protocol"), profile.protocol);
  row(t("profile.endpoint"), profile.endpoint);
  row(t("profile.transport"), profile.transport);
  row(t("profile.security"), profile.security);
  if (profile.flow) row(t("profile.flow"), profile.flow);
  row(t("profile.source"), profile.source);
  if (profile.subscription_name) row(t("profile.subscription"), profile.subscription_name);
  if (profile.warnings.length > 0) {
    dl.append(
      el("dt", { text: t("profile.warnings") }),
      el("dd", {}, profile.warnings.map((w) => el("div", { class: "chip warn", text: w }))),
    );
  }
  refs.profileBody.append(dl);

  const renameBtn = el("button", { class: "btn small", text: t("common.rename") });
  const deleteBtn = el("button", { class: "btn small danger", text: t("common.delete") });
  const row2 = el("div", { class: "btn-row mt-10" }, [renameBtn]);

  if (editing && renameState) {
    const input = el("input", { value: renameState.value, "aria-label": t("profile.renameAria") });
    const apply = () => {
      const value = input.value.trim();
      if (value) handlers.renameProfile(profile.id, value);
      renameState = null;
      cache.profilePanel = "";
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") apply();
      if (e.key === "Escape") {
        renameState = null;
        cache.profilePanel = "";
        renderProfilePanel(model, handlers);
      }
    });
    const ok = el("button", { class: "btn small primary", text: t("common.save"), onclick: apply });
    row2.append(input, ok);
    queueMicrotask(() => input.focus());
  } else {
    renameBtn.onclick = () => {
      renameState = { id: profile.id, value: profile.name };
      cache.profilePanel = "";
      renderProfilePanel(model, handlers);
    };
    deleteBtn.onclick = () => {
      const now = Date.now();
      if (pendingDelete && pendingDelete.id === profile.id && pendingDelete.until > now) {
        pendingDelete = null;
        handlers.deleteProfile(profile.id);
        return;
      }
      pendingDelete = { id: profile.id, until: now + 4000 };
      deleteBtn.textContent = t("common.confirmDelete");
      setTimeout(() => {
        if (pendingDelete?.id === profile.id) {
          pendingDelete = null;
          cache.profilePanel = "";
          renderProfilePanel(model, handlers);
        }
      }, 4000);
    };
    row2.append(deleteBtn);
  }

  refs.profileBody.append(row2);
}

function renderEventsPanel(model: AppModel): void {
  const events = model.events;
  const lastRendered = Number(refs.events.dataset.lastId ?? "0");
  if (events.length === 0) {
    if (lastRendered !== 0) {
      refs.events.replaceChildren();
      refs.events.dataset.lastId = "0";
    }
    setText(refs.eventsCount, `0 ${lineWord(0)}`);
    return;
  }

  const nearBottom =
    refs.events.scrollTop + refs.events.clientHeight >= refs.events.scrollHeight - 24;

  let appended = 0;
  for (const event of events) {
    if (event.id <= lastRendered) continue;
    refs.events.append(eventLine(event));
    appended += 1;
  }
  if (appended > 0) {
    refs.events.dataset.lastId = String(events[events.length - 1]!.id);
    while (refs.events.childElementCount > 400) {
      refs.events.firstElementChild?.remove();
    }
    if (nearBottom) refs.events.scrollTop = refs.events.scrollHeight;
  }
  setText(refs.eventsCount, `${events.length} ${lineWord(events.length)}`);
}

/** Reset internal event cache (after Clear). */
export function resetEventCache(): void {
  refs.events.replaceChildren();
  refs.events.dataset.lastId = "0";
  setText(refs.eventsCount, `0 ${lineWord(0)}`);
}

function eventLine(event: LogEntryDto): HTMLElement {
  return el("div", { class: `event-line ${event.level}` }, [
    el("span", { class: "t", text: fmtTime(event.ts_ms) }),
    el("span", { class: "src", text: event.source }),
    el("span", { class: "msg", text: event.message }),
  ]);
}

function renderStatusbar(model: AppModel): void {
  const s = model.status;
  const engine = model.engine;
  setText(
    refs.sbEngine,
    engine?.found
      ? t("statusbar.engine", { version: engine.version ?? "?", source: sourceLabel(engine.source) })
      : t("statusbar.engineNotFound"),
  );
  setText(refs.sbPorts, s ? `${socksAddress(s.socks_port)} · http ${s.http_port}` : "—");
  setText(refs.sbPhase, s ? t("statusbar.phase", { phase: s.phase }) : "");
}

export function focusFirstProfile(): void {
  document.querySelector<HTMLButtonElement>(".profile-item")?.focus();
}

export function profileSummaryLine(profile: ProfileDto): string {
  return [profile.protocol, profile.security, profile.transport].filter(Boolean).join(" · ");
}

export function currentStatusText(status: StatusDto | null): string {
  return status ? phaseLabel(status.phase) : t("status.starting");
}
