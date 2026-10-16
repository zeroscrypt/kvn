# RESEARCH: что и как проверено

Документ фиксирует источники, эксперименты и версии, на которых построен
KVN. Всё, что не проверено, помечено явно.

## Версии, зафиксированные в проекте

| Компонент | Версия | Где зафиксировано |
|---|---|---|
| Xray-core (движок) | `v26.3.27` | `core/xray.json` (pinned release + SHA-256) |
| Tauri CLI / API | 2.12.3 / 2.12.4 | `apps/desktop/package.json` |
| Vite / TypeScript | 8.3.4 / 5.9.3 | `apps/desktop/package.json` |
| Rust (toolchain) | stable 1.99 | `rust-toolchain.toml` |
| tokio / ureq / zip | 1.x / 3.4.x / 9.x | `Cargo.lock` |

## Источники, изученные перед реализацией

1. **Xray-core** (официальный репозиторий XTLS/Xray-core):
   - схема XHTTP-транспорта (`transport/internet/splithttp/config.proto`) —
     оттуда взят список известных ключей `extra`, которые KVN пропускает
     в конфиг (неизвестные — в `notes`, не молча);
   - формат клиентского конфига (inbounds/outbounds/routing/dns/stats);
   - CLI: `xray run -test -config <file>` (валидация), `xray api
     statsquery` (счётчики), `xray version`, `xray x25519` (генерация
     ключей для тестов).
2. **3x-ui** (`internal/sub/service.go`): точный формат share-ссылок,
   которые панель генерирует для VLESS/VMess/Trojan/SS — включая
   `x_padding_bytes`, вложенный `extra` (JSON-строка в query), порядок и
   кодирование параметров, percent-encoding remark.
3. **Конвенции клиентов** (v2rayN/v2rayNG): SIP002 для SS, base64-JSON
   VMess, подписочные payload-ы (plain или base64 поверх списка ссылок).

## Эксперименты с живым движком (macOS arm64)

Проведены интеграционными тестами (`crates/kvn-engine/tests/xray_integration.rs`,
запускаются реальными Xray-процессами на 127.0.0.1):

- VLESS + REALITY + TCP (`xtls-rprx-vision`), WS и XHTTP — трафик
  действительно проходит через туннель (HTTP 204 через SOCKS5).
- Готовность: `wait_for_port` недостаточно; `check_through_socks`
  подтверждает живой трафик. Латентность измеряется в момент прихода
  заголовков ответа.
- **Счётчики статистики** (`xray api statsquery`):
  - `pattern` — подстроковый фильтр (пустой = всё);
  - для VLESS исходящий `outbound>>>proxy>>>traffic>>>uplink` наблюдался
    **нулевым** при живом трафике → первичны inbound-счётчики
    (`inbound>>>socks-in>>>…`/`http-in`), outbound — фолбэк;
  - `api`-inbound добавляет служебный трафик — исключается из суммы;
  - при нулевом значении поле `value` **отсутствует**; ненулевое может
    прийти числом или строкой — парсер принимает оба варианта (на этом
    был реальный баг, исправлен и покрыт тестами).
- Остановка: серверный Xray при штатном закрытии может задерживать FIN
  (~2 с) — поэтому дренаж ответа ограничен (64 КБ / 1.5 с), а остановка
  процессов — SIGTERM + grace + kill.

## Digest-ы релиза движка (пин `core/xray.json`, встроен в бинарь)

Манифест встраивается в приложение на этапе компиляции
(`crates/kvn-engine/src/manifest.rs`), скрипты и CI читают тот же файл —
рассинхрон невозможен. Все digest-ы проверены скачиванием архивов.

`https://github.com/XTLS/Xray-core/releases/download/v26.3.27/<файл>`

| Платформа | Файл | SHA-256 |
|---|---|---|
| macOS arm64 | `Xray-macos-arm64-v8a.zip` | `2e93a67e8aa1936ecefb307e120830fcbd4c643ab9b1c46a2d0838d5f8409eaf` |
| macOS x86_64 | `Xray-macos-64.zip` | `f5b0471d3459eff1b82e48af0aeac186abcc3298210070afbbbd8437a4e8b203` |
| Linux x86_64 | `Xray-linux-64.zip` | `23cd9af937744d97776ee35ecad4972cf4b2109d1e0fe6be9930467608f7c8ae` |
| Linux arm64 | `Xray-linux-arm64-v8a.zip` | `4d30283ae614e3057f730f67cd088a42be6fdf91f8639d82cb69e48cde80413c` |
| Windows x86_64 | `Xray-windows-64.zip` | `d004c39288ce9ada487c6f398c7c545f7d749e44bdfdd59dbc9f865afba4e1ad` |

macOS-arm64 digest дополнительно подтверждён локальным скачиванием и
запуском (`xray version` → `Xray 26.3.27 …`). Остальные — из `.dgst`
файлов релиза; логика проверки одинакова, но фактический запуск этих
сборок на их платформах — **не проверено** (см. PLATFORM_SUPPORT.md).

## Платформенные факты

- macOS: `networksetup -listnetworkserviceorder` даёт пары «сервис —
  устройство»; `networksetup -getwebproxy <сервис>` печатает
  `Enabled:/Server:/Port:`. Прокси-настройки меняются **без sudo**.
  Формат вывода покрыт юнит-тестами парсеров (`sysproxy.rs`).
- Tauri 2: capability `core:default` покрывает всё, что нужно UI
  (собственные команды не требуют permissions); CSP задаётся в
  `tauri.conf.json` и допускает только свои ресурсы + IPC.
- Tauri 2 (проверено эмпирически): **релизный бинарник обязан собираться с
  фичей `custom-protocol`** (`pnpm tauri build` делает это сам). Без неё
  `tauri::generate_context!` помечает контекст как dev и грузит `devUrl` —
  бинарник, запущенный без dev-сервера, показывает пустое окно. Симптом
  ловится в журнале: нет событий «boot probe loaded»/«ui module loaded».
  Именно этот случай добавлен отдельной проверкой в CI
  (`cargo check --release -p kvn-desktop --features custom-protocol`).
- Стандартный строгий CSP (`script-src 'self'`) корректно работает с
  встроенными ассетами на схеме `tauri://localhost` — проверено
  release-сборкой.
- Vite 8 на сборке использует rolldown; `minify: "esbuild"` требует
  отдельной установки esbuild — в конфиге не используется.

## Открытые вопросы

- Реальный запуск приложения и интеграционные тесты на Linux/Windows
  (в CI — сборка; живой прогон ожидается от пользователей этих платформ).
- Поведение `extra`-ключей XHTTP при будущих версиях Xray: KVN пропускает
  известные и предупреждает о неизвестных — при обновлении движка стоит
  сверять список.
- KCP/QUIC: схемы конфигов есть, реализация сборки отложена сознательно.
- Windows-системный прокси: механизм (реестр `Internet Settings`/WinINet)
  исследован не до конца — фича не заявлена.
