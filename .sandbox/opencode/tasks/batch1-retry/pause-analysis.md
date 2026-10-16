# Задача: анализ пауз в звуке (новый крейт `cutless-pauses`)

Вырезатель пауз в Cutless сначала **размечает** паузы, потом человек проверяет и подтверждает. Ты пишешь чистый
анализ: из звука клипа — список отрезков, которые можно вырезать. Без интерфейса и без правок документа.

## Разрешённые файлы
Только новая папка `src/crates/cutless-pauses/` (`Cargo.toml`, `src/lib.rs`, при желании ещё модули внутри `src/`).
Workspace подхватывает `crates/*` сам. `src/Cargo.lock` обновится сам.

`Cargo.toml` — по образцу `src/crates/concat-media/Cargo.toml` (`version.workspace = true`, `edition.workspace = true`,
`rust-version.workspace = true`, `license.workspace = true`, `description`, секция `[lints] workspace = true`).
Зависимости: `concat-media.workspace = true` (если такой записи в `[workspace.dependencies]` нет — используй
`concat-media = { path = "../concat-media" }`), `thiserror.workspace = true`, `log = "0.4"`. Других не добавлять.

## API
```rust
pub struct PauseSettings {
    pub threshold_db: Option<f32>, // None — порог вычисляется из звука
    pub min_pause: f64,            // минимальная длина ВЫРЕЗАЕМОГО отрезка, с; по умолчанию 0.3
    pub padding: f64,              // сколько тишины оставить с каждого края, с; по умолчанию 1/3
    pub window_ms: u32,            // окно измерения громкости; по умолчанию 20
}
impl Default for PauseSettings { .. }

pub struct Range { pub start: f64, pub end: f64 }          // секунды от начала анализируемого окна

pub struct Analysis {
    pub noise_floor_db: f32,
    pub speech_db: f32,
    pub threshold_db: f32,      // применённый порог
    pub pauses: Vec<Range>,     // отрезки к вырезанию (уже с учётом отступов), по возрастанию
}

pub fn analyze_pcm(samples: &[f32], rate: u32, settings: &PauseSettings) -> Analysis;   // чистая функция
pub fn analyze_file(request: &FileRequest, settings: &PauseSettings) -> Result<Analysis, PauseError>;
pub struct FileRequest { pub path: String, pub audio_stream: Option<usize>, pub source_start: f64, pub window: f64,
                         pub filters: Vec<String> }  // filters — цепочка FFmpeg клипа, чтобы анализ слышал то же, что человек
```
`analyze_file` читает звук через `concat_media::AudioDecoder::open(path, &AudioOptions { start, duration, filters,
rate: 16_000, channels: 1, format: SampleFormat::F32, stream })` и `collect_f32()` — посмотри, как это сделано в
`src/crates/concat-speech/src/transcribe.rs`. Ошибки — свой `PauseError` на `thiserror`.

## Алгоритм `analyze_pcm`
1. RMS по окнам `window_ms`, в дБ (защита от log(0): нижняя граница −100 дБ).
2. Автопорог (если `threshold_db` = None): шумовой пол = 10-й перцентиль громкостей окон, речь = 90-й перцентиль;
   порог = пол + (речь − пол) × 0.3. Если разница меньше 6 дБ (сплошная тишина или сплошной шум) — пауз нет.
3. Тихие окна (ниже порога) склеиваются в отрезки. Короткие всплески громкости внутри тишины (≤ 60 мс)
   не разрывают паузу. Короткие «тихие» провалы внутри речи (< 100 мс) паузой не считаются.
4. Из каждого тихого отрезка [a, b] вырезаемая часть = [a + padding, b − padding]. Отрезок у самого начала окна
   (a = 0) отступ слева не требует; у самого конца (b = длина) — справа не требует.
5. Оставить только вырезаемые части длиной ≥ `min_pause`. Результат — по возрастанию, без пересечений.

## Тесты (обязательно, на синтетическом сигнале)
Синус 220 Гц как «речь», тишина / слабый шум как «пауза». Проверить: пауза 1,5 с в середине → один отрезок с
отступами 1/3 с; пауза короче `min_pause + 2·padding` → ничего; тишина в самом начале/конце; щелчок 30 мс внутри
паузы её не рвёт; ручной порог; сплошная тишина → пауз нет; автопорог между полом и речью.

## Проверка
Из `src/`: `cargo check -p cutless-pauses --tests`.
