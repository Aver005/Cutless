// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты медиатеки: импорт файла и список.

use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты медиатеки.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "media_import",
        "Probe a file and add it to the project's media bin. Returns the new media id.",
        json!({
            "type": "object",
            "properties": { "file": { "type": "string", "description": "File to import" } },
            "required": ["file"]
        }),
        ToolClass::Edit,
        media_import,
    );
    support::add(
        registry,
        "media_list",
        "List the media in the bin: id, kind, name, duration and path.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Read,
        media_list,
    );
}

/// Вход `media_import`.
#[derive(Deserialize)]
struct ImportInput {
    file: String,
}

/// Импортирует файл в медиатеку.
fn media_import(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: ImportInput = crate::input(input)?;
    let path = ctx.project()?;
    let view = support::parse_view(ctx.call(concat_api::Request::MediaImport {
        path,
        file: input.file.clone(),
    })?)?;
    let id = view.created_id.clone().unwrap_or_default();
    Ok(ToolOutput {
        text: if id.is_empty() {
            format!("{} was already in the bin", input.file)
        } else {
            format!("imported {} as {}", input.file, id)
        },
        data: json!({ "mediaId": id }),
        ..ToolOutput::default()
    })
}

/// Перечисляет медиатеку одной строкой на файл.
fn media_list(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let view = support::fetch(ctx)?;
    let mut lines = Vec::new();
    for item in &view.project.media {
        let duration = item
            .duration
            .map(|seconds| format!(" {}", support::time(seconds)))
            .unwrap_or_default();
        lines.push(format!(
            "{}  {}  \"{}\"{duration}  {}",
            item.id,
            media_kind(item.kind),
            item.name,
            item.path
        ));
    }
    let text = if lines.is_empty() {
        "no media in the bin".to_owned()
    } else {
        lines.join("\n")
    };
    Ok(ToolOutput {
        text,
        data: json!({ "count": view.project.media.len() }),
        ..ToolOutput::default()
    })
}

/// Имя вида медиа для списка.
fn media_kind(kind: concat_project::model::MediaKind) -> &'static str {
    match kind {
        concat_project::model::MediaKind::Video => "video",
        concat_project::model::MediaKind::Audio => "audio",
        concat_project::model::MediaKind::Image => "image",
    }
}
