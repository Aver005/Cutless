// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Пометки пауз: разрез клипа, отметка кусков, склейка обратно и подтверждение.
//!
//! Один шаг undo на команду. Пометка лежит в `clip.extra["cutless"]["pause"]`,
//! чтобы пережить сохранение и не трогать версию документа.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::placement::TOUCH;

use super::*;

/// Ключ объекта Cutless в `Clip::extra`.
const CUTLESS: &str = "cutless";
/// Ключ пометки паузы внутри объекта Cutless.
const PAUSE: &str = "pause";

/// Отрезок паузы в секундах таймлайна, абсолютных.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseRange {
    /// Начало отрезка.
    pub start: f64,
    /// Конец отрезка.
    pub end: f64,
}

/// Помечен ли клип как пауза.
pub fn is_pause(clip: &Clip) -> bool {
    clip.extra
        .get(CUTLESS)
        .and_then(|value| value.get(PAUSE))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Ставит пометку паузы. Возвращает true, если что-то изменилось.
fn set_pause(clip: &mut Clip) -> bool {
    if is_pause(clip) {
        return false;
    }
    let entry = clip
        .extra
        .entry(CUTLESS.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    if let Value::Object(map) = entry {
        map.insert(PAUSE.to_owned(), Value::Bool(true));
    }
    true
}

/// Снимает пометку паузы и чистит пустой объект Cutless.
fn clear_pause(clip: &mut Clip) -> bool {
    let Some(Value::Object(map)) = clip.extra.get_mut(CUTLESS) else {
        return false;
    };
    let removed = map.remove(PAUSE).is_some();
    if map.is_empty() {
        clip.extra.remove(CUTLESS);
    }
    removed
}

/// Применяет одну из команд этого модуля. Остальные сюда не маршрутизируются.
pub(super) fn apply(
    project: &mut Project,
    mint: &mut IdMint,
    command: Command,
) -> Result<Outcome, CommandError> {
    match command {
        Command::MarkPauses { clip_id, ranges } => mark_pauses(project, mint, &clip_id, &ranges),
        Command::SetClipPause { clip_ids, on } => set_pauses(project, &clip_ids, on),
        Command::RestorePauses { clip_ids } => restore_pauses(project, mint, &clip_ids),
        Command::ConfirmPauses { clip_ids } => confirm_pauses(project, clip_ids.as_deref()),
        _ => unreachable!("commands::apply routes only this module's commands here"),
    }
}

/// [`Command::MarkPauses`]: режет клип и его отсоединённый звук по границам
/// отрезков и помечает куски внутри них.
fn mark_pauses(
    project: &mut Project,
    mint: &mut IdMint,
    clip_id: &str,
    ranges: &[PauseRange],
) -> Result<Outcome, CommandError> {
    if ranges
        .iter()
        .any(|range| !range.start.is_finite() || !range.end.is_finite())
    {
        return Err(CommandError::NotANumber);
    }
    let timeline = project.active_mut();
    // Звук, отсоединённый от этого клипа, берём до разрезов.
    let sounds: Vec<String> = timeline
        .clips
        .iter()
        .filter(|clip| clip.detached_from.as_deref() == Some(clip_id))
        .map(|clip| clip.id.clone())
        .collect();
    if timeline.clip(clip_id).is_none() && sounds.is_empty() {
        return Ok(Outcome::default());
    }
    let mut created = None;
    let mut applied = false;
    if timeline.clip(clip_id).is_some() {
        let (changed, last) = cut_and_mark(timeline, mint, clip_id, ranges);
        applied |= changed;
        if last.is_some() {
            created = last;
        }
    }
    for sound in &sounds {
        let (changed, last) = cut_and_mark(timeline, mint, sound, ranges);
        applied |= changed;
        if last.is_some() {
            created = last;
        }
    }
    Ok(Outcome {
        created_id: created,
        applied,
    })
}

/// [`Command::SetClipPause`]: ставит или снимает пометку без разрезов.
fn set_pauses(
    project: &mut Project,
    clip_ids: &[String],
    on: bool,
) -> Result<Outcome, CommandError> {
    let timeline = project.active_mut();
    let mut applied = false;
    for clip_id in clip_ids {
        if let Some(clip) = timeline.clip_mut(clip_id) {
            applied |= if on {
                set_pause(clip)
            } else {
                clear_pause(clip)
            };
        }
    }
    Ok(Outcome {
        created_id: None,
        applied,
    })
}

/// [`Command::RestorePauses`]: снимает пометку и склеивает кусок с соседями,
/// которые вплотную продолжают тот же файл. Через чужие паузы не склеиваем.
fn restore_pauses(
    project: &mut Project,
    mint: &mut IdMint,
    clip_ids: &[String],
) -> Result<Outcome, CommandError> {
    let mut applied = false;
    {
        let timeline = project.active_mut();
        for clip_id in clip_ids {
            if let Some(clip) = timeline.clip_mut(clip_id) {
                applied |= clear_pause(clip);
            }
        }
    }
    // Непрерывные куски вокруг каждого имени ищем до склейки, чтобы id не сбились.
    let runs: Vec<Vec<String>> = {
        let timeline = project.active();
        let mut runs: Vec<Vec<String>> = Vec::new();
        for clip_id in clip_ids {
            if timeline.clip(clip_id).is_none() {
                continue;
            }
            let run = joinable_run(timeline, clip_id);
            let known = runs.iter().any(|existing| existing.contains(&run[0]));
            if run.len() >= 2 && !known {
                runs.push(run);
            }
        }
        runs
    };
    for run in runs {
        if why_not_merge(project.active(), &run).is_some() {
            continue;
        }
        let outcome = super::apply(project, mint, Command::MergeClips { clip_ids: run })?;
        applied |= outcome.applied;
    }
    Ok(Outcome {
        created_id: None,
        applied,
    })
}

/// [`Command::ConfirmPauses`]: удаляет помеченные куски и закрывает дыры на
/// всех дорожках, кроме музыки. None значит все помеченные на активном таймлайне.
fn confirm_pauses(
    project: &mut Project,
    clip_ids: Option<&[String]>,
) -> Result<Outcome, CommandError> {
    let timeline = project.active_mut();
    let doomed: HashSet<String> = match clip_ids {
        Some(ids) => ids
            .iter()
            .filter(|id| {
                timeline
                    .clip(id)
                    .is_some_and(|clip| is_pause(clip) && !is_music(clip))
            })
            .cloned()
            .collect(),
        None => timeline
            .clips
            .iter()
            .filter(|clip| is_pause(clip) && !is_music(clip))
            .map(|clip| clip.id.clone())
            .collect(),
    };
    if doomed.is_empty() {
        return Ok(Outcome::default());
    }
    // Отрезки времени, которые убираем, объединяем по всем дорожкам.
    let mut spans: Vec<(f64, f64)> = timeline
        .clips
        .iter()
        .filter(|clip| doomed.contains(&clip.id))
        .map(|clip| (clip.start, clip.start + clip.duration))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let spans = merge_spans(spans);

    let before = timeline.clips.len();
    timeline.clips.retain(|clip| !doomed.contains(&clip.id));
    let mut applied = timeline.clips.len() != before;

    // От последнего отрезка к первому, чтобы сдвиги не сбивали координаты.
    let mut extra: HashSet<String> = HashSet::new();
    for (start, end) in spans.iter().rev() {
        applied |= apply_span(timeline, *start, *end, &mut extra);
    }
    if !extra.is_empty() {
        let before = timeline.clips.len();
        timeline.clips.retain(|clip| !extra.contains(&clip.id));
        applied |= timeline.clips.len() != before;
    }
    Ok(Outcome {
        created_id: None,
        applied,
    })
}

/// Режет клип по границам отрезков от конца к началу и помечает куски внутри.
/// Возвращает (изменилось ли, последний созданный id).
fn cut_and_mark(
    timeline: &mut Timeline,
    mint: &mut IdMint,
    clip_id: &str,
    ranges: &[PauseRange],
) -> (bool, Option<String>) {
    let Some(clip) = timeline.clip(clip_id) else {
        return (false, None);
    };
    let clip_start = clip.start;
    let clip_end = clip.start + clip.duration;
    let clamped = clamp_ranges(ranges, clip_start, clip_end);
    if clamped.is_empty() {
        return (false, None);
    }
    // Границы строго внутри клипа, по убыванию: голова хранит id, хвост новый.
    let mut cuts: Vec<f64> = Vec::new();
    for range in &clamped {
        for at in [range.start, range.end] {
            if at > clip_start + MIN_CLIP_DURATION && at < clip_end - MIN_CLIP_DURATION {
                cuts.push(at);
            }
        }
    }
    cuts.sort_by(|a, b| b.total_cmp(a));
    cuts.dedup_by(|a, b| (*a - *b).abs() <= TOUCH);

    let mut created = None;
    let mut tails = Vec::new();
    for at in &cuts {
        if let Some(tail) = split_clip(timeline, mint, clip_id, *at) {
            created = Some(tail.clone());
            tails.push(tail);
        }
    }

    let mut marked = Vec::new();
    for id in std::iter::once(clip_id.to_owned()).chain(tails) {
        if let Some(piece) = timeline.clip(&id) {
            let end = piece.start + piece.duration;
            let inside = clamped
                .iter()
                .any(|range| piece.start >= range.start - TOUCH && end <= range.end + TOUCH);
            if inside {
                marked.push(id);
            }
        }
    }
    let mut applied = created.is_some();
    for id in marked {
        if let Some(piece) = timeline.clip_mut(&id) {
            applied |= set_pause(piece);
        }
    }
    (applied, created)
}

/// Режет клип в точке `at` (секунды таймлайна): голова хранит id, хвост новый.
/// Повторяет поведение [`Command::SplitClips`].
fn split_clip(
    timeline: &mut Timeline,
    mint: &mut IdMint,
    clip_id: &str,
    at: f64,
) -> Option<String> {
    let index = timeline.clips.iter().position(|clip| clip.id == clip_id)?;
    {
        // Кривая скорости не переживает разрез: обе половины идут к её среднему.
        let clip = timeline.clip_at_mut(index);
        let offset = at - clip.start;
        if offset > MIN_CLIP_DURATION
            && offset < clip.duration - MIN_CLIP_DURATION
            && clip.speed_curve.is_some()
        {
            clip.speed_curve = None;
        }
    }
    let clip: &Clip = &timeline.clips[index];
    let offset = at - clip.start;
    if offset <= MIN_CLIP_DURATION || offset >= clip.duration - MIN_CLIP_DURATION {
        return None;
    }
    let whole = clip.duration;
    let mut tail = clip.clone();
    tail.id = mint.next("c");
    tail.start = clip.start + offset;
    tail.duration = clip.duration - offset;
    tail.source_start = clip.source_start + offset * clip.speed;
    tail.transition_in = None;
    tail.fade_in = 0.0;
    tail.rewindow_keys(whole, offset, whole);
    let tail_id = tail.id.clone();
    let head = timeline.clip_at_mut(index);
    head.duration = offset;
    head.fade_out = 0.0;
    head.rewindow_keys(whole, 0.0, offset);
    timeline.clips.insert(index + 1, Arc::new(tail));
    Some(tail_id)
}

/// Обрезает отрезки по границам клипа, выбрасывает пустые, сортирует по началу.
fn clamp_ranges(ranges: &[PauseRange], clip_start: f64, clip_end: f64) -> Vec<PauseRange> {
    let mut out: Vec<PauseRange> = ranges
        .iter()
        .filter_map(|range| {
            let start = range.start.max(clip_start);
            let end = range.end.min(clip_end);
            (end - start > TOUCH).then_some(PauseRange { start, end })
        })
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// Фоновый трек: звук без `detached_from`. Его не режем и не двигаем.
fn is_music(clip: &Clip) -> bool {
    clip.kind == ClipKind::Audio && clip.detached_from.is_none()
}

/// Объединяет пересекающиеся и смежные отрезки (список уже отсортирован).
fn merge_spans(spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (start, end) in spans {
        match out.last_mut() {
            Some(last) if start <= last.1 + TOUCH => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Убирает отрезок `[start, end)` из дорожек: удаляет, укорачивает или сдвигает
/// влево. Музыку не трогает. Удалённые id копит в `doomed`.
fn apply_span(timeline: &mut Timeline, start: f64, end: f64, doomed: &mut HashSet<String>) -> bool {
    let len = end - start;
    if len <= 0.0 {
        return false;
    }
    let mut applied = false;
    for slot in timeline.clips.iter_mut() {
        if is_music(slot) || doomed.contains(&slot.id) {
            continue;
        }
        let clip = Arc::make_mut(slot);
        let clip_start = clip.start;
        let clip_end = clip.start + clip.duration;
        if clip_end <= start + TOUCH {
            continue;
        }
        if clip_start >= end - TOUCH {
            clip.start -= len;
            applied = true;
            continue;
        }
        if clip_start >= start - TOUCH && clip_end <= end + TOUCH {
            doomed.insert(clip.id.clone());
        } else if clip_start < start - TOUCH && clip_end > end + TOUCH {
            clip.duration -= len;
        } else if clip_start < start - TOUCH {
            clip.duration = start - clip_start;
        } else {
            clip.duration = clip_end - end;
            clip.start = start;
        }
        applied = true;
    }
    applied
}

/// Непрерывный ряд кусков вокруг клипа, которые можно склеить. Останавливается
/// на чужой паузе и на всём, что [`why_not_merge`] не примет.
fn joinable_run(timeline: &Timeline, clip_id: &str) -> Vec<String> {
    let mut run = vec![clip_id.to_owned()];
    while let Some(last) = run.last().and_then(|id| timeline.clip(id)) {
        let Some(next) = right_neighbour(timeline, last) else {
            break;
        };
        if timeline.clip(&next).is_some_and(is_pause) {
            break;
        }
        let mut candidate = run.clone();
        candidate.push(next.clone());
        if why_not_merge(timeline, &candidate).is_some() {
            break;
        }
        run.push(next);
    }
    while let Some(first) = run.first().and_then(|id| timeline.clip(id)) {
        let Some(prev) = left_neighbour(timeline, first) else {
            break;
        };
        if timeline.clip(&prev).is_some_and(is_pause) {
            break;
        }
        let mut candidate = vec![prev.clone()];
        candidate.extend(run.iter().cloned());
        if why_not_merge(timeline, &candidate).is_some() {
            break;
        }
        run.insert(0, prev);
    }
    run
}

/// Клип на той же дорожке, который начинается ровно там, где этот кончается.
fn right_neighbour(timeline: &Timeline, clip: &Clip) -> Option<String> {
    let end = clip.start + clip.duration;
    timeline.clips.iter().find_map(|other| {
        (other.id != clip.id
            && other.track_id == clip.track_id
            && (other.start - end).abs() <= TOUCH)
            .then(|| other.id.clone())
    })
}

/// Клип на той же дорожке, который кончается ровно там, где этот начинается.
fn left_neighbour(timeline: &Timeline, clip: &Clip) -> Option<String> {
    timeline.clips.iter().find_map(|other| {
        (other.id != clip.id
            && other.track_id == clip.track_id
            && (other.start + other.duration - clip.start).abs() <= TOUCH)
            .then(|| other.id.clone())
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::commands::NewMedia;
    use crate::editor::Editor;
    use crate::model::MediaKind;

    use super::*;

    fn video(path: &str, duration: f64, has_audio: bool) -> NewMedia {
        NewMedia {
            path: path.to_owned(),
            name: path.to_owned(),
            duration: Some(duration),
            kind: MediaKind::Video,
            width: Some(64),
            height: Some(64),
            frame_rate: Some(30.0),
            frame_rate_fraction: Some("30/1".to_owned()),
            video_codec: None,
            audio_codec: has_audio.then(|| "aac".to_owned()),
            has_audio,
            audio_tracks: Vec::new(),
            origin: None,
            color_space: Default::default(),
        }
    }

    /// Редактор с одним клипом `0..duration` на первой дорожке.
    fn fixture(path: &str, duration: f64, has_audio: bool) -> (Editor, String, String) {
        let mut editor = Editor::new();
        let media = editor
            .apply(Command::AddMedia {
                item: video(path, duration, has_audio),
            })
            .expect("media")
            .created_id
            .expect("id");
        let track = editor.project().active().tracks[0].id.clone();
        let clip = editor
            .apply(Command::AddClip {
                media_id: media,
                track_id: track.clone(),
                start: 0.0,
                ripple: false,
            })
            .expect("clip")
            .created_id
            .expect("id");
        (editor, track, clip)
    }

    /// Клипы дорожки, по порядку времени.
    fn lane(editor: &Editor, track: &str) -> Vec<Clip> {
        let mut clips: Vec<Clip> = editor
            .project()
            .active()
            .clips
            .iter()
            .filter(|clip| clip.track_id == track)
            .map(|clip| (**clip).clone())
            .collect();
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        clips
    }

    fn range(start: f64, end: f64) -> PauseRange {
        PauseRange { start, end }
    }

    #[test]
    fn marking_cuts_the_clip_and_marks_the_pauses() {
        let (mut editor, track, clip) = fixture("/a.mp4", 10.0, false);
        editor
            .apply(Command::MarkPauses {
                clip_id: clip.clone(),
                ranges: vec![range(2.0, 3.0), range(6.0, 7.0)],
            })
            .expect("marks");

        let pieces = lane(&editor, &track);
        let spans: Vec<(f64, f64)> = pieces.iter().map(|c| (c.start, c.duration)).collect();
        assert_eq!(
            spans,
            vec![(0.0, 2.0), (2.0, 1.0), (3.0, 3.0), (6.0, 1.0), (7.0, 3.0)]
        );
        let sources: Vec<f64> = pieces.iter().map(|c| c.source_start).collect();
        assert_eq!(sources, vec![0.0, 2.0, 3.0, 6.0, 7.0]);
        let paused: Vec<bool> = pieces.iter().map(is_pause).collect();
        assert_eq!(paused, vec![false, true, false, true, false]);
        assert_eq!(pieces[0].id, clip, "the head keeps the id");
    }

    #[test]
    fn marking_cuts_the_detached_sound_too() {
        let (mut editor, track, clip) = fixture("/a.mp4", 10.0, true);
        editor
            .apply(Command::DetachAudio {
                clip_id: clip.clone(),
            })
            .expect("detaches");
        editor
            .apply(Command::MarkPauses {
                clip_id: clip.clone(),
                ranges: vec![range(2.0, 3.0)],
            })
            .expect("marks");

        assert_eq!(lane(&editor, &track).len(), 3);
        let sounds: Vec<Clip> = {
            let mut sounds: Vec<Clip> = editor
                .project()
                .active()
                .clips
                .iter()
                .filter(|c| c.detached_from.as_deref() == Some(clip.as_str()))
                .map(|c| (**c).clone())
                .collect();
            sounds.sort_by(|a, b| a.start.total_cmp(&b.start));
            sounds
        };
        assert_eq!(sounds.len(), 3);
        let marked: Vec<&Clip> = sounds.iter().filter(|c| is_pause(c)).collect();
        assert_eq!(marked.len(), 1);
        assert_eq!((marked[0].start, marked[0].duration), (2.0, 1.0));
    }

    #[test]
    fn restoring_joins_the_pieces_back() {
        let (mut editor, track, clip) = fixture("/a.mp4", 10.0, false);
        editor
            .apply(Command::MarkPauses {
                clip_id: clip.clone(),
                ranges: vec![range(2.0, 3.0), range(6.0, 7.0)],
            })
            .expect("marks");

        let first = lane(&editor, &track)
            .into_iter()
            .find(|c| is_pause(c) && (c.start - 2.0).abs() < 1e-9)
            .expect("a pause piece")
            .id;
        editor
            .apply(Command::RestorePauses {
                clip_ids: vec![first],
            })
            .expect("restores");

        let pieces = lane(&editor, &track);
        assert_eq!(pieces.len(), 3);
        assert_eq!((pieces[0].start, pieces[0].duration), (0.0, 6.0));
        assert!(!is_pause(&pieces[0]));
        assert!(is_pause(&pieces[1]));

        let second = pieces
            .iter()
            .find(|c| is_pause(c))
            .expect("still marked")
            .id
            .clone();
        editor
            .apply(Command::RestorePauses {
                clip_ids: vec![second],
            })
            .expect("restores");
        let pieces = lane(&editor, &track);
        assert_eq!(pieces.len(), 1);
        assert_eq!((pieces[0].start, pieces[0].duration), (0.0, 10.0));
    }

    #[test]
    fn confirming_removes_pauses_and_closes_gaps() {
        let (mut editor, track, clip) = fixture("/a.mp4", 10.0, false);
        editor
            .apply(Command::MarkPauses {
                clip_id: clip.clone(),
                ranges: vec![range(2.0, 3.0), range(6.0, 7.0)],
            })
            .expect("marks");
        // Текст после второй паузы.
        editor
            .apply(Command::AddTextClip {
                track_id: None,
                above: true,
                start: 8.0,
                style: None,
                duration: Some(1.0),
                offset_y: None,
            })
            .expect("text after");
        // Текст внутри первой паузы.
        editor
            .apply(Command::AddTextClip {
                track_id: None,
                above: true,
                start: 2.4,
                style: None,
                duration: Some(0.4),
                offset_y: None,
            })
            .expect("text inside");
        // Фоновая музыка: звук без `detached_from`.
        let music_track = editor
            .apply(Command::AddTrack { bottom: false })
            .expect("track")
            .created_id
            .expect("id");
        let music = {
            let timeline = editor.project_mut().active_mut();
            let clip = Clip::blank("music1", music_track, ClipKind::Audio, "Music", 0.0, 5.0);
            timeline.clips.push(Arc::new(clip));
            "music1".to_owned()
        };

        let before = editor.project().active().duration();
        let outcome = editor
            .apply(Command::ConfirmPauses { clip_ids: None })
            .expect("confirms");
        assert!(outcome.applied);

        let pieces = lane(&editor, &track);
        assert_eq!(pieces.len(), 3);
        let end = pieces.last().map(|c| c.start + c.duration).expect("pieces");
        assert_eq!(end, 8.0);
        assert_eq!(before - editor.project().active().duration(), 2.0);

        let text: Vec<&Clip> = editor
            .project()
            .active()
            .clips
            .iter()
            .filter(|c| c.kind == ClipKind::Text)
            .map(Arc::as_ref)
            .collect();
        assert_eq!(text.len(), 1, "the text inside the pause is gone");
        assert_eq!(text[0].start, 6.0, "the text after it moved left");

        let music = editor.project().active().clip(&music).expect("music");
        assert!(is_music(music));
        assert_eq!((music.start, music.duration), (0.0, 5.0));
    }

    #[test]
    fn commands_serialise_with_their_op() {
        let mark = Command::MarkPauses {
            clip_id: "c1".to_owned(),
            ranges: vec![range(1.0, 2.0)],
        };
        let value = serde_json::to_value(&mark).expect("writes");
        assert_eq!(value["op"], "markPauses");
        assert_eq!(value["clipId"], "c1");
        assert_eq!(value["ranges"][0]["start"], 1.0);
        assert_eq!(value["ranges"][0]["end"], 2.0);

        let set = Command::SetClipPause {
            clip_ids: vec!["c1".to_owned()],
            on: true,
        };
        assert_eq!(
            serde_json::to_value(&set).expect("writes")["op"],
            "setClipPause"
        );

        let restore = Command::RestorePauses {
            clip_ids: vec!["c1".to_owned()],
        };
        assert_eq!(
            serde_json::to_value(&restore).expect("writes")["op"],
            "restorePauses"
        );

        let confirm = Command::ConfirmPauses { clip_ids: None };
        let value = serde_json::to_value(&confirm).expect("writes");
        assert_eq!(value["op"], "confirmPauses");
        assert!(value["clipIds"].is_null());

        let parsed: Command = serde_json::from_value(json!({
            "op": "markPauses",
            "clipId": "c2",
            "ranges": [{ "start": 0.5, "end": 1.5 }]
        }))
        .expect("parses");
        assert_eq!(
            parsed,
            Command::MarkPauses {
                clip_id: "c2".to_owned(),
                ranges: vec![range(0.5, 1.5)],
            }
        );
    }
}
