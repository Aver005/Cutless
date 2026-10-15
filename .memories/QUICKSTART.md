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

Нужны MSVC C++ (Visual Studio 2022 или Build Tools с компонентом CMake) и rustup. Rust 1.93.1 rustup
поставит сам по `src/rust-toolchain.toml`.

1. Зависимости сборки — один раз, из Git Bash в корне:
   ```
   bash scripts/cutless/win-deps.sh
   ```
   Скрипт кладёт в `vendor/` (папка в `.gitignore`) FFmpeg 8.1 shared, ONNX Runtime 1.28, sherpa-onnx и
   `libclang.dll` из LLVM 21.1.0. Хэши сверяются с `scripts/ci/deps.lock`, хэш LLVM закреплён в самом
   скрипте. cmake берётся из Visual Studio. Пишет `vendor/env.sh` и `vendor/env.ps1`.
2. Окружение — в каждой новой консоли:
   - Git Bash: `source vendor/env.sh`;
   - PowerShell: `. vendor/env.ps1`.

   Без него не найдутся FFmpeg и libclang, а окно и тесты не стартуют без DLL в `PATH`.
3. Окно:
   ```
   cd src
   cargo dev
   ```
4. Без окна:
   - `cargo run -p concat-cli -- api` — JSON-RPC через stdin/stdout;
   - `cargo run -p concat-cli -- api '{"method":"version"}'` — один запрос;
   - `cargo run -p concat-cli -- serve` — JSON-RPC на `127.0.0.1:7420`. Токен печатается при старте
     или задаётся через `CONCAT_API_TOKEN`; писать можно только в папки `--root`, по умолчанию — домашняя.

Как устроено в CI апстрима — `.github/workflows/ci.yml`, задача `desktop` на windows-2022. Время сборки
и результаты — `STATE.md`.
