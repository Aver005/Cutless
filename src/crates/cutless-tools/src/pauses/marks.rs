// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты уже помеченных пауз: `pauses_list`, `pauses_restore` и
//! `pauses_confirm`.

use std::sync::Arc;

use concat_project::Command;
use concat_project::commands::pauses::is_pause;
use concat_project::model::{Clip, Timeline};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Tool, ToolClass, ToolContext, ToolError, ToolOutput};

use super::{apply_command, fmt_seconds, is_music, output, read_project};

/// `pauses_list`: что уже помечено.
pub(super) struct PausesList;

/// `pauses_restore`: склеить помеченные куски обратно.
pub(super) struct PausesRestore;

/// `pauses_confirm`: вырезать помеченные куски.
pub(super) struct PausesConfirm;

/// Вход склейки: куски, которые возвращаем.
#[derive(Deserialize)]
struct RestoreInput {
    clip_ids: Vec<String>,
}

/// Вход подтверждения: какие куски режем; пустой — все помеченные.
#[derive(Deserialize)]
struct ConfirmInput {
    #[serde(default)]
    clip_ids: Option<Vec<String>>,
}

impl Tool for PausesList {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "pauses_list",
            description: "List the pieces already marked as pauses: clip id, timeline start, \
                length and the total they add up to. Reads only.",
            input_schema: empty_schema(),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, _value: Value) -> Result<ToolOutput, ToolError> {
        let project = read_project(ctx)?;
        let mut pieces: Vec<&Clip> = project
            .active()
            .clips
            .iter()
            .map(Arc::as_ref)
            .filter(|clip| is_pause(clip))
            .collect();
        if pieces.is_empty() {
            return Ok(ToolOutput::text("no marked pauses"));
        }
        pieces.sort_by(|a, b| a.start.total_cmp(&b.start));
        let seconds: f64 = pieces.iter().map(|piece| piece.duration).sum();
        let mut lines = vec![format!(
            "{} marked pause piece(s), {} s total",
            pieces.len(),
            fmt_seconds(seconds)
        )];
        for piece in &pieces {
            lines.push(format!(
                "{} {} {} ({})",
                piece.id,
                fmt_seconds(piece.start),
                fmt_seconds(piece.start + piece.duration),
                fmt_seconds(piece.duration)
            ));
        }
        let data = json!({
            "count": pieces.len(),
            "seconds": seconds,
            "pieces": pieces.iter().map(|piece| json!({
                "clip_id": piece.id,
                "start": piece.start,
                "duration": piece.duration,
            })).collect::<Vec<_>>(),
        });
        Ok(output(lines.join("\n"), data))
    }
}

impl Tool for PausesRestore {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "pauses_restore",
            description: "Un-mark the named pieces and join each back into the neighbours it \
                continues, when they touch and share the file. One undo step.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The marked pieces to restore."
                    }
                },
                "required": ["clip_ids"],
                "additionalProperties": false
            }),
            class: ToolClass::Edit,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, value: Value) -> Result<ToolOutput, ToolError> {
        let input: RestoreInput = crate::input(value)?;
        if input.clip_ids.is_empty() {
            return Err(ToolError::BadInput(
                "clip_ids must name at least one marked piece".to_owned(),
            ));
        }
        let before = read_project(ctx)?.active().clips.len();
        let requested = input.clip_ids.len();
        let applied = apply_command(
            ctx,
            Command::RestorePauses {
                clip_ids: input.clip_ids,
            },
        )?;
        let after = applied.project.active().clips.len();
        let merged = before.saturating_sub(after);
        let text = if merged > 0 {
            format!("restored {requested} piece(s), merged {merged} cut(s) back together")
        } else {
            "nothing to restore".to_owned()
        };
        let data = json!({ "requested": requested, "merged": merged });
        Ok(output(text, data))
    }
}

impl Tool for PausesConfirm {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "pauses_confirm",
            description: "Cut out the marked pauses: the named pieces, or every marked piece on \
                the active timeline when none are named. The gaps close on every track but the \
                music, and the reply says how much shorter the reel got. One undo step, and hard \
                to undo by eye, so ask the person first.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Pieces to cut. Default: every marked piece on the active timeline."
                    }
                },
                "additionalProperties": false
            }),
            class: ToolClass::Danger,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, value: Value) -> Result<ToolOutput, ToolError> {
        let input: ConfirmInput = crate::input(value)?;
        let project = read_project(ctx)?;
        let before = project.active().duration();
        let doomed = doomed_spans(project.active(), input.clip_ids.as_deref());
        if doomed.is_empty() {
            return Ok(ToolOutput::text("no marked pauses to remove"));
        }
        let removed_pauses = doomed.len();
        let shortened = merged_seconds(doomed);
        let applied = apply_command(
            ctx,
            Command::ConfirmPauses {
                clip_ids: input.clip_ids,
            },
        )?;
        let after = applied.project.active().duration();
        let text = format!(
            "removed {removed_pauses} pause(s), the reel is {} s shorter ({} -> {} s)",
            fmt_seconds(shortened),
            fmt_seconds(before),
            fmt_seconds(after)
        );
        let data = json!({
            "removed_pauses": removed_pauses,
            "before_seconds": before,
            "after_seconds": after,
            "shortened_seconds": shortened,
        });
        Ok(output(text, data))
    }
}

/// Отрезки помеченных не-музыкальных кусков, которые уйдут.
fn doomed_spans(timeline: &Timeline, clip_ids: Option<&[String]>) -> Vec<(f64, f64)> {
    timeline
        .clips
        .iter()
        .map(Arc::as_ref)
        .filter(|clip| is_pause(clip) && !is_music(clip))
        .filter(|clip| match clip_ids {
            Some(ids) => ids.iter().any(|id| id == &clip.id),
            None => true,
        })
        .map(|clip| (clip.start, clip.start + clip.duration))
        .collect()
}

/// Сумма отрезков с объединением пересечений: сколько времени уйдёт.
fn merged_seconds(mut spans: Vec<(f64, f64)>) -> f64 {
    const TOUCH: f64 = 1e-6;
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let mut current: Option<(f64, f64)> = None;
    for (start, end) in spans {
        match &mut current {
            Some((_, last)) if start <= *last + TOUCH => *last = last.max(end),
            _ => {
                if let Some((first, last)) = current.take() {
                    total += last - first;
                }
                current = Some((start, end));
            }
        }
    }
    if let Some((first, last)) = current {
        total += last - first;
    }
    total
}

/// Схема входа без полей.
fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}
