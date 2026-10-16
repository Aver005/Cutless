# AI-LAYER — проект ИИ-слоя Cutless
> Целевая схема, инструменты, мост к окну, встроенный агент, ACP. Last updated: 2026-10-16 (Ф2: реестр и MCP по stdio работают).

Это проект, а не описание существующего кода. Решения, на которых он стоит, — `DECISIONS.md`
(раздел «ИИ-слой»). Фазы — `PLANS.md` Ф2–Ф4, Ф7.

## 1. Что уже есть в Concat и на что опираемся

| Слой | Где | Что даёт |
|---|---|---|
| Команды | → `src/crates/concat-project/src/commands/mod.rs` | `enum Command` с `#[serde(tag = "op")]`: каждая правка — JSON. `Batch` — атомарная группа, один шаг undo. Результат — `Outcome { created_id, applied }`, ошибка — `CommandError` с текстом для человека |
| Undo | → `src/crates/concat-project/src/editor.rs` | 200 шагов, снимки через `Arc`. `apply_within` + `end_gesture` склеивают серию в один шаг |
| API | → `src/crates/concat-api/src/message.rs` | `Request`: `version`, `project.{create,open,close,list,get,document,save,setVideo}`, `edit.{apply,undo,redo}`, `media.{probe,import}`, `catalogue.list`, `template.{list,instantiate,save}`, `export.{run,cancel}`, `preview.frame`. События: `export.{progress,done,failed}`, `cutout.progress`. Коды ошибок: `Parse`, `Invalid`, `NotOpen`, `NotFound`, `Refused`, `Busy`, `Cancelled`, `Unauthorized`, `Failed` |
| Диспетчер | → `src/crates/concat-api/src/lib.rs` | `Api::dispatch(Request) -> Response + Events`. Не зависит от транспорта. В doc-комментарии MCP прямо назван будущим транспортом |
| Сервер | `src/crates/concat-server/src/` (`lib.rs`, `json.rs`, `grpc.rs`, `hub.rs`, `token.rs`) | JSON-RPC 2.0 по строкам через TCP или Unix-сокет. `Hub` — один поток владеет `Api`, вызовы идут по очереди. Токен, `roots` для записи |
| CLI | → `src/crates/concat-cli/src/main.rs` | `api` (stdin/stdout), `serve` |
| Окно | → `src/crates/concat/src/studio.rs` | `Studio::apply` применяет команду к проекту на экране, затем `after_change` (автосохранение, превью, звук). Сервер в окне (`panes/settings.rs`, страница Remote) **не правит** проект на экране: так решено через `OpenProjects`/`Holder` в `concat-api/src/lib.rs` |

**Чего нет:** MCP, JSON-схем команд, компактных ответов (`edit.apply` возвращает весь `Project`
через `Reply::View(EditorView)`), ревизий, глаголов для субтитров, TTS, вырезания и апскейла, моста к
окну, LLM-клиента.

## 2. Целевая схема

```
                       ┌──────────── внешние агенты ─────────────┐
                       │ Claude Code · pooprusteek · Gemini CLI · Cursor │
                       └───────┬───────────────────────┬─────────┘
                               │ MCP (stdio, HTTP)      │ ACP (Ф7)
┌──────────── Cutless ─────────┼───────────────────────┼──────────────┐
│ окно (Slint)                 │                       │              │
│  ├ панель агента / окно агента ◄── cutless-agent (цикл, провайдеры)  │
│  └ Studio::apply ◄─── мост (Ф3) ◄──┐                                │
│                                    │                                │
│ cutless-mcp (транспорт) ──► cutless-tools (реестр инструментов) ◄─ cutless-agent
│                                    │                                │
│                        concat-api (dispatch) + новые глаголы         │
│                        concat-project::Command + undo                │
└─────────────────────────────────────────────────────────────────────┘
```

### Крейты Cutless

| Крейт | Зависит от | Назначение |
|---|---|---|
| `cutless-tools` | `concat-api`, `concat-project` | Реестр инструментов: имя, описание, схема входа, обработчик. Компактные представления, ревизии, проверки |
| `cutless-mcp` | `cutless-tools`, `concat-server` | MCP поверх реестра: stdio, затем HTTP. Подкоманда `concat-cli mcp` |
| `cutless-agent` | `cutless-tools`, `ureq` | Цикл агента, провайдеры LLM, разбор вызовов, политика подтверждений. Без окна, тестируется отдельно |
| `cutless-beats` (Ф5) | `concat-media` | Анализ долей, ударов, пауз |
| `cutless-recipes` (Ф6) | `cutless-tools`, `cutless-agent` | Формат и исполнитель рецептов |

Окно зависит от `cutless-agent` и `cutless-tools`. Движок от крейтов Cutless не зависит:
стрелки идут только сверху вниз, как у апстрима.

## 3. Реестр инструментов (`cutless-tools`)

Один инструмент — это:
- имя;
- описание для модели (английское: модели понимают его лучше);
- JSON-схема входа;
- признак «только чтение / правка / опасно»;
- обработчик: получает вход и сессию, возвращает компактный текст или структуру.

### Правила для инструментов

- **Мелкие, понятные модели операции.** Не одна сумка `edit.apply(Command)`, а `split_clip`,
  `trim_clip`… Внутри — те же `Command`.
- **Каждая правка возвращает короткий ответ:** что изменилось, новые id, новая ревизия. Целиком
  документ не возвращается никогда.
- **Время — в секундах с тремя знаками** во входе и выходе. Внутри Concat переводит в `Rational`.
- **Адресация:** клипы, дорожки и эффекты — по id из `timeline_summary`. Эффект адресуется по
  стабильному id, а не по индексу (Ф2.4).
- **Ошибка — человекочитаемый текст** из `CommandError` плюс подсказка, что сделать (перечитать
  сводку, взять другую дорожку).
- **Вход проверяется по схеме до вызова.**

### Что уже есть (2026-10-16)

36 инструментов в `src/crates/cutless-tools` (`default_registry()`), имена и поля входа — snake_case:

| Группа | Инструменты |
|---|---|
| Проект | `project_open`, `project_create`, `project_save`, `project_settings`, `timeline_summary`, `clip_detail`, `undo`, `redo` |
| Медиа | `media_import`, `media_list` |
| Монтаж | `clips_split`, `clip_trim`, `clips_move`, `clips_remove`, `clip_add`, `clip_volume`, `clip_transform` |
| Текст | `text_add`, `text_update`, `text_style_track` |
| Эффекты | `effects_catalogue`, `effect_apply`, `effect_remove` |
| Глаза и выход | `frame_at`, `export_start` |
| Речь | `speech_models`, `transcribe`, `transcript_get`, `transcript_search`, `captions_make` |
| Паузы и звук | `pauses_analyze`, `pauses_mark`, `pauses_list`, `pauses_restore`, `pauses_confirm`, `audio_clean` |

Сквозная проверка — `scripts/cutless/mcp-smoke.py --speech речь.wav` (нужен `concat-cli` профиля quick).

### Набор первой волны (Ф2.5–Ф2.8), план

| Группа | Инструменты |
|---|---|
| Проект | `project_open`, `project_create`, `project_save`, `project_settings` (размер, fps), `timeline_summary`, `clip_detail`, `project_diff` |
| Медиа | `media_import`, `media_list`, `media_probe` |
| Монтаж | `add_clip`, `move_clips`, `trim_clip`, `split_clips`, `remove_clips` (ripple), `merge_clips`, `freeze_frame`, `detach_audio`, `set_speed` |
| Вид | `set_transform`, `set_crop`, `set_opacity`, `set_blend`, `set_key`, `clear_keys` |
| Текст | `add_text`, `update_text`, `add_captions` (из транскрипта) |
| Эффекты | `catalogue_search`, `apply_effect`, `update_effect`, `remove_effect`, `set_transition` |
| Звук | `set_volume`, `set_fades`, `apply_audio_effect` |
| Дорожки | `add_track`, `set_track_flags` |
| История | `undo`, `redo` |
| Речь (Ф2.6) | `transcribe`, `transcript_get` (сегменты с временем, поиск по фразе), `synthesize_speech` |
| Изображение (Ф2.6) | `cutout`, `enhance`, `reverse` |
| Глаза (Ф2.7) | `frame_at`, `contact_sheet` |
| Проверки (Ф2.8) | `lint` |
| Выход | `export_start`, `export_status`, `export_cancel` |

Долгие задачи (транскрипция, экспорт, вырезание) — пара «запустить + статус». Ход агента не висит
на минутной задаче.

### `timeline_summary` — главный инструмент «зрения»

Текст, который помещается в контекст и читается моделью без картинок:

```
project "promo" rev 42 · 1080x1920 @30 · 00:19.800
track t1 [video]
  c1  00:00.000–00:03.270  media m1 "queue.mp4" src 00:12.400  scale 1.20 key(scale×2)
  c2  00:03.270–00:06.430  media m2 "fuel.mp4"  src 00:00.000  fx: grade.teal(#e1)
track t2 [text]
  c7  00:00.200–00:03.000  text "ОПЯТЬ НА ЗАПРАВКУ?"  y -0.30
track t3 [audio]
  c9  00:00.000–00:19.800  media m5 "ko.wav" vol 0.85 fade-out 0.3
beats: 120.04 bpm, phase 0.27 (cutless.beats)
lint: 1 warning — c7 text 24 chars/2.8s = 8.6 cps ok; c2 outside safe zone? no
```

Формат уточняется в Ф2.2. Принцип — одна строка на клип, только то, что нужно для решения.

## 4. MCP (`cutless-mcp`)

- Транспорт: сначала stdio (`concat-cli mcp`), затем HTTP на том же `Hub`.
- Подключение: Claude Code — `claude mcp add cutless -- <путь>\concat-cli.exe mcp`; pooprusteek — `/mcp add`
  с той же командой. Ошибка инструмента — результат с `isError: true`, не ошибка JSON-RPC.
- `tools/list` отдаёт реестр; `tools/call` вызывает обработчик.
- Картинки (`frame_at`, `contact_sheet`) уходят как MCP image content и **дублируются текстом**
  (время, размер, путь к файлу). MCP-клиент pooprusteek сводит картинку к `[Image: mime]`.
- Проверять с двумя клиентами: Claude Code (есть зрение) и pooprusteek на DeepSeek Web (зрения нет).
- Проблемы безопасности сервера апстрима (утечка fd, подключения до авторизации, §15 в их
  `docs/ARCHITECTURE.md`) для stdio не критичны, для HTTP — закрыть до релиза.

## 5. Мост к окну (Ф3)

Задача: внешняя или встроенная правка применяется к проекту **на экране**.

1. Новый способ держать сессию: API не создаёт свою `Session`, а передаёт команды окну. Абстракция
   «где живёт проект» в `concat-api` — своя сессия или делегированная.
2. Окно получает `Msg::Agent { command, reply }` через `slint::invoke_from_event_loop`. Внутри — тот
   же `Studio::apply`, но в варианте, который возвращает `Result` (сейчас ошибка уходит тостом).
3. Пока пользователь тянет мышью (`echo`, `flush_commit` в `studio.rs`), правки агента ждут в очереди.
4. Ход агента открывает жест, закрывает его `end_gesture` → один шаг undo с подписью.
5. Ревизия: если документ изменился после того, как агент его прочитал, правка отклоняется, и агент
   перечитывает сводку.

Риски: `studio.rs` — около 11 тыс. строк, часто меняется апстримом. Вставки держать минимальными,
логику — в своём модуле `panes/agent.rs`.

## 6. Встроенный агент (`cutless-agent`, Ф4)

### Цикл

```
запрос пользователя + контекст (сводка, выделение, курсор, транскрипт рядом, lint)
  → провайдер (стрим) → текст и/или вызовы инструментов
  → политика: нужно ли подтверждение → карточка в панели → да/нет
  → выполнение через реестр → короткий результат в историю
  → следующий шаг, пока модель не закончит или не упрётся в лимит
```

- Лимиты: шагов на ход, вызовов на шаг, токенов контекста. Отмена в любой момент.
- Поток: `host::spawn`, без tokio. Результаты в окно — через `invoke_from_event_loop`.
- История хода хранится рядом с проектом `[?]`: решить, сохранять ли переписку в `extra` или в
  отдельном файле папки проекта.

### Провайдеры

- Трейт `Provider`: `stream(request) -> события (текст, вызов инструмента, конец, ошибка)`,
  `supports_native_tools`, `supports_images`.
- Первый — **OpenAI-совместимый** Chat Completions со стримом SSE. Он покрывает OpenRouter,
  DeepSeek API, Ollama, LM Studio и будущий сервер Cutless Pro.
- Затем Anthropic Messages и Gemini. Образцы — `pooprusteek/src/provider/{openai_compat,anthropic_compat,gemini_compat}.rs`.
- Модели без нативных инструментов получают текстовый протокол вызовов. Разбор — порт
  `pooprusteek/src/agent/tool_parser/` (понимает DSML DeepSeek, Hermes, Claude-подобный и другие
  форматы, у вызовов есть уровни доверия).

### Политика подтверждений

| Класс | Примеры | По умолчанию |
|---|---|---|
| Чтение | `timeline_summary`, `transcript_get`, `frame_at`, `lint` | Без вопросов |
| Правка | `split_clips`, `add_text`, `apply_effect` | Пользователь выбирает: каждую, весь ход, всегда |
| Опасно | `remove_clips` (много), экспорт поверх файла, `media_import` вне `roots` | Всегда с вопросом |

### Интерфейс

- Панель агента в доке (`panes/agent.rs` по схеме state + `Msg` + `update` + `data`,
  `ui/workspace/agent-pane.slint`). Та же панель — **отдельным окном**: Slint показывает несколько
  окон верхнего уровня через `show()`.
- Элементы:
  - лента сообщений;
  - план хода;
  - карточки вызовов (что, где, diff ревизий);
  - кнопки «применить / отклонить / стоп»;
  - выбор модели;
  - индикатор работы.
- Клипы, изменённые последним ходом, подсвечиваются на таймлайне.
- Все строки через `I18n.t`, все контролы с `accessible-label`.

## 7. ACP (Ф7)

- Cutless как ACP-клиент: внешний агент запускается подпроцессом, панель агента показывает его поток.
  Инструменты Cutless агент получает через MCP, переданный при создании сессии `[?]` (сверить со
  спецификацией ACP).
- pooprusteek: нынешний `--acp` — заглушка-ретранслятор (`pooprusteek/src/acp/server.rs`),
  несовместимая с ACP. Нужен настоящий сервер. Образцы — `pooprusteek/src/harness/driver.rs` и
  ACP-сервер poopseek (`poopseek/src/acp/server/`).

## 8. Риски

| Риск | Что делать |
|---|---|
| Токены: проект большой, модель тонет в деталях | Компактные сводки, `clip_detail` по запросу, diff вместо документа |
| Агент с устаревшим состоянием затирает правки | Ревизии (Ф2.3), стабильные id эффектов (Ф2.4) |
| Модель без зрения не видит результат | Текстовые проверки и сводки, транскрипт, сетка долей |
| Апстрим меняет `studio.rs` и API | Свои крейты, точечные вставки, частая синхронизация, PR общего в апстрим |
| Облачный LLM против «ничего не уходит с диска» | Агент выключен по умолчанию, локальные модели как полноправный вариант, явное согласие |
