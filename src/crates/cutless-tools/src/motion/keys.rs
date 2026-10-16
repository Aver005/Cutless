// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Ключи: `clip_keys` для свойств клипа и `effect_keys` для параметров эффекта в его цепочке.

use concat_project::Command;
use concat_project::commands::keyspans::{KeyTarget, SpanKey};
use concat_project::model::{Clip, KeyEase};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{KeyInput, at_of, ease_of, keys_schema, package, param_keys};
use crate::edit::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты ключей.
pub(super) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "clip_keys",
        "Animate one property of a clip with keys at timeline seconds, as one undo step. Values: \
         scale 1 = fills the frame; offset_x/offset_y a fraction of the frame width/height from \
         centre, positive right/down; rotation degrees clockwise; opacity 0..1; volume gain, 1 = \
         unity. clear drops every key of the property first; remove drops the keys at those times. \
         A layer clip ignores scale, offsets and rotation: use effect_keys on cutless.camera.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "property": {
                    "type": "string",
                    "enum": ["scale", "offset_x", "offset_y", "rotation", "opacity", "volume"]
                },
                "keys": keys_schema("The property's value there, in its own units."),
                "remove": { "type": "array", "items": { "type": "number" }, "description": "Timeline seconds of keys to drop." },
                "clear": { "type": "boolean", "description": "Drop every key of the property before adding." }
            },
            "required": ["clip_id", "property"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        clip_keys,
    );
    support::add(
        registry,
        "effect_keys",
        "Animate one parameter of an effect in a clip's picture chain with keys at timeline \
         seconds, as one undo step. Name the effect by effect_id (its first entry in the chain) \
         or by entry index. Values are clamped into the parameter's range from effects_catalogue.",
        json!({
            "type": "object",
            "properties": {
                "clip_id": { "type": "string" },
                "effect_id": { "type": "string", "description": "Package id of an effect already in the chain." },
                "entry": { "type": "integer", "description": "Index in the chain, when the same effect is there twice." },
                "param": { "type": "string", "description": "The parameter key." },
                "keys": keys_schema("The parameter's value there, in its own units."),
                "remove": { "type": "array", "items": { "type": "number" }, "description": "Timeline seconds of keys to drop." },
                "clear": { "type": "boolean", "description": "Drop every key of the parameter before adding." }
            },
            "required": ["clip_id", "param"],
            "additionalProperties": false
        }),
        ToolClass::Edit,
        effect_keys,
    );
}

/// Вход `clip_keys`.
#[derive(Deserialize)]
struct ClipKeysInput {
    clip_id: String,
    property: String,
    #[serde(default)]
    keys: Vec<KeyInput>,
    #[serde(default)]
    remove: Vec<f64>,
    #[serde(default)]
    clear: bool,
}

/// Вход `effect_keys`.
#[derive(Deserialize)]
struct EffectKeysInput {
    clip_id: String,
    #[serde(default)]
    effect_id: Option<String>,
    #[serde(default)]
    entry: Option<usize>,
    param: String,
    #[serde(default)]
    keys: Vec<KeyInput>,
    #[serde(default)]
    remove: Vec<f64>,
    #[serde(default)]
    clear: bool,
}

/// Пустой запрос — ошибка: модель должна знать, что ничего не сделано.
fn something_to_do(keys: &[KeyInput], remove: &[f64], clear: bool) -> Result<(), ToolError> {
    if keys.is_empty() && remove.is_empty() && !clear {
        return Err(ToolError::BadInput(
            "nothing to do: pass keys, remove or clear".to_owned(),
        ));
    }
    Ok(())
}

/// Ставит и снимает ключи свойства клипа.
fn clip_keys(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: ClipKeysInput = crate::input(input)?;
    let property = support::property_of(&input.property)?;
    something_to_do(&input.keys, &input.remove, input.clear)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?;
    let clip_id = clip.id.clone();

    let mut commands = Vec::new();
    if input.clear {
        commands.push(Command::ClearClipKeys {
            clip_id: clip_id.clone(),
            property,
        });
    }
    for time in &input.remove {
        commands.push(Command::ClearClipKey {
            clip_id: clip_id.clone(),
            property,
            at: at_of(clip, *time)?,
        });
    }
    for key in &input.keys {
        commands.push(exact_key(
            &clip_id,
            KeyTarget::Property { property },
            at_of(clip, key.time)?,
            key.value,
            ease_of(key.ease.as_deref())?,
        ));
    }
    let after = support::apply(ctx, support::batch(commands))?;
    let count = support::find_clip(&after.project, &clip_id)?
        .keys_on(property)
        .count();
    Ok(ToolOutput {
        text: format!("{} of {clip_id}: {count} key(s) now", input.property),
        data: json!({ "clip_id": clip_id, "property": input.property, "keys": count }),
        ..ToolOutput::default()
    })
}

/// Ставит и снимает ключи параметра эффекта.
fn effect_keys(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: EffectKeysInput = crate::input(input)?;
    something_to_do(&input.keys, &input.remove, input.clear)?;
    let view = support::fetch(ctx)?;
    let clip = support::find_clip(&view.project, &input.clip_id)?;
    let entry = entry_of(clip, input.effect_id.as_deref(), input.entry)?;
    let effect_id = clip.video_effects[entry].id.clone();
    let packages = support::catalogue(ctx, None)?;
    let package = package(&packages, &effect_id)?;
    let param = package
        .params
        .iter()
        .find(|param| param.key == input.param)
        .ok_or_else(|| {
            ToolError::BadInput(format!(
                "{effect_id} has no parameter {:?}; its parameters: {}",
                input.param,
                param_keys(package)
            ))
        })?;
    if param.kind != "float" && param.kind != "int" {
        return Err(ToolError::BadInput(format!(
            "{} of {effect_id} is a {} and holds still: set it with effect_apply params",
            param.key, param.kind
        )));
    }

    let clip_id = clip.id.clone();
    let key = input.param.clone();
    let mut commands = Vec::new();
    if input.clear {
        commands.push(Command::ClearEffectKeys {
            clip_id: clip_id.clone(),
            entry,
            key: key.clone(),
        });
    }
    for time in &input.remove {
        commands.push(Command::ClearEffectKey {
            clip_id: clip_id.clone(),
            entry,
            key: key.clone(),
            at: at_of(clip, *time)?,
        });
    }
    let mut clamped = 0usize;
    for held in &input.keys {
        let value = held.value.clamp(param.min, param.max);
        if (value - held.value).abs() > 1e-9 {
            clamped += 1;
        }
        commands.push(exact_key(
            &clip_id,
            KeyTarget::Effect {
                entry,
                key: key.clone(),
            },
            at_of(clip, held.time)?,
            value,
            ease_of(held.ease.as_deref())?,
        ));
    }
    let after = support::apply(ctx, support::batch(commands))?;
    let count = support::find_clip(&after.project, &clip_id)?
        .video_effects
        .get(entry)
        .map_or(0, |effect| effect.keys_on(&key).len());
    let mut text = format!("{key} of {effect_id} (entry {entry}) on {clip_id}: {count} key(s) now");
    if clamped > 0 {
        text.push_str(&format!(
            "; {clamped} value(s) clamped into {}..{}",
            param.min, param.max
        ));
    }
    Ok(ToolOutput {
        text,
        data: json!({ "clip_id": clip_id, "entry": entry, "effect_id": effect_id, "keys": count }),
        ..ToolOutput::default()
    })
}

/// Индекс эффекта в цепочке: по номеру (сверяя id, если он дан) или по первому вхождению id.
fn entry_of(
    clip: &Clip,
    effect_id: Option<&str>,
    entry: Option<usize>,
) -> Result<usize, ToolError> {
    let chain = &clip.video_effects;
    let ids = || {
        let ids: Vec<&str> = chain.iter().map(|effect| effect.id.as_str()).collect();
        if ids.is_empty() {
            "none".to_owned()
        } else {
            ids.join(", ")
        }
    };
    match (entry, effect_id) {
        (Some(index), id) => {
            let found = chain.get(index).ok_or_else(|| {
                ToolError::BadInput(format!(
                    "{} has no entry {index}; its chain: {}",
                    clip.id,
                    ids()
                ))
            })?;
            if id.is_some_and(|id| id != found.id) {
                return Err(ToolError::BadInput(format!(
                    "entry {index} of {} is {}, not {}",
                    clip.id,
                    found.id,
                    id.unwrap_or_default()
                )));
            }
            Ok(index)
        }
        (None, Some(id)) => chain
            .iter()
            .position(|effect| effect.id == id)
            .ok_or_else(|| {
                ToolError::BadInput(format!(
                    "{} has no {id} in its chain ({}); add it with effect_apply first",
                    clip.id,
                    ids()
                ))
            }),
        (None, None) => Err(ToolError::BadInput(
            "name the effect: effect_id or entry".to_owned(),
        )),
    }
}

/// Ключ ровно в точке `at`: в отличие от `SetClipKey`, не съедает соседние ключи частой серии.
fn exact_key(clip_id: &str, target: KeyTarget, at: f64, value: f64, ease: KeyEase) -> Command {
    Command::SetKeySpan {
        clip_id: clip_id.to_owned(),
        target,
        from: at,
        to: at,
        keys: vec![SpanKey { at, value, ease }],
    }
}
