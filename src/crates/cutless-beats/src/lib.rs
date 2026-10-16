// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка долей трека: темп, фаза, такты, бочка, малый и дропы — анализом звука без FFT.
//! Результат — `concat_project::beats::BeatGrid` во времени файла.

pub mod envelope;
pub mod filter;
pub mod structure;
pub mod tempo;

use std::sync::atomic::{AtomicBool, Ordering};

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use concat_project::beats::{BeatGrid, VERSION};

use crate::envelope::{Accumulator, Envelopes, FPS, RATE};

/// Короче этого ритм не ищется, секунды.
const MIN_SECONDS: f64 = 8.0;

/// Почему сетку не построить.
#[derive(Debug, thiserror::Error)]
pub enum BeatError {
    /// Звук не прочитался.
    #[error("could not read the sound: {0}")]
    Media(#[from] concat_media::Error),
    /// Звука слишком мало.
    #[error("the sound is shorter than {MIN_SECONDS} seconds")]
    TooShort,
    /// Ритма не слышно: тишина, шум, речь.
    #[error("no steady beat was heard")]
    NoRhythm,
    /// Разбор остановили.
    #[error("beat analysis cancelled")]
    Cancelled,
}

/// Сетка по готовым сэмплам моно на 16 кГц ([`envelope::RATE`]).
pub fn analyze_pcm(samples: &[f32]) -> Result<BeatGrid, BeatError> {
    let mut accumulator = Accumulator::default();
    accumulator.push(samples);
    grid_from(&accumulator.finish())
}

/// Сетка всего файла по одному звуковому потоку; звук читается потоком, кусками.
/// `stop` проверяется между кусками: поднятый флаг прерывает разбор.
pub fn analyze_file(
    path: &str,
    stream: Option<usize>,
    stop: &AtomicBool,
) -> Result<BeatGrid, BeatError> {
    let mut decoder = AudioDecoder::open(
        path,
        &AudioOptions {
            start: None,
            duration: None,
            // Цепочку клипа не берём: темп и время — файла, а `atempo` их бы сдвинул.
            filters: Vec::new(),
            rate: RATE,
            channels: 1,
            format: SampleFormat::F32,
            stream,
        },
    )?;
    let mut accumulator = Accumulator::default();
    while let Some(chunk) = decoder.next_f32()? {
        if stop.load(Ordering::Relaxed) {
            return Err(BeatError::Cancelled);
        }
        accumulator.push(&chunk);
    }
    grid_from(&accumulator.finish())
}

/// Сетка по огибающим.
fn grid_from(envelopes: &Envelopes) -> Result<BeatGrid, BeatError> {
    let duration = envelopes.len() as f64 / FPS;
    if duration < MIN_SECONDS {
        return Err(BeatError::TooShort);
    }
    let tempo = tempo::estimate(envelopes).ok_or(BeatError::NoRhythm)?;
    let beat = 60.0 / tempo.bpm;
    let downbeat = structure::downbeat(envelopes, beat, tempo.phase);
    let (kicks, snares) = structure::hits(envelopes);
    let drops = structure::drops(envelopes, beat, tempo.phase, downbeat);
    log::debug!(
        "сетка долей: {:.3} BPM, фаза {:.3} с, сильная доля {downbeat}, {} бочек, {} малых, {} дропов",
        tempo.bpm,
        tempo.phase,
        kicks.len(),
        snares.len(),
        drops.len()
    );
    Ok(BeatGrid {
        version: VERSION,
        bpm: tempo.bpm,
        phase: tempo.phase,
        bar: structure::BAR,
        downbeat,
        duration,
        confidence: if tempo.confidence.is_finite() {
            tempo.confidence
        } else {
            99.0
        },
        alt_bpm: tempo.alt_bpm,
        steady: tempo.steady,
        kicks,
        snares,
        drops,
    })
}

#[cfg(test)]
mod tests;
