// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструмент чистки звука `audio_clean`: добавляет звуковой пакет к
//! цепочке клипа, не стирая уже настроенную.

use std::collections::BTreeMap;
use std::sync::Arc;

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{AppliedFilter, Clip, Project};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Tool, ToolClass, ToolContext, ToolError, ToolOutput};

use super::{apply_command, is_music, is_sound_source, output, package_of, read_project};

/// `audio_clean`: звуковой пакет в цепочку фильтров клипа.
pub(super) struct AudioClean;

/// Вход чистки: пресет и, при желании, его ручки.
#[derive(Deserialize)]
struct CleanInput {
    #[serde(default)]
    clip_ids: Option<Vec<String>>,
    preset: String,
    #[serde(default)]
    params: Option<BTreeMap<String, f64>>,
}

impl Tool for AudioClean {
    fn spec(&self) -> crate::ToolSpec {
        crate::ToolSpec {
            name: "audio_clean",
            description: "Add a sound-cleaning package to the audio chain of clips, in front of \
                whatever is already there. Applying the same preset again updates its knobs \
                instead of adding a second copy. One undo step.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Clips to clean. Default: every clip with sound on the active timeline, except background music."
                    },
                    "preset": {
                        "type": "string",
                        "enum": ["phone_voice", "noise_gate", "band_cut", "denoise", "enhance_voice"],
                        "description": "phone_voice: rumble and hiss, voice forward; noise_gate: quiet between phrases; band_cut: low and high cut; denoise: noise reduction; enhance_voice: full voice cleanup."
                    },
                    "params": {
                        "type": "object",
                        "description": "Knob values for the preset, keyed by the package's parameter names (e.g. amount, threshold).",
                        "additionalProperties": { "type": "number" }
                    }
                },
                "required": ["preset"],
                "additionalProperties": false
            }),
            class: ToolClass::Edit,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, value: Value) -> Result<ToolOutput, ToolError> {
        let input: CleanInput = crate::input(value)?;
        let package = package_of(&input.preset)?;
        let params = input.params.unwrap_or_default();
        let project = read_project(ctx)?;
        let chosen = chosen_clips(&project, input.clip_ids.as_deref())?;
        if chosen.is_empty() {
            return Ok(ToolOutput::text("no clips with sound to clean"));
        }

        let mut commands = Vec::with_capacity(chosen.len());
        let mut clips = Vec::with_capacity(chosen.len());
        let mut added = 0usize;
        let mut updated = 0usize;
        for clip in chosen {
            let mut chain = clip.filters.clone();
            if let Some(entry) = chain.iter_mut().find(|entry| entry.id == package) {
                entry.enabled = true;
                for (key, value) in &params {
                    entry.params.insert(key.clone(), *value);
                }
                updated += 1;
            } else {
                let mut entry = AppliedFilter::new(package);
                for (key, value) in &params {
                    entry.params.insert(key.clone(), *value);
                }
                chain.push(entry);
                added += 1;
            }
            clips.push(clip.id.clone());
            commands.push(Command::UpdateClip {
                clip_id: clip.id.clone(),
                patch: ClipPatch {
                    filters: Some(chain),
                    ..ClipPatch::default()
                },
            });
        }
        apply_command(ctx, Command::Batch { commands })?;

        let text = format!(
            "{} on {} clip(s): {added} added, {updated} updated",
            input.preset,
            clips.len()
        );
        let data = json!({
            "preset": input.preset,
            "package_id": package,
            "clips": clips,
            "added": added,
            "updated": updated,
        });
        Ok(output(text, data))
    }
}

/// Клипы, которым добавляем пакет: названные, иначе все со звуком без музыки.
fn chosen_clips<'a>(
    project: &'a Project,
    clip_ids: Option<&[String]>,
) -> Result<Vec<&'a Clip>, ToolError> {
    let timeline = project.active();
    match clip_ids {
        Some(ids) => {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                let clip = timeline.clip(id).ok_or_else(|| {
                    ToolError::Failed(format!("clip {id} is not on the active timeline"))
                })?;
                if !is_sound_source(project, clip) {
                    return Err(ToolError::Failed(format!(
                        "clip {} has no sound to clean",
                        clip.id
                    )));
                }
                out.push(clip);
            }
            Ok(out)
        }
        None => Ok(timeline
            .clips
            .iter()
            .map(Arc::as_ref)
            .filter(|clip| !is_music(clip) && is_sound_source(project, clip))
            .collect()),
    }
}
