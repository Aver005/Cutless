# Задача: модели распознавания — каталог, свои файлы, автопоиск, язык

Движок уже заменён на `transcribe-cpp` (`src/crates/concat-speech/src/transcribe.rs`): он грузит GGUF-модели
Whisper / Parakeet / Nemotron / GigaAM и старые `.bin` whisper.cpp. Прочитай `transcribe.rs` целиком,
`.memories/reference/ASR.md`, окно субтитров `src/crates/concat/src/panes/captions.rs` + `ui/dialogs/captions.slint`,
страницу моделей в настройках (`src/crates/concat/src/panes/settings.rs`, `ui/dialogs/settings.slint` — ищи
`Transcriber::status`, `ModelData`).

## Цель
Пользователь, который ничего не понимает в моделях, должен получить хорошие субтитры на русском:
1. **Каталог для скачивания** дополняется GGUF-моделями handy-computer с Hugging Face:
   - Parakeet TDT 0.6B v3 (25 европейских языков, русский) — Q8_0;
   - Nemotron 3.5 ASR streaming 0.6B (русский есть) — Q8_0.
   Найди точные имена репозиториев и файлов через API Hugging Face (`curl https://huggingface.co/api/models?author=handy-computer`
   и `.../api/models/<repo>/tree/main`), возьми размер и SHA-256 (поле `lfs.oid` в дереве файлов). Ссылку на файл
   закрепи на конкретный commit (`/resolve/<sha>/<file>`), как сделано для whisper. Зеркала у этих моделей нет —
   скачивание идёт напрямую с Hugging Face (посмотри, как `crate::fetch_model` / `concat_host::models` берёт
   источники, и добавь путь «только upstream» без поломки старых моделей). Лицензии: Parakeet — CC-BY-4.0,
   Nemotron — OpenMDW-1.1 — допиши в описание модели короткую пометку.
2. **Автопоиск уже скачанных моделей**: кэш Hugging Face (`HF_HUB_CACHE`, иначе `HF_HOME/hub`, иначе
   `~/.cache/huggingface/hub`) — папки `models--*--*/snapshots/*/` с файлами `*.gguf` или `ggml-*.bin` известных
   семейств (по имени: whisper, parakeet, nemotron, gigaam, canary). Найденные модели показываются в списке как
   «уже на диске» и не скачиваются заново.
3. **Своя модель**: пользователь указывает файл `.gguf`/`.bin` через диалог выбора файла (в коде уже используется
   `rfd` — найди, как открываются диалоги файлов в `platform.rs`/`studio.rs`). Путь хранится в настройках
   (`src/crates/concat/src/prefs.rs`), а не в проекте.
4. **Выбор языка** в окне субтитров: «Авто» + Русский + English + список из `capabilities().languages` выбранной модели,
   если её удалось загрузить (иначе короткий список популярных). Значение уходит в `TranscribeRequest.language`.
   По умолчанию — язык интерфейса, если модель его знает, иначе «Авто».
5. Модель по умолчанию в окне субтитров: первая найденная на диске из Parakeet/Nemotron, иначе текущий Whisper.

## Разрешённые файлы
`src/crates/concat-speech/src/transcribe.rs` и новый `src/crates/concat-speech/src/catalog.rs` (каталог и автопоиск —
чистые функции с тестами), `src/crates/concat-speech/src/lib.rs`, `src/crates/concat/src/panes/captions.rs`,
`src/crates/concat/src/panes/settings.rs`, `src/crates/concat/src/prefs.rs`, `src/crates/concat/ui/dialogs/captions.slint`,
`src/crates/concat/ui/dialogs/settings.slint`, `src/crates/concat/ui/app.slint` (только пробросы свойств/колбэков),
`src/crates/concat/src/lib.rs` (только связывание колбэков), `src/crates/concat-host/src/models.rs` (только если нужен
путь скачивания без зеркала), `src/crates/concat/locales/*.json` (en, ru переведены; остальным — английский).
Новых зависимостей не добавлять.

## Проверка
Из `src/`: `cargo check -p concat-speech --tests`, `cargo check -p concat-host --tests`, `cargo check -p concat`.
Из корня: `python scripts/locales.py --check`. Если `scripts/models.py --check` ругается на модели вне манифеста —
опиши в отчёте, не ломай манифест.
