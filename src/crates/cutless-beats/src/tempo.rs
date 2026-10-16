// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Темп и фаза: автокорреляция огибающей, выбор среди ×2, ×½, ×3/2, ×2/3 по контрасту доли и полудоли,
//! уточнение перебором и наименьшими квадратами по пикам у каждой доли.

use crate::envelope::{Envelopes, FPS, LEAD, percentile};

/// Темпы, которые сетка вообще рассматривает.
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
/// Октавный соперник (×2, ×½) сообщается как `alt_bpm`, пока лучший сильнее него меньше чем
/// во столько раз: полутемп и двойной темп — законные прочтения одного трека.
pub const DOUBTFUL_OCTAVE: f64 = 2.0;
/// То же для пунктирных соперников (×3/2, ×2/3…): они реже бывают верными.
pub const DOUBTFUL_DOTTED: f64 = 1.3;
/// Родственные темпы автокорреляционного кандидата: октавы и пунктирные соотношения.
const RELATIVES: [f64; 7] = [1.0, 2.0, 0.5, 1.5, 0.75, 2.0 / 3.0, 4.0 / 3.0];
/// Вес опоры на восьмые в счёте темпа.
const OFFBEAT_WEIGHT: f64 = 0.5;
/// Доли выделяются меньше этого — ритма нет: тишина, шум, речь.
const MIN_SALIENCE: f64 = 1.0;
/// Доли ложатся на сетку точнее этого — темп ровный, секунды.
const STEADY: f64 = 0.02;

/// Найденный темп.
#[derive(Clone, Debug, PartialEq)]
pub struct Tempo {
    /// Долей в минуту.
    pub bpm: f64,
    /// Секунда, на которую падает доля, `0..beat`.
    pub phase: f64,
    /// Во сколько раз лучший кандидат сильнее второго.
    pub confidence: f64,
    /// Второй кандидат, если выбор сомнителен.
    pub alt_bpm: Option<f64>,
    /// Доли ложатся на сетку точнее 20 мс.
    pub steady: bool,
}

/// Огибающая, по которой ищутся доли: сумма полос, сглаженная по трём кадрам.
struct Curve {
    values: Vec<f64>,
    /// Среднее по треку: от него меряется, насколько доли выделяются.
    mean: f64,
}

impl Curve {
    fn new(envelopes: &Envelopes) -> Curve {
        let raw: Vec<f64> = (0..envelopes.len())
            .map(|frame| envelopes.combined(frame))
            .collect();
        let values = (0..raw.len())
            .map(|i| {
                let at = |j: isize| raw[(i as isize + j).clamp(0, raw.len() as isize - 1) as usize];
                0.25 * at(-1) + 0.5 * at(0) + 0.25 * at(1)
            })
            .collect::<Vec<f64>>();
        let mean = (values.iter().sum::<f64>() / values.len().max(1) as f64).max(1e-9);
        Curve { values, mean }
    }

    /// Значение в секунду `time`, между кадрами — по прямой.
    fn at(&self, time: f64) -> f64 {
        let position = time * FPS;
        if position < 0.0 {
            return 0.0;
        }
        let index = position.floor() as usize;
        if index + 1 >= self.values.len() {
            return 0.0;
        }
        let share = position - index as f64;
        self.values[index] * (1.0 - share) + self.values[index + 1] * share
    }

    fn seconds(&self) -> f64 {
        self.values.len() as f64 / FPS
    }
}

/// Априорный вес темпа: логнормальный вокруг 120 BPM, октава — одна сигма.
fn prior(bpm: f64) -> f64 {
    (-0.5 * (bpm / 120.0).log2().powi(2)).exp()
}

/// Насколько доли сетки сильнее среднего по треку (0 — не сильнее) и фаза, где это сильнее всего.
/// Полудоли не вычитаются: хэт на «и» иначе отдал бы победу сетке через полторы доли.
fn salience(curve: &Curve, bpm: f64, phase_step: f64) -> (f64, f64) {
    let beat = 60.0 / bpm;
    let seconds = curve.seconds();
    let mut best = (f64::MIN, 0.0);
    let mut phase = 0.0;
    while phase < beat {
        let mut on = 0.0;
        let mut count = 0usize;
        let mut time = phase;
        while time < seconds {
            on += curve.at(time);
            count += 1;
            time += beat;
        }
        if count > 0 {
            let score = on / count as f64 / curve.mean - 1.0;
            if score > best.0 {
                best = (score, phase);
            }
        }
        phase += phase_step;
    }
    best
}

/// Счёт темпа: доли выделяются, полудоли звучат слабее долей. У ложной «пунктирной» сетки на
/// полудолях пусто; у сетки вдвое медленнее настоящей полудоли так же сильны, как доли — штраф.
fn metrical(curve: &Curve, bpm: f64) -> f64 {
    let (on, phase) = salience(curve, bpm, 0.008);
    let beat = 60.0 / bpm;
    let mut sum = 0.0;
    let mut count = 0usize;
    let mut time = phase + beat / 2.0;
    while time < curve.seconds() {
        sum += curve.at(time);
        count += 1;
        time += beat;
    }
    let off = if count > 0 {
        sum / count as f64 / curve.mean - 1.0
    } else {
        0.0
    };
    let half = on / 2.0;
    on + OFFBEAT_WEIGHT * off.min(half) - (off - half).max(0.0)
}

/// Кандидат темпа по автокорреляции огибающей с вычтенным скользящим средним.
fn autocorrelation_bpm(curve: &Curve) -> Option<f64> {
    let values = &curve.values;
    let n = values.len();
    let half = FPS as usize / 2;
    let mut prefix = vec![0.0; n + 1];
    for (i, value) in values.iter().enumerate() {
        prefix[i + 1] = prefix[i] + value;
    }
    let detrended: Vec<f64> = (0..n)
        .map(|i| {
            let (from, to) = (i.saturating_sub(half), (i + half + 1).min(n));
            values[i] - (prefix[to] - prefix[from]) / (to - from) as f64
        })
        .collect();
    let lag_of = |bpm: f64| 60.0 * FPS / bpm;
    let (shortest, longest) = (
        lag_of(MAX_BPM + 10.0).floor() as usize,
        lag_of(MIN_BPM - 5.0).ceil() as usize,
    );
    if n <= longest + 2 {
        return None;
    }
    let correlation = |lag: usize| -> f64 {
        let sum: f64 = (0..n - lag)
            .map(|i| detrended[i] * detrended[i + lag])
            .sum();
        sum / (n - lag) as f64
    };
    let scores: Vec<(usize, f64)> = (shortest..=longest)
        .map(|lag| (lag, correlation(lag)))
        .collect();
    let (index, _) = scores
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| *r > 0.0)
        .map(|(index, (lag, r))| (index, r * prior(lag_of(*lag as f64))))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    // Пик уточняется параболой по соседним лагам, иначе шаг лага — почти 2 BPM.
    let lag = scores[index].0 as f64;
    let refined = if index > 0 && index + 1 < scores.len() {
        let (a, b, c) = (scores[index - 1].1, scores[index].1, scores[index + 1].1);
        let bend = a - 2.0 * b + c;
        // Сдвиг вершины не дальше полулага: на почти прямом участке парабола улетает.
        if bend.abs() > f64::EPSILON {
            lag + (0.5 * (a - c) / bend).clamp(-0.5, 0.5)
        } else {
            lag
        }
    } else {
        lag
    };
    Some(lag_of(refined))
}

/// Темп и фаза трека; `None`, если ритма не слышно.
pub fn estimate(envelopes: &Envelopes) -> Option<Tempo> {
    let curve = Curve::new(envelopes);
    let base = autocorrelation_bpm(&curve)?;
    let mut candidates: Vec<(f64, f64)> = RELATIVES
        .iter()
        .map(|factor| base * factor)
        .filter(|bpm| (MIN_BPM..=MAX_BPM).contains(bpm))
        .map(|bpm| (bpm, prior(bpm) * metrical(&curve, bpm)))
        .collect();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    log::debug!("темп: автокорреляция {base:.2}, кандидаты {candidates:?}");
    let (best, best_score) = *candidates.first()?;
    if salience(&curve, best, 0.008).0 < MIN_SALIENCE {
        return None;
    }
    let second = candidates
        .iter()
        .find(|(bpm, _)| (bpm - best).abs() / best > 0.03)
        .copied();
    let confidence = match second {
        Some((_, score)) if score > 0.0 => best_score / score,
        _ => f64::INFINITY,
    };
    // Соперник для подсказки: лучший октавный, если он близок, иначе любой близкий.
    let octave = candidates
        .iter()
        .find(|(bpm, _)| {
            [2.0, 0.5]
                .iter()
                .any(|ratio| (bpm / best - ratio).abs() < 0.03)
        })
        .copied();
    let alt_bpm = match octave {
        Some((bpm, score)) if score > 0.0 && best_score / score < DOUBTFUL_OCTAVE => Some(bpm),
        _ => second
            .filter(|_| confidence < DOUBTFUL_DOTTED)
            .map(|(bpm, _)| bpm),
    };

    // Уточнение: перебор рядом с лучшим, затем наименьшие квадраты по пикам у долей.
    let (mut bpm, mut phase) = (best, 0.0);
    let mut top = f64::MIN;
    let mut candidate = best - 1.5;
    while candidate <= best + 1.5 {
        let (score, at) = salience(&curve, candidate, 0.004);
        if score > top {
            (top, bpm, phase) = (score, candidate, at);
        }
        candidate += 0.05;
    }
    let mut residual = f64::INFINITY;
    for _ in 0..3 {
        if let Some((fitted_bpm, fitted_phase, rms)) = least_squares(&curve, bpm, phase) {
            (bpm, phase, residual) = (fitted_bpm, fitted_phase, rms);
        }
    }
    let beat = 60.0 / bpm;
    let phase = (half_beat(envelopes, beat, phase) + LEAD).rem_euclid(beat);
    Some(Tempo {
        bpm,
        phase,
        confidence,
        alt_bpm,
        steady: residual < STEADY,
    })
}

/// Подгонка `t_k = φ + k·B` по пикам огибающей в ±30 мс от каждой доли сетки.
fn least_squares(curve: &Curve, bpm: f64, phase: f64) -> Option<(f64, f64, f64)> {
    let beat = 60.0 / bpm;
    let reach = (0.03 * FPS).round() as isize;
    let floor = percentile(&curve.values, 0.9);
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut index = 0i64;
    loop {
        let expected = phase + index as f64 * beat;
        if expected >= curve.seconds() {
            break;
        }
        let centre = (expected * FPS).round() as isize;
        let best = (-reach..=reach)
            .map(|offset| centre + offset)
            .filter(|frame| *frame > 0 && (*frame as usize) + 1 < curve.values.len())
            .max_by(|a, b| curve.values[*a as usize].total_cmp(&curve.values[*b as usize]));
        if let Some(frame) = best {
            let frame = frame as usize;
            if curve.values[frame] >= floor {
                // Вершина пика — параболой по соседним кадрам.
                let (a, b, c) = (
                    curve.values[frame - 1],
                    curve.values[frame],
                    curve.values[frame + 1],
                );
                let bend = a - 2.0 * b + c;
                let shift = if bend.abs() > f64::EPSILON {
                    0.5 * (a - c) / bend
                } else {
                    0.0
                };
                points.push((index as f64, (frame as f64 + shift.clamp(-0.5, 0.5)) / FPS));
            }
        }
        index += 1;
    }
    if points.len() < 8 {
        return None;
    }
    let n = points.len() as f64;
    let (sum_k, sum_t) = points
        .iter()
        .fold((0.0, 0.0), |(k, t), (pk, pt)| (k + pk, t + pt));
    let (mean_k, mean_t) = (sum_k / n, sum_t / n);
    let (mut spread, mut covariance) = (0.0, 0.0);
    for (k, t) in &points {
        spread += (k - mean_k).powi(2);
        covariance += (k - mean_k) * (t - mean_t);
    }
    if spread <= 0.0 {
        return None;
    }
    let fitted_beat = covariance / spread;
    if !(60.0 / MAX_BPM * 0.9..=60.0 / MIN_BPM * 1.1).contains(&fitted_beat) {
        return None;
    }
    let fitted_phase = mean_t - fitted_beat * mean_k;
    let rms = (points
        .iter()
        .map(|(k, t)| (t - (fitted_phase + fitted_beat * k)).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    Some((60.0 / fitted_beat, fitted_phase, rms))
}

/// Доли сетки или «и» между ними: где сильнее бас с серединой, там и доли.
fn half_beat(envelopes: &Envelopes, beat: f64, phase: f64) -> f64 {
    let strength = |offset: f64| -> f64 {
        let mut sum = 0.0;
        let mut time = phase + offset;
        let seconds = envelopes.len() as f64 / FPS;
        while time < seconds {
            let frame = (time * FPS).round() as isize;
            let window = (frame - 1).max(0) as usize
                ..=((frame + 1).max(0) as usize).min(envelopes.len() - 1);
            sum += window
                .map(|i| envelopes.low_onset[i] + envelopes.mid_onset[i])
                .fold(0.0, f64::max);
            time += beat;
        }
        sum
    };
    if strength(beat / 2.0) > strength(0.0) * 1.1 {
        phase + beat / 2.0
    } else {
        phase
    }
}
