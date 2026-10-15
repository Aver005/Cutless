# MAP
> Где что лежит. Last updated: 2026-10-16.

Полная таблица крейтов апстрима — `src/README.md`. Здесь — дерево с акцентом на места, которые
трогает Cutless. `*` — точки вставки Cutless (план).

## Корень

| Путь | Что |
|---|---|
| `src/` | Cargo-workspace (**все `cargo` отсюда**), `rust-toolchain.toml`, `.cargo/config.toml` (алиас `cargo dev`), `rustfmt.toml`, `.config/nextest.toml` |
| `src/crates/` | Все крейты апстрима; сюда же лягут `cutless-*` |
| `src/patches/i-slint-renderer-skia/` | Вендоренный патч рендерера Slint: ClearType, привязка глифов к пикселям |
| `assets/` | Иконки, логотипы, инсталляторы (`windows/`, `linux/`, `arch/`). Ребрендинг — `reference/REBRAND.md` |
| `models/manifest.toml` | 23 скачиваемые модели с SHA-256 и лицензиями |
| `scripts/` | `locales.py`, `models.py` (проверки), `readme/` (генератор README), `ci/` (смоук, зависимости) |
| `.github/workflows/` | `ci.yml`, `release.yml`, `build-app.yml`, `mobile.yml`, `nix.yml`, `models.yml` |
| `.claude/skills/run-concat/` | Навык апстрима: как собрать и запустить окно |
| `audit/` | Отчёты ревью кода апстрима, последний — `AUDIT-2026-10-12-QUICK.md` |
| `docs/` | `KEYBOARD_SHORTCUTS.md` и 25 переводов README |
| `ARCHITECTURE.md`, `CONTRIBUTING.md`, `TRADEMARK.md`, `LICENSE-EXCEPTIONS.md`, `CLA.md` | Документы апстрима |
| `CLAUDE.md`, `.memories/` | Cutless: мост для агентов и база знаний |

## Движок — `src/crates/`

| Крейт | Главные файлы для нас |
|---|---|
| `concat-core` | Время `Rational`, `Timeline` движка |
| `concat-project` | `model.rs` (документ), `commands/mod.rs` (`Command`) и `commands/{clips,properties,audio,tracks,timelines,media}.rs`, `editor.rs` (undo, жесты), `doc.rs` (`concat.json`, миграции, `extra`), `placement.rs` (правило размещения) |
| `concat-media` | `decode.rs`, `encode.rs` (кодеки), `hardware.rs`, `pool.rs`, `prefetch.rs`, `peaks.rs` (волны), `samples.rs` (PCM — источник для `cutless-beats`) |
| `concat-render` | `plan.rs` (`plan_frame`), `gpu.rs` (композитор) |
| `concat-effects` | `packages/<id>/effect.{toml,wgsl}` — 123 пакета; сюда лягут анимации текста и модификаторы Ф5 |
| `concat-text` | Рендер титров. Раскладка по словам для Ф5.3 — здесь `[?]` |
| `concat-export` | `lib.rs` (цикл кадров), `flatten.rs`, `resolve.rs` |
| `concat-host` | `session.rs` (`Session::apply`), `dirs.rs` (`IDENTIFIER`), `updates.rs`, `models.rs`, `proxy.rs`, `playback.rs`, `jobs.rs` (`SingleFlight`), `templates.rs`, `reverse.rs`, `record.rs` |
| `concat-speech` | `transcribe.rs` (whisper, `Segment {start,end,text}`), `tts.rs` |
| `concat-vision` | `segment.rs` (вырезание), `enhance.rs` (апскейл) |
| `concat-api` | `message.rs` (`Request`, `Reply`, `Event`, `ErrorCode`), `lib.rs` (`Api::dispatch`, `OpenProjects`/`Holder`) `*` новые глаголы |
| `concat-server` | `lib.rs`, `json.rs`, `grpc.rs`, `hub.rs`, `token.rs` `*` MCP-транспорт |
| `concat-cli` | `main.rs`: `probe`, `render`, `api`, `serve`, `preview`, `check` `*` подкоманда `mcp` |

## Окно — `src/crates/concat/`

| Путь | Что |
|---|---|
| `src/lib.rs` | Связка колбэков Slint |
| `src/studio.rs` | Контроллер (≈11 тыс. строк): `Studio::handle`, `apply`, `after_change`, `publish` `*` мост агента |
| `src/panes/*.rs` | 12 панелей; `export.rs` — образец, `settings.rs` — страница Remote, `captions.rs`, `speech.rs` `*` `agent.rs` |
| `src/panes/mod.rs` | Перечисление `Msg` `*` `Msg::Agent` |
| `src/host.rs` | `spawn`, `spawn_in_project`, epoch |
| `src/dock.rs` | Дерево сплитов дока `*` панель агента |
| `src/gpu.rs`, `platform.rs`, `prefs.rs`, `i18n.rs`, `fonts.rs` | GPU, платформы, настройки, переводы, шрифты |
| `ui/app.slint` | Корень интерфейса, горячие клавиши (`KeyBinding`), `title` |
| `ui/theme/` | Все цвета: `theme.slint`, `dark.slint`, `light.slint`, `accent.slint` |
| `ui/primitives/` | Свои виджеты: knob, select, color-picker, modal, toast, tooltip… |
| `ui/workspace/`, `ui/timeline/`, `ui/inspector/`, `ui/dialogs/` | Панели, таймлайн, инспектор, диалоги `*` `workspace/agent-pane.slint` |
| `ui/phone/` | Мобильная оболочка |
| `locales/*.json` | Переводы, `en.json` — полный список ключей |
| `build.rs` | Компиляция `.slint` в потоке со стеком 512 МБ |

## Будущие крейты Cutless

| Крейт | Фаза |
|---|---|
| `src/crates/cutless-tools` | Ф2 |
| `src/crates/cutless-mcp` | Ф2 |
| `src/crates/cutless-agent` | Ф4 |
| `src/crates/cutless-beats` | Ф5 |
| `src/crates/cutless-recipes` | Ф6 |
