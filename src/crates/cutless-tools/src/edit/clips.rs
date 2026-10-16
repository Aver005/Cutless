// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты монтажа: разрезать, подрезать, сдвинуть, удалить, добавить клип, звук и кадр.

use concat_project::commands::{ClipMove, ClipPatch, TrimEdge, links};
use concat_project::model::MediaKind;
use concat_project::{Command, Project};
use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты монтажа.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "clips_split",
        "Cut each named clip in two at one timeline time, in seconds. A clip in a linked group cuts with its group.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "time": { "type": "number", "description": "Cut point in seconds" },
                "alone": { "type": "boolean", "description": "Only these clips, not the rest of their linked groups" }
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
        "Move any number of clips in one edit: new start in seconds and, optionally, a new track. A clip in a linked group takes its group along by the same shift.",
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
                },
                "alone": { "type": "boolean", "description": "Only these clips, not the rest of their linked groups" }
            },
            "required": ["moves"]
        }),
        ToolClass::Edit,
        clips_move,
    );
    support::add(
        registry,
        "clips_remove",
        "Delete clips from the active timeline. ripple closes the gaps they leave. A clip in a linked group goes with its group.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "ripple": { "type": "boolean" },
                "alone": { "type": "boolean", "description": "Only these clips, not the rest of their linked groups" }
            },
            "required": ["clip_ids"]
        }),
        ToolClass::Edit,
        clips_remove,
    );
    support::add(
        registry,
        "clip_add",
        "Place a media clip on the timeline. Without a track it lands on the first free lane. A video with several audio tracks is split into one sound clip per track, linked with the video, unless split_audio is false.",
        json!({
            "type": "object",
            "properties": {
                "media_id": { "type": "string" },
                "start": { "type": "number" },
                "track_id": { "type": "string" },
                "split_audio": { "type": "boolean" }
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
struct SplitInput {
    clip_ids: Vec<String>,
    time: f64,
    #[serde(default)]
    alone: bool,
}

/// Вход `clip_trim`.
#[derive(Deserialize)]
struct TrimInput {
    clip_id: String,
    edge: TrimEdge,
    delta: f64,
    #[serde(default)]
    ripple: bool,
}

/// Вход `clips_move`.
#[derive(Deserialize)]
struct MoveInput {
    moves: Vec<MoveIn>,
    #[serde(default)]
    alone: bool,
}

/// Один сдвиг во входе.
#[derive(Deserialize)]
struct MoveIn {
    clip_id: String,
    start: f64,
    #[serde(default)]
    track_id: Option<String>,
}

/// Вход `clips_remove`.
#[derive(Deserialize)]
struct RemoveInput {
    clip_ids: Vec<String>,
    #[serde(default)]
    ripple: bool,
    #[serde(default)]
    alone: bool,
}

/// Вход `clip_add`.
#[derive(Deserialize)]
struct AddInput {
    media_id: String,
    start: f64,
    #[serde(default)]
    track_id: Option<String>,
    /// Разделить звук по дорожкам; по умолчанию — да, если дорожек несколько.
    #[serde(default)]
    split_audio: Option<bool>,
}

/// Вход `clip_volume`.
#[derive(Deserialize)]
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
    let clip_ids = grouped(ctx, &input.clip_ids, input.alone)?;
    let view = support::apply(
        ctx,
        Command::SplitClips {
            clip_ids,
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
    let wanted: Vec<(String, f64)> = input
        .moves
        .iter()
        .map(|one| (one.clip_id.clone(), one.start))
        .collect();
    let followers = if input.alone {
        Vec::new()
    } else {
        group_followers(&support::fetch(ctx)?.project, &wanted)
    };
    let moves: Vec<ClipMove> = input
        .moves
        .into_iter()
        .map(|one| ClipMove {
            clip_id: one.clip_id,
            start: one.start,
            track_id: one.track_id.unwrap_or_default(),
        })
        .chain(followers.iter().map(|(clip_id, start)| ClipMove {
            clip_id: clip_id.clone(),
            start: *start,
            track_id: String::new(),
        }))
        .collect();
    let view = support::apply(ctx, Command::MoveClips { moves })?;
    // Редактор не даёт клипам наложиться и может оставить клип на месте: модель должна это знать.
    let mut lines = Vec::new();
    for (clip_id, start) in &wanted {
        let actual = support::find_clip(&view.project, clip_id)?.start;
        if (actual - start).abs() < 0.001 {
            lines.push(format!("{clip_id} now starts at {}", support::time(actual)));
        } else {
            lines.push(format!(
                "{clip_id} stays at {} (asked {}): it would overlap a clip on its track",
                support::time(actual),
                support::time(*start)
            ));
        }
    }
    if !followers.is_empty() {
        let ids: Vec<&str> = followers.iter().map(|(id, _)| id.as_str()).collect();
        lines.push(format!("moved with their groups: {}", ids.join(", ")));
    }
    Ok(ToolOutput::text(lines.join("\n")))
}

/// Клипы и их группы целиком; с `alone` — только сами клипы.
fn grouped(
    ctx: &mut ToolContext<'_>,
    clip_ids: &[String],
    alone: bool,
) -> Result<Vec<String>, ToolError> {
    if alone {
        return Ok(clip_ids.to_vec());
    }
    let view = support::fetch(ctx)?;
    Ok(links::with_linked(view.project.active(), clip_ids))
}

/// Остальные клипы групп: (id, новое начало) с тем же сдвигом, что у клипа своей группы.
fn group_followers(project: &Project, wanted: &[(String, f64)]) -> Vec<(String, f64)> {
    let timeline = project.active();
    let mut followers: Vec<(String, f64)> = Vec::new();
    for (clip_id, start) in wanted {
        let Some(clip) = timeline.clip(clip_id) else {
            continue;
        };
        let shift = start - clip.start;
        for member in links::with_linked(timeline, std::slice::from_ref(clip_id)) {
            let taken = wanted.iter().any(|(id, _)| *id == member)
                || followers.iter().any(|(id, _)| *id == member);
            if let (false, Some(other)) = (taken, timeline.clip(&member)) {
                followers.push((member, other.start + shift));
            }
        }
    }
    followers
}

/// Удаляет клипы.
fn clips_remove(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: RemoveInput = crate::input(input)?;
    if input.clip_ids.is_empty() {
        return Err(ToolError::BadInput("clip_ids is empty".to_owned()));
    }
    let clip_ids = grouped(ctx, &input.clip_ids, input.alone)?;
    let count = clip_ids.len();
    support::apply(
        ctx,
        Command::RemoveClips {
            clip_ids,
            ripple: input.ripple,
        },
    )?;
    Ok(ToolOutput::text(format!("removed {count} clips")))
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
    let tracks = view
        .project
        .media_by_id(&input.media_id)
        .filter(|media| media.kind == MediaKind::Video)
        .map_or(0, |media| media.audio_tracks.len());
    let mut text = format!("added clip {} at {}", id, support::time(input.start));
    if tracks > 1 && input.split_audio != Some(false) {
        let view = support::apply(
            ctx,
            Command::DetachAudioLinked {
                clip_id: id.clone(),
            },
        )?;
        let group = links::with_linked(view.project.active(), std::slice::from_ref(&id));
        text.push_str(&format!(
            "; its {tracks} audio tracks are sound clips linked with it: {}",
            group.join(", ")
        ));
    }
    Ok(ToolOutput {
        text,
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
