// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Огибающие онсетов трёх полос: энергия по шагам 5 мс, окно Ханна, логарифм, положительная разность.
//! Сэмплы идут потоком: в памяти только по числу на шаг и полосу, часовой микс не держится целиком.

use crate::filter::Band;

/// Частота, на которой разбирается звук.
pub const RATE: u32 = 16_000;
/// Шаг огибающей в сэмплах: 5 мс.
pub const HOP: usize = 80;
/// Кадров огибающей в секунду.
pub const FPS: f64 = RATE as f64 / HOP as f64;
/// На сколько онсет огибающей опережает удар: пик разности приходится на шаг раньше удара.
pub const LEAD: f64 = HOP as f64 / RATE as f64;

/// Полуширина окна Ханна в шагах: бас — 25 мс, середина и всё — 15 мс.
const LOW_HALF: usize = 2;
const MID_HALF: usize = 1;

/// Сколько энергии на шаг накопила одна полоса.
struct Lane {
    band: Band,
    sum: f64,
    powers: Vec<f64>,
}

impl Lane {
    fn new(band: Band) -> Lane {
        Lane {
            band,
            sum: 0.0,
            powers: Vec::new(),
        }
    }

    fn push(&mut self, x: f64) {
        let y = self.band.process(x);
        self.sum += y * y;
    }

    fn close_hop(&mut self) {
        self.powers.push(self.sum / HOP as f64);
        self.sum = 0.0;
    }
}

/// Накопитель энергии полос: сэмплы приходят кусками, по числу на шаг остаётся.
pub struct Accumulator {
    full: Lane,
    low: Lane,
    mid: Lane,
    filled: usize,
}

impl Default for Accumulator {
    fn default() -> Self {
        let rate = f64::from(RATE);
        Accumulator {
            // Без НЧ: срез 8 кГц на 16 кГц лёг бы на Найквиста.
            full: Lane::new(Band::new(Some((20.0, 2)), None, rate)),
            low: Lane::new(Band::new(Some((30.0, 2)), Some((150.0, 4)), rate)),
            mid: Lane::new(Band::new(Some((1_200.0, 4)), Some((6_000.0, 2)), rate)),
            filled: 0,
        }
    }
}

impl Accumulator {
    /// Принимает кусок сэмплов моно на частоте [`RATE`].
    pub fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            let x = f64::from(sample);
            self.full.push(x);
            self.low.push(x);
            self.mid.push(x);
            self.filled += 1;
            if self.filled == HOP {
                self.full.close_hop();
                self.low.close_hop();
                self.mid.close_hop();
                self.filled = 0;
            }
        }
    }

    /// Готовые огибающие.
    pub fn finish(self) -> Envelopes {
        let full = smooth(&self.full.powers, MID_HALF);
        let low = smooth(&self.low.powers, LOW_HALF);
        let mid = smooth(&self.mid.powers, MID_HALF);
        Envelopes {
            full_onset: onset(&full),
            low_onset: onset(&low),
            mid_onset: onset(&mid),
            full_power: full,
            low_power: low,
            mid_power: mid,
        }
    }
}

/// Огибающие трека: онсеты трёх полос (0…≈1) и мощность для громкости.
#[derive(Clone, Debug, Default)]
pub struct Envelopes {
    /// Онсеты всего звука.
    pub full_onset: Vec<f64>,
    /// Онсеты баса (бочка).
    pub low_onset: Vec<f64>,
    /// Онсеты середины (малый).
    pub mid_onset: Vec<f64>,
    /// Средняя мощность всего звука по кадрам.
    pub full_power: Vec<f64>,
    /// Средняя мощность баса по кадрам.
    pub low_power: Vec<f64>,
    /// Средняя мощность середины по кадрам.
    pub mid_power: Vec<f64>,
}

impl Envelopes {
    /// Кадров в огибающих.
    pub fn len(&self) -> usize {
        self.full_onset.len()
    }

    /// Огибающие пусты: звука не было.
    pub fn is_empty(&self) -> bool {
        self.full_onset.is_empty()
    }

    /// Сумма трёх онсетов кадра — по ней ищутся доли.
    pub fn combined(&self, frame: usize) -> f64 {
        self.full_onset[frame] + self.low_onset[frame] + self.mid_onset[frame]
    }
}

/// Мощность по окну Ханна шириной `2·half + 1` шагов вокруг каждого кадра.
fn smooth(powers: &[f64], half: usize) -> Vec<f64> {
    let width = 2 * half + 1;
    let weights: Vec<f64> = (0..width)
        .map(|i| {
            let phase = (i as f64 + 1.0) / (width as f64 + 1.0);
            (std::f64::consts::PI * phase).sin().powi(2)
        })
        .collect();
    let total: f64 = weights.iter().sum();
    (0..powers.len())
        .map(|frame| {
            weights
                .iter()
                .enumerate()
                .map(|(i, weight)| {
                    let at = (frame + i).saturating_sub(half).min(powers.len() - 1);
                    weight * powers[at]
                })
                .sum::<f64>()
                / total
        })
        .collect()
}

/// Онсеты: `ln(1 + C·e)` с `C = 1000 / P95`, положительная разность, деление на P99,5.
fn onset(powers: &[f64]) -> Vec<f64> {
    if powers.is_empty() {
        return Vec::new();
    }
    let scale = 1000.0 / percentile(powers, 0.95).max(1e-12);
    let logs: Vec<f64> = powers.iter().map(|power| (scale * power).ln_1p()).collect();
    let rises: Vec<f64> = std::iter::once(0.0)
        .chain(logs.windows(2).map(|pair| (pair[1] - pair[0]).max(0.0)))
        .collect();
    let top = percentile(&rises, 0.995).max(1e-9);
    rises.into_iter().map(|rise| rise / top).collect()
}

/// Значение на доле `share` упорядоченных данных.
pub fn percentile(values: &[f64], share: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * share.clamp(0.0, 1.0)).round() as usize;
    sorted[index]
}
