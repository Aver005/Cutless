// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Список «Ход» на вкладке скорости: строка 0 — ровная скорость, дальше пресеты
//! `cutless-motion` по порядку, последняя — своя кривая клипа.

use concat_project::model::SpeedPoint;
use cutless_motion::SpeedPreset;

/// Строка списка для кривой клипа.
pub fn row_of(curve: Option<&[SpeedPoint]>) -> i32 {
    let Some(points) = curve else {
        return 0;
    };
    let row = match SpeedPreset::matching(points) {
        Some(preset) => SpeedPreset::ALL
            .iter()
            .position(|known| *known == preset)
            .map_or(SpeedPreset::ALL.len() + 1, |index| index + 1),
        None => SpeedPreset::ALL.len() + 1,
    };
    i32::try_from(row).unwrap_or(i32::MAX)
}

/// Кривая для выбранной строки; строка «своя» ничего не меняет — `None`.
pub fn curve_of(row: usize) -> Option<Option<Vec<SpeedPoint>>> {
    match row {
        0 => Some(None),
        row => SpeedPreset::ALL
            .get(row - 1)
            .map(|preset| Some(preset.points(0.5))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_row_round_trips() {
        assert_eq!(row_of(None), 0);
        assert_eq!(curve_of(0), Some(None));
        for row in 1..=SpeedPreset::ALL.len() {
            let curve = curve_of(row).flatten().expect("a preset curve");
            assert_eq!(row_of(Some(&curve)), row as i32);
        }
        let own = [
            SpeedPoint {
                at: 0.0,
                speed: 1.0,
            },
            SpeedPoint {
                at: 1.0,
                speed: 7.0,
            },
        ];
        assert_eq!(row_of(Some(&own)), SpeedPreset::ALL.len() as i32 + 1);
        assert_eq!(
            curve_of(SpeedPreset::ALL.len() + 1),
            None,
            "the own row keeps the curve"
        );
    }
}
