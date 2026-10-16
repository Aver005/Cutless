// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Биквадные фильтры 2-го порядка (рецепты RBJ): полосы для огибающих без FFT.

use std::f64::consts::PI;

/// Один биквад: коэффициенты и память двух прошлых отсчётов.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

/// Добротность Баттерворта для 2-го порядка.
const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

impl Biquad {
    /// Фильтр нижних частот: срез `cutoff` Гц при частоте `rate`, добротность `q`.
    pub fn low_pass(cutoff: f64, rate: f64, q: f64) -> Biquad {
        let (cos, alpha) = Biquad::angles(cutoff, rate, q);
        Biquad::normalised(
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    /// Фильтр верхних частот: срез `cutoff` Гц при частоте `rate`, добротность `q`.
    pub fn high_pass(cutoff: f64, rate: f64, q: f64) -> Biquad {
        let (cos, alpha) = Biquad::angles(cutoff, rate, q);
        Biquad::normalised(
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    /// Пропускает один отсчёт.
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn angles(cutoff: f64, rate: f64, q: f64) -> (f64, f64) {
        // Срез не выше 0,45 частоты дискретизации: у самого Найквиста рецепт вырождается.
        let cutoff = cutoff.clamp(1.0, rate * 0.45);
        let omega = 2.0 * PI * cutoff / rate;
        (omega.cos(), omega.sin() / (2.0 * q))
    }

    fn normalised(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Biquad {
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

/// Добротности ступеней Баттерворта: 2-й порядок — одна, 4-й — две.
fn butterworth(order: usize) -> &'static [f64] {
    if order >= 4 {
        &BUTTERWORTH_4
    } else {
        &[BUTTERWORTH_Q]
    }
}

/// Добротности двух биквадов, дающих Баттерворт 4-го порядка.
const BUTTERWORTH_4: [f64; 2] = [0.541_196_1, 1.306_563];

/// Полоса частот: каскад ВЧ и НЧ заданных порядков (2 или 4); без среза — `None`.
#[derive(Clone, Debug)]
pub struct Band {
    stages: Vec<Biquad>,
}

impl Band {
    /// Полоса: ВЧ на `low` Гц порядка `low_order`, НЧ на `high` Гц порядка `high_order`.
    pub fn new(low: Option<(f64, usize)>, high: Option<(f64, usize)>, rate: f64) -> Band {
        let mut stages = Vec::new();
        if let Some((cutoff, order)) = low {
            stages.extend(
                butterworth(order)
                    .iter()
                    .map(|q| Biquad::high_pass(cutoff, rate, *q)),
            );
        }
        if let Some((cutoff, order)) = high {
            stages.extend(
                butterworth(order)
                    .iter()
                    .map(|q| Biquad::low_pass(cutoff, rate, *q)),
            );
        }
        Band { stages }
    }

    /// Пропускает один отсчёт через все ступени.
    pub fn process(&mut self, x: f64) -> f64 {
        self.stages
            .iter_mut()
            .fold(x, |signal, stage| stage.process(signal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 16_000.0;

    /// Средняя амплитуда синуса `hz` после полосы, на установившемся участке.
    fn gain(band: &mut Band, hz: f64) -> f64 {
        let n = 16_000;
        let mut sum = 0.0;
        for i in 0..n {
            let x = (2.0 * PI * hz * i as f64 / RATE).sin();
            let y = band.process(x);
            if i > n / 2 {
                sum += y.abs();
            }
        }
        sum / (n / 2) as f64 / (2.0 / PI)
    }

    #[test]
    fn a_band_passes_its_middle_and_stops_its_edges() {
        let kick = || Band::new(Some((30.0, 2)), Some((150.0, 4)), RATE);
        assert!(gain(&mut kick(), 70.0) > 0.8);
        assert!(
            gain(&mut kick(), 250.0) < 0.2,
            "a snare's body is held down"
        );
        assert!(gain(&mut kick(), 2_000.0) < 0.001);
        let snare = || Band::new(Some((1_200.0, 4)), Some((6_000.0, 2)), RATE);
        assert!(gain(&mut snare(), 3_000.0) > 0.8);
        assert!(gain(&mut snare(), 100.0) < 0.001);
    }
}
