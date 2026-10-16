// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты движения для эдитов: ключи клипа и эффектов, скорость, слои, переходы, вид.
//! Время во входе — секунды таймлайна; в доли клипа переводит [`at_of`].

mod hit;
mod keys;
mod look;

use concat_api::PackageInfo;
use concat_project::model::{Clip, KeyEase};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Registry, ToolError};

/// Регистрирует инструменты движения.
pub fn register(registry: &mut Registry) {
    keys::register(registry);
    look::register(registry);
    hit::register(registry);
}

/// Насколько время может выйти за край клипа и всё ещё считаться краем, в секундах.
const EDGE_SLACK: f64 = 0.001;

/// Один ключ во входе: время таймлайна, значение, плавность.
#[derive(Deserialize)]
struct KeyInput {
    time: f64,
    value: f64,
    #[serde(default)]
    ease: Option<String>,
}

/// Схема списка ключей; `value` описывает единицы значения.
fn keys_schema(value: &str) -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "time": { "type": "number", "description": "Timeline seconds, inside the clip." },
                "value": { "type": "number", "description": value },
                "ease": {
                    "type": "string",
                    "enum": ["linear", "in", "out", "in_out"],
                    "description": "How the key is approached from the one before it; linear when absent."
                }
            },
            "required": ["time", "value"],
            "additionalProperties": false
        }
    })
}

/// Плавность по имени из входа.
fn ease_of(name: Option<&str>) -> Result<KeyEase, ToolError> {
    match name.unwrap_or("linear") {
        "linear" => Ok(KeyEase::LINEAR),
        "in" => Ok(KeyEase::IN),
        "out" => Ok(KeyEase::OUT),
        "in_out" => Ok(KeyEase::IN_OUT),
        other => Err(ToolError::BadInput(format!(
            "unknown ease {other:?}: linear, in, out or in_out"
        ))),
    }
}

/// Доля клипа для секунды таймлайна; время вне клипа — ошибка с его границами.
fn at_of(clip: &Clip, time: f64) -> Result<f64, ToolError> {
    let end = clip.start + clip.duration;
    if !time.is_finite()
        || clip.duration <= 0.0
        || time < clip.start - EDGE_SLACK
        || time > end + EDGE_SLACK
    {
        return Err(ToolError::BadInput(format!(
            "time {time} is outside clip {} ({:.3}-{:.3})",
            clip.id, clip.start, end
        )));
    }
    Ok(((time - clip.start) / clip.duration).clamp(0.0, 1.0))
}

/// Пакет по id или понятная ошибка.
fn package<'a>(packages: &'a [PackageInfo], id: &str) -> Result<&'a PackageInfo, ToolError> {
    packages
        .iter()
        .find(|package| package.id == id)
        .ok_or_else(|| ToolError::BadInput(format!("no package {id:?} - see effects_catalogue")))
}

/// Ключи параметров пакета через запятую, для подсказок.
fn param_keys(package: &PackageInfo) -> String {
    let keys: Vec<&str> = package
        .params
        .iter()
        .map(|param| param.key.as_str())
        .collect();
    keys.join(", ")
}

#[cfg(test)]
mod tests;
