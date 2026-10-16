// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты монтажа: разрезать, подрезать, сдвинуть, удалить, добавить клип, звук и кадр.

use concat_project::Command;
use concat_project::commands::{ClipMove, ClipPatch, TrimEdge};
use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты монтажа.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "clips_split",
        "Cut each named clip in two at one timeline time, in seconds.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "time": { "type": "number", "description": "Cut point in seconds" }
            },
            "required": ["clip_ids", "time"]
        }),
        ToolClass::Edit,
        clips_split,
    );
    support::add(
        registry,
        "clip_trim",
        "Drag one edge of a clip by seconds: start moves the in-point, end lengthens or shortens. ripple closes the gap.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "edge": { "type": "string", "enum": ["start", "end"] },
                "delta": { "type": "number" },
                "ripple": { "type": "boolean" }
            },
            "required": ["clip_id", "edge", "delta"]
        }),
        ToolClass::Edit,
        clip_trim,
    );
    support::add(
        registry,
        "clips_move",
        "Move any number of clips in one edit: new start in seconds and, optionally, a new track.",
        json!({
            "type": "object",
            "properties": {
                "moves": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "clip_id": { "type": "string" },
                            "start": { "type": "number" },
                            "track_id": { "type": "string" }
                        },
                        "required": ["clip_id", "start"]
                    }
                }
            },
            "required": ["moves"]
        }),
        ToolClass::Edit,
        clips_move,
    );
    support::add(
        registry,
        "clips_remove",
        "Delete clips from the active timeline. ripple closes the gaps they leave.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "ripple": { "type": "boolean" }
            },
            "required": ["clip_ids"]
        }),
        ToolClass::Edit,
        clips_remove,
    );
    support::add(
        registry,
        "clip_add",
        "Place a media clip on the timeline. Without a track it lands on the first free lane.",
        json!({
            "type": "object",
            "properties": {
                "media_id": { "type": "string" },
                "start": { "type": "number" },
                "track_id": { "type": "string" }
            },
            "required": ["media_id", "start"]
        }),
        ToolClass::Edit,
        clip_add,
    );
    support::add(
        registry,
        "clip_volume",
        "Set gain and audio fades of clips. Any field left out is unchanged.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "volume": { "type": "number" },
                "fade_in": { "type": "number" },
                "fade_out": { "type": "number" }
            },
            "required": ["clip_ids"]
        }),
        ToolClass::Edit,
        clip_volume,
    );
    support::add(
        registry,
        "clip_transform",
        "Place a picture: scale, offsets as frame fractions, and rotation in degrees.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "scale": { "type": "number" },
                "offset_x": { "type": "number" },
                "offset_y": { "type": "number" },
                "rotation": { "type": "number" }
            },
            "required": ["clip_id"]
        }),
        ToolClass::Edit,
        clip_transform,
    );
}

/// Вход `clips_split`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitInput {
    clip_ids: Vec<String>,
    time: f64,
}

/// Вход `clip_trim`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrimInput {
    clip_id: String,
    edge: TrimEdge,
    delta: f64,
    #[serde(default)]
    ripple: bool,
}

/// Вход `clips_move`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInput {
    moves: Vec<MoveIn>,
}

/// Один сдвиг во входе.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveIn {
    clip_id: String,
    start: f64,
    #[serde(default)]
    track_id: Option<String>,
}

/// Вход `clips_remove`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveInput {
    clip_ids: Vec<String>,
    #[serde(default)]
    ripple: bool,
}

/// Вход `clip_add`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddInput {
    media_id: String,
    start: f64,
    #[serde(default)]
    track_id: Option<String>,
}

/// Вход `clip_volume`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VolumeInput {
    clip_ids: Vec<String>,
    #[serde(default)]
    volume: Option<f64>,
    #[serde(default)]
    fade_in: Option<f64>,
    #[serde(default)]
    fade_out: Option<f64>,
}

/// Вход `clip_transform`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransformInput {
    clip_id: String,
    #[serde(default)]
    scale: Option<f64>,
    #[serde(default)]
    offset_x: Option<f64>,
    #[serde(default)]
    offset_y: Option<f64>,
    #[serde(default)]
    rotation: Option<f64>,
}

/// Режет клипы в указанное время.
fn clips_split(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: SplitInput = crate::input(input)?;
    if input.clip_ids.is_empty() {
        return Err(ToolError::BadInput("clip_ids is empty".to_owned()));
    }
    let view = support::apply(
        ctx,
        Command::SplitClips {
            clip_ids: input.clip_ids.clone(),
            time: input.time,
        },
    )?;
    Ok(ToolOutput {
        text: format!(
            "split at {}: {} clips now on the timeline",
            support::time(input.time),
            view.project.active().clips.len()
        ),
        data: json!({ "createdId": view.created_id }),
        ..ToolOutput::default()
    })
}

/// Тянет край клипа на секунды.
fn clip_trim(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: TrimInput = crate::input(input)?;
    support::apply(
        ctx,
        Command::TrimClip {
            clip_id: input.clip_id.clone(),
            edge: input.edge,
            delta: input.delta,
            ripple: input.ripple,
        },
    )?;
    Ok(ToolOutput {
        text: format!(
            "trimmed {} {} by {}s",
            input.clip_id,
            edge_name(input.edge),
            support::num(input.delta)
        ),
        ..ToolOutput::default()
    })
}

/// Сдвигает клипы.
fn clips_move(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: MoveInput = crate::input(input)?;
    if input.moves.is_empty() {
        return Err(ToolError::BadInput("moves is empty".to_owned()));
    }
    let moves: Vec<ClipMove> = input
        .moves
        .into_iter()
        .map(|one| ClipMove {
            clip_id: one.clip_id,
            start: one.start,
            track_id: one.track_id.unwrap_or_default(),
        })
        .collect();
    support::apply(ctx, Command::MoveClips { moves })?;
    Ok(ToolOutput::text("moved"))
}

/// Удаляет клипы.
fn clips_remove(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: RemoveInput = crate::input(input)?;
    if input.clip_ids.is_empty() {
        return Err(ToolError::BadInput("clip_ids is empty".to_owned()));
    }
    support::apply(
        ctx,
        Command::RemoveClips {
            clip_ids: input.clip_ids.clone(),
            ripple: input.ripple,
        },
    )?;
    Ok(ToolOutput::text(format!(
        "removed {} clips",
        input.clip_ids.len()
    )))
}

/// Ставит клип из медиатеки на дорожку.
fn clip_add(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: AddInput = crate::input(input)?;
    let command = match input.track_id {
        Some(track_id) => Command::AddClip {
            media_id: input.media_id.clone(),
            track_id,
            start: input.start,
            ripple: false,
        },
        None => Command::AddClipAtFirstFree {
            media_id: input.media_id.clone(),
            start: input.start,
        },
    };
    let view = support::apply(ctx, command)?;
    let id = view.created_id.clone().unwrap_or_default();
    Ok(ToolOutput {
        text: format!("added clip {} at {}", id, support::time(input.start)),
        data: json!({ "createdId": id }),
        ..ToolOutput::default()
    })
}

/// Ставит громкость и фейды клипов.
fn clip_volume(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: VolumeInput = crate::input(input)?;
    if input.clip_ids.is_empty() {
        return Err(ToolError::BadInput("clip_ids is empty".to_owned()));
    }
    if input.volume.is_none() && input.fade_in.is_none() && input.fade_out.is_none() {
        return Err(ToolError::BadInput(
            "give volume, fade_in or fade_out".to_owned(),
        ));
    }
    let commands = input
        .clip_ids
        .iter()
        .map(|clip_id| Command::UpdateClip {
            clip_id: clip_id.clone(),
            patch: ClipPatch {
                volume: input.volume,
                fade_in: input.fade_in,
                fade_out: input.fade_out,
                ..ClipPatch::default()
            },
        })
        .collect();
    support::apply(ctx, support::batch(commands))?;
    Ok(ToolOutput::text(format!(
        "volume set on {} clips",
        input.clip_ids.len()
    )))
}

/// Размещает кадр: масштаб, смещения, поворот.
fn clip_transform(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: TransformInput = crate::input(input)?;
    if input.scale.is_none()
        && input.offset_x.is_none()
        && input.offset_y.is_none()
        && input.rotation.is_none()
    {
        return Err(ToolError::BadInput(
            "give scale, offset_x, offset_y or rotation".to_owned(),
        ));
    }
    support::apply(
        ctx,
        Command::SetClipTransform {
            clip_id: input.clip_id.clone(),
            scale: input.scale,
            offset_x: input.offset_x,
            offset_y: input.offset_y,
            rotation: input.rotation,
            stretch_x: None,
            stretch_y: None,
        },
    )?;
    Ok(ToolOutput::text(format!("placed {}", input.clip_id)))
}

/// Имя края для ответа.
fn edge_name(edge: TrimEdge) -> &'static str {
    match edge {
        TrimEdge::Start => "start",
        TrimEdge::End => "end",
    }
}
