// Import / subscriptions / settings modals and the shared overlay host.

import { api, ApiError } from "./api";
import { clear, el } from "./dom";
import { getLang, profileWord, sourceLabel, t } from "./i18n";
import type { StringKey } from "./i18n";
import type {
  EngineStatusDto,
  SettingsDto,
  SubscriptionDto,
  UiContext,
} from "./types";

let overlayEl: HTMLElement | null = null;
let lastFocused: HTMLElement | null = null;

function overlayFocusables(): HTMLElement[] {
  if (!overlayEl) return [];
  return Array.from(
    overlayEl.querySelectorAll<HTMLElement>(
      'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((node) => node.getClientRects().length > 0);
}

/** Keep Tab navigation inside the open dialog. */
function trapFocus(e: KeyboardEvent): void {
  if (e.key !== "Tab" || !overlayEl) return;
  const focusables = overlayFocusables();
  if (focusables.length === 0) return;
  const first = focusables[0]!;
  const last = focusables[focusables.length - 1]!;
  const active = document.activeElement as HTMLElement | null;
  if (e.shiftKey && (active === first || !overlayEl.contains(active))) {
    last.focus();
    e.preventDefault();
  } else if (!e.shiftKey && active === last) {
    first.focus();
    e.preventDefault();
  }
}

export function initOverlay(): void {
  overlayEl = document.getElementById("overlay");
  if (overlayEl) {
    overlayEl.onclick = (e) => {
      if (e.target === overlayEl) closeOverlay();
    };
    overlayEl.onkeydown = trapFocus;
  }
}

export function openOverlay(node: HTMLElement): void {
  if (!overlayEl) return;
  lastFocused =
    document.activeElement instanceof HTMLElement ? document.activeElement : null;
  clear(overlayEl);
  overlayEl.append(node);
  overlayEl.hidden = false;
  const first = node.querySelector<HTMLElement>("input, textarea, select, button");
  first?.focus();
}

export function closeOverlay(): void {
  if (!overlayEl) return;
  overlayEl.hidden = true;
  clear(overlayEl);
  if (lastFocused && document.contains(lastFocused)) lastFocused.focus();
  lastFocused = null;
}

export function overlayOpen(): boolean {
  return !!overlayEl && !overlayEl.hidden;
}

function noteLine(text: string): HTMLElement {
  return el("div", { class: "note", text });
}

function errorLine(message: string, detail?: string | null): HTMLElement {
  return el("div", {
    class: "error-box",
    text: detail ? `${message}\n${detail}` : message,
  });
}

// ---------------------------------------------------------------- import ----

export function showImportModal(ctx: UiContext): void {
  const textarea = el("textarea", {
    placeholder: t("import.placeholder"),
    "aria-label": t("import.textareaAria"),
  });
  const fileInput = el("input", {
    type: "file",
    accept: ".txt,.json,text/plain,application/json",
    "aria-label": t("import.fileAria"),
  });
  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      textarea.value = String(reader.result ?? "");
    };
    reader.readAsText(file);
  });

  const resultBox = el("div", { class: "note" });

  const importBtn = el("button", { class: "btn primary", text: t("import.btn") });
  importBtn.addEventListener("click", async () => {
    const payload = textarea.value.trim();
    if (!payload) {
      resultBox.textContent = t("import.nothing");
      return;
    }
    importBtn.disabled = true;
    importBtn.textContent = t("import.btnBusy");
    try {
      const outcome = await api.importFromText(payload);
      const summary = t("import.result", {
        added: outcome.added,
        replaced: outcome.replaced,
        failed: outcome.failed,
      });
      const lines = [summary, ...outcome.issues.map((i) => `· ${i}`)];
      resultBox.textContent = lines.join("\n");
      resultBox.className = outcome.failed > 0 ? "note warn" : "note";
      ctx.pushLocal("info", "import", summary);
      await ctx.refreshProfiles();
      if (outcome.added + outcome.replaced > 0 && outcome.failed === 0) {
        setTimeout(() => closeOverlay(), 600);
      }
    } catch (e) {
      const err = e as ApiError;
      resultBox.textContent = err.detail
        ? `${err.message} — ${err.detail}`
        : err.message;
      resultBox.className = "note";
      ctx.pushLocal("error", "import", err.message);
    } finally {
      importBtn.disabled = false;
      importBtn.textContent = t("import.btn");
    }
  });

  const modal = el("div", { class: "modal", role: "dialog", "aria-label": t("import.modalAria") });
  modal.append(
    el("h2", { class: "modal-title", text: t("import.title") }),
    el("div", { class: "field" }, [
      el("label", { text: t("import.label") }),
      textarea,
      noteLine(t("import.hint")),
    ]),
    el("div", { class: "field" }, [
      el("label", { text: t("import.fromFile") }),
      fileInput,
    ]),
    resultBox,
    el("div", { class: "modal-actions" }, [
      el("button", {
        class: "btn",
        text: t("common.close"),
        onclick: () => closeOverlay(),
      }),
      importBtn,
    ]),
  );

  // subscriptions block
  const subsHost = el("div", { class: "field" });
  modal.insertBefore(subsHost, modal.querySelector(".modal-actions"));
  renderSubscriptions(ctx, subsHost);

  openOverlay(modal);
}

function renderSubscriptions(ctx: UiContext, host: HTMLElement): void {
  clear(host);
  host.append(el("label", { text: t("subs.label") }));

  const urlInput = el("input", {
    placeholder: t("subs.urlPlaceholder"),
    "aria-label": t("subs.urlAria"),
  });
  const addBtn = el("button", { class: "btn small", text: t("subs.add") });
  const status = el("div", { class: "note" });

  const addNow = async () => {
    const url = urlInput.value.trim();
    if (!url) {
      status.textContent = t("subs.required");
      return;
    }
    addBtn.disabled = true;
    addBtn.textContent = t("subs.adding");
    try {
      const name = nameFromUrl(url);
      await api.subscriptionAdd(name, url);
      ctx.pushLocal("info", "subs", t("subs.addedLog", { name }));
      urlInput.value = "";
      await ctx.refreshProfiles();
      await renderSubscriptionsList(host);
    } catch (e) {
      const err = e as ApiError;
      status.textContent = err.message;
      ctx.pushLocal("error", "subs", err.message);
    } finally {
      addBtn.disabled = false;
      addBtn.textContent = t("subs.add");
    }
  };

  addBtn.addEventListener("click", () => void addNow());
  urlInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") void addNow();
  });

  host.append(
    el("div", { class: "field-row" }, [
      el("div", { class: "grow-1" }, [urlInput]),
      addBtn,
    ]),
    status,
  );
  void renderSubscriptionsList(host);
}

/** A short human name for a subscription: its host, else a generic label. */
function nameFromUrl(url: string): string {
  try {
    const host = new URL(url).hostname;
    if (host) return host;
  } catch {
    /* not a URL: fall through */
  }
  return "subscription";
}

async function renderSubscriptionsList(host: HTMLElement): Promise<void> {
  let subs: SubscriptionDto[] = [];
  try {
    subs = await api.subscriptions();
  } catch {
    return;
  }
  const old = host.querySelector(".subs-list");
  old?.remove();
  const list = el("ul", { class: "profile-list subs-list" });
  if (subs.length === 0) {
    list.append(
      el("li", {}, [el("div", { class: "note", text: t("subs.none") })]),
    );
  }
  for (const sub of subs) {
    const updated = sub.last_updated_ms
      ? new Date(sub.last_updated_ms).toLocaleString(getLang() === "ru" ? "ru-RU" : "en-US")
      : t("subs.never");
    const updateBtn = el("button", { class: "btn small", text: t("common.update") });
    updateBtn.addEventListener("click", async () => {
      updateBtn.disabled = true;
      updateBtn.textContent = t("common.updating");
      try {
        await api.subscriptionUpdate(sub.id);
        ctxLocalInfo(t("subs.updatedLog", { name: sub.name }));
        await renderSubscriptionsList(host);
      } catch (e) {
        ctxLocalError((e as ApiError).message);
      }
    });
    const delBtn = el("button", { class: "btn small danger", text: t("common.delete") });
    delBtn.addEventListener("click", async () => {
      try {
        await api.subscriptionDelete(sub.id);
        ctxLocalInfo(t("subs.deletedLog", { name: sub.name }));
        await renderSubscriptionsList(host);
      } catch (e) {
        ctxLocalError((e as ApiError).message);
      }
    });
    list.append(
      el("li", {}, [
        el("div", { class: "name" }, [
          el("span", { text: sub.name }),
          el("span", {
            class: "faint",
            text: ` · ${sub.profile_count} ${profileWord(sub.profile_count)} · ${updated}`,
          }),
        ]),
        el("div", { class: "sub" }, [el("span", { text: sub.url_redacted })]),
        el("div", { class: "btn-row mt-6" }, [
          updateBtn,
          delBtn,
        ]),
      ]),
    );
  }
  host.append(list);
}

// uiLog binding resolved lazily via module state to avoid carrying ctx around
let ctxLocalInfo: (msg: string) => void = () => {};
let ctxLocalError: (msg: string) => void = () => {};

export function bindSubscriptionLogging(
  info: (msg: string) => void,
  error: (msg: string) => void,
): void {
  ctxLocalInfo = info;
  ctxLocalError = error;
}

// -------------------------------------------------------------- settings ----

function section(titleKey: StringKey, ...children: Node[]): HTMLElement {
  const box = el("section", { class: "settings-section" }, [
    el("h3", { class: "settings-section-title", text: t(titleKey) }),
  ]);
  box.append(...children);
  return box;
}

/** Language picker: applies immediately and rebuilds the interface. */
function languageSection(ctx: UiContext): HTMLElement {
  const row = el("div", { class: "segmented", role: "radiogroup", "aria-label": t("settings.language") });
  const langs: Array<{ id: "ru" | "en"; label: string }> = [
    { id: "ru", label: "Русский" },
    { id: "en", label: "English" },
  ];
  for (const lang of langs) {
    const active = getLang() === lang.id;
    const btn = el("button", {
      class: `seg${active ? " active" : ""}`,
      type: "button",
      role: "radio",
      "aria-checked": String(active),
      text: lang.label,
    });
    btn.addEventListener("click", async () => {
      if (getLang() === lang.id) return;
      try {
        await ctx.setLanguage(lang.id);
      } catch (e) {
        ctx.pushLocal("error", "settings", (e as ApiError).message);
        return;
      }
      closeOverlay();
      showSettingsModal(ctx);
    });
    row.append(btn);
  }
  return section(
    "settings.sectionInterface",
    el("div", { class: "field" }, [el("label", { text: t("settings.language") }), row]),
  );
}

export function showSettingsModal(ctx: UiContext): void {
  const host = el("div", { class: "settings-grid" });
  const modal = el("div", {
    class: "modal modal-settings",
    role: "dialog",
    "aria-label": t("settings.title"),
  });
  const title = el("h2", { class: "modal-title", text: t("settings.title") });

  const actions = el("div", { class: "modal-actions" }, [
    el("button", {
      class: "btn",
      text: t("common.close"),
      onclick: () => closeOverlay(),
    }),
  ]);
  const saveBtn = el("button", { class: "btn primary", text: t("common.save") });
  actions.append(saveBtn);

  modal.append(title, host, actions);
  openOverlay(modal);

  let settings: SettingsDto | null = null;

  const statusLine = el("div", { class: "note" });

  void (async () => {
    host.append(statusLine);
    statusLine.textContent = t("settings.loading");
    try {
      settings = await api.settingsGet();
      statusLine.remove();
      host.append(
        languageSection(ctx),
        buildSettingsForm(settings, saveBtn),
        el("hr", { class: "rule" }),
      );
      await renderEngineSection(host, ctx);
    } catch (e) {
      statusLine.textContent = (e as ApiError).message;
    }
  })();

  saveBtn.addEventListener("click", async () => {
    if (!settings) return;
    const form = host.querySelector<HTMLFormElement>("form");
    if (!form) return;
    const data = new FormData(form);
    const num = (k: string) => Number(data.get(k));
    const lines = (k: string) =>
      String(data.get(k) ?? "")
        .split(/[\n,]+/)
        .map((s) => s.trim())
        .filter(Boolean);
    saveBtn.disabled = true;
    saveBtn.textContent = t("common.saving");
    try {
      const updated = await api.settingsSet({
        socks_port: num("socks_port"),
        http_port: num("http_port"),
        dns_remote: String(data.get("dns_remote") ?? "").trim(),
        bypass_lan: data.get("bypass_lan") === "on",
        bypass_domains: lines("bypass_domains"),
        bypass_ips: lines("bypass_ips"),
        fragment_enabled: data.get("fragment_enabled") === "on",
        fragment_packets: String(data.get("fragment_packets") ?? ""),
        fragment_length: String(data.get("fragment_length") ?? ""),
        fragment_interval: String(data.get("fragment_interval") ?? ""),
        sniffing: data.get("sniffing") === "on",
        log_level: String(data.get("log_level") ?? "warning"),
        test_mode: data.get("test_mode") === "on",
        system_proxy_desired: data.get("system_proxy_desired") === "on",
      });
      settings = updated;
      ctx.pushLocal("info", "settings", t("settings.savedLog"));
      closeOverlay();
    } catch (e) {
      ctx.pushLocal("error", "settings", (e as ApiError).message);
      saveBtn.disabled = false;
      saveBtn.textContent = t("common.save");
    }
  });
}

function buildSettingsForm(s: SettingsDto, saveBtn: HTMLElement): HTMLElement {
  const field = (label: string, input: HTMLElement, hint?: string) => {
    const wrap = el("div", { class: "field" }, [
      el("label", { text: label }),
      input,
    ]);
    if (hint) wrap.append(noteLine(hint));
    return wrap;
  };
  const text = (name: string, value: string, aria: string) =>
    el("input", { name, value, "aria-label": aria });
  const check = (name: string, checked: boolean, aria: string) => {
    const box = el("input", { name, type: "checkbox", "aria-label": aria });
    (box as HTMLInputElement).checked = checked;
    return box;
  };
  const toggle = (name: string, checked: boolean, aria: string, label: string) =>
    el("label", { class: "toggle" }, [check(name, checked, aria), el("span", { text: label })]);

  const form = el("form");
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    saveBtn.click();
  });

  const bypassDomains = el("textarea", { name: "bypass_domains", "aria-label": t("settings.ariaBypassDomains") });
  bypassDomains.value = s.bypass_domains.join("\n");
  const bypassIps = el("textarea", { name: "bypass_ips", "aria-label": t("settings.ariaBypassIps") });
  bypassIps.value = s.bypass_ips.join("\n");

  const logSelect = el("select", { name: "log_level", "aria-label": t("settings.ariaLogLevel") });
  for (const level of ["debug", "info", "warning", "error"]) {
    const option = el("option", { value: level, text: level });
    if (level === s.log_level) option.selected = true;
    logSelect.append(option);
  }

  form.append(
    section(
      "settings.sectionConnection",
      el("div", { class: "field-row" }, [
        field(t("settings.labelSocks"), text("socks_port", String(s.socks_port), t("settings.ariaSocks"))),
        field(t("settings.labelHttp"), text("http_port", String(s.http_port), t("settings.ariaHttp"))),
      ]),
      field(t("settings.labelDns"), text("dns_remote", s.dns_remote, t("settings.ariaDns"))),
    ),
    section(
      "settings.sectionRouting",
      el("div", { class: "field-row" }, [
        toggle("bypass_lan", s.bypass_lan, t("settings.ariaBypassLan"), t("settings.labelBypassLan")),
        toggle("sniffing", s.sniffing, t("settings.ariaSniffing"), t("settings.labelSniffing")),
      ]),
      field(t("settings.labelBypassDomains"), bypassDomains, t("settings.hintBypassDomains")),
      field(t("settings.labelBypassIps"), bypassIps, t("settings.hintBypassIps")),
    ),
    section(
      "settings.sectionFragment",
      el("div", { class: "field-row" }, [
        toggle("fragment_enabled", s.fragment_enabled, t("settings.ariaFragment"), t("settings.labelFragment")),
      ]),
      el("div", { class: "field-row" }, [
        field(t("settings.labelPackets"), text("fragment_packets", s.fragment_packets, t("settings.ariaPackets"))),
        field(t("settings.labelLength"), text("fragment_length", s.fragment_length, t("settings.ariaLength"))),
        field(t("settings.labelInterval"), text("fragment_interval", s.fragment_interval, t("settings.ariaInterval"))),
      ]),
      el("div", { class: "warn-note", text: t("settings.warnFragment") }),
    ),
    section(
      "settings.sectionOther",
      el("div", { class: "field-row" }, [
        field(t("settings.labelLogLevel"), logSelect),
      ]),
      el("div", { class: "field-row" }, [
        toggle("system_proxy_desired", s.system_proxy_desired, t("settings.ariaSysproxy"), t("settings.labelSysproxy")),
        toggle("test_mode", s.test_mode, t("settings.ariaTestMode"), t("settings.labelTestMode")),
      ]),
      el("div", { class: "warn-note", text: t("settings.warnTestMode") }),
    ),
  );

  return form;
}

async function renderEngineSection(host: HTMLElement, ctx: UiContext): Promise<void> {
  const box = el("div", { class: "field" });
  host.append(box);

  const draw = (engine: EngineStatusDto) => {
    clear(box);
    box.append(el("label", { text: t("engine.title") }));
    const dl = el("dl", { class: "kv" });
    const row = (k: string, v: string) => {
      dl.append(el("dt", { text: k }), el("dd", { text: v }));
    };
    row(t("engine.found"), engine.found ? t("common.yes") : t("common.no"));
    row(t("engine.version"), engine.version ?? "—");
    row(t("engine.path"), engine.path ?? "—");
    row(t("engine.source"), sourceLabel(engine.source));
    row(t("engine.pinned"), engine.pinned_version);
    row(t("engine.asset"), engine.platform_asset ?? t("engine.unsupported"));
    box.append(dl);
    for (const note of engine.notes) {
      box.append(el("div", { class: "warn-note", text: note }));
    }

    const installBtn = el("button", {
      class: "btn small primary",
      text: t("engine.install"),
    });
    installBtn.disabled = !engine.install_supported;
    installBtn.addEventListener("click", async () => {
      installBtn.disabled = true;
      installBtn.textContent = t("engine.installing");
      installBtn.classList.add("spinner");
      try {
        const updated = await api.engineInstall();
        ctx.pushLocal("info", "engine", t("engine.installedLog", { version: updated.version ?? "?" }));
        draw(updated);
      } catch (e) {
        ctx.pushLocal("error", "engine", (e as ApiError).message);
        installBtn.disabled = false;
        installBtn.textContent = t("engine.install");
        installBtn.classList.remove("spinner");
      }
    });

    const pathInput = el("input", {
      placeholder: t("engine.pathPlaceholder"),
      value: "",
      "aria-label": t("engine.pathAria"),
    });
    const applyBtn = el("button", { class: "btn small", text: t("engine.usePath") });
    applyBtn.addEventListener("click", async () => {
      const value = pathInput.value.trim();
      try {
        const updated = await api.engineSetPath(value || null);
        ctx.pushLocal("info", "engine", value ? t("engine.pathSetLog") : t("engine.pathClearedLog"));
        draw(updated);
      } catch (e) {
        ctx.pushLocal("error", "engine", (e as ApiError).message);
      }
    });

    const diagBtn = el("button", { class: "btn small", text: t("engine.diag") });
    diagBtn.addEventListener("click", async () => {
      try {
        const path = await api.diagnosticsExport();
        ctx.pushLocal("info", "diagnostics", t("engine.diagWritten", { path }));
      } catch (e) {
        ctx.pushLocal("error", "diagnostics", (e as ApiError).message);
      }
    });

    const checkBtn = el("button", { class: "btn small", text: t("engine.checkUpdates") });
    const checkResult = el("span", { class: "note", text: "" });
    checkBtn.addEventListener("click", async () => {
      checkBtn.disabled = true;
      checkBtn.textContent = t("engine.checkingUpdates");
      try {
        const update = await api.engineCheckUpdate();
        if (update.update_available) {
          checkResult.textContent = t("engine.updateAvailable", {
            current: update.current,
            latest: update.latest ?? "?",
          });
        } else {
          checkResult.textContent = t("engine.upToDate", { version: update.current });
        }
      } catch {
        checkResult.textContent = t("engine.updateCheckFailed");
      } finally {
        checkBtn.disabled = false;
        checkBtn.textContent = t("engine.checkUpdates");
      }
    });

    box.append(
      el("div", { class: "field-row mt-8" }, [
        installBtn,
        diagBtn,
      ]),
      el("div", { class: "note", text: t("engine.notePinned") }),
      el("div", { class: "field-row mt-6" }, [checkBtn, checkResult]),
      el("div", { class: "note", text: t("engine.noteUpdateCheck") }),
      el("div", { class: "field-row mt-6" }, [
        el("div", { class: "grow-1" }, [pathInput]),
        applyBtn,
      ]),
      el("div", { class: "note", text: t("engine.noteCustom") }),
    );
  };

  try {
    draw(await api.engineStatus());
  } catch (e) {
    box.append(errorLine((e as ApiError).message));
  }
}
