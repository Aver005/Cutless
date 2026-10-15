# CLAUDE.md

**Cutless** — форк видеоредактора Concat (Rust + Slint) с ИИ-слоем: агент монтирует
командами, человек — руками, оба в одном проекте.

Начинай сессию с `.memories/INDEX.md`. Если заметка расходится с кодом — прав код, чини заметку.
Архитектура самого Concat — в `docs/ARCHITECTURE.md` и `src/README.md` (это файлы апстрима).

## Сборка и гейт

Все команды `cargo` — из `src/`, не из корня репозитория.

```
cd src
cargo dev                                   # запуск окна, профиль quick
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 ../scripts/locales.py --check       # при правке строк интерфейса
python3 ../scripts/models.py --check        # при правке моделей
```

## Нельзя

- Писать в `concat.json` в обход `concat_project::Command`. Любая правка — команда, иначе ломается undo.
- Использовать имя и логотип «Concat» как имя продукта (`docs/upstream/TRADEMARK.md`).
- Удалять SPDX-шапки апстрима. Лицензия форка — AGPL-3.0-or-later.
- Пушить и трогать `origin`/`upstream` без явного разрешения владельца.

Остальные правила: `.memories/CONVENTIONS.md`.
