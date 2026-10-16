// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка долей файла: лежит на медиа (`extra["cutless"]["beats"]`) по звуковому потоку,
//! временем файла, и переводится на таймлайн через окно и скорость клипа.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::cutless;
use crate::model::{Clip, MediaItem};

/// Версия формата сетки.
pub const VERSION: u32 = 1;
/// Поле Cutless у медиа: сетки по потокам.
const FIELD: &str = "beats";

/// Сетка долей одного звукового потока файла. Все времена — секунды файла.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatGrid {
    /// Версия формата.
    pub version: u32,
    /// Долей в минуту.
    pub bpm: f64,
    /// Секунда файла, на которую падает доля с номером 0.
    pub phase: f64,
    /// Долей в такте.
    pub bar: u32,
    /// Номер доли (по модулю `bar`), с которой начинается такт.
    pub downbeat: u32,
    /// Длина разобранного звука, секунды.
    pub duration: f64,
    /// Насколько темп надёжнее ближайшего соперника (×2, ×½…): меньше 1,15 — сомнительно.
    #[serde(default)]
    pub confidence: f64,
    /// Второй по силе темп, когда выбор сомнителен.
    #[serde(default)]
    pub alt_bpm: Option<f64>,
    /// Темп ровный по всему треку (доли ложатся на сетку точнее 20 мс).
    #[serde(default)]
    pub steady: bool,
    /// Удары бочки.
    #[serde(default)]
    pub kicks: Vec<f64>,
    /// Удары малого.
    #[serde(default)]
    pub snares: Vec<f64>,
    /// Дропы — начала громких частей после тихих.
    #[serde(default)]
    pub drops: Vec<f64>,
}

/// Что выбрать из сетки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Каждая доля.
    Beats,
    /// Сильные доли — начала тактов.
    Bars,
    /// Удары бочки.
    Kicks,
    /// Удары малого.
    Snares,
    /// Дропы.
    Drops,
}

impl Pick {
    /// Все виды в порядке, в котором их перечисляют инструменты.
    pub const ALL: [Pick; 5] = [
        Pick::Beats,
        Pick::Bars,
        Pick::Kicks,
        Pick::Snares,
        Pick::Drops,
    ];

    /// Имя для агента.
    pub fn name(self) -> &'static str {
        match self {
            Pick::Beats => "beats",
            Pick::Bars => "bars",
            Pick::Kicks => "kicks",
            Pick::Snares => "snares",
            Pick::Drops => "drops",
        }
    }

    /// Вид по имени.
    pub fn from_name(name: &str) -> Option<Pick> {
        Pick::ALL.into_iter().find(|pick| pick.name() == name)
    }
}

impl BeatGrid {
    /// Длина доли, секунды.
    pub fn beat(&self) -> f64 {
        60.0 / self.bpm.max(1.0)
    }

    /// Номер ближайшей доли к секунде файла `time`.
    pub fn index_at(&self, time: f64) -> i64 {
        ((time - self.phase) / self.beat()).round() as i64
    }

    /// Начинается ли такт с доли `index`.
    pub fn is_bar(&self, index: i64) -> bool {
        let bar = i64::from(self.bar.max(1));
        (index - i64::from(self.downbeat)).rem_euclid(bar) == 0
    }

    /// Секунды файла выбранного вида на отрезке `from..=to`, по возрастанию.
    pub fn pick(&self, pick: Pick, from: f64, to: f64) -> Vec<f64> {
        let inside = |time: &f64| *time >= from && *time <= to;
        match pick {
            Pick::Kicks => self.kicks.iter().copied().filter(inside).collect(),
            Pick::Snares => self.snares.iter().copied().filter(inside).collect(),
            Pick::Drops => self.drops.iter().copied().filter(inside).collect(),
            Pick::Beats | Pick::Bars => {
                let beat = self.beat();
                let first = ((from - self.phase) / beat).ceil() as i64;
                let last = ((to - self.phase) / beat).floor() as i64;
                (first..=last)
                    .filter(|index| pick == Pick::Beats || self.is_bar(*index))
                    .map(|index| self.phase + index as f64 * beat)
                    .collect()
            }
        }
    }

    /// Сетка годится: все числа конечны, темп и такт положительны. Иначе команда отклоняется.
    pub fn is_sane(&self) -> bool {
        self.bpm > 0.0 && self.bar > 0 && self.is_finite()
    }

    /// Все числа сетки конечны.
    fn is_finite(&self) -> bool {
        [self.bpm, self.phase, self.duration, self.confidence]
            .into_iter()
            .chain(self.alt_bpm)
            .chain(self.kicks.iter().copied())
            .chain(self.snares.iter().copied())
            .chain(self.drops.iter().copied())
            .all(f64::is_finite)
    }
}

/// Ключ потока в сетках медиа: явный поток или первый звуковой файла. У файла без описанных
/// дорожек (старый проект) все клипы делят одну сетку: декодер и так берёт первый поток.
pub fn stream_key(media: &MediaItem, stream: Option<u32>) -> String {
    if media.audio_tracks.is_empty() {
        return "default".to_owned();
    }
    match stream.or_else(|| media.audio_tracks.first().map(|track| track.index)) {
        Some(index) => format!("s{index}"),
        None => "default".to_owned(),
    }
}

/// Сетка потока `stream` файла, если её считали.
pub fn grid(media: &MediaItem, stream: Option<u32>) -> Option<BeatGrid> {
    let grids = cutless::get_in(&media.extra, FIELD)?;
    let value = grids.get(stream_key(media, stream))?;
    serde_json::from_value(value.clone()).ok()
}

/// Кладёт или убирает сетку потока. Возвращает true, если что-то изменилось.
pub fn set_grid(media: &mut MediaItem, stream: Option<u32>, grid: Option<&BeatGrid>) -> bool {
    let key = stream_key(media, stream);
    let mut grids = match cutless::get_in(&media.extra, FIELD) {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    };
    let changed = match grid.and_then(|grid| serde_json::to_value(grid).ok()) {
        Some(value) => grids.insert(key, value.clone()) != Some(value),
        None => grids.remove(&key).is_some(),
    };
    if !changed {
        return false;
    }
    if grids.is_empty() {
        cutless::remove_in(&mut media.extra, FIELD);
    } else {
        cutless::set_in(&mut media.extra, FIELD, Value::Object(grids));
    }
    true
}

/// Голова сетки: темп, фаза, такт и сильная доля, без списков ударов. Читается из документа
/// по полям, без разбора массивов — для рисок и прилипания на каждом движении мыши.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridHead {
    /// Долей в минуту.
    pub bpm: f64,
    /// Секунда файла доли с номером 0.
    pub phase: f64,
    /// Долей в такте.
    pub bar: u32,
    /// Номер доли, с которой начинается такт.
    pub downbeat: u32,
}

impl GridHead {
    /// Длина доли, секунды.
    pub fn beat(&self) -> f64 {
        60.0 / self.bpm.max(1.0)
    }

    /// Начинается ли такт с доли `index`.
    pub fn is_bar(&self, index: i64) -> bool {
        let bar = i64::from(self.bar.max(1));
        (index - i64::from(self.downbeat)).rem_euclid(bar) == 0
    }
}

/// Голова сетки потока, если сетка есть.
pub fn head(media: &MediaItem, stream: Option<u32>) -> Option<GridHead> {
    let grid = cutless::get_in(&media.extra, FIELD)?.get(stream_key(media, stream))?;
    let number = |field: &str| grid.get(field).and_then(Value::as_f64);
    let count = |field: &str| {
        grid.get(field)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    let head = GridHead {
        bpm: number("bpm")?,
        phase: number("phase")?,
        bar: count("bar")?,
        downbeat: count("downbeat")?,
    };
    (head.bpm.is_finite() && head.bpm > 0.0 && head.phase.is_finite()).then_some(head)
}

/// Дропы потока, секунды файла.
pub fn drops(media: &MediaItem, stream: Option<u32>) -> Vec<f64> {
    cutless::get_in(&media.extra, FIELD)
        .and_then(|grids| grids.get(stream_key(media, stream)))
        .and_then(|grid| grid.get("drops"))
        .and_then(Value::as_array)
        .map(|drops| drops.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

/// Где на таймлайне звучит секунда файла `file` этого клипа; `None` вне его окна.
pub fn file_to_timeline(clip: &Clip, file: f64) -> Option<f64> {
    let offset = file - clip.source_start;
    let covered = clip.duration * clip.speed;
    if !offset.is_finite() || offset < -1e-9 || offset > covered + 1e-9 || clip.duration <= 0.0 {
        return None;
    }
    let curve = clip.speed_curve.as_deref().and_then(crate::speed::curve_of);
    let Some(curve) = curve else {
        return Some(clip.start + offset / clip.speed.max(f64::EPSILON));
    };
    // Обратная кривая: `consumed` растёт монотонно, поэтому делим пополам.
    let target = offset / clip.duration;
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    for _ in 0..50 {
        let middle = (low + high) / 2.0;
        if curve.consumed(middle) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(clip.start + (low + high) / 2.0 * clip.duration)
}

/// Какая секунда файла звучит в момент таймлайна `time` у этого клипа; `None` вне клипа.
pub fn timeline_to_file(clip: &Clip, time: f64) -> Option<f64> {
    let x = (time - clip.start) / clip.duration;
    if !x.is_finite() || !(-1e-9..=1.0 + 1e-9).contains(&x) {
        return None;
    }
    let x = x.clamp(0.0, 1.0);
    let consumed = match clip.speed_curve.as_deref().and_then(crate::speed::curve_of) {
        Some(curve) => curve.consumed(x) * clip.duration,
        None => x * clip.duration * clip.speed,
    };
    Some(clip.source_start + consumed)
}

/// Моменты выбранного вида на таймлайне под этим клипом, по возрастанию.
pub fn timeline_picks(grid: &BeatGrid, clip: &Clip, pick: Pick) -> Vec<f64> {
    let (Some(from), Some(to)) = (
        timeline_to_file(clip, clip.start),
        timeline_to_file(clip, clip.start + clip.duration),
    ) else {
        return Vec::new();
    };
    grid.pick(pick, from, to)
        .into_iter()
        .filter_map(|file| file_to_timeline(clip, file))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClipKind, SpeedPoint};

    fn grid() -> BeatGrid {
        BeatGrid {
            version: VERSION,
            bpm: 120.0,
            phase: 0.25,
            bar: 4,
            downbeat: 1,
            duration: 10.0,
            confidence: 2.0,
            alt_bpm: None,
            steady: true,
            kicks: vec![0.75, 1.75, 2.75],
            snares: vec![1.25],
            drops: vec![4.75],
        }
    }

    fn clip() -> Clip {
        let mut clip = Clip::blank(
            "c1".to_owned(),
            "T1".to_owned(),
            ClipKind::Audio,
            "music".to_owned(),
            10.0,
            4.0,
        );
        clip.source_start = 1.0;
        clip
    }

    #[test]
    fn beats_and_bars_follow_the_phase_and_the_downbeat() {
        let grid = grid();
        assert_eq!(grid.pick(Pick::Beats, 0.0, 1.5), vec![0.25, 0.75, 1.25]);
        assert_eq!(grid.pick(Pick::Bars, 0.0, 5.0), vec![0.75, 2.75, 4.75]);
        assert!(grid.is_bar(-3));
        assert_eq!(grid.pick(Pick::Kicks, 1.0, 3.0), vec![1.75, 2.75]);
    }

    #[test]
    fn file_seconds_land_on_the_timeline_through_the_window_and_speed() {
        let mut clip = clip();
        assert_eq!(file_to_timeline(&clip, 1.5), Some(10.5));
        assert_eq!(file_to_timeline(&clip, 0.5), None, "before the window");
        clip.speed = 2.0;
        clip.duration = 2.0;
        assert_eq!(file_to_timeline(&clip, 3.0), Some(11.0));
        assert_eq!(timeline_to_file(&clip, 11.0), Some(3.0));
        let bars = timeline_picks(&grid(), &clip, Pick::Bars);
        assert_eq!(bars, vec![10.875, 11.875]);
    }

    #[test]
    fn a_speed_curve_maps_both_ways() {
        let mut clip = clip();
        clip.speed_curve = Some(vec![
            SpeedPoint {
                at: 0.0,
                speed: 0.5,
            },
            SpeedPoint {
                at: 1.0,
                speed: 1.5,
            },
        ]);
        for time in [10.0, 11.3, 12.9, 14.0] {
            let file = timeline_to_file(&clip, time).expect("inside");
            let back = file_to_timeline(&clip, file).expect("inside");
            assert!((back - time).abs() < 1e-6, "{time} -> {file} -> {back}");
        }
    }

    #[test]
    fn grids_sit_on_the_media_by_stream() {
        let track = |index: u32| crate::model::AudioTrack {
            index,
            codec: "aac".to_owned(),
            channels: 2,
            sample_rate: 48_000,
            title: String::new(),
            language: String::new(),
        };
        let mut media = MediaItem {
            audio_tracks: vec![track(1), track(2)],
            ..MediaItem::default()
        };
        assert!(set_grid(&mut media, Some(2), Some(&grid())));
        assert!(
            !set_grid(&mut media, Some(2), Some(&grid())),
            "the same grid again"
        );
        assert_eq!(grid_of(&media, Some(2)), Some(grid()));
        assert_eq!(grid_of(&media, Some(1)), None);
        let head = head(&media, Some(2)).expect("a head");
        assert_eq!((head.bpm, head.bar, head.downbeat), (120.0, 4, 1));
        assert_eq!(drops(&media, Some(2)), vec![4.75]);
        assert!(set_grid(&mut media, Some(2), None));
        assert!(media.extra.is_empty());
        // Без описанных дорожек клип видео и его отделённый звук делят одну сетку.
        let mut old = MediaItem::default();
        assert!(set_grid(&mut old, None, Some(&grid())));
        assert_eq!(grid_of(&old, Some(1)), Some(grid()));
    }

    fn grid_of(media: &MediaItem, stream: Option<u32>) -> Option<BeatGrid> {
        super::grid(media, stream)
    }
}
