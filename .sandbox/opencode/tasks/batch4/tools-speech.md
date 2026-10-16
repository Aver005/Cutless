# Задача: инструменты агента — речь, транскрипт, поиск, субтитры (`cutless-tools/src/speech.rs`)

Сценарии владельца: «сделай субтитры в стиле X» и «найди момент, где я говорю про …» (смысл понимает сама модель,
читая транскрипт; твоя задача — дать ей транскрипт с таймкодами и быстрый поиск по фразе).

Файлы: `src/crates/cutless-tools/src/speech.rs` (+ `src/crates/cutless-tools/src/speech/*.rs`).

Нарезка титров по словам уже написана в окне: `src/crates/concat/src/panes/caption_lines.rs` (`caption_lines`, `LineRules`).
Окно — верхний крейт, `cutless-tools` от него зависеть не может. **Перенеси** модуль в
`src/crates/concat-speech/src/caption_lines.rs` (публичный, `pub mod` в `concat-speech/src/lib.rs`), а в окне поменяй только
пути импорта (`src/crates/concat/src/panes/captions.rs`, `src/crates/concat/src/panes/mod.rs`), старый файл удали. Логику и
тесты не менять — они переезжают целиком.

## Инструменты
- `speech_models {}` — какие модели есть (скачаны / найдены на диске / в каталоге): см. `concat_speech::catalog` и
  `Transcriber::status`.
- `transcribe {clip_id, model_id?, language?}` — распознать речь клипа: путь медиа, окно клипа (`source_start`,
  длительность с учётом скорости) — как это делает `src/crates/concat/src/panes/captions.rs`. Синхронно — для первой
  версии нормально. Кэшируй результат в памяти инструмента по (медиа, окно, модель, язык), чтобы `transcript_get` и
  `captions_make` не распознавали заново. Время результата — в **секундах таймлайна**.
- `transcript_get {clip_id, from?, to?, words?}` — сегменты (и слова при `words: true`) с таймкодами, компактно.
- `transcript_search {query, clip_id?}` — найти фразу: без учёта регистра, ё=е, без пунктуации; плюс нечёткое
  совпадение (доля совпавших слов запроса ≥ 0.6). Ответ — совпадения с началом/концом и контекстом.
- `captions_make {clip_id, style?, rules?}` — титры из слов (`caption_lines`) одним `Command::Batch` из `AddTextClip`
  над видео (`above: true`). Стиль — из входа (шрифт, размер, цвет, обводка, подложка, положение) или по умолчанию для
  вертикального видео: крупный, жирный, по центру нижней трети, белый с тёмной обводкой. Ответ — сколько титров и их id.

## Проверка
Из `src/`: `cargo check -p cutless-tools --tests`, `cargo check -p concat-speech --tests`, `cargo check -p concat`.
