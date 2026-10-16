// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Скорость, слой-обработка, переход и вид клипа: `clip_speed`, `layer_add`, `transition_set`, `clip_look`.

use std::collections::BTreeMap;

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{SpeedPoint, Transition};
use cutless_motion::SpeedPreset;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{package, param_keys};
use crate::edit::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Режимы наложения, которые знает движок.
const BLENDS: [&str; 6] = ["normal", "multiply", "screen", "add", "lighten", "darken"];

/// Регистрирует инструменты вида.
pub(super) fn register(registry: &mut Registry) {
    let presets: Vec<&str> = SpeedPreset::ALL
        .iter()
        .map(|preset| preset.name())
        .collect();
    support::add(
        registry,
        "clip_speed",
        "Retime clips: a constant speed, a speed ramp preset, or your own curve; constant: true \
         drops a curve. The source covered stays the same, so the clip gets longer or shorter. A \
         constant slow-down pushes later clips on the lane along; a curve or preset moves nothing \
         and may overlap the next clip - check timeline_summary. Retime first, then add hits: \
         keys sit at fractions of the clip and stretch with it.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "speed": { "type": "number", "description": "Constant rate, 0.0625..16." },
                "preset": { "type": "string", "enum": presets },
                "hit": { "type": "number", "description": "ramp_to_hit only: where the slow-mo peak falls, a fraction of the clip 0.2..0.8." },
                "hit_source": { "type": "number", "description": "ramp_to_hit only: the second of the clip's file the peak should land on, e.g. the kill in a game recording." },
                "points": {
                    "type": "array",
                    "description": "Own curve: speed (0.0625..16) at fractions of the clip's timeline length (0..1), joined by straight lines.",
                    "items": {
                        "type": "object",
                        "properties": { "at": { "type": "number" }, "speed": { "type": "number" } },
                        "required": ["at", "speed"],
                        "additionalProperties": false
                    }
                },
                "constant": { "type": "boolean", "description": "Drop the curve, keep the mean rate." }
            },
            "required": ["clip_ids"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        clip_speed,
    );
    support::add(
        registry,
        "layer_add",
        "Place an adjustment layer: one effect over everything beneath it for a span of the \
         timeline. Use cutless.camera for punch, shake and spin across cuts (a layer has no \
         transform of its own), or any look. params sets knobs from the start.",
        json!({
            "type": "object",
            "properties": {
                "effect_id": { "type": "string" },
                "start": { "type": "number", "description": "Timeline seconds." },
                "duration": { "type": "number", "description": "Seconds; a few seconds when absent." },
                "params": { "type": "object", "additionalProperties": { "type": "number" } },
                "track_id": { "type": "string", "description": "A lane above the video; the first free one when absent." },
                "name": { "type": "string" }
            },
            "required": ["effect_id", "start"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        layer_add,
    );
    support::add(
        registry,
        "transition_set",
        "Put a transition on the cut into a clip, or remove it (no transition_id). The clip must \
         start where another ends on its track. Ids from effects_catalogue with kind transition.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string", "description": "The incoming clip." },
                "transition_id": { "type": "string" },
                "duration": { "type": "number", "description": "Seconds, 0.5 when absent; held to both clips." }
            },
            "required": ["clip_id"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        transition_set,
    );
    support::add(
        registry,
        "clip_look",
        "Set how clips sit in the picture: opacity 0..1, blend mode, horizontal and vertical flip.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "opacity": { "type": "number" },
                "blend": { "type": "string", "enum": BLENDS },
                "flip_h": { "type": "boolean" },
                "flip_v": { "type": "boolean" }
            },
            "required": ["clip_ids"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        clip_look,
    );
}

/// Вход `clip_speed`.
#[derive(Deserialize)]
struct SpeedInput {
    clip_ids: Vec<String>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    hit: Option<f64>,
    #[serde(default)]
    hit_source: Option<f64>,
    #[serde(default)]
    points: Option<Vec<SpeedPoint>>,
    #[serde(default)]
    constant: bool,
}

/// Вход `layer_add`.
#[derive(Deserialize)]
struct LayerInput {
    effect_id: String,
    start: f64,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    params: BTreeMap<String, f64>,
    #[serde(default)]
    track_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// Вход `transition_set`.
#[derive(Deserialize)]
struct TransitionInput {
    clip_id: String,
    #[serde(default)]
    transition_id: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
}

/// Вход `clip_look`.
#[derive(Deserialize)]
struct LookInput {
    clip_ids: Vec<String>,
    #[serde(default)]
    opacity: Option<f64>,
    #[serde(default)]
    blend: Option<String>,
    #[serde(default)]
    flip_h: Option<bool>,
    #[serde(default)]
    flip_v: Option<bool>,
}

/// Меняет скорость клипов и сообщает их новую длину.
fn clip_speed(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: SpeedInput = crate::input(input)?;
    support::some_clips(&input.clip_ids)?;
    let chosen = [
        input.speed.is_some(),
        input.preset.is_some(),
        input.points.is_some(),
        input.constant,
    ];
    if chosen.iter().filter(|set| **set).count() != 1 {
        return Err(ToolError::BadInput(
            "give exactly one of speed, preset, points or constant".to_owned(),
        ));
    }
    let preset = match input.preset.as_deref() {
        Some(name) => Some(SpeedPreset::from_name(name).ok_or_else(|| {
            ToolError::BadInput(format!("unknown preset {name:?} - see the tool schema"))
        })?),
        None => None,
    };
    if input.points.as_ref().is_some_and(|points| points.len() < 2) {
        return Err(ToolError::BadInput(
            "a curve needs at least two points".to_owned(),
        ));
    }

    let before = support::fetch(ctx)?;
    let mut commands = Vec::new();
    let mut peaks = Vec::new();
    for clip_id in &input.clip_ids {
        let clip = support::find_clip(&before.project, clip_id)?;
        let curve = match (preset, &input.points) {
            (Some(SpeedPreset::RampToHit), _) => {
                let hit = ramp_hit(clip, input.hit, input.hit_source)?;
                peaks.push((clip_id.clone(), hit));
                Some(SpeedPreset::RampToHit.points(hit))
            }
            (Some(preset), _) => Some(preset.points(0.5)),
            (None, Some(points)) => Some(points.clone()),
            (None, None) => None,
        };
        commands.push(match (curve, input.speed) {
            (Some(curve), _) => Command::SetClipSpeedCurve {
                clip_id: clip_id.clone(),
                curve: Some(curve),
            },
            // Постоянная скорость сама снимает кривую.
            (None, Some(speed)) => Command::SetClipSpeed {
                clip_id: clip_id.clone(),
                speed,
            },
            (None, None) => Command::SetClipSpeedCurve {
                clip_id: clip_id.clone(),
                curve: None,
            },
        });
    }
    let after = support::apply(ctx, support::batch(commands))?;
    let mut lines = Vec::new();
    for clip_id in &input.clip_ids {
        let old = support::find_clip(&before.project, clip_id)?;
        let new = support::find_clip(&after.project, clip_id)?;
        let shape = if new.speed_curve.is_some() {
            "curve"
        } else {
            "constant"
        };
        let mut line = format!(
            "{clip_id}: {}-{} ({} s, was {} s), {shape} speed, mean {}",
            support::time(new.start),
            support::time(new.start + new.duration),
            support::num(new.duration),
            support::num(old.duration),
            support::num(new.speed)
        );
        if let Some((_, hit)) = peaks.iter().find(|(id, _)| id == clip_id) {
            line.push_str(&format!(
                ", slow-mo peak at {}",
                support::time(new.start + hit * new.duration)
            ));
        }
        lines.push(line);
    }
    Ok(ToolOutput::text(lines.join(
        "
",
    )))
}

/// Где пик рампы: по секунде исходника, по доле клипа или посередине.
fn ramp_hit(
    clip: &concat_project::model::Clip,
    hit: Option<f64>,
    hit_source: Option<f64>,
) -> Result<f64, ToolError> {
    let Some(source) = hit_source else {
        return Ok(hit.unwrap_or(0.5));
    };
    let covered = clip.duration * clip.speed;
    let share = (source - clip.source_start) / covered.max(f64::EPSILON);
    if !(0.0..=1.0).contains(&share) {
        return Err(ToolError::BadInput(format!(
            "hit_source {source} s is outside what {} plays of its file ({:.3}-{:.3} s)",
            clip.id,
            clip.source_start,
            clip.source_start + covered
        )));
    }
    Ok(cutless_motion::speed::ramp_hit_for_share(share))
}

/// Ставит слой-обработку с пакетом.
fn layer_add(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: LayerInput = crate::input(input)?;
    let packages = support::catalogue(ctx, None)?;
    let package = package(&packages, &input.effect_id)?;
    if package.kind != "effect" && package.kind != "filter" {
        return Err(ToolError::BadInput(format!(
            "{} is a {}, a layer takes a picture effect or filter",
            package.id, package.kind
        )));
    }
    // Значения — в границы манифеста, как у effect_keys; модель узнает, что поправлено.
    let mut params = BTreeMap::new();
    let mut clamped = 0usize;
    for (key, value) in &input.params {
        let param = package
            .params
            .iter()
            .find(|param| &param.key == key)
            .ok_or_else(|| {
                ToolError::BadInput(format!(
                    "{} has no parameter {key:?}; its parameters: {}",
                    package.id,
                    param_keys(package)
                ))
            })?;
        let held = value.clamp(param.min, param.max);
        if (held - value).abs() > 1e-9 {
            clamped += 1;
        }
        params.insert(key.clone(), held);
    }
    let name = input.name.clone().unwrap_or_else(|| package.name.clone());
    let view = support::apply(
        ctx,
        Command::AddLayerClip {
            track_id: input.track_id.clone(),
            start: input.start,
            duration: input.duration,
            effect_id: input.effect_id.clone(),
            name,
            params,
        },
    )?;
    let id = view
        .created_id
        .clone()
        .ok_or_else(|| ToolError::Failed("the editor placed no layer".to_owned()))?;
    let clip = support::find_clip(&view.project, &id)?;
    let mut text = format!(
        "added layer {id} {}-{} on {} with {}",
        support::time(clip.start),
        support::time(clip.start + clip.duration),
        clip.track_id,
        input.effect_id
    );
    if clamped > 0 {
        text.push_str(&format!("; {clamped} param(s) clamped into range"));
    }
    Ok(ToolOutput {
        text,
        data: json!({ "createdId": id }),
        ..ToolOutput::default()
    })
}

/// Ставит или снимает переход на входе клипа.
fn transition_set(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: TransitionInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?;
    let Some(transition_id) = input.transition_id else {
        support::apply(
            ctx,
            Command::UpdateClip {
                clip_id: input.clip_id.clone(),
                patch: ClipPatch {
                    transition_in: Some(None),
                    ..ClipPatch::default()
                },
            },
        )?;
        return Ok(ToolOutput::text(format!(
            "removed the transition into {}",
            input.clip_id
        )));
    };
    let transitions = support::catalogue(ctx, Some("transition"))?;
    package(&transitions, &transition_id)?;

    let frame = view.settings.frame();
    let outgoing = view
        .project
        .active()
        .clips
        .iter()
        .filter(|other| other.id != clip.id && other.track_id == clip.track_id)
        .find(|other| (other.start + other.duration - clip.start).abs() < frame / 2.0)
        .ok_or_else(|| {
            ToolError::Failed(format!(
                "no clip ends where {} starts on its track: a transition needs a cut",
                clip.id
            ))
        })?;
    let room = outgoing.duration.min(clip.duration);
    if room < frame {
        return Err(ToolError::Failed(
            "the clips at the cut are shorter than a frame".to_owned(),
        ));
    }
    let duration = input.duration.unwrap_or(0.5).clamp(frame, room);
    support::apply(
        ctx,
        Command::UpdateClip {
            clip_id: clip.id.clone(),
            patch: ClipPatch {
                transition_in: Some(Some(Transition {
                    id: transition_id.clone(),
                    duration,
                })),
                ..ClipPatch::default()
            },
        },
    )?;
    Ok(ToolOutput::text(format!(
        "{transition_id} into {} from {}, {} s",
        clip.id,
        outgoing.id,
        support::num(duration)
    )))
}

/// Меняет прозрачность, наложение и отражение клипов.
fn clip_look(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: LookInput = crate::input(input)?;
    support::some_clips(&input.clip_ids)?;
    if let Some(blend) = input.blend.as_deref()
        && !BLENDS.contains(&blend)
    {
        return Err(ToolError::BadInput(format!(
            "unknown blend {blend:?}: {}",
            BLENDS.join(", ")
        )));
    }
    let patch = ClipPatch {
        opacity: input.opacity.map(|opacity| opacity.clamp(0.0, 1.0)),
        blend: input.blend.clone(),
        flip_h: input.flip_h,
        flip_v: input.flip_v,
        ..ClipPatch::default()
    };
    if patch == ClipPatch::default() {
        return Err(ToolError::BadInput(
            "nothing to change: opacity, blend, flip_h or flip_v".to_owned(),
        ));
    }
    let view = support::fetch(ctx)?;
    let mut commands = Vec::new();
    for clip_id in &input.clip_ids {
        support::find_clip(&view.project, clip_id)?;
        commands.push(Command::UpdateClip {
            clip_id: clip_id.clone(),
            patch: patch.clone(),
        });
    }
    support::apply(ctx, support::batch(commands))?;
    Ok(ToolOutput::text(format!(
        "changed the look of {} clip(s)",
        input.clip_ids.len()
    )))
}
