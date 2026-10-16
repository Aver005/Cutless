# Задача: инструменты агента — проект, монтаж, текст, эффекты, «зрение» (`cutless-tools/src/edit.rs`)

Владелец хочет, чтобы агент монтировал вместе с ним: «сделай субтитры как в …», «почисти звук», «найди момент».
Ты делаешь базовую группу. Файлы: `src/crates/cutless-tools/src/edit.rs` (+ подмодули `src/crates/cutless-tools/src/edit/*.rs`,
если файл растёт). Больше ничего не трогать.

## Инструменты (имена — точно такие)
- `project_open {path}` / `project_create {location, name, width?, height?, fps?}` — запоминают папку в `ctx.project`.
- `project_save {}`, `project_settings {width?, height?, fps?}` (через `Request::ProjectSetVideo`).
- `timeline_summary {}` — **главный инструмент «зрения»**: компактный текст, одна строка на клип. Шапка:
  `project "<имя>" rev <n> · 1080x1920 @30 · <длительность>`, затем по дорожкам `track <id>` и строки
  `  <clip id>  <start>-<end>  <kind> "<имя медиа или текст титра>"  src <source_start>  <только отличия от нормы:
  скорость, громкость, фейды, эффекты по id, пометка паузы cutless.pause>`. Время — `mm:ss.mmm`.
- `clip_detail {clip_id}` — все свойства клипа JSON-ом из документа.
- `media_import {file}`, `media_list {}`.
- `clips_split {clip_ids, time}`, `clip_trim {clip_id, edge: "start"|"end", delta, ripple?}`,
  `clips_move {moves:[{clip_id, start, track_id?}]}`, `clips_remove {clip_ids, ripple?}`, `clip_add {media_id, start, track_id?}`.
- `text_add {text, start, duration?, style?}` и `text_update {clip_id, text?, style?}` — стиль: шрифт, размер, цвет,
  обводка, тень, подложка, положение по вертикали, выравнивание — смотри `TextStyle` в
  `src/crates/concat-project/src/model.rs` и что принимают `AddTextClip` / `UpdateClip`.
  `text_style_track {clip_id}` — применить стиль этого титра ко всем титрам его дорожки (команда из апстрима #359 —
  найди её в `Command`).
- `effects_catalogue {kind?, query?}` (через `Request::CatalogueList`: коротко id, имя, параметры),
  `effect_apply {clip_ids, effect_id, params?}` — **добавляет** эффект к цепочке, не стирая существующие (`UpdateClip`
  заменяет цепочку целиком — прочитай текущую из документа и допиши), `effect_remove {clip_id, effect_id}`.
- `clip_volume {clip_ids, volume?, fade_in?, fade_out?}`, `clip_transform {clip_id, scale?, offset_x?, offset_y?, rotation?}`.
- `undo {}`, `redo {}`.
- `frame_at {time, width?}` — `Request::PreviewFrame` без `output` → картинка base64 в `ToolOutput.images`
  (mime `image/png`), в тексте — время и размер.
- `export_start {output}` (через `Request::ExportRun`; ответ — id задания).

## Проверка
Из `src/`: `cargo check -p cutless-tools --tests`.
