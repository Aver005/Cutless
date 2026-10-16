// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты эффектов: каталог, добавить эффект в цепочку, убрать его.

use std::collections::BTreeMap;

use concat_api::PackageInfo;
use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::AppliedFilter;
use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// До скольких найденных пакетов каталог показывает диапазоны параметров.
const DETAIL_LIMIT: usize = 12;

/// Регистрирует инструменты эффектов.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "effects_catalogue",
        "List effect packages with their ids, names and parameter keys. Filter by kind or a text query; when 12 or fewer match, each parameter shows min..max=default and whether it takes keys.",
        json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["effect", "filter", "audio", "transition", "generator"] },
                "query": { "type": "string" }
            }
        }),
        ToolClass::Read,
        effects_catalogue,
    );
    support::add(
        registry,
        "effect_apply",
        "Add an effect to each named clip's picture chain, keeping the effects already there. params is a map of key to value.",
        json!({
            "type": "object",
            "properties": {
                "clip_ids": { "type": "array", "items": { "type": "string" } },
                "effect_id": { "type": "string" },
                "params": { "type": "object" }
            },
            "required": ["clip_ids", "effect_id"]
        }),
        ToolClass::Edit,
        effect_apply,
    );
    support::add(
        registry,
        "effect_remove",
        "Remove every entry of one effect id from a clip's picture chain.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "effect_id": { "type": "string" }
            },
            "required": ["clip_id", "effect_id"]
        }),
        ToolClass::Edit,
        effect_remove,
    );
}

/// Вход `effects_catalogue`.
#[derive(Deserialize)]
struct CatalogueInput {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

/// Вход `effect_apply`.
#[derive(Deserialize)]
struct ApplyInput {
    clip_ids: Vec<String>,
    effect_id: String,
    #[serde(default)]
    params: Option<BTreeMap<String, f64>>,
}

/// Вход `effect_remove`.
#[derive(Deserialize)]
struct RemoveInput {
    clip_id: String,
    effect_id: String,
}

/// Перечисляет каталог эффектов.
fn effects_catalogue(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: CatalogueInput = crate::input(input)?;
    let packages: Vec<PackageInfo> = support::catalogue(ctx, input.kind.as_deref())?;
    let query = input.query.as_deref().map(str::to_lowercase);
    let matched: Vec<&PackageInfo> = packages
        .iter()
        .filter(|package| {
            query.as_deref().is_none_or(|needle| {
                package.id.to_lowercase().contains(needle)
                    || package.name.to_lowercase().contains(needle)
                    || package.description.to_lowercase().contains(needle)
            })
        })
        .collect();
    // Узкая выборка — с диапазонами и пометкой ключей, широкая — только имена, чтобы ответ не раздувался.
    let detailed = matched.len() <= DETAIL_LIMIT;
    let lines: Vec<String> = matched
        .iter()
        .map(|package| {
            let params: Vec<String> = package
                .params
                .iter()
                .map(|param| {
                    if detailed {
                        let keys = if param.animate { ", keys" } else { "" };
                        format!(
                            "{}({}..{}={}{keys})",
                            param.key,
                            support::num(param.min),
                            support::num(param.max),
                            support::num(param.default)
                        )
                    } else {
                        param.key.clone()
                    }
                })
                .collect();
            let knobs = if params.is_empty() {
                String::new()
            } else {
                format!("  params: {}", params.join(","))
            };
            format!(
                "{} — {} [{}]{}",
                package.id, package.name, package.kind, knobs
            )
        })
        .collect();
    let text = if lines.is_empty() {
        "no matching packages".to_owned()
    } else {
        lines.join("\n")
    };
    let ids: Vec<&str> = matched.iter().map(|package| package.id.as_str()).collect();
    Ok(ToolOutput {
        text,
        data: json!({ "ids": ids }),
        ..ToolOutput::default()
    })
}

/// Добавляет эффект в цепочку каждого клипа.
fn effect_apply(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: ApplyInput = crate::input(input)?;
    support::some_clips(&input.clip_ids)?;
    let view = support::fetch(ctx)?;
    let mut commands = Vec::new();
    for clip_id in &input.clip_ids {
        let clip = support::find_clip(&view.project, clip_id)?;
        let mut chain = clip.video_effects.clone();
        chain.push(AppliedFilter {
            id: input.effect_id.clone(),
            params: input.params.clone().unwrap_or_default(),
            enabled: true,
            keys: BTreeMap::new(),
        });
        commands.push(Command::UpdateClip {
            clip_id: clip_id.clone(),
            patch: ClipPatch {
                video_effects: Some(chain),
                ..ClipPatch::default()
            },
        });
    }
    support::apply(ctx, support::batch(commands))?;
    Ok(ToolOutput::text(format!(
        "applied {} to {} clips",
        input.effect_id,
        input.clip_ids.len()
    )))
}

/// Убирает эффект из цепочки клипа.
fn effect_remove(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: RemoveInput = crate::input(input)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?;
    let chain: Vec<AppliedFilter> = clip
        .video_effects
        .iter()
        .filter(|effect| effect.id != input.effect_id)
        .cloned()
        .collect();
    if chain.len() == clip.video_effects.len() {
        return Ok(ToolOutput::text(format!(
            "{} has no effect {}",
            input.clip_id, input.effect_id
        )));
    }
    support::apply(
        ctx,
        Command::UpdateClip {
            clip_id: input.clip_id.clone(),
            patch: ClipPatch {
                video_effects: Some(chain),
                ..ClipPatch::default()
            },
        },
    )?;
    Ok(ToolOutput::text(format!(
        "removed {} from {}",
        input.effect_id, input.clip_id
    )))
}
