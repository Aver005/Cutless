# QUICKSTART
> Ориентир за 15 секунд. Last updated: 2026-10-16.

## Что это

**Cutless** — форк Concat, нативного видеоредактора уровня CapCut на Rust и Slint. Своё в форке — **ИИ-слой**:
- агент монтирует через те же команды, что и человек: один проект, общий undo;
- переносимые сценарии монтажа («рецепты»);
- монтажные примитивы для вертикальных роликов и монтажа под музыку.

Цель — инструмент для себя и платная подписка поверх бесплатного ядра (`PRODUCT.md`).

## Где горячие места

| Файл | Роль для нас |
|---|---|
| `src/crates/concat-project/src/commands/mod.rs` | `Command` — все правки проекта. Агент работает только через них |
| `src/crates/concat-project/src/model.rs` | Модель документа: `Project`, `Timeline`, `Track`, `Clip` |
| `src/crates/concat-project/src/editor.rs` | Undo, склейка жестов в один шаг |
| `src/crates/concat-api/src/{message,lib}.rs` | Методы API и диспетчер. Сюда добавляем глаголы |
| `src/crates/concat-server/src/` | JSON-RPC по сокету. Сюда ляжет MCP |
| `src/crates/concat-cli/src/main.rs` | Режимы без окна: `api`, `serve`. Сюда — `mcp` |
| `src/crates/concat/src/studio.rs` | Контроллер окна (≈11 тыс. строк). `Studio::apply` — точка моста агента к экрану |
| `src/crates/concat/src/panes/` | Панели окна. Образец для панели агента — `export.rs` |
| `src/crates/concat/ui/` | Интерфейс на Slint. Тема — `ui/theme/`, виджеты — `ui/primitives/` |
| `src/crates/concat-speech/src/transcribe.rs` | Whisper — источник транскрипта для агента |

Карта целиком — `MAP.md`.

## Как собрать (Windows)

1. Rust ставится сам по `src/rust-toolchain.toml` (1.93.1). Нужны cmake и MSVC C++.
2. FFmpeg 7+ — сборка BtbN `win64-gpl-shared`. Переменная `FFMPEG_DIR` указывает на неё, папка `bin`
   лежит в `PATH` (без DLL тесты и окно не стартуют).
3. Окно:
   ```
   cd src
   cargo dev
   ```
   Первый билд идёт несколько минут.
4. Без окна:
   - `cargo run -p concat-cli -- api` — JSON-RPC через stdin/stdout;
   - `cargo run -p concat-cli -- api '{"method":"version"}'` — один запрос;
   - `cargo run -p concat-cli -- serve` — JSON-RPC на `127.0.0.1:7420`. Токен печатается при старте
     или задаётся через `CONCAT_API_TOKEN`; писать можно только в папки `--root`, по умолчанию — домашняя.

Подробности — `src/README.md` и README апстрима, раздел Build from source. Локально пока ничего не
собиралось (`STATE.md`).
