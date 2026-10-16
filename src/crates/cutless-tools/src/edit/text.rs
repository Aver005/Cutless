// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты титров: добавить, изменить и растянуть стиль на всю дорожку.

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{ClipKind, KeyProperty, TextAlign, TextStyle};
use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты титров.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "text_add",
        "Add a title over the picture. Style covers font, size, colour, outline, shadow, plate, vertical position and alignment.",
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "start": { "type": "number" },
                "duration": { "type": "number" },
                "style": { "type": "object" }
            },
            "required": ["text", "start"]
        }),
        ToolClass::Edit,
        text_add,
    );
    support::add(
        registry,
        "text_update",
        "Change a title's words and/or style. Only the fields given change.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "text": { "type": "string" },
                "style": { "type": "object" }
            },
            "required": ["clip_id"]
        }),
        ToolClass::Edit,
        text_update,
    );
    support::add(
        registry,
        "text_style_track",
        "Apply one title's look to every other title on its track, keeping each one's own words.",
        json!({
            "type": "object",
            "properties": { "clip_id": { "type": "string" } },
            "required": ["clip_id"]
        }),
        ToolClass::Edit,
        text_style_track,
    );
}

/// Частичный стиль титра: меняются только заданные поля.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StylePatch {
    font_family: Option<String>,
    font_size: Option<f64>,
    font_weight: Option<f64>,
    italic: Option<bool>,
    color: Option<String>,
    align: Option<TextAlign>,
    opacity: Option<f64>,
    stroke_width: Option<f64>,
    stroke_color: Option<String>,
    shadow: Option<bool>,
    background: Option<String>,
    background_radius: Option<f64>,
    background_padding_x: Option<f64>,
    background_padding_y: Option<f64>,
    line_height: Option<f64>,
    tracking: Option<f64>,
    max_width: Option<f64>,
    max_height: Option<f64>,
    /// Вертикальное положение титра долей высоты кадра.
    offset_y: Option<f64>,
}

/// Вход `text_add`.
#[derive(Deserialize)]
struct AddInput {
    text: String,
    start: f64,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    style: Option<StylePatch>,
}

/// Вход `text_update`.
#[derive(Deserialize)]
struct UpdateInput {
    clip_id: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    style: Option<StylePatch>,
}

/// Вход `text_style_track`.
#[derive(Deserialize)]
struct TrackInput {
    clip_id: String,
}

/// Накладывает частичный стиль на готовый.
fn style_with(base: TextStyle, patch: Option<&StylePatch>) -> TextStyle {
    let Some(patch) = patch else {
        return base;
    };
    let mut style = base;
    if let Some(value) = &patch.font_family {
        style.font_family = value.clone();
    }
    if let Some(value) = patch.font_size {
        style.font_size = value;
    }
    if let Some(value) = patch.font_weight {
        style.font_weight = value;
    }
    if let Some(value) = patch.italic {
        style.italic = value;
    }
    if let Some(value) = &patch.color {
        style.color = value.clone();
    }
    if let Some(value) = patch.align {
        style.align = value;
    }
    if let Some(value) = patch.opacity {
        style.opacity = value;
    }
    if let Some(value) = patch.stroke_width {
        style.stroke_width = value;
    }
    if let Some(value) = &patch.stroke_color {
        style.stroke_color = value.clone();
    }
    if let Some(value) = patch.shadow {
        style.shadow = value;
    }
    if let Some(value) = &patch.background {
        style.background = value.clone();
    }
    if let Some(value) = patch.background_radius {
        style.background_radius = value;
    }
    if let Some(value) = patch.background_padding_x {
        style.background_padding_x = value;
    }
    if let Some(value) = patch.background_padding_y {
        style.background_padding_y = value;
    }
    if let Some(value) = patch.line_height {
        style.line_height = value;
    }
    if let Some(value) = patch.tracking {
        style.tracking = value;
    }
    if let Some(value) = patch.max_width {
        style.max_width = value;
    }
    if let Some(value) = patch.max_height {
        style.max_height = value;
    }
    style
}

/// Добавляет титр.
fn text_add(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: AddInput = crate::input(input)?;
    let mut style = style_with(TextStyle::default(), input.style.as_ref());
    style.content = input.text.clone();
    let offset_y = input.style.as_ref().and_then(|style| style.offset_y);
    let view = support::apply(
        ctx,
        Command::AddTextClip {
            track_id: None,
            above: true,
            start: input.start,
            style: Some(style),
            duration: input.duration,
            offset_y,
        },
    )?;
    let id = view.created_id.clone().unwrap_or_default();
    Ok(ToolOutput {
        text: format!("added title {} at {}", id, support::time(input.start)),
        data: json!({ "createdId": id }),
        ..ToolOutput::default()
    })
}

/// Меняет слова и/или стиль титра.
fn text_update(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: UpdateInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?.clone();
    if clip.kind != ClipKind::Text {
        return Err(ToolError::Failed(format!(
            "{} is not a title",
            input.clip_id
        )));
    }
    let base = clip.text.clone().unwrap_or_default();
    let mut style = style_with(base, input.style.as_ref());
    if let Some(text) = &input.text {
        style.content = text.clone();
    }
    let mut commands = vec![Command::UpdateClip {
        clip_id: input.clip_id.clone(),
        patch: ClipPatch {
            text: Some(Some(style)),
            ..ClipPatch::default()
        },
    }];
    if let Some(offset_y) = input.style.as_ref().and_then(|style| style.offset_y) {
        commands.push(Command::SetClipTransform {
            clip_id: input.clip_id.clone(),
            scale: None,
            offset_x: None,
            offset_y: Some(offset_y),
            rotation: None,
            stretch_x: None,
            stretch_y: None,
        });
    }
    support::apply(ctx, support::batch(commands))?;
    Ok(ToolOutput::text(format!("updated title {}", input.clip_id)))
}

/// Применяет стиль титра ко всем титрам его дорожки.
fn text_style_track(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: TrackInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let source = support::find_clip(&view.project, &input.clip_id)?;
    if source.kind != ClipKind::Text {
        return Err(ToolError::Failed(format!(
            "{} is not a title",
            input.clip_id
        )));
    }
    let Some(style) = source.text.clone() else {
        return Err(ToolError::Failed(format!(
            "{} has no text style",
            input.clip_id
        )));
    };
    let track_id = source.track_id.clone();
    let (offset_x, offset_y) = (source.offset_x, source.offset_y);
    let mut commands = Vec::new();
    for clip in &view.project.active().clips {
        if clip.id == input.clip_id || clip.track_id != track_id || clip.kind != ClipKind::Text {
            continue;
        }
        let content = clip.text.clone().unwrap_or_default().content;
        let restyled = TextStyle {
            content,
            ..style.clone()
        };
        commands.push(Command::UpdateClip {
            clip_id: clip.id.clone(),
            patch: ClipPatch {
                text: Some(Some(restyled)),
                ..ClipPatch::default()
            },
        });
        if clip.offset_x != offset_x || clip.offset_y != offset_y {
            commands.push(Command::SetClipTransform {
                clip_id: clip.id.clone(),
                scale: None,
                offset_x: Some(offset_x),
                offset_y: Some(offset_y),
                rotation: None,
                stretch_x: None,
                stretch_y: None,
            });
        }
        for property in [KeyProperty::OffsetX, KeyProperty::OffsetY] {
            if clip.is_keyed(property) {
                commands.push(Command::ClearClipKeys {
                    clip_id: clip.id.clone(),
                    property,
                });
            }
        }
    }
    if commands.is_empty() {
        return Ok(ToolOutput::text("no other titles on that track"));
    }
    support::apply(ctx, support::batch(commands))?;
    Ok(ToolOutput::text("applied the title's look to its track"))
}
