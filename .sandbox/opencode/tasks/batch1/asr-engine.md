# Задача: движок распознавания речи transcribe-cpp вместо whisper-rs

Субтитры в редакторе делает `src/crates/concat-speech/src/transcribe.rs` через `whisper-rs` (whisper.cpp).
Заменяем движок на **`transcribe-cpp` 0.4.0** (MIT): он понимает GGUF-модели Whisper, Parakeet, Nemotron, GigaAM и
старые файлы whisper.cpp `.bin`. Проверено интегратором на этой машине: крейт собирается, модель Nemotron 3.5 GGUF
распознаёт русский с пунктуацией и таймкодами слов.

Исходники крейта для чтения API: `~/.cargo/registry/src/index.crates.io-*/transcribe-cpp-0.4.0/` (README.md,
src/lib.rs, src/model.rs, src/session.rs, src/result.rs, src/cancel.rs, src/types.rs). Ключевое:
`Model::load(path)`, `model.capabilities()?` (поле `languages: Vec<String>` с кодами вида `"ru-RU"`, `"en-US"`;
`max_audio_ms` — 0 значит без предела), `model.session()?`, `session.set_cancel_token(&CancelToken)`,
`session.run(&pcm_16k_mono_f32, &RunOptions { language: Option<String>, timestamps: TimestampKind, .. })` →
`Transcript { text, language, segments: Vec<Segment{t0_ms,t1_ms,text,..}>, words: Vec<Word{t0_ms,t1_ms,text,..}>, .. }`.
Код языка `"ru"` модель Nemotron не принимает — нужно передавать код из её списка (`"ru-RU"`).

## Разрешённые файлы
- `src/crates/concat-speech/Cargo.toml` — убрать `whisper-rs` (обе строки, включая macOS с `metal`), добавить
  `transcribe-cpp = { version = "0.4", default-features = false }` для всех целей и
  `transcribe-cpp = { version = "0.4", features = ["metal"] }` для macOS (там, где раньше был `whisper-rs` с metal).
  Поправь комментарий про движок над зависимостью. Других зависимостей не добавлять.
- `src/crates/concat-speech/src/transcribe.rs` — переписать на новый движок.
- `src/crates/concat-speech/src/lib.rs` — только если нужно поправить doc-комментарий/реэкспорт.
- `src/crates/concat/src/panes/captions.rs` — ТОЛЬКО добавить новое поле в конструктор `TranscribeRequest`
  (`language: None`). Больше в этом файле ничего не менять.
- `src/Cargo.lock` — обновится сам при `cargo check`.

## Требования
1. Публичный API сохранить: `Transcriber` (`new`, `status`, `download_model`, `cancel_download`, `delete_model`,
   `loaded_model`, `transcribe`, `cancel`, `is_busy`), `ModelStatus`, `TranscriberStatus`, `TranscribeRequest`, `Segment`.
   Таблица `KNOWN_MODELS` и скачивание `.bin` с SHA-256 остаются как есть (старые модели движок читает).
2. `TranscribeRequest` получает поле `language: Option<String>` с `#[serde(default)]`: `None` — автоопределение.
   Короткий код (`"ru"`) сопоставляй с кодами модели: точное совпадение, иначе первый код, начинающийся с `"ru-"`.
   Если модель язык не знает — понятная ошибка. Для англоязычных моделей Whisper (`*.en`) язык всегда английский
   (как раньше: найти `"en"`/`"en-…"` в списке модели или не передавать язык, если список пуст).
3. `Segment` получает поле `words: Vec<Word>` (`#[serde(default)]`), новый тип `pub struct Word { start: f64, end: f64,
   text: String }` в секундах от начала окна. Слова раскладывай по сегментам по их `seg_index`.
   Фильтр `keep_segment` (ремарки в скобках, пустые и вырожденные отрезки) сохранить.
4. Отмена: флаг `cancel` из `SingleFlight` — `Arc<AtomicBool>`. Создай `CancelToken`, на время `run` запусти лёгкий
   поток, который раз в ~50 мс проверяет флаг и отменяет токен; поток должен завершаться после `run`.
   После отмены — ошибка `"transcription cancelled"`, как сейчас.
5. Длинное аудио: если `capabilities().max_audio_ms > 0` и окно длиннее — режь PCM на куски не длиннее предела
   (по возможности по ~0,5 с с запасом на стык), сдвигай таймкоды кусков, прогресс `JobStage::Working { done, next }`
   по кускам. Если предела нет — один `run`, прогресс `done: 0 → 1`.
6. Загруженная модель кэшируется между запусками, как сейчас (`LoadedModel` держит `Model` и id).
7. Логи движка: если в крейте есть `init_logging`/`disable_logging` — направь в `log` или отключи шум.
8. Тест `real_audio` (ignored) оставь рабочим: пусть берёт путь модели из `CONCAT_TRANSCRIBE_MODEL`, если задан.
9. Юнит-тесты: сопоставление кодов языка, раскладка слов по сегментам, нарезка длинного PCM со сдвигом таймкодов
   (вынеси это в чистые функции, чтобы тестировать без модели).

## Проверка
Из `src/`: `cargo check -p concat-speech --tests`, затем `cargo check -p concat`. Первый check соберёт C++ часть
transcribe.cpp через cmake — это несколько минут, это нормально.
