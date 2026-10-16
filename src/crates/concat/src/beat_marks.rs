// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка долей на таймлайне: риски клипа для `ClipData` и ближайшая доля для прилипания.
//! Сетка лежит на медиа временем файла; здесь — перевод через окно и скорость клипа.

use concat_project::beats::{self, GridHead};
use concat_project::model::{Clip, MediaItem, Project};

/// Сколько дропов рисуется на клипе.
const DROPS: usize = 4;

/// Риски одного клипа: всё в секундах от начала клипа, как их рисует `lanes.slint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    /// Первая доля под клипом.
    pub first: f32,
    /// Шаг доли на таймлайне.
    pub step: f32,
    /// Сколько долей под клипом; 0 — сетки нет.
    pub count: i32,
    /// Номер первой сильной доли среди долей клипа.
    pub bar: i32,
    /// Долей в такте.
    pub bar_len: i32,
    /// Дропы под клипом; −1 — нет.
    pub drops: [f32; DROPS],
}

impl Default for Marks {
    fn default() -> Self {
        Marks {
            first: 0.0,
            step: 0.0,
            count: 0,
            bar: 0,
            bar_len: 4,
            drops: [-1.0; DROPS],
        }
    }
}

/// Окно клипа в секундах файла; у клипа с кривой скорости риски и прилипание не считаются.
fn window(clip: &Clip) -> Option<(f64, f64)> {
    if clip.speed_curve.is_some() || clip.speed <= 0.0 {
        return None;
    }
    let from = clip.source_start;
    Some((from, from + clip.duration * clip.speed))
}

/// Медиа клипа и голова его сетки, если сетка есть.
fn media_head<'a>(project: &'a Project, clip: &Clip) -> Option<(&'a MediaItem, GridHead)> {
    let media = project.media_by_id(&clip.media_id)?;
    let head = beats::head(media, clip.audio_stream)?;
    Some((media, head))
}

/// Риски клипа; у клипа без сетки (или с кривой скорости) — пусто.
pub fn marks(project: &Project, clip: &Clip) -> Marks {
    let Some((from, to)) = window(clip) else {
        return Marks::default();
    };
    let Some((media, head)) = media_head(project, clip) else {
        return Marks::default();
    };
    let beat = head.beat();
    let first_index = ((from - head.phase) / beat).ceil() as i64;
    let first_file = head.phase + first_index as f64 * beat;
    if first_file > to {
        return Marks::default();
    }
    let count = ((to - first_file) / beat).floor() as i64 + 1;
    let bar_len = i64::from(head.bar.max(1));
    let bar = (i64::from(head.downbeat) - first_index).rem_euclid(bar_len);
    let mut drops = [-1.0_f32; DROPS];
    let inside = beats::drops(media, clip.audio_stream)
        .into_iter()
        .filter(|drop| *drop >= from && *drop <= to)
        .take(DROPS);
    for (slot, drop) in drops.iter_mut().zip(inside) {
        *slot = ((drop - from) / clip.speed) as f32;
    }
    Marks {
        first: ((first_file - from) / clip.speed) as f32,
        step: (beat / clip.speed) as f32,
        count: i32::try_from(count).unwrap_or(i32::MAX),
        bar: bar as i32,
        bar_len: bar_len as i32,
        drops,
    }
}

/// Ближайшая к `time` доля клипа на таймлайне; только такты, если доли ближе `2 × pull`.
pub fn nearest(project: &Project, clip: &Clip, time: f64, pull: f64) -> Option<f64> {
    let (from, to) = window(clip)?;
    let file = from + (time - clip.start) * clip.speed;
    if file < from - pull * clip.speed || file > to + pull * clip.speed {
        return None;
    }
    let (_, head) = media_head(project, clip)?;
    let beat = head.beat();
    let mut index = ((file - head.phase) / beat).round() as i64;
    // Доли плотнее двойного притяжения: прилипать к каждой — не поставить ничего между ними.
    if beat / clip.speed < 2.0 * pull {
        let bar = i64::from(head.bar.max(1));
        let downbeat = i64::from(head.downbeat);
        index = downbeat + ((index - downbeat) as f64 / bar as f64).round() as i64 * bar;
    }
    let target = head.phase + index as f64 * beat;
    (target >= from && target <= to).then(|| clip.start + (target - from) / clip.speed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::Command;
    use concat_project::beats::{BeatGrid, VERSION};
    use concat_project::commands::NewMedia;
    use concat_project::model::MediaKind;

    /// Проект с музыкой 120 BPM (доля 0,5 с, такт с доли 1), клип с 3-й секунды файла на 10-й таймлайна.
    fn project() -> (concat_project::Editor, String) {
        let mut editor = concat_project::Editor::new();
        let media_id = editor
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: "/m.wav".to_owned(),
                    name: "m.wav".to_owned(),
                    duration: Some(60.0),
                    kind: MediaKind::Audio,
                    width: None,
                    height: None,
                    frame_rate: None,
                    frame_rate_fraction: None,
                    video_codec: None,
                    audio_codec: Some("pcm".to_owned()),
                    has_audio: true,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("media")
            .created_id
            .expect("id");
        editor
            .apply(Command::SetMediaBeats {
                media_id: media_id.clone(),
                stream: None,
                grid: Some(BeatGrid {
                    version: VERSION,
                    bpm: 120.0,
                    phase: 0.25,
                    bar: 4,
                    downbeat: 1,
                    duration: 60.0,
                    confidence: 2.0,
                    alt_bpm: None,
                    steady: true,
                    kicks: Vec::new(),
                    snares: Vec::new(),
                    drops: vec![5.25],
                }),
            })
            .expect("grid");
        let clip_id = editor
            .apply(Command::AddClipAtFirstFree {
                media_id,
                start: 10.0,
            })
            .expect("clip")
            .created_id
            .expect("id");
        editor
            .apply(Command::TrimClip {
                clip_id: clip_id.clone(),
                edge: concat_project::commands::TrimEdge::Start,
                delta: 3.0,
                ripple: false,
            })
            .expect("trim");
        (editor, clip_id)
    }

    #[test]
    fn marks_start_at_the_first_beat_under_the_clip() {
        let (editor, id) = project();
        let clip = editor.project().active().clip(&id).expect("clip");
        let marks = marks(editor.project(), clip);
        // Окно с 3,0 с файла: первая доля 3,25 (номер 6), такты на долях 1, 5, 9 → первый — 9-я (3,0 + …).
        assert!((marks.first - 0.25).abs() < 1e-6, "{marks:?}");
        assert!((marks.step - 0.5).abs() < 1e-6);
        assert_eq!(marks.bar, 3, "index 6 + 3 = 9 starts a bar");
        assert!((marks.drops[0] - 2.25).abs() < 1e-6, "{marks:?}");
        assert_eq!(marks.drops[1], -1.0);
    }

    #[test]
    fn the_playhead_pulls_to_beats_or_only_to_bars() {
        let (editor, id) = project();
        let clip = editor.project().active().clip(&id).expect("clip");
        let start = clip.start;
        let near = nearest(editor.project(), clip, start + 0.3, 0.1).expect("a beat");
        assert!((near - (start + 0.25)).abs() < 1e-9, "{near}");
        // Притяжение 0,3 с при доле 0,5: только такты (каждые 2 с, первый — 1,75 с от начала клипа).
        let bar = nearest(editor.project(), clip, start + 1.6, 0.3).expect("a bar");
        assert!((bar - (start + 1.75)).abs() < 1e-9, "{bar}");
    }
}
