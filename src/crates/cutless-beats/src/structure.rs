// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Что сверх темпа: сильная доля такта, удары бочки и малого, дропы.

use crate::envelope::{Envelopes, FPS, LEAD, percentile};

/// Долей в такте: 4/4 — почти вся музыка для эдитов.
pub const BAR: u32 = 4;
/// Сколько тактов в начале трека не бывает дропом.
const DROP_SKIP: usize = 4;
/// Ближе этого числа тактов друг к другу дропов нет.
const DROP_GAP: usize = 16;
/// Больше дропов на трек не отмечается.
const MAX_DROPS: usize = 4;
/// Бонус дропу на границе фразы, дБ: части музыки начинаются с такта, кратного четырём.
const PHRASE_BONUS: f64 = 3.0;

/// Кадр огибающей для секунды.
fn frame_of(time: f64) -> isize {
    (time * FPS).round() as isize
}

/// Наибольшее значение в ±`reach` кадрах.
fn peak_near(values: &[f64], frame: isize, reach: isize) -> f64 {
    let last = values.len() as isize - 1;
    if last < 0 {
        return 0.0;
    }
    ((frame - reach).max(0)..=(frame + reach).min(last))
        .map(|i| values[i as usize])
        .fold(0.0, f64::max)
}

/// Секунды долей сетки внутри трека.
fn beat_times(envelopes: &Envelopes, beat: f64, phase: f64) -> Vec<f64> {
    let seconds = envelopes.len() as f64 / FPS;
    let mut times = Vec::new();
    let mut time = phase;
    while time < seconds {
        times.push(time);
        time += beat;
    }
    times
}

/// С какой доли (по модулю такта) начинаются такты: бочка на сильной, малый на 2 и 4.
pub fn downbeat(envelopes: &Envelopes, beat: f64, phase: f64) -> u32 {
    let times = beat_times(envelopes, beat, phase);
    let low: Vec<f64> = times
        .iter()
        .map(|time| peak_near(&envelopes.low_onset, frame_of(*time), 2))
        .collect();
    let mid: Vec<f64> = times
        .iter()
        .map(|time| peak_near(&envelopes.mid_onset, frame_of(*time), 2))
        .collect();
    let bar = BAR as usize;
    let sum = |values: &[f64], residue: usize| -> f64 {
        values
            .iter()
            .enumerate()
            .filter(|(index, _)| index % bar == residue % bar)
            .map(|(_, value)| value)
            .sum()
    };
    let scores: Vec<f64> = (0..bar)
        .map(|s| sum(&low, s) - 0.5 * sum(&mid, s) + sum(&mid, s + 1) + sum(&mid, s + 3))
        .collect();
    let best = (0..bar)
        .max_by(|a, b| scores[*a].total_cmp(&scores[*b]))
        .unwrap_or(0);
    // Сильная доля и третья почти равны — решает новизна: части трека начинаются с такта.
    let rival = (best + 2) % bar;
    let close = scores[best] > 0.0 && (scores[best] - scores[rival]) / scores[best] < 0.1;
    let chosen = if close && novelty(envelopes, &times, rival) > novelty(envelopes, &times, best) {
        rival
    } else {
        best
    };
    chosen as u32
}

/// Скачки громкости между тактами, начатыми с доли `start`, в квадрате: резкая граница
/// части весит больше, чем та же разница, размазанная на два такта.
fn novelty(envelopes: &Envelopes, times: &[f64], start: usize) -> f64 {
    let levels = bar_levels(&envelopes.full_power, times, start);
    levels
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).powi(2))
        .sum()
}

/// Громкость тактов в дБ: средняя мощность от начала такта до следующего.
fn bar_levels(power: &[f64], times: &[f64], start: usize) -> Vec<f64> {
    let bar = BAR as usize;
    let starts: Vec<usize> = times
        .iter()
        .enumerate()
        .filter(|(index, _)| *index >= start && (index - start).is_multiple_of(bar))
        .map(|(_, time)| frame_of(*time).max(0) as usize)
        .collect();
    starts
        .windows(2)
        .map(|pair| {
            let (from, to) = (pair[0].min(power.len()), pair[1].min(power.len()));
            if to <= from {
                return -120.0;
            }
            let mean = power[from..to].iter().sum::<f64>() / (to - from) as f64;
            10.0 * mean.max(1e-12).log10()
        })
        .collect()
}

/// Дропы: такт, где бас громче четырёх прошлых на 6 дБ, всё — на 2 дБ, и следующие держат уровень.
pub fn drops(envelopes: &Envelopes, beat: f64, phase: f64, downbeat: u32) -> Vec<f64> {
    let times = beat_times(envelopes, beat, phase);
    let start = downbeat as usize;
    let low = bar_levels(&envelopes.low_power, &times, start);
    let full = bar_levels(&envelopes.full_power, &times, start);
    let bar_time = |bar: usize| times.get(start + bar * BAR as usize).copied();
    let power_mean = |levels: &[f64]| {
        let mean = levels.iter().map(|db| 10f64.powf(db / 10.0)).sum::<f64>() / levels.len() as f64;
        10.0 * mean.max(1e-12).log10()
    };
    // Кандидаты на всех тактах; на границе фразы (такт кратен 4) — с бонусом. Отбор по силе,
    // а не по времени: слабое вступление баса не заслонит настоящий дроп через 12 тактов.
    let mut candidates: Vec<(usize, f64)> = Vec::new();
    for bar in DROP_SKIP..low.len() {
        let rise_low = low[bar] - power_mean(&low[bar - 4..bar]);
        let rise_full = full[bar] - power_mean(&full[bar - 4..bar]);
        let after = &low[bar + 1..(bar + 5).min(low.len())];
        let holds = after
            .iter()
            .filter(|level| **level >= low[bar] - 3.0)
            .count();
        if rise_low >= 6.0 && rise_full >= 2.0 && after.len() >= 2 && holds >= after.len().min(3) {
            let phrase = if bar.is_multiple_of(4) {
                PHRASE_BONUS
            } else {
                0.0
            };
            candidates.push((bar, rise_low + phrase));
        }
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut found: Vec<(usize, f64)> = Vec::new();
    for (bar, score) in candidates {
        let apart = found
            .iter()
            .all(|(kept, _)| bar.abs_diff(*kept) >= DROP_GAP);
        if found.len() < MAX_DROPS && apart {
            found.push((bar, score));
        }
    }
    let mut drops: Vec<f64> = found
        .into_iter()
        .filter_map(|(bar, _)| bar_time(bar))
        .collect();
    drops.sort_by(f64::total_cmp);
    drops
}

/// Доля прироста мощности на ударе, пришедшая на бас, с которой он — бочка (у шума малого — около 2 %).
const KICK_SHARE: f64 = 0.2;
/// Прирост баса бочки — не меньше этой доли типичного (верхний дециль ударов трека).
const KICK_FLOOR: f64 = 0.25;
/// Малый не тише типичного малого больше чем на 5 дБ: хэт тише на 10 дБ и отсеивается.
const SNARE_FLOOR: f64 = 0.3;

/// Насколько мощность поднялась к удару: пик в ±15 мс минус наименьшее за 40 мс до него.
fn rise(power: &[f64], frame: isize) -> f64 {
    let last = power.len() as isize - 1;
    if last < 0 {
        return 0.0;
    }
    let floor = ((frame - 8).clamp(0, last)..=frame.clamp(0, last))
        .map(|i| power[i as usize])
        .fold(f64::INFINITY, f64::min);
    (peak_near(power, frame, 3) - floor).max(0.0)
}

/// Удары бочки и малого: пики онсетов всего звука, разобранные по мощности баса и середины.
/// Мощность, а не онсет: после логарифма любой подъём из тишины выглядит большим.
pub fn hits(envelopes: &Envelopes) -> (Vec<f64>, Vec<f64>) {
    let full = &envelopes.full_onset;
    let frames = |seconds: f64| (seconds * FPS).round() as isize;
    let (half, before, after, wait) = (frames(0.06), frames(0.15), frames(0.10), frames(0.09));
    let mut peaks: Vec<isize> = Vec::new();
    let mut last: isize = isize::MIN / 2;
    for i in 0..full.len() as isize {
        let value = full[i as usize];
        if value <= 0.0 || i - last < wait || peak_near(full, i, half) > value {
            continue;
        }
        let (from, to) = (
            (i - before).max(0) as usize,
            ((i + after) as usize).min(full.len() - 1),
        );
        let mean = full[from..=to].iter().sum::<f64>() / (to - from + 1) as f64;
        if value < mean + 0.12 {
            continue;
        }
        last = i;
        peaks.push(i);
    }
    let rise_low: Vec<f64> = peaks
        .iter()
        .map(|i| rise(&envelopes.low_power, *i))
        .collect();
    let rise_all: Vec<f64> = peaks
        .iter()
        .map(|i| rise(&envelopes.full_power, *i))
        .collect();
    let mid: Vec<f64> = peaks
        .iter()
        .map(|i| peak_near(&envelopes.mid_power, *i, 3))
        .collect();
    // Бочка — когда на ударе прибавился бас, а не когда бас просто громок: саб или 808
    // держат басовую мощность весь трек, и по уровню бочкой стал бы каждый клэп.
    // И прирост баса заметный сам по себе: у тихого хэта на тянущемся басу доля велика,
    // но это лишь пульсация баса.
    let kick_reference = percentile(&rise_low, 0.9);
    let is_kick: Vec<bool> = (0..peaks.len())
        .map(|n| {
            rise_all[n] > 0.0
                && rise_low[n] / rise_all[n] >= KICK_SHARE
                && rise_low[n] >= KICK_FLOOR * kick_reference
        })
        .collect();
    let snare_mids: Vec<f64> = (0..peaks.len())
        .filter(|n| !is_kick[*n])
        .map(|n| mid[n])
        .collect();
    let snare_reference = percentile(&snare_mids, 0.9);
    let mut kicks = Vec::new();
    let mut snares = Vec::new();
    for (n, frame) in peaks.iter().enumerate() {
        let time = *frame as f64 / FPS + LEAD;
        if is_kick[n] {
            kicks.push(time);
        } else if mid[n] >= SNARE_FLOOR * snare_reference {
            snares.push(time);
        }
    }
    (kicks, snares)
}
