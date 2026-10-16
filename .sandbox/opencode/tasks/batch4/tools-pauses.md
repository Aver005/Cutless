# Задача: инструменты агента — паузы и чистка звука (`cutless-tools/src/pauses.rs`)

Сценарий владельца: «вырежи паузы, но оставь вдохи», «почисти звук». Ядро есть: анализ `cutless_pauses::analyze_file`
(`src/crates/cutless-pauses/src/lib.rs`), команды `MarkPauses`, `SetClipPause`, `RestorePauses`, `ConfirmPauses` и
`is_pause` (`src/crates/concat-project/src/commands/pauses.rs`), интерфейс окна — `src/crates/concat/src/panes/pauses.rs`
(посмотри, как он переводит время окна клипа в таймлайн и выбирает клипы: выделенные, иначе все со звуком, кроме
музыки = клип вида `Audio` без `detached_from`). Решения — `.memories/reference/PAUSES.md`.

Файлы: `src/crates/cutless-tools/src/pauses.rs` (+ `src/crates/cutless-tools/src/pauses/*.rs`). Больше ничего.

## Инструменты
- `pauses_analyze {clip_ids?, threshold_db?, min_pause?, padding?}` — найти паузы (по умолчанию все клипы со звуком,
  кроме музыки), вернуть отрезки в секундах таймлайна + шумовой пол, уровень речи, порог. **Не размечает.**
- `pauses_mark {clip_ids?, threshold_db?, min_pause?, padding?, ranges?: [{clip_id, start, end}]}` — разметить по анализу
  или по отрезкам, которые выбрала модель (так она оставляет «вдохи»). Один шаг undo.
- `pauses_list {}` — помеченные куски: id, время, длина, сумма.
- `pauses_restore {clip_ids}` — вернуть (склеить обратно).
- `pauses_confirm {clip_ids?}` — вырезать (по умолчанию все помеченные); ответ — на сколько укоротился ролик.
- `audio_clean {clip_ids?, preset: "phone_voice"|"noise_gate"|"band_cut"|"denoise"|"enhance_voice", params?}` —
  добавить звуковой пакет к цепочке звука клипа, **не стирая** существующие (`UpdateClip` заменяет `filters` целиком —
  прочитай текущую цепочку и допиши; повторно тот же пакет не добавляй — обнови параметры). Id пакетов:
  `cutless.phone-voice`, `cutless.noise-gate`, `cutless.band-cut`, `concat.denoise`, `concat.enhance-voice`.

## Проверка
Из `src/`: `cargo check -p cutless-tools --tests`.
