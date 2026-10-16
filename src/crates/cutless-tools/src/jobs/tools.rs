// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты задач: `job_status` ждёт и читает итог, `job_cancel` останавливает.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{JobKind, JobState, WAIT_MAX, wait_from};
use crate::{Registry, Tool, ToolClass, ToolContext, ToolError, ToolOutput, ToolSpec};

/// Регистрирует инструменты задач.
pub(super) fn register(registry: &mut Registry) {
    registry.register(Box::new(JobStatus));
    registry.register(Box::new(JobCancel));
}

/// `job_status` — где задача, с ожиданием её конца.
struct JobStatus;

/// Вход `job_status`.
#[derive(Deserialize)]
struct StatusInput {
    #[serde(default)]
    job_id: Option<String>,
    #[serde(default)]
    wait_seconds: Option<f64>,
}

impl Tool for JobStatus {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "job_status",
            description: "Wait for a background job (transcribe, export_start) and return its \
                          result: a finished transcription answers like transcribe, a finished \
                          export names the file. Without job_id, lists every job of the session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "The job to wait for; every job is listed when absent." },
                    "wait_seconds": {
                        "type": "number",
                        "description": format!("How long to wait for the job to end; 20 when absent, at most {WAIT_MAX}. 0 only looks.")
                    }
                },
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: StatusInput = crate::input(input)?;
        let Some(id) = input.job_id else {
            let jobs = ctx.jobs.list();
            if jobs.is_empty() {
                return Ok(ToolOutput::text("No jobs in this session."));
            }
            let text: Vec<String> = jobs.iter().map(|job| job.line()).collect();
            let data: Vec<Value> = jobs.iter().map(|job| job.data()).collect();
            return Ok(ToolOutput {
                text: text.join("\n"),
                data: json!({ "jobs": data }),
                ..ToolOutput::default()
            });
        };
        let job = ctx
            .jobs
            .wait(&id, wait_from(input.wait_seconds))
            .ok_or_else(|| ToolError::BadInput(format!("no job {id} - see job_status")))?;
        match &job.state {
            // Итог задачи отдаём целиком, с данными и картинками, как отдал бы сам инструмент.
            JobState::Done(output) => Ok(ToolOutput {
                text: job.line(),
                ..output.clone()
            }),
            JobState::Failed(error) => {
                Err(ToolError::Failed(format!("job {} failed: {error}", job.id)))
            }
            JobState::Running { .. } => Ok(ToolOutput {
                text: job.hint(),
                data: job.data(),
                ..ToolOutput::default()
            }),
        }
    }
}

/// `job_cancel` — остановить задачу.
struct JobCancel;

/// Вход `job_cancel`.
#[derive(Deserialize)]
struct CancelInput {
    job_id: String,
}

impl Tool for JobCancel {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "job_cancel",
            description: "Stop a running background job. A cancelled export writes no file; a \
                          cancelled transcription leaves no transcript.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "The job to stop." }
                },
                "required": ["job_id"],
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: CancelInput = crate::input(input)?;
        let job = ctx.jobs.get(&input.job_id).ok_or_else(|| {
            ToolError::BadInput(format!("no job {} - see job_status", input.job_id))
        })?;
        if !job.is_running() {
            return Ok(ToolOutput::text(format!("{} Nothing to stop.", job.line())));
        }
        match job.kind {
            JobKind::Export => {
                ctx.call(concat_api::Request::ExportCancel {
                    job: job.id.clone(),
                })?;
            }
            JobKind::Transcribe => ctx.jobs.cancel(&job.id)?,
        }
        Ok(ToolOutput::text(format!(
            "Asked job {} to stop; job_status shows when it has.",
            job.id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Jobs;
    use crate::{Editor, ToolContext};
    use concat_api::{ApiError, Event, Request};

    /// Редактор, который запоминает запросы и на всё отвечает пустым объектом.
    #[derive(Default)]
    struct Seen {
        requests: Vec<Request>,
    }

    impl Editor for Seen {
        fn dispatch(&mut self, request: Request) -> Result<Value, ApiError> {
            self.requests.push(request);
            Ok(json!({}))
        }
    }

    fn run(
        tool: &dyn Tool,
        jobs: &Jobs,
        editor: &mut Seen,
        input: Value,
    ) -> Result<ToolOutput, ToolError> {
        let mut ctx = ToolContext {
            editor,
            project: None,
            jobs: jobs.clone(),
        };
        tool.call(&mut ctx, input)
    }

    #[test]
    fn status_returns_the_finished_jobs_output() {
        let jobs = Jobs::default();
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                "c1",
                || {},
                |_| {
                    Ok(ToolOutput {
                        text: "Transcribed clip c1".to_owned(),
                        data: json!({ "clip_id": "c1" }),
                        ..ToolOutput::default()
                    })
                },
            )
            .expect("started");
        let mut editor = Seen::default();
        let output = run(
            &JobStatus,
            &jobs,
            &mut editor,
            json!({ "job_id": id, "wait_seconds": 5 }),
        )
        .expect("status");
        assert!(
            output.text.contains("done. Transcribed clip c1"),
            "{}",
            output.text
        );
        assert_eq!(output.data["clip_id"], "c1");
        jobs.shutdown();
    }

    #[test]
    fn status_of_a_failed_job_is_an_error() {
        let jobs = Jobs::default();
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                "c1",
                || {},
                |_| Err("no model".to_owned()),
            )
            .expect("started");
        let mut editor = Seen::default();
        let error = run(
            &JobStatus,
            &jobs,
            &mut editor,
            json!({ "job_id": id, "wait_seconds": 5 }),
        )
        .expect_err("failed");
        assert!(error.to_string().contains("no model"), "{error}");
        jobs.shutdown();
    }

    #[test]
    fn status_lists_jobs_and_refuses_an_unknown_one() {
        let jobs = Jobs::default();
        let mut editor = Seen::default();
        let empty = run(&JobStatus, &jobs, &mut editor, json!({})).expect("list");
        assert_eq!(empty.text, "No jobs in this session.");

        jobs.track("j1", JobKind::Export, "/out.mp4");
        let listed = run(&JobStatus, &jobs, &mut editor, json!({})).expect("list");
        assert!(
            listed.text.contains("job j1 (export /out.mp4"),
            "{}",
            listed.text
        );

        let error =
            run(&JobStatus, &jobs, &mut editor, json!({ "job_id": "nope" })).expect_err("unknown");
        assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
    }

    #[test]
    fn cancelling_an_export_asks_the_editor() {
        let jobs = Jobs::default();
        jobs.track("j1", JobKind::Export, "/out.mp4");
        let mut editor = Seen::default();
        run(&JobCancel, &jobs, &mut editor, json!({ "job_id": "j1" })).expect("cancel");
        assert_eq!(
            editor.requests,
            vec![Request::ExportCancel {
                job: "j1".to_owned()
            }]
        );

        jobs.record(&Event::ExportFailed {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            error: ApiError::new(concat_api::ErrorCode::Cancelled, "cancelled"),
        });
        let again = run(&JobCancel, &jobs, &mut editor, json!({ "job_id": "j1" })).expect("ended");
        assert!(again.text.contains("Nothing to stop"), "{}", again.text);
        assert_eq!(
            editor.requests.len(),
            1,
            "an ended job is not cancelled twice"
        );
    }
}
