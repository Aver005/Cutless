// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты вывода: кадр «зрения» и запуск экспорта.

use serde::Deserialize;
use serde_json::{Value, json};

use super::support;
use crate::jobs::JobKind;
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolImage, ToolOutput};

/// Регистрирует инструменты вывода.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "frame_at",
        "Render the composited frame at a timeline time, in seconds, and return it as a PNG image.",
        json!({
            "type": "object",
            "properties": {
                "time": { "type": "number" },
                "width": { "type": "integer", "description": "Frame width; the timeline's when absent" }
            },
            "required": ["time"]
        }),
        ToolClass::Read,
        frame_at,
    );
    support::add(
        registry,
        "export_start",
        "Render the active timeline to a file as a background job and return its id; \
         job_status waits for the file and job_cancel stops it.",
        json!({
            "type": "object",
            "properties": { "output": { "type": "string" } },
            "required": ["output"]
        }),
        ToolClass::Danger,
        export_start,
    );
}

/// Вход `frame_at`.
#[derive(Deserialize)]
struct FrameInput {
    time: f64,
    #[serde(default)]
    width: Option<u32>,
}

/// Вход `export_start`.
#[derive(Deserialize)]
struct ExportInput {
    output: String,
}

/// Отдаёт кадр картинкой в base64.
fn frame_at(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: FrameInput = crate::input(input)?;
    let path = ctx.project()?;
    let (width, height) = match input.width {
        Some(width) => {
            let view = support::fetch(ctx)?;
            let aspect = view.settings.height as f64 / view.settings.width.max(1) as f64;
            let height = ((width as f64 * aspect).round() as u32).max(1);
            (Some(width), Some(height))
        }
        None => (None, None),
    };
    let value = ctx.call(concat_api::Request::PreviewFrame {
        path,
        time: input.time,
        output: None,
        width,
        height,
    })?;
    let picture: concat_api::Picture =
        serde_json::from_value(value).map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput {
        text: format!(
            "frame at {} ({}x{})",
            support::time(input.time),
            picture.width,
            picture.height
        ),
        data: json!({ "width": picture.width, "height": picture.height }),
        images: vec![ToolImage {
            mime: "image/png".to_owned(),
            base64: picture.png,
        }],
    })
}

/// Запускает экспорт дорожки времени в файл.
fn export_start(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: ExportInput = crate::input(input)?;
    let path = ctx.project()?;
    let value = ctx.call(concat_api::Request::ExportRun {
        path,
        spec: concat_api::ExportSpec {
            output: input.output.clone(),
            ..concat_api::ExportSpec::default()
        },
    })?;
    let started: concat_api::Started =
        serde_json::from_value(value).map_err(|error| ToolError::Failed(error.to_string()))?;
    ctx.jobs.track(&started.job, JobKind::Export, &input.output);
    Ok(ToolOutput {
        text: format!(
            "Export started as job {} -> {}. Call job_status with job_id \"{}\" to wait for the file.",
            started.job, input.output, started.job
        ),
        data: json!({ "job": started.job, "output": input.output }),
        ..ToolOutput::default()
    })
}
