// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Анализ пауз в звуке: находит тихие отрезки, которые можно вырезать.
//!
//! Разметка чистая: `analyze_pcm` считает по готовым сэмплам, а `analyze_file`
//! читает окно клипа через `concat_media` и зовёт первое. Интерфейс и правки
//! документа — не здесь.

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};

/// Частота, на которой анализируем звук: для речи и пауз этого хватает.
const ANALYSIS_RATE: u32 = 16_000;
/// Тихее этого порога считаем абсолютной тишиной, чтобы не делить на log(0).
const DB_FLOOR: f32 = -100.0;
/// Доля размаха «пол → речь», на которой ставим автоматический порог.
const AUTO_FRACTION: f32 = 0.3;
/// Размах меньше этого — сплошной шум или сплошная тишина, пауз нет.
const MIN_DYNAMIC_RANGE_DB: f32 = 6.0;
/// Всплеск громкости короче этого не разрывает паузу.
const BURST_MS: f64 = 60.0;
/// Тихий провал короче этого паузой не считается.
const DIP_MS: f64 = 100.0;

/// Настройки разметки пауз.
#[derive(Clone, Debug, PartialEq)]
pub struct PauseSettings {
    /// Порог тишины, дБ; `None` — вычислить из самого звука.
    pub threshold_db: Option<f32>,
    /// Минимальная длина вырезаемого отрезка, с.
    pub min_pause: f64,
    /// Сколько тишины оставить с каждого края, с.
    pub padding: f64,
    /// Длина окна измерения громкости, мс.
    pub window_ms: u32,
}

impl Default for PauseSettings {
    fn default() -> Self {
        Self {
            threshold_db: None,
            min_pause: 0.3,
            padding: 1.0 / 3.0,
            window_ms: 20,
        }
    }
}

/// Отрезок времени в секундах от начала анализируемого окна.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    /// Начало, с.
    pub start: f64,
    /// Конец, с.
    pub end: f64,
}

/// Итог анализа: что услышали и какие отрезки предложить к вырезанию.
#[derive(Clone, Debug)]
pub struct Analysis {
    /// Громкость шумового пола, дБ.
    pub noise_floor_db: f32,
    /// Громкость речи, дБ.
    pub speech_db: f32,
    /// Применённый порог тишины, дБ.
    pub threshold_db: f32,
    /// Отрезки к вырезанию, уже с отступами, по возрастанию.
    pub pauses: Vec<Range>,
}

/// Что анализировать в файле.
#[derive(Clone, Debug)]
pub struct FileRequest {
    /// Путь к файлу.
    pub path: String,
    /// Какой поток звука, по индексу; `None` — первый.
    pub audio_stream: Option<usize>,
    /// Сколько секунд от начала файла берём.
    pub source_start: f64,
    /// Длина окна клипа, с.
    pub window: f64,
    /// Цепочка фильтров FFmpeg клипа: анализ слышит то же, что человек.
    pub filters: Vec<String>,
}

/// Ошибка анализа файла.
#[derive(Debug, thiserror::Error)]
pub enum PauseError {
    /// Звук не удалось прочитать.
    #[error("не удалось прочитать звук: {0}")]
    Media(#[from] concat_media::Error),
}

/// Размечает паузы по готовым сэмплам — чистая функция без обращений к файлам.
pub fn analyze_pcm(samples: &[f32], rate: u32, settings: &PauseSettings) -> Analysis {
    let window = window_samples(rate, settings.window_ms);
    let levels = window_rms_db(samples, window);
    let (noise_floor_db, speech_db) = levels_stats(&levels);
    let threshold_db = settings
        .threshold_db
        .unwrap_or_else(|| noise_floor_db + (speech_db - noise_floor_db) * AUTO_FRACTION);
    // Автопорог на однородном звуке не отделяет речь от шума — пауз нет.
    let range_is_wide =
        settings.threshold_db.is_some() || (speech_db - noise_floor_db) >= MIN_DYNAMIC_RANGE_DB;
    let pauses = if range_is_wide && !samples.is_empty() {
        find_pauses(samples, rate, window, settings, &levels, threshold_db)
    } else {
        Vec::new()
    };
    Analysis {
        noise_floor_db,
        speech_db,
        threshold_db,
        pauses,
    }
}

/// Читает окно клипа и размечает паузы по нему.
pub fn analyze_file(
    request: &FileRequest,
    settings: &PauseSettings,
) -> Result<Analysis, PauseError> {
    let mut decoder = AudioDecoder::open(
        &request.path,
        &AudioOptions {
            start: Some(request.source_start),
            duration: Some(request.window),
            filters: request.filters.clone(),
            rate: ANALYSIS_RATE,
            channels: 1,
            format: SampleFormat::F32,
            stream: request.audio_stream,
        },
    )?;
    let samples = decoder.collect_f32()?;
    let analysis = analyze_pcm(&samples, ANALYSIS_RATE, settings);
    log::debug!(
        "анализ пауз: {} отсчётов, {} отрезков к вырезанию",
        samples.len(),
        analysis.pauses.len()
    );
    Ok(analysis)
}

/// Длина окна в сэмплах, но не меньше одного.
fn window_samples(rate: u32, window_ms: u32) -> usize {
    (u64::from(rate) * u64::from(window_ms) / 1000).max(1) as usize
}

/// RMS каждого окна в дБ, с нижней границей на тишине.
fn window_rms_db(samples: &[f32], window: usize) -> Vec<f32> {
    samples
        .chunks(window)
        .map(|chunk| {
            let mean_square = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            let rms = mean_square.sqrt();
            if rms > 0.0 {
                (20.0 * rms.log10()).max(DB_FLOOR)
            } else {
                DB_FLOOR
            }
        })
        .collect()
}

/// Шумовой пол (10-й перцентиль) и речь (90-й перцентиль) по окнам.
fn levels_stats(levels: &[f32]) -> (f32, f32) {
    (percentile(levels, 0.10), percentile(levels, 0.90))
}

/// Перцентиль с линейной интерполяцией между соседними значениями.
fn percentile(values: &[f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return DB_FLOOR;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let position = fraction * (sorted.len() - 1) as f32;
    let low = position.floor() as usize;
    let high = position.ceil() as usize;
    if low == high {
        sorted[low]
    } else {
        let weight = position - low as f32;
        sorted[low] * (1.0 - weight) + sorted[high] * weight
    }
}

/// Тихие окна склеивает в отрезки и превращает их в вырезаемые части.
fn find_pauses(
    samples: &[f32],
    rate: u32,
    window: usize,
    settings: &PauseSettings,
    levels: &[f32],
    threshold_db: f32,
) -> Vec<Range> {
    let mut quiet: Vec<bool> = levels.iter().map(|level| *level < threshold_db).collect();
    fill_short_runs(&mut quiet, true, BURST_MS, settings.window_ms);
    clear_short_runs(&mut quiet, DIP_MS, settings.window_ms);

    let total = samples.len();
    runs_where(&quiet, true)
        .into_iter()
        .filter_map(|(start, end)| {
            let first = start * window;
            let last = (end * window).min(total);
            if last <= first {
                return None;
            }
            let begin = first as f64 / f64::from(rate);
            let finish = last as f64 / f64::from(rate);
            // У самого края окна отступ не нужен: резать нечего.
            let cut_start = if start == 0 {
                begin
            } else {
                begin + settings.padding
            };
            let cut_end = if last == total {
                finish
            } else {
                finish - settings.padding
            };
            (cut_end - cut_start >= settings.min_pause).then_some(Range {
                start: cut_start,
                end: cut_end,
            })
        })
        .collect()
}

/// Заливает короткие отрезки со значением `!target` значением `target`.
fn fill_short_runs(values: &mut [bool], target: bool, limit_ms: f64, window_ms: u32) {
    for (start, end) in runs_where(values, !target) {
        if run_ms(end - start, window_ms) <= limit_ms {
            values[start..end].fill(target);
        }
    }
}

/// Убирает короткие отрезки со значением `true`, считая их не паузой.
fn clear_short_runs(values: &mut [bool], limit_ms: f64, window_ms: u32) {
    for (start, end) in runs_where(values, true) {
        if run_ms(end - start, window_ms) < limit_ms {
            values[start..end].fill(false);
        }
    }
}

/// Границы всех отрезков со значением `wanted`: `(начало, конец)` окон.
fn runs_where(values: &[bool], wanted: bool) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut index = 0;
    while index < values.len() {
        if values[index] != wanted {
            index += 1;
            continue;
        }
        let start = index;
        while index < values.len() && values[index] == wanted {
            index += 1;
        }
        runs.push((start, index));
    }
    runs
}

/// Длина отрезка в миллисекундах.
fn run_ms(windows: usize, window_ms: u32) -> f64 {
    windows as f64 * f64::from(window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    /// Синус 220 Гц — наша «речь».
    fn tone(seconds: f64, amplitude: f32) -> Vec<f32> {
        let count = (seconds * f64::from(RATE)).round() as usize;
        (0..count)
            .map(|index| {
                let t = index as f32 / RATE as f32;
                amplitude * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
            })
            .collect()
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; (seconds * f64::from(RATE)).round() as usize]
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn a_pause_in_the_middle_keeps_a_third_of_a_second_on_both_edges() {
        let mut samples = tone(2.0, 0.5);
        samples.extend(silence(1.5));
        samples.extend(tone(2.0, 0.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert_eq!(analysis.pauses.len(), 1);
        assert!(close(analysis.pauses[0].start, 2.0 + 1.0 / 3.0));
        assert!(close(analysis.pauses[0].end, 3.5 - 1.0 / 3.0));
    }

    #[test]
    fn a_pause_shorter_than_min_plus_padding_is_dropped() {
        let mut samples = tone(2.0, 0.5);
        samples.extend(silence(0.9));
        samples.extend(tone(2.0, 0.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert!(analysis.pauses.is_empty());
    }

    #[test]
    fn silence_at_the_start_is_cut_from_zero() {
        let mut samples = silence(1.5);
        samples.extend(tone(2.0, 0.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert_eq!(analysis.pauses.len(), 1);
        assert!(close(analysis.pauses[0].start, 0.0));
        assert!(close(analysis.pauses[0].end, 1.5 - 1.0 / 3.0));
    }

    #[test]
    fn silence_at_the_end_is_cut_to_the_last_sample() {
        let mut samples = tone(2.0, 0.5);
        samples.extend(silence(1.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert_eq!(analysis.pauses.len(), 1);
        assert!(close(analysis.pauses[0].start, 2.0 + 1.0 / 3.0));
        assert!(close(analysis.pauses[0].end, 3.5));
    }

    #[test]
    fn a_thirty_millisecond_click_does_not_break_a_pause() {
        let mut samples = tone(1.0, 0.5);
        samples.extend(silence(0.75));
        samples.extend(tone(0.03, 0.5));
        samples.extend(silence(0.72));
        samples.extend(tone(1.0, 0.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert_eq!(analysis.pauses.len(), 1);
        assert!(close(analysis.pauses[0].start, 1.0 + 1.0 / 3.0));
        assert!(close(analysis.pauses[0].end, 2.5 - 1.0 / 3.0));
    }

    #[test]
    fn a_manual_threshold_is_used_as_given() {
        let mut samples = tone(2.0, 0.5);
        samples.extend(silence(1.5));
        samples.extend(tone(2.0, 0.5));
        let settings = PauseSettings {
            threshold_db: Some(-40.0),
            ..PauseSettings::default()
        };
        let analysis = analyze_pcm(&samples, RATE, &settings);
        assert_eq!(analysis.threshold_db, -40.0);
        assert_eq!(analysis.pauses.len(), 1);
    }

    #[test]
    fn pure_silence_yields_no_pauses() {
        let samples = silence(3.0);
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert!(analysis.pauses.is_empty());
    }

    #[test]
    fn the_auto_threshold_sits_between_the_floor_and_speech() {
        let mut samples = tone(2.0, 0.5);
        samples.extend(silence(1.5));
        samples.extend(tone(2.0, 0.5));
        let analysis = analyze_pcm(&samples, RATE, &PauseSettings::default());
        assert!(analysis.noise_floor_db < analysis.threshold_db);
        assert!(analysis.threshold_db < analysis.speech_db);
    }
}
