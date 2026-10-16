// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Пресеты кривой скорости для эдитов: замедление на ударе, рывок между ними.
//! Точки — доли длины клипа и скорость там; движок соединяет их прямыми.

use concat_project::model::SpeedPoint;

/// Готовая кривая скорости.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedPreset {
    /// Медленно, рывок в середине, снова медленно.
    Velocity,
    /// Быстро и медленно по очереди, дважды.
    Montage,
    /// Быстро, долгий слоу-мо в середине, быстро.
    Hero,
    /// Как Hero, но замедление почти до стоп-кадра.
    Bullet,
    /// Четыре резкие ступени: быстро, медленно, быстро, медленно.
    JumpCut,
    /// Влёт на большой скорости и плавный выход к обычной.
    FlashIn,
    /// Обычная скорость и разгон в конце.
    FlashOut,
    /// Разгон к удару, слоу-мо на нём, возврат к обычной.
    RampToHit,
}

impl SpeedPreset {
    /// Все пресеты в порядке кнопок.
    pub const ALL: [SpeedPreset; 8] = [
        SpeedPreset::Velocity,
        SpeedPreset::Montage,
        SpeedPreset::Hero,
        SpeedPreset::Bullet,
        SpeedPreset::JumpCut,
        SpeedPreset::FlashIn,
        SpeedPreset::FlashOut,
        SpeedPreset::RampToHit,
    ];

    /// Имя для агента и интерфейса, `snake_case`.
    pub fn name(self) -> &'static str {
        match self {
            SpeedPreset::Velocity => "velocity",
            SpeedPreset::Montage => "montage",
            SpeedPreset::Hero => "hero",
            SpeedPreset::Bullet => "bullet",
            SpeedPreset::JumpCut => "jump_cut",
            SpeedPreset::FlashIn => "flash_in",
            SpeedPreset::FlashOut => "flash_out",
            SpeedPreset::RampToHit => "ramp_to_hit",
        }
    }

    /// Пресет по имени.
    pub fn from_name(name: &str) -> Option<SpeedPreset> {
        SpeedPreset::ALL
            .into_iter()
            .find(|preset| preset.name() == name)
    }

    /// Пресет, который дал эти точки (рампа — с ударом посередине), если такой есть.
    pub fn matching(points: &[SpeedPoint]) -> Option<SpeedPreset> {
        let same = |a: &[SpeedPoint], b: &[SpeedPoint]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| (x.at - y.at).abs() < 1e-6 && (x.speed - y.speed).abs() < 1e-6)
        };
        SpeedPreset::ALL
            .into_iter()
            .find(|preset| same(&preset.points(0.5), points))
    }

    /// Точки кривой. `hit` — доля клипа, где удар; нужна только `RampToHit`.
    pub fn points(self, hit: f64) -> Vec<SpeedPoint> {
        let table: &[(f64, f64)] = match self {
            SpeedPreset::Velocity => &[
                (0.0, 0.5),
                (0.35, 0.5),
                (0.45, 3.0),
                (0.55, 3.0),
                (0.65, 0.5),
                (1.0, 0.5),
            ],
            SpeedPreset::Montage => &[
                (0.0, 2.0),
                (0.2, 2.0),
                (0.3, 0.5),
                (0.45, 0.5),
                (0.55, 2.0),
                (0.7, 2.0),
                (0.8, 0.5),
                (1.0, 0.5),
            ],
            SpeedPreset::Hero => &[
                (0.0, 2.5),
                (0.3, 2.5),
                (0.4, 0.3),
                (0.6, 0.3),
                (0.7, 2.5),
                (1.0, 2.5),
            ],
            SpeedPreset::Bullet => &[
                (0.0, 3.0),
                (0.35, 3.0),
                (0.4, 0.15),
                (0.65, 0.15),
                (0.7, 3.0),
                (1.0, 3.0),
            ],
            SpeedPreset::JumpCut => &[
                (0.0, 3.0),
                (0.24, 3.0),
                (0.25, 0.4),
                (0.49, 0.4),
                (0.5, 3.0),
                (0.74, 3.0),
                (0.75, 0.4),
                (1.0, 0.4),
            ],
            SpeedPreset::FlashIn => &[(0.0, 5.0), (0.25, 1.0), (1.0, 1.0)],
            SpeedPreset::FlashOut => &[(0.0, 1.0), (0.75, 1.0), (1.0, 5.0)],
            SpeedPreset::RampToHit => return ramp_to_hit(hit),
        };
        table
            .iter()
            .map(|&(at, speed)| SpeedPoint { at, speed })
            .collect()
    }
}

/// Где поставить пик `RampToHit`, чтобы на него пришлась доля `share` пройденного исходника.
/// Сама кривая меняет, сколько исходника пройдено к пику, поэтому ищем делением пополам.
pub fn ramp_hit_for_share(share: f64) -> f64 {
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let covered = |hit: f64| {
        let points = ramp_to_hit(hit);
        area(&points, hit) / area(&points, 1.0)
    };
    let (mut low, mut high) = (RAMP_HIT_MIN, RAMP_HIT_MAX);
    for _ in 0..40 {
        let middle = (low + high) / 2.0;
        if covered(middle) < share {
            low = middle;
        } else {
            high = middle;
        }
    }
    (low + high) / 2.0
}

/// Сколько исходника пройдено к доле `to` клипа: площадь под кривой скорости.
fn area(points: &[SpeedPoint], to: f64) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let end = b.at.min(to);
            if end <= a.at {
                return 0.0;
            }
            let speed_at = |x: f64| a.speed + (b.speed - a.speed) * (x - a.at) / (b.at - a.at);
            (end - a.at) * (a.speed + speed_at(end)) / 2.0
        })
        .sum()
}

/// Самое раннее и самое позднее место удара в клипе: разгону и возврату нужно место.
const RAMP_HIT_MIN: f64 = 0.2;
const RAMP_HIT_MAX: f64 = 0.8;

/// Разгон к удару и слоу-мо на нём; удар держится внутри клипа.
fn ramp_to_hit(hit: f64) -> Vec<SpeedPoint> {
    let hit = if hit.is_finite() { hit } else { 0.5 };
    let hit = hit.clamp(RAMP_HIT_MIN, RAMP_HIT_MAX);
    [
        (0.0, 1.0),
        (hit - 0.15, 2.5),
        (hit, 0.3),
        (hit + 0.1, 0.3),
        ((hit + 0.2).min(1.0), 1.0),
        (1.0, 1.0),
    ]
    .into_iter()
    .fold(Vec::new(), |mut points: Vec<SpeedPoint>, (at, speed)| {
        // Совпавшая по месту точка заменяет прежнюю, чтобы кривая шла строго вперёд.
        if points.last().is_some_and(|last| at - last.at < 1e-9) {
            points.pop();
        }
        points.push(SpeedPoint { at, speed });
        points
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Минимум и максимум скорости движка.
    const MIN: f64 = 0.0625;
    const MAX: f64 = 16.0;

    #[test]
    fn every_preset_is_a_valid_curve() {
        for preset in SpeedPreset::ALL {
            for hit in [0.0, 0.5, 1.0, f64::NAN] {
                let points = preset.points(hit);
                assert!(points.len() >= 2, "{preset:?}");
                assert_eq!(points.first().map(|p| p.at), Some(0.0), "{preset:?}");
                assert_eq!(points.last().map(|p| p.at), Some(1.0), "{preset:?}");
                for pair in points.windows(2) {
                    assert!(pair[1].at > pair[0].at, "{preset:?} goes back at {hit}");
                }
                for point in &points {
                    assert!((MIN..=MAX).contains(&point.speed), "{preset:?}");
                }
            }
        }
    }

    #[test]
    fn names_round_trip() {
        for preset in SpeedPreset::ALL {
            assert_eq!(SpeedPreset::from_name(preset.name()), Some(preset));
            assert_eq!(SpeedPreset::matching(&preset.points(0.5)), Some(preset));
        }
        assert_eq!(SpeedPreset::matching(&[]), None);
        assert_eq!(SpeedPreset::from_name("warp"), None);
    }

    #[test]
    fn the_ramp_peak_lands_on_the_asked_share_of_the_source() {
        for share in [0.3, 0.5, 0.7] {
            let hit = ramp_hit_for_share(share);
            let points = SpeedPreset::RampToHit.points(hit);
            let covered = area(&points, hit) / area(&points, 1.0);
            assert!((covered - share).abs() < 1e-6, "{share} -> {covered}");
        }
    }

    #[test]
    fn the_ramp_slows_down_on_the_hit() {
        let points = SpeedPreset::RampToHit.points(0.6);
        let at_hit = points
            .iter()
            .find(|point| (point.at - 0.6).abs() < 1e-9)
            .expect("a point on the hit");
        assert!(at_hit.speed < 1.0);
    }
}
