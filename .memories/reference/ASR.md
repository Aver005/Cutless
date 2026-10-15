# ASR — распознавание речи для субтитров
> Форматы моделей, движки, свои модели. Last updated: 2026-10-16 (движок `transcribe-cpp` одобрен и проверен на Windows).

## Пожелание владельца

Подключать разные модели — GGUF, ONNX, GGML и другие, — в том числе уже лежащие на диске, и не качать
заново. Лучшее качество у владельца — **Parakeet** и **Nemotron Streaming 3.5** (через Handy). Коллеги
дообучают Whisper.

## Что сейчас в Concat

- whisper.cpp через `whisper-rs` 0.16, модели `ggml-*.bin` из манифеста с SHA-256
  (`concat-speech/src/transcribe.rs`, `concat-host/src/models.rs`).
- **Язык не выбирается:** `"auto"` для многоязычных моделей, `"en"` для английских (`transcribe.rs` ~399).
- **Свой путь к модели не задать:** только id из `KNOWN_MODELS`.
- Тайм-коды только по сегментам, по словам — нет.
- `sherpa-onnx` 1.13.7 уже в сборке (для TTS) и делит с `ort` один `onnxruntime.dll` 1.28.

## Что на машине владельца (проверено)

- Handy 0.9.6 в `H:\Handy`, настройки — `%APPDATA%\com.pais.handy\settings_store.json`.
- Модель — **Nemotron 3.5 ASR streaming 0.6B, GGUF Q8_0, 751 МБ** в кэше Hugging Face:
  `~/.cache/huggingface/hub/models--handy-computer--nemotron-3.5-asr-streaming-0.6b-gguf/`.
- Handy 0.9 запускает **всё** через `transcribe-cpp` 0.4.0 (MIT): GGUF/ggml, 16 семейств моделей —
  Parakeet, Nemotron, Whisper, GigaAM, Canary, Qwen3-ASR и другие, тайм-коды по словам.

## Почему нельзя «любой формат»

Формат файла — это только упаковка весов. У каждого семейства моделей свой декодер: TDT у Parakeet,
RNNT у Nemotron, seq2seq у Whisper, CTC у GigaAM, плюс свой токенизатор. Поэтому реально поддержать
**движок**, который знает набор семейств, а не формат вообще.

| Путь | Что даёт | Минусы |
|---|---|---|
| **GGUF через `transcribe-cpp`** | Один движок — Whisper, Parakeet, Nemotron, GigaAM и др. Модели Handy с диска как есть. Квантование Q8 — меньше и быстрее на CPU, есть Vulkan | Новая зависимость. Свой ggml 0.26 конфликтует с ggml 0.9 из `whisper-rs` — правильнее **заменить `whisper-rs` целиком**. Молодой крейт `[?]`. Сборка на Windows не проверена `[?]` |
| **ONNX через `sherpa-onnx`** (уже в сборке) | Parakeet v3, Nemotron 3.5 (потоковый), GigaAM, Whisper. Новых зависимостей нет | Нужен экспорт именно под sherpa (encoder/decoder/joiner/tokens); файлы Handy не подходят. Модели 475–490 МБ |
| **GGML через `whisper-rs`** (как сейчас) | Работает | Только Whisper |
| Свой декодер поверх `ort` | Гибкость | 200–400 строк на каждое семейство — повтор того, что уже есть в sherpa и transcribe.cpp |

## Модели для русского

- **Parakeet TDT 0.6B v3** — 25 языков, русский есть, пунктуация, тайм-коды слов. CC-BY-4.0 (атрибуция).
- **Nemotron 3.5 ASR streaming 0.6B** — 28–40 языков, русский есть, потоковая, RNNT. OpenMDW-1.1.
- **GigaAM v3** (ai-sage, MIT) — заточена под русский.
- **Whisper** — то, что дообучают коллеги. Конвертируется в ggml/GGUF скриптами whisper.cpp.

## Проверка `transcribe-cpp` 0.4.0 на машине владельца (2026-10-16)

- **Сборка:** отдельный пробный крейт, `default-features = false` (без `metal`), Windows + MSVC + cmake из
  Visual Studio — **161 с, без ошибок**, exe 3,6 МБ. Сборка целиком из исходников, без скачиваний.
- **Модель:** Nemotron 3.5 Q8_0 GGUF прямо из кэша Hugging Face (Handy), загрузка 0,8–1 с, `arch=parakeet`.
- **Русская речь** (16,7 с, синтез голосом Windows Irina): **1,6 с на CPU** (~10× быстрее реального времени),
  пунктуация и заглавные есть, тайм-коды по словам (`TimestampKind::Token`), `language=None` — автоопределение.
  Ошибки: «Каттлесс» → «котлес» (лечится `RunOptions::vocabulary`), «вырежем» → «вырежим».
- **Коды языков у модели — BCP-47** (`ru-RU`, `en-US`, … 32 штуки), не `ru`. `language: Some("ru")` →
  ошибка `unsupported language`. Брать коды из `model.capabilities().languages`.
- **Старые модели whisper.cpp `.bin` движок тоже читает** (`transcribe-bin-loader.cpp`) — уже скачанные
  пользователями модели Whisper не придётся качать заново.
- В крейте есть ещё **VAD (Silero)**, определение языка и разделение по говорящим — VAD пригодится
  вырезателю пауз (`reference/PAUSES.md`).
- `Capabilities.max_audio_ms = 0` у Nemotron — без практического предела длины; у других семейств
  предел есть — длинное аудио резать по VAD.

## Рекомендация (одобрена владельцем 2026-10-16: `transcribe-cpp`)

1. Трейт движка в `concat-speech`: `AsrEngine { load(ModelSource), run(pcm, options{language, words}) }`.
2. Источник модели `ModelSource`:
   - `Download(id)` — как сейчас;
   - `Folder(path)` — выбранная пользователем папка или файл, тип определяется по составу;
   - `Detected` — автопоиск в кэше Hugging Face и папках известных приложений.

   Путь хранится в настройках, не в проекте.
3. **Основной движок — `transcribe-cpp` (GGUF) вместо `whisper-rs`**: закрывает Whisper, Parakeet,
   Nemotron и модели Handy одним ходом. `sherpa-onnx` для ONNX — вторым движком позже.
4. Выбор языка в окне субтитров (русский по умолчанию для владельца) и тайм-коды по словам — для
   караоке и монтажа по словам.
5. Длинное аудио резать по VAD (Silero есть в `sherpa_onnx::vad`).

Риски:
- лицензии моделей — атрибуция в `docs/THIRD_PARTY_NOTICES.md`;
- размер моделей;
- проверка `models.py --check` при добавлении в манифест;
- расхождение с апстримом из-за замены `whisper-rs`.
