# ARCHITECTURE
> Как Cutless ложится на Concat. Last updated: 2026-10-16 (кода Cutless ещё нет; описана целевая схема).

Архитектура самого Concat — в `docs/ARCHITECTURE.md` и `src/README.md`. Это документы апстрима,
они подробные и актуальные. Здесь — только то, что нужно знать, чтобы строить Cutless поверх них.

## Concat в пяти фактах

1. **Движок отделён от окна.** Cargo-workspace в `src/`, зависимости идут в одну сторону:
   `concat` (окно) → `concat-host` → `{export, media, render, project, effects}` → `concat-core`.
   API (`concat-api`, `concat-server`, `concat-cli`) живёт рядом с окном, а не под ним.
2. **Любая правка документа — `concat_project::Command`.** Сериализуемая JSON-команда с undo. Окно
   своей модели документа не держит: жесты работают по «эху» и фиксируются одной командой.
3. **Окно устроено как Elm.** Колбэк Slint превращается в `Msg`, дальше `Studio::handle`, `pane.update`
   и `publish()`, который пересобирает модели Slint. Панели (`panes/*.rs`) — state + `Msg` + `update` + `data`.
4. **Потоки, не async.** Тяжёлое — `host::spawn`/`spawn_in_project`, результат — через
   `slint::invoke_from_event_loop`, устаревшее отбрасывается по epoch проекта. tokio есть только в
   gRPC за фичей.
5. **Один GPU-композитор на превью и экспорт** (`concat-render`), общий wgpu-девайс с окном. Часы
   воспроизведения — счётчик сэмплов звуковой карты.

## Слой Cutless

```
окно Concat (Slint) ── точечные вставки ──► панели и окна Cutless (agent, recipes)
        │                                            │
        ▼                                            ▼
concat-api ◄── cutless-tools (реестр инструментов) ◄── cutless-agent (LLM-цикл)
        │              ▲
        │              └── cutless-mcp (MCP-транспорт) ◄── внешние агенты
        ▼
concat-project::Command + undo  ·  concat-host  ·  движок
```

- **Свои крейты** `cutless-*` в `src/crates/` (workspace подхватывает `crates/*`). Движок апстрима от
  них не зависит.
- **В файлах апстрима — только точечные вставки:**
  - регистрация панели и окна;
  - вариант `Msg`;
  - вызов моста в `Studio`;
  - подкоманда в `concat-cli`.

  Всё остальное — в своих модулях.
- **Данные Cutless в проекте** — в `Project.extra` под префиксом `cutless.` (сетка долей, рецепт,
  история агента). Формат апстрима (`DOCUMENT_VERSION = 1`) не трогаем.
- **Ход агента = один шаг undo** через жест или `Batch`.
- **Агент видит проект текстом** (`timeline_summary`), картинки — дополнительно.

Подробный проект: инструменты, MCP, мост, агент, ACP — `reference/AI-LAYER.md`.

## Потоки данных

**Человек правит.** Slint-колбэк → `Msg` → `Studio::apply(Command)` → `Session::apply` → `after_change`
(автосохранение, превью, звук) → `publish()`.

**Внешний агент без окна (Ф2).** MCP `tools/call` → `cutless-tools` → `Api::dispatch(edit.apply)` →
своя `Session` API → компактный ответ + ревизия.

**Агент правит проект на экране (Ф3).** MCP или встроенный агент → `cutless-tools` → мост →
`invoke_from_event_loop` → `Msg::Agent` → `Studio::apply` → тот же путь, что у человека.

**Встроенный агент (Ф4).** Панель агента → `cutless-agent` в потоке `host::spawn` → провайдер LLM (SSE)
→ вызовы инструментов → политика подтверждений (карточка в панели) → `cutless-tools` → мост → окно.

## Почему так

Решения с причинами — `DECISIONS.md`. Главные:
- Slint, а не веб: превью — GPU-текстура без копирования;
- мягкий форк: апстрим очень быстрый;
- один реестр инструментов для MCP и встроенного агента;
- без tokio;
- сначала работа без окна, потом мост.
