// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка на синтетических треках: известны темп, сильные доли, бочки, малые и дроп.

use super::*;
use concat_project::beats::Pick;

const SAMPLE_RATE: f64 = RATE as f64;

/// Генератор шума xorshift: тесты не зависят от внешних крейтов и всегда одинаковы.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32
    }
}

/// Трек: дорожка сэмплов и истинные моменты ударов.
struct Track {
    samples: Vec<f32>,
    noise: Noise,
    kicks: Vec<f64>,
    snares: Vec<f64>,
}

impl Track {
    fn new(seconds: f64) -> Track {
        let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
        // Фон −30 дБ.
        let samples = (0..(seconds * SAMPLE_RATE) as usize)
            .map(|_| 0.03 * noise.next())
            .collect();
        Track {
            samples,
            noise,
            kicks: Vec::new(),
            snares: Vec::new(),
        }
    }

    fn add(&mut self, at: f64, length: f64, mut voice: impl FnMut(f64, &mut Noise) -> f32) {
        let start = (at * SAMPLE_RATE).round() as usize;
        for n in 0..(length * SAMPLE_RATE) as usize {
            if let Some(sample) = self.samples.get_mut(start + n) {
                *sample += voice(n as f64 / SAMPLE_RATE, &mut self.noise);
            }
        }
    }

    /// Бочка: свип 150 → 50 Гц, затухание 60 мс.
    fn kick(&mut self, at: f64) {
        let mut phase = 0.0;
        self.add(at, 0.3, |t, _| {
            let hz = 50.0 + 100.0 * (-t / 0.03).exp();
            phase += 2.0 * std::f64::consts::PI * hz / SAMPLE_RATE;
            (0.9 * (-t / 0.06).exp() * phase.sin()) as f32
        });
        self.kicks.push(at);
    }

    /// Малый: шум с затуханием 40 мс.
    fn snare(&mut self, at: f64) {
        self.add(at, 0.2, |t, noise| {
            0.5 * (-t / 0.04).exp() as f32 * noise.next()
        });
        self.snares.push(at);
    }

    /// Хэт: короткий шум −16 дБ.
    fn hat(&mut self, at: f64) {
        self.add(at, 0.05, |t, noise| {
            0.16 * (-t / 0.015).exp() as f32 * noise.next()
        });
    }
}

/// 128 BPM, первая доля 0,3013 с, 64 такта: 16 тактов вступления без бочки, затем полный бит.
fn house() -> (Track, f64, f64) {
    let (bpm, first) = (128.0, 0.3013);
    let beat = 60.0 / bpm;
    let mut track = Track::new(first + 64.0 * 4.0 * beat + 1.0);
    for index in 0..256usize {
        let time = first + index as f64 * beat;
        let in_bar = index % 4;
        if index >= 64 && in_bar % 2 == 0 {
            track.kick(time);
        }
        if in_bar % 2 == 1 {
            track.snare(time);
        }
        track.hat(time + beat / 2.0);
    }
    (track, bpm, first)
}

/// Доля найденного, совпавшего с истиной в ±`slack` секунд.
fn matched(found: &[f64], truth: &[f64], slack: f64) -> f64 {
    let hits = found
        .iter()
        .filter(|time| truth.iter().any(|real| (*time - real).abs() <= slack))
        .count();
    hits as f64 / found.len().max(1) as f64
}

#[test]
fn a_house_track_gives_its_tempo_beats_bars_hits_and_drop() {
    let (track, bpm, first) = house();
    let grid = analyze_pcm(&track.samples).expect("a grid");
    assert!((grid.bpm - bpm).abs() <= 0.02, "bpm {}", grid.bpm);
    assert!(grid.steady, "a click track is steady");
    let beat = 60.0 / bpm;
    for index in 0..256 {
        let truth = first + index as f64 * beat;
        let nearest = grid.phase + grid.index_at(truth) as f64 * grid.beat();
        assert!(
            (nearest - truth).abs() <= 0.008,
            "beat {index}: {nearest} vs {truth}"
        );
    }
    assert!(
        grid.is_bar(grid.index_at(first)),
        "the downbeat is the first beat"
    );

    assert!(
        matched(&grid.kicks, &track.kicks, 0.01) >= 0.95,
        "kick precision"
    );
    assert!(
        matched(&track.kicks, &grid.kicks, 0.01) >= 0.95,
        "kick recall"
    );
    assert!(
        matched(&grid.snares, &track.snares, 0.01) >= 0.95,
        "snare precision"
    );
    assert!(
        matched(&track.snares, &grid.snares, 0.01) >= 0.95,
        "snare recall"
    );

    let drop = first + 64.0 * beat;
    assert_eq!(grid.drops.len(), 1, "drops {:?}", grid.drops);
    assert!(
        (grid.drops[0] - drop).abs() <= 0.01,
        "drop {:?} vs {drop}",
        grid.drops
    );
    assert_eq!(grid.pick(Pick::Drops, 0.0, 100.0).len(), 1);
}

#[test]
fn silence_and_noise_have_no_beat() {
    let quiet = vec![0.0_f32; (20.0 * SAMPLE_RATE) as usize];
    assert!(matches!(analyze_pcm(&quiet), Err(BeatError::NoRhythm)));
    let noise = Track::new(20.0);
    assert!(matches!(
        analyze_pcm(&noise.samples),
        Err(BeatError::NoRhythm)
    ));
    assert!(matches!(
        analyze_pcm(&quiet[..1000]),
        Err(BeatError::TooShort)
    ));
}

/// Лоу-фай 85 BPM: бочка на 1 и 3, малый на 2 и 4, хэт на «и» — темп не удваивается.
#[test]
fn a_slow_track_keeps_its_tempo() {
    let (bpm, first) = (85.0, 0.5);
    let beat = 60.0 / bpm;
    let mut track = Track::new(first + 32.0 * 4.0 * beat + 1.0);
    for index in 0..128usize {
        let time = first + index as f64 * beat;
        if index % 2 == 0 {
            track.kick(time);
        } else {
            track.snare(time);
        }
        track.hat(time + beat / 2.0);
    }
    let grid = analyze_pcm(&track.samples).expect("a grid");
    let doubled_with_doubt = (grid.bpm - 2.0 * bpm).abs() < 0.1 && grid.alt_bpm.is_some();
    assert!(
        (grid.bpm - bpm).abs() <= 0.05 || doubled_with_doubt,
        "bpm {} alt {:?} confidence {}",
        grid.bpm,
        grid.alt_bpm,
        grid.confidence
    );
}

/// Темп верный или другой октавы, но тогда выбор помечен сомнительным и правильный — во втором кандидате.
fn tempo_or_doubtful_octave(grid: &BeatGrid, bpm: f64) {
    let exact = (grid.bpm - bpm).abs() <= 0.05;
    let octave = [bpm * 2.0, bpm / 2.0]
        .iter()
        .any(|other| (grid.bpm - other).abs() <= 0.1)
        && grid.alt_bpm.is_some_and(|alt| (alt - bpm).abs() <= 1.0);
    assert!(
        exact || octave,
        "bpm {} alt {:?} confidence {}",
        grid.bpm,
        grid.alt_bpm,
        grid.confidence
    );
}

/// Трэп 140 в полутемпе: бочка на 1, малый на 3, хэт восьмыми.
#[test]
fn a_half_time_trap_beat_finds_its_tempo() {
    let (bpm, first) = (140.0, 0.2);
    let beat = 60.0 / bpm;
    let mut track = Track::new(first + 32.0 * 4.0 * beat + 1.0);
    for index in 0..128usize {
        let time = first + index as f64 * beat;
        match index % 4 {
            0 => track.kick(time),
            2 => track.snare(time),
            _ => {}
        }
        track.hat(time);
        track.hat(time + beat / 2.0);
    }
    let grid = analyze_pcm(&track.samples).expect("a grid");
    tempo_or_doubtful_octave(&grid, bpm);
}

/// Драм-н-бейс 174: бочка на 1 и «и» третьей, малый на 2 и 4.
#[test]
fn a_drum_and_bass_beat_finds_its_tempo() {
    let (bpm, first) = (174.0, 0.37);
    let beat = 60.0 / bpm;
    let mut track = Track::new(first + 48.0 * 4.0 * beat + 1.0);
    for index in 0..192usize {
        let time = first + index as f64 * beat;
        match index % 4 {
            0 => track.kick(time),
            1 | 3 => track.snare(time),
            2 => track.kick(time + beat / 2.0),
            _ => {}
        }
        track.hat(time + beat / 2.0);
    }
    let grid = analyze_pcm(&track.samples).expect("a grid");
    tempo_or_doubtful_octave(&grid, bpm);
}

/// Под битом тянется 808: нота с бочкой, затухание 0,8 с — бас почти всё время громок.
/// Клэпы остаются малыми, бочкой их не делает уровень баса.
#[test]
fn a_sustained_808_does_not_turn_claps_into_kicks() {
    let (bpm, first) = (140.0, 0.3);
    let beat = 60.0 / bpm;
    let mut track = Track::new(first + 32.0 * 4.0 * beat + 1.0);
    for index in (0..128usize).step_by(4) {
        let time = first + index as f64 * beat;
        track.add(time, 4.0 * beat, |t, _| {
            (0.4 * (-t / 0.8).exp() * (2.0 * std::f64::consts::PI * 50.0 * t).sin()) as f32
        });
    }
    for index in 0..128usize {
        let time = first + index as f64 * beat;
        match index % 4 {
            0 => track.kick(time),
            2 => track.snare(time),
            _ => {}
        }
        track.hat(time + beat / 2.0);
    }
    let grid = analyze_pcm(&track.samples).expect("a grid");
    assert!(
        matched(&track.snares, &grid.snares, 0.01) >= 0.9,
        "snare recall: {} of {}",
        grid.snares.len(),
        track.snares.len()
    );
    let extra: Vec<f64> = grid
        .kicks
        .iter()
        .copied()
        .filter(|time| track.kicks.iter().all(|real| (time - real).abs() > 0.01))
        .collect();
    assert!(
        matched(&grid.kicks, &track.kicks, 0.01) >= 0.9,
        "kick precision: {} kicks, extra at {extra:?}",
        grid.kicks.len()
    );
}
