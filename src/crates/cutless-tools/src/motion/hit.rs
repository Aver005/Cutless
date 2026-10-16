// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! `hit_fx` — удары в моменты музыки: наезд, тряска, вспышка и другие, одним шагом undo.

use concat_project::Command;
use concat_project::model::ClipKind;
use cutless_motion::hits::{
    self, CAMERA, CAMERA_X, CAMERA_Y, CAMERA_ZOOM, HitKind, Hits, ParamRange,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::package;
use crate::beats::{PickInput, picked};
use crate::edit::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструмент ударов.
pub(super) fn register(registry: &mut Registry) {
    let kinds: Vec<&str> = HitKind::ALL.iter().map(|kind| kind.name()).collect();
    let picks: Vec<&str> = concat_project::beats::Pick::ALL
        .iter()
        .map(|pick| pick.name())
        .collect();
    support::add(
        registry,
        "hit_fx",
        "Accent moments of a clip, usually beats: punch (zoom in and back), shake, flash, bw \
         (black and white), rgb (channel split), glow, blur (zoom blur). Keys every frame of the \
         decay, one undo step. strength 1 is a normal hit, 2 the drop. On an adjustment layer \
         punch and shake move a cutless.camera, so the whole picture under it moves across cuts. \
         Do not flash every cut: one flash per section reads as a drop, more reads as noise. \
         Hits snap to frames; a hit on a cut belongs to the incoming clip or to a layer over the \
         cut. Calling it again on the same times stacks: clear the keys first with clip_keys or \
         effect_keys.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "kind": { "type": "string", "enum": kinds },
                "times": { "type": "array", "items": { "type": "number" }, "description": "Timeline seconds inside the clip; or use `on`." },
                "on": {
                    "type": "object",
                    "description": "Hit on a music clip's beat grid instead of times: moments outside the hit clip are skipped; every/offset count from the music clip's first moment in from..to.",
                    "properties": {
                        "clip_id": { "type": "string", "description": "The music clip with a grid (beats_analyze)." },
                        "what": { "type": "string", "enum": picks },
                        "from": { "type": "number" },
                        "to": { "type": "number" },
                        "every": { "type": "integer" },
                        "offset": { "type": "integer" }
                    },
                    "required": ["clip_id", "what"],
                    "additionalProperties": false
                },
                "strength": { "type": "number", "description": "0.1..3, 1 when absent." },
                "decay": { "type": "number", "description": "Seconds the hit takes to die away, a frame..2; each kind has its own default." }
            },
            "required": ["clip_id", "kind"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        hit_fx,
    );
}

/// Вход `hit_fx`.
#[derive(Deserialize)]
struct HitInput {
    clip_id: String,
    kind: String,
    #[serde(default)]
    times: Option<Vec<f64>>,
    #[serde(default)]
    on: Option<PickInput>,
    #[serde(default)]
    strength: Option<f64>,
    #[serde(default)]
    decay: Option<f64>,
}

/// Ставит удары на клип.
fn hit_fx(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: HitInput = crate::input(input)?;
    let kind = HitKind::from_name(&input.kind)
        .ok_or_else(|| ToolError::BadInput(format!("unknown hit kind {:?}", input.kind)))?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?;
    if clip.kind == ClipKind::Audio {
        return Err(ToolError::BadInput(format!(
            "{} is a sound clip; hit the video, a title or a layer over them",
            clip.id
        )));
    }
    let param = ensure_package(ctx, kind, clip.kind)?;
    let (times, skipped) = hit_times(
        &view.project,
        clip,
        input.times.as_deref(),
        input.on.as_ref(),
    )?;

    let settings = &view.settings;
    let hits = Hits {
        clip,
        fps: settings.rate(),
        aspect: settings.aspect(),
        times: times.clone(),
        strength: input.strength.unwrap_or(1.0),
        decay: input.decay,
        param,
    };
    let commands =
        hits::commands(kind, &hits).map_err(|error| ToolError::BadInput(error.to_string()))?;
    let keys: usize = commands
        .iter()
        .map(|command| match command {
            Command::SetKeySpan { keys, .. } => keys.len(),
            _ => 0,
        })
        .sum();
    let added = commands.iter().find_map(|command| match command {
        Command::UpdateClip { patch, .. } => patch
            .video_effects
            .as_ref()
            .and_then(|chain| chain.last())
            .map(|link| link.id.clone()),
        _ => None,
    });
    support::apply(ctx, support::batch(commands))?;
    let mut text = format!(
        "{} x{} on {}: {keys} key(s), one undo step",
        kind.name(),
        times.len(),
        input.clip_id
    );
    if skipped > 0 {
        text.push_str(&format!(
            "; {skipped} moment(s) outside the clip or past the limit skipped"
        ));
    }
    if let Some(package) = &added {
        text.push_str(&format!("; added {package} to its chain at rest"));
    }
    Ok(ToolOutput {
        text,
        data: json!({ "clip_id": input.clip_id, "kind": kind.name(), "keys": keys, "added": added }),
        ..ToolOutput::default()
    })
}

/// Пакет, который качает удар, есть в каталоге со всеми нужными параметрами.
/// Для удара-эффекта отдаёт границы и умолчание его параметра из манифеста.
fn ensure_package(
    ctx: &mut ToolContext<'_>,
    kind: HitKind,
    clip: ClipKind,
) -> Result<Option<ParamRange>, ToolError> {
    let (id, params): (&str, Vec<&str>) = match kind.effect() {
        Some(effect) => (effect.package, vec![effect.param]),
        None if clip == ClipKind::Layer => (
            CAMERA,
            [CAMERA_ZOOM, CAMERA_X, CAMERA_Y]
                .iter()
                .map(|param| param.key)
                .collect(),
        ),
        None => return Ok(None),
    };
    let packages = support::catalogue(ctx, None)?;
    let found = package(&packages, id).map_err(|_| {
        ToolError::Failed(format!(
            "{} needs the {id} package, which is not installed",
            kind.name()
        ))
    })?;
    let mut range = None;
    for key in params {
        let known = found
            .params
            .iter()
            .find(|known| known.key == key)
            .ok_or_else(|| ToolError::Failed(format!("{id} has no parameter {key}")))?;
        range = Some(ParamRange {
            min: known.min,
            max: known.max,
            default: known.default,
        });
    }
    Ok(range.filter(|_| kind.effect().is_some()))
}

/// Больше ударов за один вызов не ставится: у каждого — ключ на каждый кадр затухания.
const MAX_HITS: usize = 128;

/// Моменты ударов: данные времена или моменты сетки внутри клипа; сколько пропущено.
fn hit_times(
    project: &concat_project::model::Project,
    clip: &concat_project::model::Clip,
    times: Option<&[f64]>,
    on: Option<&PickInput>,
) -> Result<(Vec<f64>, usize), ToolError> {
    let all = match (times, on) {
        (Some(times), None) => return Ok((times.to_vec(), 0)),
        (None, Some(on)) => picked(project, on)?,
        _ => {
            return Err(ToolError::BadInput(
                "give exactly one of times or on".to_owned(),
            ));
        }
    };
    let total = all.len();
    let end = clip.start + clip.duration;
    let inside: Vec<f64> = all
        .into_iter()
        .filter(|time| *time >= clip.start && *time <= end)
        .take(MAX_HITS)
        .collect();
    if inside.is_empty() {
        return Err(ToolError::BadInput(format!(
            "none of the {total} moment(s) fall inside {} ({}-{})",
            clip.id,
            support::time(clip.start),
            support::time(end)
        )));
    }
    Ok((inside.clone(), total - inside.len()))
}
