// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка долей для агента: `beats_analyze` разбирает трек фоновой задачей и кладёт сетку на медиа,
//! `beats_get` отдаёт доли, такты, бочки, малые и дропы секундами таймлайна.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use concat_project::Command;
use concat_project::beats::{self, BeatGrid, Pick};
use concat_project::model::{Clip, Project};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::edit::support;
use crate::jobs::{JobKind, JobState, WAIT_MAX, wait_from};
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Больше моментов в одном ответе `beats_get` не отдаётся.
const MAX_TIMES: usize = 200;

/// Регистрирует инструменты сетки долей.
pub fn register(registry: &mut Registry) {
    let picks: Vec<&str> = Pick::ALL.iter().map(|pick| pick.name()).collect();
    support::add(
        registry,
        "beats_analyze",
        "Find a music clip's beat grid: tempo, bars, kick and snare hits and drops, from the whole \
         file. Runs as a background job; a short track answers at once, a long one with a job id \
         for job_status - call beats_analyze again afterwards to store the result. bpm_hint \
         settles a doubtful tempo (half or double).",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string", "description": "The music clip." },
                "bpm_hint": { "type": "number", "description": "The tempo you or the person know, to pick between half and double." },
                "again": { "type": "boolean", "description": "Analyse again even when a grid is stored." },
                "wait_seconds": { "type": "number", "description": format!("How long to wait for the analysis; 20 when absent, at most {WAIT_MAX}.") }
            },
            "required": ["clip_id"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        beats_analyze,
    );
    support::add(
        registry,
        "beats_get",
        "Read moments of a music clip's beat grid as timeline seconds: beats, bars (downbeats), \
         kicks, snares or drops, optionally inside from..to, every Nth starting at offset. \
         Run beats_analyze first.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string", "description": "The music clip with a grid." },
                "what": { "type": "string", "enum": picks },
                "from": { "type": "number", "description": "Timeline seconds." },
                "to": { "type": "number", "description": "Timeline seconds." },
                "every": { "type": "integer", "description": "Keep every Nth moment, 1 when absent." },
                "offset": { "type": "integer", "description": "Start from this moment of each group of `every`." }
            },
            "required": ["clip_id", "what"],
            "additionalProperties": false
        }),
        ToolClass::Read,
        beats_get,
    );
}

/// Вход `beats_analyze`.
#[derive(Deserialize)]
struct AnalyzeInput {
    clip_id: String,
    #[serde(default)]
    bpm_hint: Option<f64>,
    #[serde(default)]
    again: bool,
    #[serde(default)]
    wait_seconds: Option<f64>,
}

/// Какие моменты сетки взять: общий вход `beats_get` и `hit_fx.on`.
#[derive(Deserialize)]
pub(crate) struct PickInput {
    /// Музыкальный клип с сеткой.
    pub clip_id: String,
    /// Что брать.
    pub what: String,
    /// С какой секунды таймлайна.
    #[serde(default)]
    pub from: Option<f64>,
    /// До какой секунды таймлайна.
    #[serde(default)]
    pub to: Option<f64>,
    /// Каждый N-й момент.
    #[serde(default)]
    pub every: Option<usize>,
    /// С какого момента в каждой группе из `every`.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// Медиа и поток клипа: по ним сетка лежит на медиа.
fn source_of<'a>(
    project: &'a Project,
    clip: &Clip,
) -> Result<&'a concat_project::model::MediaItem, ToolError> {
    project
        .media_by_id(&clip.media_id)
        .ok_or_else(|| ToolError::BadInput(format!("{} has no media to hear", clip.id)))
}

/// Предмет задачи разбора: файл и поток, как их ключует сетка. Не id медиа: доска задач одна
/// на сессию, а `m1` в другом открытом проекте — другой файл.
fn subject_of(media: &concat_project::model::MediaItem, clip: &Clip) -> String {
    format!(
        "{}#{}",
        media.path,
        beats::stream_key(media, clip.audio_stream)
    )
}

/// Разбирает трек клипа и кладёт сетку на медиа.
fn beats_analyze(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: AnalyzeInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?.clone();
    let media = source_of(&view.project, &clip)?.clone();
    if !input.again
        && let Some(grid) = beats::grid(&media, clip.audio_stream)
    {
        // Подсказка к уже сохранённой сетке — только смена октавы, без нового разбора.
        let Some(hint) = input.bpm_hint else {
            return Ok(described(&grid, &clip, "stored"));
        };
        let hinted = hinted(grid.clone(), hint);
        if hinted != grid {
            support::apply(
                ctx,
                Command::SetMediaBeats {
                    media_id: media.id.clone(),
                    stream: clip.audio_stream,
                    grid: Some(hinted.clone()),
                },
            )?;
        }
        return Ok(described(&hinted, &clip, "hinted"));
    }

    let subject = subject_of(&media, &clip);
    let job = match ctx.jobs.latest_for(JobKind::Beats, &subject) {
        // Упавший разбор не кэшируется: повторный вызов начинает заново.
        Some(job)
            if job.is_running() || (!input.again && matches!(job.state, JobState::Done(_))) =>
        {
            job
        }
        _ => {
            let stop = Arc::new(AtomicBool::new(false));
            let stopper = Arc::clone(&stop);
            let path = media.path.clone();
            let stream = clip.audio_stream.map(|index| index as usize);
            let id = ctx.jobs.spawn(
                JobKind::Beats,
                &subject,
                move || stopper.store(true, Ordering::Relaxed),
                move |_| {
                    let grid = cutless_beats::analyze_file(&path, stream, &stop)
                        .map_err(|error| error.to_string())?;
                    let data = serde_json::to_value(&grid).map_err(|error| error.to_string())?;
                    Ok(ToolOutput {
                        text: format!(
                            "{:.2} BPM heard - call beats_analyze again to store the grid",
                            grid.bpm
                        ),
                        data,
                        ..ToolOutput::default()
                    })
                },
            )?;
            ctx.jobs
                .get(&id)
                .ok_or_else(|| ToolError::Failed(format!("job {id} is gone")))?
        }
    };
    let job = ctx
        .jobs
        .wait(&job.id, wait_from(input.wait_seconds))
        .ok_or_else(|| ToolError::Failed(format!("job {} is gone", job.id)))?;
    let output = match job.state {
        JobState::Running { .. } => {
            return Ok(ToolOutput {
                text: format!("Beat analysis is still running: {}", job.hint()),
                data: job.data(),
                ..ToolOutput::default()
            });
        }
        JobState::Failed(error) => return Err(ToolError::Failed(error)),
        JobState::Done(output) => output,
    };
    let mut grid: BeatGrid = serde_json::from_value(output.data)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    if let Some(hint) = input.bpm_hint {
        grid = hinted(grid, hint);
    }
    support::apply(
        ctx,
        Command::SetMediaBeats {
            media_id: media.id.clone(),
            stream: clip.audio_stream,
            grid: Some(grid.clone()),
        },
    )?;
    Ok(described(&grid, &clip, "found"))
}

/// Подсказка темпа выбирает октаву: ×2 или ×½, если так ближе к подсказке.
fn hinted(mut grid: BeatGrid, hint: f64) -> BeatGrid {
    if !hint.is_finite() || hint <= 0.0 {
        return grid;
    }
    let options = [grid.bpm, grid.bpm * 2.0, grid.bpm / 2.0];
    let best = options
        .into_iter()
        .min_by(|a, b| (a - hint).abs().total_cmp(&(b - hint).abs()))
        .unwrap_or(grid.bpm);
    if (best - grid.bpm).abs() > 1e-9 {
        // Удвоение сохраняет доли и добавляет между ними. При половине остаётся каждая вторая
        // доля: если сильная была нечётной, фаза сдвигается на одну старую долю, чтобы она уцелела.
        let bar = grid.bar.max(1);
        if best > grid.bpm {
            grid.downbeat = (grid.downbeat * 2) % bar;
        } else {
            if grid.downbeat % 2 == 1 {
                grid.phase += grid.beat();
                grid.downbeat -= 1;
            }
            grid.downbeat = (grid.downbeat / 2) % bar;
        }
        // Темп выбран подсказкой: сомнение снято.
        grid.alt_bpm = None;
        grid.bpm = best;
    }
    grid
}

/// Короткое описание сетки для модели: темп, первая сильная доля под клипом, удары, дропы.
fn described(grid: &BeatGrid, clip: &Clip, how: &str) -> ToolOutput {
    let bars = beats::timeline_picks(grid, clip, Pick::Bars);
    let kicks = beats::timeline_picks(grid, clip, Pick::Kicks);
    let snares = beats::timeline_picks(grid, clip, Pick::Snares);
    let drops = beats::timeline_picks(grid, clip, Pick::Drops);
    let drops_text: Vec<String> = drops.iter().map(|time| support::time(*time)).collect();
    let mut text = format!(
        "{} grid of {}: {:.2} BPM, {}/4, first bar at {}, under the clip {} bars, {} kicks, {} snares",
        how,
        clip.id,
        grid.bpm,
        grid.bar,
        bars.first()
            .map_or("-".to_owned(), |time| support::time(*time)),
        bars.len(),
        kicks.len(),
        snares.len()
    );
    text.push_str(&if drops_text.is_empty() {
        ", no drop".to_owned()
    } else {
        format!(", drops at {}", drops_text.join(", "))
    });
    if let Some(alt) = grid.alt_bpm {
        text.push_str(&format!(
            "; the tempo may be {alt:.2} BPM instead - pass bpm_hint if you know it"
        ));
    }
    if !grid.steady {
        text.push_str("; the tempo drifts, beats may wander off the grid");
    }
    ToolOutput {
        text,
        data: json!({
            "clip_id": clip.id,
            "bpm": grid.bpm,
            "alt_bpm": grid.alt_bpm,
            "bars": bars.len(),
            "kicks": kicks.len(),
            "snares": snares.len(),
            "drops": drops,
        }),
        ..ToolOutput::default()
    }
}

/// Моменты сетки музыкального клипа на таймлайне по входу; `None` в `every` — каждый.
pub(crate) fn picked(project: &Project, input: &PickInput) -> Result<Vec<f64>, ToolError> {
    let pick = Pick::from_name(&input.what).ok_or_else(|| {
        let names: Vec<&str> = Pick::ALL.iter().map(|pick| pick.name()).collect();
        ToolError::BadInput(format!("unknown {:?}: {}", input.what, names.join(", ")))
    })?;
    let clip = support::find_clip(project, &input.clip_id)?;
    let media = source_of(project, clip)?;
    let grid = beats::grid(media, clip.audio_stream).ok_or_else(|| {
        ToolError::BadInput(format!(
            "{} has no beat grid yet - call beats_analyze first",
            clip.id
        ))
    })?;
    let every = input.every.unwrap_or(1).max(1);
    let offset = input.offset.unwrap_or(0) % every;
    let times = beats::timeline_picks(&grid, clip, pick)
        .into_iter()
        .filter(|time| input.from.is_none_or(|from| *time >= from))
        .filter(|time| input.to.is_none_or(|to| *time <= to))
        .enumerate()
        .filter(|(index, _)| index % every == offset)
        .map(|(_, time)| time)
        .collect();
    Ok(times)
}

/// Отдаёт моменты сетки.
fn beats_get(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: PickInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let times = picked(&view.project, &input)?;
    let total = times.len();
    let shown: Vec<f64> = times.into_iter().take(MAX_TIMES).collect();
    let listed: Vec<String> = shown.iter().map(|time| format!("{time:.3}")).collect();
    let mut text = format!("{} {}: {}", total, input.what, listed.join(" "));
    if total > MAX_TIMES {
        text.push_str(&format!(
            " ... {total} in all, {MAX_TIMES} shown - narrow from/to"
        ));
    }
    Ok(ToolOutput {
        text,
        data: json!({ "times": shown, "total": total }),
        ..ToolOutput::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(bpm: f64) -> BeatGrid {
        BeatGrid {
            version: beats::VERSION,
            bpm,
            phase: 0.1,
            bar: 4,
            downbeat: 1,
            duration: 60.0,
            confidence: 1.1,
            alt_bpm: Some(bpm * 2.0),
            steady: true,
            kicks: Vec::new(),
            snares: Vec::new(),
            drops: Vec::new(),
        }
    }

    #[test]
    fn a_hint_picks_the_octave_and_keeps_the_bars() {
        let doubled = hinted(grid(70.0), 140.0);
        assert_eq!(doubled.bpm, 140.0);
        assert_eq!(doubled.downbeat, 2);
        assert_eq!(doubled.alt_bpm, None, "the hint settles the doubt");
        let same = hinted(grid(128.0), 125.0);
        assert_eq!(same.bpm, 128.0);
        // 140 с сильной на доле 1 (0,1 + 0,4286 с): при 70 та же секунда — доля 0 новой сетки.
        let halved = hinted(grid(140.0), 70.0);
        let old_downbeat = 0.1 + 60.0 / 140.0;
        assert!(
            (halved.phase - old_downbeat).abs() < 1e-9,
            "{}",
            halved.phase
        );
        assert_eq!(halved.downbeat, 0);
        assert!(halved.is_bar(halved.index_at(old_downbeat)));
    }
}
