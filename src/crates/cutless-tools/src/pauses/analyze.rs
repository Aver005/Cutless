// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты, которые ищут паузы и размечают их: `pauses_analyze` и
//! `pauses_mark`.

use std::collections::BTreeMap;

use concat_project::Command;
use concat_project::commands::pauses::PauseRange;
use concat_project::model::Project;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Tool, ToolClass, ToolContext, ToolError, ToolOutput};

use super::{
    ClipPauses, analyse, apply_command, fmt_seconds, output, read_project, settings_from, targets,
};

/// `pauses_analyze`: находит паузы, но ничего не размечает.
pub(super) struct PausesAnalyze;

/// `pauses_mark`: размечает найденные паузы или выбранные моделью отрезки.
pub(super) struct PausesMark;

/// Общий вход анализа: что слушать и по каким порогам.
#[derive(Deserialize)]
struct PauseInput {
    #[serde(default)]
    clip_ids: Option<Vec<String>>,
    #[serde(default)]
    threshold_db: Option<f32>,
    #[serde(default)]
    min_pause: Option<f64>,
    #[serde(default)]
    padding: Option<f64>,
}

/// Вход разметки: к анализу добавляются готовые отрезки модели.
#[derive(Deserialize)]
struct MarkInput {
    #[serde(default)]
    clip_ids: Option<Vec<String>>,
    #[serde(default)]
    threshold_db: Option<f32>,
    #[serde(default)]
    min_pause: Option<f64>,
    #[serde(default)]
    padding: Option<f64>,
    #[serde(default)]
    ranges: Option<Vec<RangeInput>>,
}

/// Один отрезок, который выбрала модель, в секундах таймлайна.
#[derive(Deserialize)]
struct RangeInput {
    clip_id: String,
    start: f64,
    end: f64,
}

impl Tool for PausesAnalyze {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "pauses_analyze",
            description: "Find pauses (silent stretches) in the sound of clips and report their \
                ranges in timeline seconds, with the noise floor, the speech level and the \
                threshold used. It only looks: nothing is marked. Use it before pauses_mark so \
                the person can see what would be cut.",
            input_schema: analyze_schema(),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, value: Value) -> Result<ToolOutput, ToolError> {
        let input: PauseInput = crate::input(value)?;
        let settings = settings_from(input.threshold_db, input.min_pause, input.padding)?;
        let project = read_project(ctx)?;
        let targets = targets(&project, input.clip_ids.as_deref())?;
        if targets.is_empty() {
            return Ok(ToolOutput::text("no clips with sound to analyse"));
        }
        let clips = analyse(&targets, &settings)?;
        Ok(analyze_output(&clips))
    }
}

impl Tool for PausesMark {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "pauses_mark",
            description: "Mark pauses in clips so they can be reviewed and later removed. Either \
                pass ranges the model chose (each with clip_id, start and end in timeline seconds, \
                which is how breaths are kept), or let the tool find them from the sound. The \
                clip is cut at every range edge and the pieces inside are marked. One undo step.",
            input_schema: mark_schema(),
            class: ToolClass::Edit,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, value: Value) -> Result<ToolOutput, ToolError> {
        let input: MarkInput = crate::input(value)?;
        let project = read_project(ctx)?;
        let groups: Vec<(String, Vec<PauseRange>)> = match input.ranges {
            Some(ranges) if !ranges.is_empty() => match_ranges(&project, &ranges)?,
            _ => {
                let settings = settings_from(input.threshold_db, input.min_pause, input.padding)?;
                let targets = targets(&project, input.clip_ids.as_deref())?;
                analyse(&targets, &settings)?
                    .into_iter()
                    .filter(|clip| !clip.ranges.is_empty())
                    .map(|clip| (clip.clip_id, clip.ranges))
                    .collect()
            }
        };
        if groups.is_empty() {
            return Ok(ToolOutput::text("nothing to mark: no pauses found"));
        }
        let marked: usize = groups.iter().map(|(_, ranges)| ranges.len()).sum();
        let seconds: f64 = groups
            .iter()
            .flat_map(|(_, ranges)| ranges.iter())
            .map(|range| range.end - range.start)
            .sum();
        let clips: Vec<String> = groups.iter().map(|(clip_id, _)| clip_id.clone()).collect();
        let commands: Vec<Command> = groups
            .into_iter()
            .map(|(clip_id, ranges)| Command::MarkPauses { clip_id, ranges })
            .collect();
        let applied = apply_command(ctx, Command::Batch { commands })?;

        let text = format!(
            "marked {marked} pause(s) in {} clip(s), {} s total",
            clips.len(),
            fmt_seconds(seconds)
        );
        let data = json!({
            "clips": clips,
            "marked": marked,
            "seconds": seconds,
            "created_id": applied.created_id,
        });
        Ok(output(text, data))
    }
}

/// Группирует отрезки по клипу, проверяя, что клип на таймлайне есть.
fn match_ranges(
    project: &Project,
    ranges: &[RangeInput],
) -> Result<Vec<(String, Vec<PauseRange>)>, ToolError> {
    let timeline = project.active();
    let mut order: Vec<String> = Vec::new();
    let mut grouped: BTreeMap<String, Vec<PauseRange>> = BTreeMap::new();
    for range in ranges {
        if !range.start.is_finite() || !range.end.is_finite() {
            return Err(ToolError::BadInput(
                "every range needs a finite start and end".to_owned(),
            ));
        }
        if timeline.clip(&range.clip_id).is_none() {
            return Err(ToolError::Failed(format!(
                "clip {} is not on the active timeline",
                range.clip_id
            )));
        }
        let entry = grouped.entry(range.clip_id.clone()).or_default();
        if entry.is_empty() {
            order.push(range.clip_id.clone());
        }
        entry.push(PauseRange {
            start: range.start,
            end: range.end,
        });
    }
    Ok(order
        .into_iter()
        .map(|clip_id| {
            let ranges = grouped.remove(&clip_id).unwrap_or_default();
            (clip_id, ranges)
        })
        .collect())
}

/// Текст и данные ответа анализа.
fn analyze_output(clips: &[ClipPauses]) -> ToolOutput {
    let total: usize = clips.iter().map(|clip| clip.ranges.len()).sum();
    let seconds: f64 = clips
        .iter()
        .flat_map(|clip| clip.ranges.iter())
        .map(|range| range.end - range.start)
        .sum();
    let mut lines = vec![format!(
        "{total} pause(s) in {} clip(s), {} s total",
        clips.len(),
        fmt_seconds(seconds)
    )];
    for clip in clips {
        lines.push(format!(
            "{}: threshold {} dB, noise {} dB, speech {} dB",
            clip.clip_id,
            fmt_seconds(f64::from(clip.threshold_db)),
            fmt_seconds(f64::from(clip.noise_floor_db)),
            fmt_seconds(f64::from(clip.speech_db)),
        ));
        for range in &clip.ranges {
            lines.push(format!(
                "  {} {} {} ({})",
                clip.clip_id,
                fmt_seconds(range.start),
                fmt_seconds(range.end),
                fmt_seconds(range.end - range.start)
            ));
        }
    }
    let data = json!({
        "clips": clips.iter().map(|clip| json!({
            "clip_id": clip.clip_id,
            "noise_floor_db": clip.noise_floor_db,
            "speech_db": clip.speech_db,
            "threshold_db": clip.threshold_db,
            "pauses": clip.ranges.iter().map(|range| json!({
                "start": range.start,
                "end": range.end,
                "duration": range.end - range.start,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "total_pauses": total,
        "total_seconds": seconds,
    });
    output(lines.join("\n"), data)
}

/// Схема входа анализа.
fn analyze_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "clip_ids": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Clips to analyse. Default: every clip with sound on the active timeline, except background music."
            },
            "threshold_db": {
                "type": "number",
                "description": "Silence threshold in dB. Omit to compute it from the sound."
            },
            "min_pause": {
                "type": "number",
                "description": "Shortest pause to report, in seconds. Default 0.3."
            },
            "padding": {
                "type": "number",
                "description": "Silence left on each edge of a pause, in seconds. Default 1/3."
            }
        },
        "additionalProperties": false
    })
}

/// Схема входа разметки.
fn mark_schema() -> Value {
    let mut schema = analyze_schema();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(
            "ranges".to_owned(),
            json!({
                "type": "array",
                "description": "Ranges the model chose, in timeline seconds. When present these are used instead of analysing the sound, which is how breaths are kept.",
                "items": {
                    "type": "object",
                    "properties": {
                        "clip_id": { "type": "string" },
                        "start": { "type": "number" },
                        "end": { "type": "number" }
                    },
                    "required": ["clip_id", "start", "end"],
                    "additionalProperties": false
                }
            }),
        );
    }
    schema
}
