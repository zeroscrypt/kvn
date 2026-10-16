# KVN — терминальный клиент для 3x-ui / Xray

Минималистичный десктопный клиент собственного VPN-сервера на
**3x-ui + Xray-core**. Тёмный терминальный интерфейс (моноширинный Geist
Mono, графит + сдержанный циан), всё важное — на одном экране: профили,
статус туннеля с реальной латентностью и трафиком, журнал событий,
командная палитра. Интерфейс переключается RU/EN, работает в меню-баре
(трей) и собирается под macOS, Linux и Windows из одной кодовой базы.

> Статус: 0.1.0. Проверено вживую на macOS (arm64); для Linux и Windows
> настроен CI (сборка и тесты на каждый PR), **живой прогон туннеля на них
> ещё не выполнялся** (см. [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md)).
> Релизные сборки macOS не подписаны (нет Apple Developer account) — в
> релизах к артефактам приложены `SHA256SUMS` и build-provenance
> аттестация GitHub.

```
┌─ KVN ──────────────────────────────────────────────────────────────┐
│  PROFILES                    │  STATUS                            │
│  ▸ fast-edge (vless)         │  disconnected                      │
│    backup (vmess)            │  ────────────────────────────────  │
│    old (trojan)              │  profile    fast-edge              │
│                              │  endpoint   vpn.example.com:443    │
│                              │  protocol   vless / reality / xhttp│
│                              │  latency    —                      │
│  ────────────────────────    │  up/down    —                      │
│  [c]onnect [d]isconnect      │  ────────────────────────────────  │
│  [i]mport  [s]ettings        │  EVENTS                            │
│  ⌘K palette                  │  12:00:01 info  engine ready       │
│                              │  12:00:02 info  tunnel verified    │
└────────────────────────────────────────────────────────────────────┘
```

## Возможности

- **Протоколы**: VLESS (в т.ч. REALITY и XHTTP), VMess, Trojan,
  Shadowsocks (включая SS-2022) — импорт из share-ссылок.
- **Транспорты**: TCP (+HTTP-маскировка), WebSocket, gRPC, HTTPUpgrade,
  XHTTP/SplitHTTP. KCP/QUIC распознаются и честно отклоняются при
  подключении.
- **Импорт**: ссылка из буфера, файл, список ссылок, подписка (URL).
- **Готовность = реальный трафик**: «подключено» появляется только после
  успешного HTTP-запроса через туннель, а не после старта процесса.
- **Статистика**: объём и скорость трафика через stats API Xray,
  латентность health-check.
- **Логи**: журнал событий в UI + файл `logs/kvn.log` для пост-мортема.
- **Языки**: весь интерфейс переключается «Русский | English»
  (по умолчанию русский), выбор сохраняется.
- **Меню-бар (трей)**: значок-щит в меню-баре — статус, подключение/
  отключение, открытие окна и настроек, выход; закрытие окна скрывает
  приложение в трей, туннель продолжает работать.
- **Безопасность**: движок Xray-core поставляется в комплекте (sidecar)
  и/или скачивается только из официального релиза с проверкой SHA-256
  (пин — `core/xray.json`); секреты не попадают в логи и диагностику
  (см. [SECURITY.md](docs/SECURITY.md)).
- **Приватность**: никакой телеметрии; сеть — только движок, подписки,
  health-check, а проверка обновлений ядра — только по явной кнопке.
- **Тестовый режим**: полный прогон UI без реального VPN.
- **Системный прокси**: macOS — да (`networksetup`, с восстановлением
  после падений), Linux — только GNOME, Windows — пока нет.

## Сборка и запуск (macOS)

Требуется: Rust stable, Node.js ≥ 20, pnpm, python3 (скрипты проверки
digest-ов).

```bash
cd apps/desktop
pnpm install

# sidecar-движок: релизные сборки несут Xray-core в комплекте (ADR 0005),
# эта команда скачивает закреплённый релиз и проверяет SHA-256 из core/xray.json
../scripts/fetch-xray.sh --bundle

# dev-режим (Vite-сервер поднимается автоматически)
pnpm tauri dev

# релизная сборка (.app/.dmg в target/release/bundle)
pnpm tauri build
```

> Важно: релизные бинарники обязаны собираться с фичей `custom-protocol`
> (`pnpm tauri build` включает её сам). «Голый» `cargo build --release`
> без неё даст пустое окно — без неё Tauri грузит `devUrl` вместо
> встроенного фронтенда. Если нужен именно сырой бинарник:
>
> ```bash
> pnpm build   # обязательный шаг: без dist сборка не пройдёт
> cargo build --release -p kvn-desktop --features custom-protocol
> ./target/release/kvn-desktop
> ```

Движок: релизные сборки включают закреплённый Xray-core (в настройках
видно источник — «в комплекте», «установлен приложением», «свой путь»,
«из PATH»). Кнопка Install в Settings → Engine переустанавливает/обновляет
движок до закреплённого релиза; кнопка «Проверить обновления» сверяет пин
с последним релизом Xray-core на GitHub (информационно — новое ядро
приезжает с новой версией KVN, см. ADR 0005). Для разработки и
интеграционных тестов:

```bash
scripts/fetch-xray.sh            # в .tools/xray-<platform>/ + проверка SHA-256
scripts/fetch-xray.sh --bundle   # sidecar-бинарь для tauri build
scripts/update-core-pin.py v26.4.0   # пере-пиннинг (обычно это делает CI-PR)
```

### Linux

```bash
# системные зависимости (Debian/Ubuntu-подобные)
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
    librsvg2-dev patchelf libssl-dev
# дальше как на macOS
cd apps/desktop && pnpm install && pnpm build && pnpm tauri build
```

### Windows

Требуется WebView2 (в Windows 11 уже есть) и MSVC toolchain:

```powershell
cd apps\desktop
pnpm install
pnpm build
pnpm tauri build
```

Статус проверки каждой платформы — в
[docs/PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md).

## Тесты

```bash
# ядро и движок (без живого Xray)
cargo test -p kvn-core -p kvn-engine

# приложение
cargo test -p kvn-desktop

# интеграционные тесты с реальным Xray (VLESS+REALITY: TCP, WS, XHTTP)
XRAY_BIN=$PWD/.tools/xray-macos-arm64/xray cargo test -p kvn-engine
```

CI прогоняет тесты (включая интеграционные с живым Xray) на macOS, Linux и
Windows; плюс `fmt`, `clippy -D warnings`, `cargo-deny` и проверки
conventional-commit/DCO. Все цифры тестов и замеров — в
[CHANGELOG.md](CHANGELOG.md) и [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## Релизы

Тег (`v0.1.0`) → сборка на всех платформах в GitHub Actions → артефакты
с build-provenance аттестацией и `SHA256SUMS` прикладываются к **черновику**
релиза; публикация черновика — это и есть выпуск (байты не пересобираются).
Версии ведёт release-please по Conventional Commits, пин движка обновляет
`core-update.yml` отдельным проверяемым PR.

## Структура

```
core/xray.json       пин движка: релиз, архивы, SHA-256, target-триплеты
crates/kvn-core      парсеры share-ссылок, генерация конфига Xray,
                     машина состояний, Secret-обёртка
crates/kvn-engine    процесс Xray: запуск, health-check, stats,
                     проверенная установка, манифест движка
apps/desktop         Tauri 2 приложение: Rust-бэкенд + vanilla TS UI
scripts/             fetch-xray.sh, update-core-pin.py, gen-icon.py,
                     gen-tray-icon.py, benchmark-app.py
docs/                ADR, RESEARCH, COMPATIBILITY, ARCHITECTURE,
                     SECURITY, BENCHMARKS, PLATFORM_SUPPORT
```

## Документация

| Документ | О чём |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | слои, поток подключения, машина состояний |
| [docs/SECURITY.md](docs/SECURITY.md) | модель угроз, секреты, цепочка поставки движка |
| [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) | матрица протоколов/транспортов |
| [docs/RESEARCH.md](docs/RESEARCH.md) | источники, эксперименты, версии |
| [docs/BENCHMARKS.md](docs/BENCHMARKS.md) | воспроизводимые замеры |
| [docs/PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) | что где проверено (и что нет) |
| [docs/adr/](docs/adr/) | решения по архитектуре |
| [CONTRIBUTING.md](CONTRIBUTING.md) | как собрать и прислать изменение |
| [SECURITY.md](SECURITY.md) | как сообщить об уязвимости |

## Чего KVN не делает принципиально

- Не спрашивает пароль администратора панели 3x-ui и не обращается к
  панели по сети — только ссылки/подписки, которые вы сами скопировали.
- Не отправляет телеметрию и не «звонит домой» при старте.
- Не отключает проверку TLS-сертификата по своей воле (если ссылка
  содержит `insecure=1`, это явно помечается предупреждением).
- Не изобретает криптографию: всё шифрование — Xray-core.

## Лицензия

MIT. См. [LICENSE](LICENSE). В бандл входят сторонние компоненты
(Xray-core — MPL-2.0, шрифт Geist Mono — OFL-1.1); текст лицензий и
уведомления — в ресурсах бандла (`THIRD_PARTY_NOTICES.md`).
