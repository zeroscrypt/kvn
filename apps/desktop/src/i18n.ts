// Interface strings (ru / en). Russian is the default.
//
// Technical messages coming from the Rust backend (engine errors, import
// issues from kvn-core) stay as-is: they are diagnostics and appear in logs
// and the diagnostics bundle. Everything the interface itself says is here.

export type Lang = "ru" | "en";

const ru = {
  // ------------------------------------------------------------- common ----
  "common.save": "Сохранить",
  "common.saving": "Сохранение…",
  "common.close": "Закрыть",
  "common.clear": "Очистить",
  "common.delete": "Удалить",
  "common.update": "Обновить",
  "common.updating": "Обновление…",
  "common.rename": "Переименовать",
  "common.confirmDelete": "Подтвердить удаление?",
  "common.yes": "да",
  "common.no": "нет",

  // ------------------------------------------------------------- topbar ----
  "topbar.suffix": "консоль для xray-core",
  "topbar.palette": "Палитра {mod}K",
  "topbar.settings": "Настройки",
  "topbar.chipTestMode": "тест-режим",
  "topbar.chipEngine": "движок {version}",
  "topbar.chipEngineMissing": "движок не найден",
  "topbar.chipSysproxy": "системный прокси",

  // ------------------------------------------------------------ sidebar ----
  "sidebar.profiles": "Профили",
  "sidebar.import": "+ Импорт",
  "sidebar.subs": "Подписки",
  "sidebar.empty": "Профилей нет. Импортируйте share-ссылку.",
  "sidebar.itemTitle": "Enter: подключить · {mod}K: палитра",

  // -------------------------------------------------------------- phase ----
  "phase.disconnected": "Отключено",
  "phase.connecting": "Подключение…",
  "phase.checking": "Проверка…",
  "phase.connected": "Подключено",
  "phase.reconnecting": "Переподключение…",
  "phase.stopping": "Остановка…",
  "phase.error": "Ошибка",

  // ------------------------------------------------------------- status ----
  "status.panelAria": "Статус",
  "status.starting": "Запуск…",
  "status.noProfile": "активный профиль не выбран",
  "status.attempt": " · попытка {attempt}/{max}",
  "metrics.latency": "Задержка",
  "metrics.uptime": "Время",
  "metrics.down": "Приём",
  "metrics.up": "Отдача",

  "buttons.connect": "Подключить",
  "buttons.disconnect": "Отключить",
  "buttons.cancel": "Отмена",
  "buttons.stopping": "Остановка…",
  "buttons.testRtt": "Проверить RTT",
  "buttons.testRttTitle": "TCP-задержка до {endpoint}",
  "buttons.testRttNeedProfile": "Сначала выберите профиль",
  "buttons.sysproxyOn": "Сист. прокси: вкл",
  "buttons.sysproxyOff": "Сист. прокси: выкл",
  "buttons.sysproxyTitle": "HTTP/HTTPS системный прокси на локальный тоннель",
  "buttons.sysproxyUnavailable": "на этой платформе пока недоступно",

  "errorHint.engine_missing":
    "Установите движок в настройках или укажите свой бинарник xray.",
  "errorHint.config_invalid":
    "Движок отклонил сгенерированный конфиг — подробности в журнале событий.",
  "errorHint.engine_failed":
    "Процесс движка не запустился или завершился. Смотрите события и диагностику.",
  "errorHint.health_check_failed":
    "Тоннель не пропустил реальный трафик. Сервер может быть недоступен или параметры профиля неверны.",
  "errorHint.network_unreachable":
    "Сервер не ответил из этой сети (DNS, фаервол или сервер выключен).",

  // ------------------------------------------------------------ profile ----
  "profile.title": "Выбранный профиль",
  "profile.selectNote": "Выберите профиль в списке.",
  "profile.name": "Имя",
  "profile.protocol": "Протокол",
  "profile.endpoint": "Сервер",
  "profile.transport": "Транспорт",
  "profile.security": "Безопасность",
  "profile.flow": "Flow",
  "profile.source": "Источник",
  "profile.subscription": "Подписка",
  "profile.warnings": "Предупреждения",
  "profile.renameAria": "Новое имя профиля",
  "src.custom": "свой путь",
  "src.installed": "установлен KVN",
  "src.bundled": "встроен в приложение",
  "src.path": "из PATH",

  // ------------------------------------------------------------- events ----
  "events.title": "События",
  "events.line1": "строка",
  "events.line2": "строки",
  "events.line5": "строк",

  // ---------------------------------------------------------- statusbar ----
  "statusbar.engine": "движок {version} ({source})",
  "statusbar.engineNotFound": "движок не найден",
  "statusbar.phase": "фаза: {phase}",
  "statusbar.hint": "{mod}K палитра · ↑↓ навигация · ↵ действие",

  // ------------------------------------------------------------- import ----
  "import.title": "Импорт",
  "import.modalAria": "Импорт профилей",
  "import.label": "Share-ссылки или тело подписки",
  "import.textareaAria": "Share-ссылки или тело подписки",
  "import.placeholder":
    "vless://… | vmess://… | trojan://… | ss://…\nНесколько ссылок, base64 или тело подписки",
  "import.hint":
    "Вставьте ссылки VLESS / VMess / Trojan / Shadowsocks, base64-подписку или загрузите файл.",
  "import.fromFile": "Из файла",
  "import.fileAria": "Загрузить ссылки из файла",
  "import.btn": "Импортировать",
  "import.btnBusy": "Импорт…",
  "import.nothing": "нечего импортировать",
  "import.result": "добавлено {added}, обновлено {replaced}, ошибок {failed}",

  // ------------------------------------------------------ subscriptions ----
  "subs.label": "Подписки",
  "subs.namePlaceholder": "имя",
  "subs.urlPlaceholder": "https://…/sub/…",
  "subs.nameAria": "Имя подписки",
  "subs.urlAria": "URL подписки",
  "subs.add": "Добавить и загрузить",
  "subs.adding": "Загрузка…",
  "subs.required": "укажите URL подписки",
  "subs.none": "подписок пока нет",
  "subs.never": "ещё не обновлялось",
  "subs.profile1": "профиль",
  "subs.profile2": "профиля",
  "subs.profile5": "профилей",
  "subs.updatedLog": "подписка обновлена: {name}",
  "subs.deletedLog": "подписка удалена: {name}",
  "subs.addedLog": "подписка добавлена: {name}",

  // ----------------------------------------------------------- settings ----
  "settings.title": "Настройки",
  "settings.loading": "загрузка…",
  "settings.sectionInterface": "Интерфейс",
  "settings.language": "Язык интерфейса",
  "settings.languageChanged": "язык интерфейса: {lang}",
  "settings.sectionConnection": "Соединение",
  "settings.sectionRouting": "Маршрутизация",
  "settings.sectionFragment": "Фрагментация",
  "settings.sectionOther": "Прочее",
  "settings.labelSocks": "SOCKS-порт",
  "settings.labelHttp": "HTTP-порт",
  "settings.labelDns": "DNS (удалённый)",
  "settings.ariaSocks": "SOCKS-порт",
  "settings.ariaHttp": "HTTP-порт",
  "settings.ariaDns": "Удалённый DNS",
  "settings.labelBypassLan": "Обходить локальные сети",
  "settings.ariaBypassLan": "Обход локальных сетей",
  "settings.labelSniffing": "Sniffing",
  "settings.ariaSniffing": "Sniffing",
  "settings.labelBypassDomains": "Домены в обход",
  "settings.ariaBypassDomains": "Домены в обход",
  "settings.hintBypassDomains":
    "По одному в строке. Полезно, когда сайты должны идти напрямую при включённом VPN.",
  "settings.labelBypassIps": "IP в обход",
  "settings.ariaBypassIps": "IP в обход",
  "settings.hintBypassIps": "По одному адресу/CIDR в строке, например 192.168.0.0/16",
  "settings.labelFragment": "TCP-фрагментация (VLESS anti-DPI)",
  "settings.ariaFragment": "TCP-фрагментация",
  "settings.labelPackets": "Пакеты",
  "settings.ariaPackets": "Фрагментируемые пакеты",
  "settings.labelLength": "Длина",
  "settings.ariaLength": "Длина фрагментов",
  "settings.labelInterval": "Интервал",
  "settings.ariaInterval": "Интервал фрагментации",
  "settings.labelLogLevel": "Уровень логов",
  "settings.ariaLogLevel": "Уровень логов",
  "settings.labelSysproxy":
    "Системный прокси автоматически при подключении",
  "settings.ariaSysproxy": "Системный прокси автоматически",
  "settings.labelTestMode": "Тестовый режим (без реального VPN)",
  "settings.ariaTestMode": "Тестовый режим",
  "settings.warnTestMode":
    "Тестовый режим запускает симулированный движок: реального трафика нет. Для проверки интерфейса без сервера.",
  "settings.warnFragment":
    "TCP-фрагментация локально режет VLESS-поток; работает только с VLESS и может ломать некоторые серверы. Включайте только при необходимости.",
  "settings.savedLog": "настройки сохранены",

  // ------------------------------------------------------------- engine ----
  "engine.title": "Движок (Xray-core)",
  "engine.found": "Найден",
  "engine.version": "Версия",
  "engine.path": "Путь",
  "engine.source": "Источник",
  "engine.pinned": "Закреплённый релиз",
  "engine.asset": "Платформенный файл",
  "engine.unsupported": "нет для этой платформы",
  "engine.install": "Установить закреплённый движок",
  "engine.installing": "Скачивание…",
  "engine.installedLog": "движок установлен: {version}",
  "engine.notePinned":
    "Закреплённый движок — официальный релиз Xray-core, проверяемый по SHA-256 перед использованием.",
  "engine.noteCustom":
    "Пользовательский бинарник запускается как есть; проверяйте его происхождение сами. Перед запуском он проверяется командой `xray version`.",
  "engine.usePath": "Использовать путь",
  "engine.pathPlaceholder": "/путь/к/xray (пусто — авто)",
  "engine.pathAria": "Свой путь к движку",
  "engine.pathSetLog": "свой путь к движку задан",
  "engine.pathClearedLog": "свой путь убран — снова автоопределение",
  "engine.diag": "Экспорт диагностики",
  "engine.diagWritten": "записано в {path}",
  "engine.checkUpdates": "Проверить обновления ядра",
  "engine.checkingUpdates": "Проверка…",
  "engine.upToDate": "Установлен последний релиз ядра ({version})",
  "engine.updateAvailable": "Доступно обновление ядра: {latest} (сейчас {current})",
  "engine.updateCheckFailed": "Не удалось проверить обновления",
  "engine.noteUpdateCheck":
    "Информационная проверка последнего релиза Xray-core на GitHub. Обновление движка приходит вместе с новой версией KVN (пин в core/xray.json).",

  // ------------------------------------------------------------ palette ----
  "palette.placeholder": "наберите команду…",
  "palette.label": "Командная палитра",
  "palette.empty": "ничего не найдено",
  "cmd.connect": "Подключить",
  "cmd.disconnect": "Отключить",
  "cmd.import": "Импорт профилей…",
  "cmd.settings": "Настройки…",
  "cmd.ping": "Проверить RTT сервера",
  "cmd.sysproxyEnable": "Включить системный прокси",
  "cmd.sysproxyDisable": "Выключить системный прокси",
  "cmd.diagnostics": "Экспорт диагностики",
  "cmd.connectProfile": "Подключить: {name}",
  "cmd.hintNoProfile": "профиль не выбран",
  "cmd.hintNotAvailable": "недоступно здесь",
  "cmd.hintRedacted": "без секретов",

  // ---------------------------------------------------------------- log ----
  "log.statusFailed": "не удалось получить статус: {message}",
  "log.testModeOn": "включён тестовый режим — соединения симулируются",
  "log.ready": "kvn {version} готов — {profiles}",
  "log.profileDeleted": "профиль удалён",
  "log.renamed": "переименован в «{name}»",
  "log.sysproxyOn": "системный прокси включён",
  "log.sysproxyOff": "системный прокси выключен",
  "log.rtt": "{name}: TCP RTT {ms} мс",
} as const;

export type StringKey = keyof typeof ru;

const en: Record<StringKey, string> = {
  "common.save": "Save",
  "common.saving": "Saving…",
  "common.close": "Close",
  "common.clear": "Clear",
  "common.delete": "Delete",
  "common.update": "Update",
  "common.updating": "Updating…",
  "common.rename": "Rename",
  "common.confirmDelete": "Confirm delete?",
  "common.yes": "yes",
  "common.no": "no",

  "topbar.suffix": "console for xray-core",
  "topbar.palette": "Palette {mod}K",
  "topbar.settings": "Settings",
  "topbar.chipTestMode": "test mode",
  "topbar.chipEngine": "engine {version}",
  "topbar.chipEngineMissing": "engine missing",
  "topbar.chipSysproxy": "system proxy",

  "sidebar.profiles": "Profiles",
  "sidebar.import": "+ Import",
  "sidebar.subs": "Subs",
  "sidebar.empty": "No profiles. Import a share link to begin.",
  "sidebar.itemTitle": "Enter: connect · {mod}K: palette",

  "phase.disconnected": "Disconnected",
  "phase.connecting": "Connecting…",
  "phase.checking": "Checking…",
  "phase.connected": "Connected",
  "phase.reconnecting": "Reconnecting…",
  "phase.stopping": "Stopping…",
  "phase.error": "Error",

  "status.panelAria": "Status",
  "status.starting": "Starting…",
  "status.noProfile": "no active profile",
  "status.attempt": " · attempt {attempt}/{max}",
  "metrics.latency": "Latency",
  "metrics.uptime": "Uptime",
  "metrics.down": "Down",
  "metrics.up": "Up",

  "buttons.connect": "Connect",
  "buttons.disconnect": "Disconnect",
  "buttons.cancel": "Cancel",
  "buttons.stopping": "Stopping…",
  "buttons.testRtt": "Test RTT",
  "buttons.testRttTitle": "TCP round trip to {endpoint}",
  "buttons.testRttNeedProfile": "Select a profile first",
  "buttons.sysproxyOn": "Sys proxy: on",
  "buttons.sysproxyOff": "Sys proxy: off",
  "buttons.sysproxyTitle": "HTTP/HTTPS system proxy pointing at the local tunnel",
  "buttons.sysproxyUnavailable": "not available on this platform yet",

  "errorHint.engine_missing":
    "Install the engine in Settings or point KVN at your own xray binary.",
  "errorHint.config_invalid":
    "The engine rejected the generated config — details are in the events log.",
  "errorHint.engine_failed":
    "The engine process failed to start or died. Check events and diagnostics.",
  "errorHint.health_check_failed":
    "The tunnel did not carry real traffic. The server may be down or the profile params wrong.",
  "errorHint.network_unreachable":
    "The server did not answer from this network (DNS, firewall or server down).",

  "profile.title": "Selected profile",
  "profile.selectNote": "Select a profile in the sidebar.",
  "profile.name": "Name",
  "profile.protocol": "Protocol",
  "profile.endpoint": "Endpoint",
  "profile.transport": "Transport",
  "profile.security": "Security",
  "profile.flow": "Flow",
  "profile.source": "Source",
  "profile.subscription": "Subscription",
  "profile.warnings": "Warnings",
  "profile.renameAria": "New profile name",
  "src.custom": "custom path",
  "src.installed": "installed by KVN",
  "src.bundled": "bundled with app",
  "src.path": "PATH",

  "events.title": "Events",
  "events.line1": "line",
  "events.line2": "lines",
  "events.line5": "lines",

  "statusbar.engine": "engine {version} ({source})",
  "statusbar.engineNotFound": "engine: not found",
  "statusbar.phase": "phase: {phase}",
  "statusbar.hint": "{mod}K palette · ↑↓ navigate · ↵ action",

  "import.title": "Import",
  "import.modalAria": "Import profiles",
  "import.label": "Share links or subscription body",
  "import.textareaAria": "Share links or subscription body",
  "import.placeholder":
    "vless://… | vmess://… | trojan://… | ss://…\nMultiple links, base64 payload or a subscription body",
  "import.hint":
    "Paste VLESS / VMess / Trojan / Shadowsocks links, a base64 subscription payload, or load a file.",
  "import.fromFile": "From file",
  "import.fileAria": "Load links from a file",
  "import.btn": "Import",
  "import.btnBusy": "Importing…",
  "import.nothing": "nothing to import",
  "import.result": "added {added}, replaced {replaced}, failed {failed}",

  "subs.label": "Subscriptions",
  "subs.namePlaceholder": "name",
  "subs.urlPlaceholder": "https://…/sub/…",
  "subs.nameAria": "Subscription name",
  "subs.urlAria": "Subscription URL",
  "subs.add": "Add + fetch",
  "subs.adding": "Fetching…",
  "subs.required": "subscription URL is required",
  "subs.none": "no subscriptions yet",
  "subs.never": "never updated",
  "subs.profile1": "profile",
  "subs.profile2": "profiles",
  "subs.profile5": "profiles",
  "subs.updatedLog": "subscription updated: {name}",
  "subs.deletedLog": "subscription deleted: {name}",
  "subs.addedLog": "subscription added: {name}",

  "settings.title": "Settings",
  "settings.loading": "loading…",
  "settings.sectionInterface": "Interface",
  "settings.language": "Interface language",
  "settings.languageChanged": "interface language: {lang}",
  "settings.sectionConnection": "Connection",
  "settings.sectionRouting": "Routing",
  "settings.sectionFragment": "Fragmentation",
  "settings.sectionOther": "Other",
  "settings.labelSocks": "SOCKS port",
  "settings.labelHttp": "HTTP port",
  "settings.labelDns": "DNS (remote)",
  "settings.ariaSocks": "SOCKS port",
  "settings.ariaHttp": "HTTP port",
  "settings.ariaDns": "Remote DNS",
  "settings.labelBypassLan": "Bypass local networks",
  "settings.ariaBypassLan": "Bypass LAN",
  "settings.labelSniffing": "Sniffing",
  "settings.ariaSniffing": "Sniffing",
  "settings.labelBypassDomains": "Bypass domains",
  "settings.ariaBypassDomains": "Bypass domains",
  "settings.hintBypassDomains":
    "One per line. Useful when routes or sites must go direct while still connected.",
  "settings.labelBypassIps": "Bypass IPs",
  "settings.ariaBypassIps": "Bypass IPs",
  "settings.hintBypassIps": "One CIDR/address per line, e.g. 192.168.0.0/16",
  "settings.labelFragment": "TCP fragmentation (VLESS anti-DPI)",
  "settings.ariaFragment": "Fragment",
  "settings.labelPackets": "Packets",
  "settings.ariaPackets": "Fragment packets",
  "settings.labelLength": "Length",
  "settings.ariaLength": "Fragment length",
  "settings.labelInterval": "Interval",
  "settings.ariaInterval": "Fragment interval",
  "settings.labelLogLevel": "Log level",
  "settings.ariaLogLevel": "Log level",
  "settings.labelSysproxy":
    "Set system proxy automatically while connected",
  "settings.ariaSysproxy": "System proxy desired",
  "settings.labelTestMode": "Test mode (no real VPN)",
  "settings.ariaTestMode": "Test mode",
  "settings.warnTestMode":
    "Test mode runs a simulated engine: no real tunnel traffic is created. Use it to rehearse the interface without a server.",
  "settings.warnFragment":
    "TCP fragmentation splits the VLESS stream locally; it only applies to VLESS profiles and may break some servers. Leave it off unless needed.",
  "settings.savedLog": "settings saved",

  "engine.title": "Engine (Xray-core)",
  "engine.found": "Found",
  "engine.version": "Version",
  "engine.path": "Path",
  "engine.source": "Source",
  "engine.pinned": "Pinned release",
  "engine.asset": "Platform asset",
  "engine.unsupported": "unsupported platform",
  "engine.install": "Install pinned engine",
  "engine.installing": "Downloading…",
  "engine.installedLog": "engine installed: {version}",
  "engine.notePinned":
    "The pinned engine is the official Xray-core release, verified by SHA-256 before use.",
  "engine.noteCustom":
    "A user-provided binary is run as-is; verify its origin yourself. It is probed with `xray version` first.",
  "engine.usePath": "Use path",
  "engine.pathPlaceholder": "/path/to/xray (leave empty for auto)",
  "engine.pathAria": "Custom engine path",
  "engine.pathSetLog": "custom engine path set",
  "engine.pathClearedLog": "custom engine path cleared",
  "engine.diag": "Export diagnostics",
  "engine.diagWritten": "written to {path}",
  "engine.checkUpdates": "Check for engine updates",
  "engine.checkingUpdates": "Checking…",
  "engine.upToDate": "Running the latest engine release ({version})",
  "engine.updateAvailable": "Engine update available: {latest} (currently {current})",
  "engine.updateCheckFailed": "Could not check for updates",
  "engine.noteUpdateCheck":
    "Informational check of the latest Xray-core release on GitHub. Engine updates arrive with a new KVN release (pin in core/xray.json).",

  "palette.placeholder": "type a command…",
  "palette.label": "Command palette",
  "palette.empty": "nothing found",
  "cmd.connect": "Connect",
  "cmd.disconnect": "Disconnect",
  "cmd.import": "Import profiles…",
  "cmd.settings": "Settings…",
  "cmd.ping": "Test server RTT",
  "cmd.sysproxyEnable": "Enable system proxy",
  "cmd.sysproxyDisable": "Disable system proxy",
  "cmd.diagnostics": "Export diagnostics",
  "cmd.connectProfile": "Connect: {name}",
  "cmd.hintNoProfile": "no profile",
  "cmd.hintNotAvailable": "not available here",
  "cmd.hintRedacted": "redacted bundle",

  "log.statusFailed": "status poll failed: {message}",
  "log.testModeOn": "test mode is enabled — connections are simulated",
  "log.ready": "kvn {version} ready — {profiles}",
  "log.profileDeleted": "profile deleted",
  "log.renamed": "renamed to \"{name}\"",
  "log.sysproxyOn": "system proxy enabled",
  "log.sysproxyOff": "system proxy disabled",
  "log.rtt": "{name}: TCP RTT {ms} ms",
};

const dictionaries = { ru, en };

let current: Lang = "ru";

/** Set the active language (anything but "en" falls back to the default "ru"). */
export function setLang(value: string | null | undefined): void {
  current = value === "en" ? "en" : "ru";
  document.documentElement.lang = current;
}

export function getLang(): Lang {
  return current;
}

/** Translate a key; unknown keys return the key itself (visible, not silent). */
export function t(
  key: StringKey,
  params?: Record<string, string | number>,
): string {
  let text: string = dictionaries[current][key];
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      text = text.split(`{${name}}`).join(String(value));
    }
  }
  return text;
}

/**
 * Correct plural form per language.
 * ru: [one, few, many]; en: [one, many].
 */
export function plural(
  n: number,
  ruForms: [string, string, string],
  enForms: [string, string],
): string {
  if (current === "en") {
    return enForms[n === 1 ? 0 : 1];
  }
  const a = Math.abs(n) % 100;
  const b = a % 10;
  if (a > 10 && a < 20) return ruForms[2];
  if (b > 1 && b < 5) return ruForms[1];
  if (b === 1) return ruForms[0];
  return ruForms[2];
}

/** "строка/строки/строк" (ru) or "line/lines" (en). */
export function lineWord(n: number): string {
  return plural(
    n,
    [t("events.line1"), t("events.line2"), t("events.line5")],
    [t("events.line1"), t("events.line5")],
  );
}

/** "профиль/профиля/профилей" (ru) or "profile/profiles" (en). */
export function profileWord(n: number): string {
  return plural(
    n,
    [t("subs.profile1"), t("subs.profile2"), t("subs.profile5")],
    [t("subs.profile1"), t("subs.profile5")],
  );
}

const PHASE_KEYS: Record<string, StringKey> = {
  disconnected: "phase.disconnected",
  connecting: "phase.connecting",
  checking: "phase.checking",
  connected: "phase.connected",
  reconnecting: "phase.reconnecting",
  stopping: "phase.stopping",
  error: "phase.error",
};

export function phaseLabel(phase: string | null | undefined): string {
  if (!phase) return "—";
  const key = PHASE_KEYS[phase];
  return key ? t(key) : phase;
}

const SOURCE_KEYS: Record<string, StringKey> = {
  "custom path": "src.custom",
  "installed by KVN": "src.installed",
  "bundled with app": "src.bundled",
  PATH: "src.path",
};

export function sourceLabel(source: string | null | undefined): string {
  if (!source) return "—";
  const key = SOURCE_KEYS[source];
  return key ? t(key) : source;
}

const ERROR_HINT_KEYS: Record<string, StringKey> = {
  engine_missing: "errorHint.engine_missing",
  config_invalid: "errorHint.config_invalid",
  engine_failed: "errorHint.engine_failed",
  health_check_failed: "errorHint.health_check_failed",
  network_unreachable: "errorHint.network_unreachable",
};

export function errorHint(kind: string): string | null {
  const key = ERROR_HINT_KEYS[kind];
  return key ? t(key) : null;
}
