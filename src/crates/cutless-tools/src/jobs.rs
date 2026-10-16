// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Фоновые задачи агента: распознавание и экспорт идут дольше, чем клиент ждёт ответа.
//! Инструмент запускает задачу и сразу отдаёт её id, а `job_status` ждёт и отдаёт итог.

mod tools;

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use concat_api::Event;
use serde_json::json;

use crate::{Registry, ToolError, ToolOutput};

/// Сколько инструмент ждёт задачу, если не сказано иное, в секундах.
pub const WAIT_DEFAULT: f64 = 20.0;
/// Дольше одного вызова не ждём: клиенты MCP рвут запрос на 60 с.
pub const WAIT_MAX: f64 = 45.0;

/// Регистрирует инструменты задач.
pub fn register(registry: &mut Registry) {
    tools::register(registry);
}

/// Время ожидания из входа: по умолчанию [`WAIT_DEFAULT`], не дольше [`WAIT_MAX`].
pub fn wait_from(seconds: Option<f64>) -> Duration {
    let seconds = seconds.unwrap_or(WAIT_DEFAULT);
    let seconds = if seconds.is_finite() { seconds } else { 0.0 };
    Duration::from_secs_f64(seconds.clamp(0.0, WAIT_MAX))
}

/// Что за задача.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// Распознавание речи клипа.
    Transcribe,
    /// Сетка долей трека.
    Beats,
    /// Экспорт таймлайна в файл.
    Export,
}

impl JobKind {
    /// Имя для текста ответа.
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Transcribe => "transcribe",
            JobKind::Beats => "beats",
            JobKind::Export => "export",
        }
    }
}

/// Где задача сейчас.
#[derive(Clone, Debug, PartialEq)]
pub enum JobState {
    /// Идёт.
    Running {
        /// Этап: чтение звука, загрузка модели, кадры видео.
        stage: String,
        /// Доля сделанного, `0..=1`.
        fraction: f32,
    },
    /// Закончилась; ответ такой же, как у обычного инструмента.
    Done(ToolOutput),
    /// Не вышло; текст причины.
    Failed(String),
}

/// Снимок одной задачи.
#[derive(Clone, Debug)]
pub struct Job {
    /// id для `job_status` и `job_cancel`.
    pub id: String,
    /// Что за задача.
    pub kind: JobKind,
    /// О чём она: клип или файл экспорта.
    pub subject: String,
    /// Где она сейчас.
    pub state: JobState,
    /// Когда началась.
    pub started: Instant,
    /// Когда закончилась, если закончилась.
    pub ended: Option<Instant>,
}

impl Job {
    /// Идёт ли ещё.
    pub fn is_running(&self) -> bool {
        matches!(self.state, JobState::Running { .. })
    }

    /// Сколько секунд шла или идёт.
    pub fn seconds(&self) -> f64 {
        self.ended
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
            .as_secs_f64()
    }

    /// Одна строка о задаче для модели.
    pub fn line(&self) -> String {
        let head = format!(
            "job {} ({} {}, {:.1} s)",
            self.id,
            self.kind.as_str(),
            self.subject,
            self.seconds()
        );
        match &self.state {
            JobState::Running { stage, fraction } => {
                format!("{head}: running, {stage} {:.0}%", fraction * 100.0)
            }
            JobState::Done(output) => format!("{head}: done. {}", output.text),
            JobState::Failed(error) => format!("{head}: failed. {error}"),
        }
    }

    /// Подсказка, как дождаться задачи, пока она идёт.
    pub fn hint(&self) -> String {
        format!(
            "{}. Call job_status with job_id \"{}\" to wait for it.",
            self.line(),
            self.id
        )
    }

    /// Короткие данные о задаче для интерфейса и тестов.
    pub fn data(&self) -> serde_json::Value {
        let (state, detail) = match &self.state {
            JobState::Running { stage, fraction } => {
                ("running", json!({ "stage": stage, "fraction": fraction }))
            }
            JobState::Done(output) => ("done", output.data.clone()),
            JobState::Failed(error) => ("failed", json!({ "error": error })),
        };
        json!({
            "job_id": self.id,
            "kind": self.kind,
            "subject": self.subject,
            "state": state,
            "seconds": self.seconds(),
            "detail": detail,
        })
    }
}

/// Как задача сообщает о себе из своего потока.
#[derive(Clone)]
pub struct Reporter {
    jobs: Jobs,
    id: String,
}

impl Reporter {
    /// Новый этап и доля сделанного.
    pub fn stage(&self, stage: &str, fraction: f32) {
        self.jobs.progress(&self.id, stage, fraction);
    }
}

/// Как остановить задачу, которую запустили мы сами.
type Cancel = Arc<dyn Fn() + Send + Sync>;

/// Задача на доске вместе с тем, как её остановить.
struct Entry {
    job: Job,
    cancel: Option<Cancel>,
}

/// Все задачи сессии и потоки, которые их выполняют.
#[derive(Default)]
struct Board {
    entries: Vec<Entry>,
    minted: u64,
    threads: Vec<JoinHandle<()>>,
}

impl Board {
    fn entry(&mut self, id: &str) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.job.id == id)
    }
}

#[derive(Default)]
struct Shared {
    board: Mutex<Board>,
    changed: Condvar,
}

/// Доска фоновых задач одной сессии агента. Клон смотрит на ту же доску.
#[derive(Clone, Default)]
pub struct Jobs {
    shared: Arc<Shared>,
}

impl Jobs {
    fn board(&self) -> MutexGuard<'_, Board> {
        // Паника в потоке задачи не должна закрыть доску для всех остальных.
        self.shared
            .board
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Запускает работу в своём потоке и сразу отдаёт id задачи.
    /// `cancel` просит работу остановиться; работа сама решает, когда.
    pub fn spawn(
        &self,
        kind: JobKind,
        subject: &str,
        cancel: impl Fn() + Send + Sync + 'static,
        work: impl FnOnce(&Reporter) -> Result<ToolOutput, String> + Send + 'static,
    ) -> Result<String, ToolError> {
        let id = {
            let mut board = self.board();
            board.minted += 1;
            let id = format!("{}{}", kind_prefix(kind), board.minted);
            board.entries.push(Entry {
                job: running(&id, kind, subject),
                cancel: Some(Arc::new(cancel)),
            });
            id
        };
        let reporter = Reporter {
            jobs: self.clone(),
            id: id.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name(format!("cutless job {id}"))
            .spawn(move || {
                let outcome = work(&reporter);
                reporter.jobs.end(&reporter.id, outcome);
            });
        match spawned {
            Ok(thread) => {
                let mut board = self.board();
                board.threads.retain(|thread| !thread.is_finished());
                board.threads.push(thread);
                Ok(id)
            }
            Err(error) => {
                let message = format!("could not start a thread: {error}");
                self.end(&id, Err(message.clone()));
                Err(ToolError::Failed(message))
            }
        }
    }

    /// Берёт на доску задачу, которую ведёт редактор (экспорт).
    /// События могли прийти раньше: тогда задача уже есть, ей только дают имя.
    pub fn track(&self, id: &str, kind: JobKind, subject: &str) {
        let mut board = self.board();
        match board.entry(id) {
            Some(entry) => {
                entry.job.kind = kind;
                entry.job.subject = subject.to_owned();
            }
            None => board.entries.push(Entry {
                job: running(id, kind, subject),
                cancel: None,
            }),
        }
    }

    /// Событие редактора о его задаче: ход экспорта, файл записан или нет.
    pub fn record(&self, event: &Event) {
        match event {
            Event::ExportProgress {
                job,
                frame,
                total,
                stage,
                ..
            } => {
                let fraction = if *total > 0 {
                    (*frame as f32 / *total as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                self.export(job);
                self.progress(job, stage, fraction);
            }
            Event::CutoutProgress { job, fraction, .. } => {
                self.export(job);
                self.progress(job, "cutout", *fraction);
            }
            Event::ExportDone {
                job,
                output,
                width,
                height,
                ..
            } => {
                self.export(job);
                self.end(
                    job,
                    Ok(ToolOutput {
                        text: format!("wrote {output} ({width}x{height})"),
                        data: json!({ "output": output, "width": width, "height": height }),
                        ..ToolOutput::default()
                    }),
                );
            }
            Event::ExportFailed { job, error, .. } => {
                self.export(job);
                self.end(job, Err(error.to_string()));
            }
        }
    }

    /// Приёмник событий API, который пишет их на доску.
    pub fn sink(&self) -> concat_api::EventSink {
        let jobs = self.clone();
        Arc::new(move |event| jobs.record(&event))
    }

    /// Снимок задачи по id.
    pub fn get(&self, id: &str) -> Option<Job> {
        self.board().entry(id).map(|entry| entry.job.clone())
    }

    /// Все задачи сессии, от старых к новым.
    pub fn list(&self) -> Vec<Job> {
        self.board()
            .entries
            .iter()
            .map(|entry| entry.job.clone())
            .collect()
    }

    /// Идущая задача этого вида о том же предмете.
    pub fn running_for(&self, kind: JobKind, subject: &str) -> Option<Job> {
        self.board()
            .entries
            .iter()
            .map(|entry| &entry.job)
            .find(|job| job.kind == kind && job.subject == subject && job.is_running())
            .cloned()
    }

    /// Последняя задача этого вида о том же предмете, в каком бы состоянии она ни была.
    pub fn latest_for(&self, kind: JobKind, subject: &str) -> Option<Job> {
        self.board()
            .entries
            .iter()
            .rev()
            .map(|entry| &entry.job)
            .find(|job| job.kind == kind && job.subject == subject)
            .cloned()
    }

    /// Ждёт конца задачи не дольше `timeout`; отдаёт её снимок, каким бы он ни был.
    pub fn wait(&self, id: &str, timeout: Duration) -> Option<Job> {
        let deadline = Instant::now() + timeout;
        let mut board = self.board();
        loop {
            let job = board.entry(id)?.job.clone();
            let left = deadline.saturating_duration_since(Instant::now());
            if !job.is_running() || left.is_zero() {
                return Some(job);
            }
            board = self
                .shared
                .changed
                .wait_timeout(board, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    /// Просит свою задачу остановиться. Экспорт останавливает редактор, не доска.
    pub fn cancel(&self, id: &str) -> Result<(), ToolError> {
        let cancel = {
            let mut board = self.board();
            let entry = board
                .entry(id)
                .ok_or_else(|| ToolError::BadInput(format!("no job {id}")))?;
            if !entry.job.is_running() {
                return Err(ToolError::Failed(format!("job {id} has already ended")));
            }
            entry.cancel.clone().ok_or_else(|| {
                ToolError::Failed(format!("job {id} is run by the editor, not here"))
            })?
        };
        cancel();
        Ok(())
    }

    /// Останавливает свои задачи и ждёт их потоки: сессия закрывается, итог некому читать.
    pub fn shutdown(&self) {
        let (cancels, threads) = {
            let mut board = self.board();
            let cancels: Vec<Cancel> = board
                .entries
                .iter()
                .filter(|entry| entry.job.is_running())
                .filter_map(|entry| entry.cancel.clone())
                .collect();
            (cancels, std::mem::take(&mut board.threads))
        };
        for cancel in cancels {
            cancel();
        }
        for thread in threads {
            let _ = thread.join();
        }
    }

    /// Задача экспорта, о которой доска ещё не знает: событие пришло раньше ответа.
    fn export(&self, id: &str) {
        let mut board = self.board();
        if board.entry(id).is_none() {
            board.entries.push(Entry {
                job: running(id, JobKind::Export, ""),
                cancel: None,
            });
        }
    }

    fn progress(&self, id: &str, stage: &str, fraction: f32) {
        let mut board = self.board();
        if let Some(entry) = board.entry(id)
            && entry.job.is_running()
        {
            entry.job.state = JobState::Running {
                stage: stage.to_owned(),
                fraction,
            };
        }
    }

    fn end(&self, id: &str, outcome: Result<ToolOutput, String>) {
        let mut board = self.board();
        if let Some(entry) = board.entry(id) {
            entry.job.state = match outcome {
                Ok(output) => JobState::Done(output),
                Err(error) => JobState::Failed(error),
            };
            entry.job.ended = Some(Instant::now());
        }
        self.shared.changed.notify_all();
    }
}

/// Буква id по виду задачи; экспорт получает id от редактора (`j1`).
fn kind_prefix(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Transcribe => "t",
        JobKind::Beats => "b",
        JobKind::Export => "x",
    }
}

/// Только что начатая задача.
fn running(id: &str, kind: JobKind, subject: &str) -> Job {
    Job {
        id: id.to_owned(),
        kind,
        subject: subject.to_owned(),
        state: JobState::Running {
            stage: "starting".to_owned(),
            fraction: 0.0,
        },
        started: Instant::now(),
        ended: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    #[test]
    fn a_spawned_job_reports_and_ends_with_its_output() {
        let jobs = Jobs::default();
        let (go, gate) = mpsc::channel::<()>();
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                "c1",
                || {},
                move |report| {
                    report.stage("transcribing", 0.5);
                    gate.recv().map_err(|error| error.to_string())?;
                    Ok(ToolOutput::text("heard it"))
                },
            )
            .expect("started");
        assert!(id.starts_with('t'), "{id}");
        assert!(jobs.running_for(JobKind::Transcribe, "c1").is_some());

        let early = jobs.wait(&id, Duration::from_millis(20)).expect("job");
        assert!(early.is_running(), "{early:?}");

        go.send(()).expect("go");
        let job = jobs.wait(&id, Duration::from_secs(5)).expect("job");
        assert_eq!(job.state, JobState::Done(ToolOutput::text("heard it")));
        assert!(job.line().contains("done. heard it"), "{}", job.line());
        assert!(jobs.running_for(JobKind::Transcribe, "c1").is_none());
        jobs.shutdown();
    }

    #[test]
    fn a_failed_job_keeps_its_reason() {
        let jobs = Jobs::default();
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                "c2",
                || {},
                |_| Err("no model".to_owned()),
            )
            .expect("started");
        let job = jobs.wait(&id, Duration::from_secs(5)).expect("job");
        assert_eq!(job.state, JobState::Failed("no model".to_owned()));
        jobs.shutdown();
    }

    #[test]
    fn shutdown_cancels_a_running_job_and_joins_it() {
        let jobs = Jobs::default();
        let stop = Arc::new(AtomicBool::new(false));
        let asked = Arc::clone(&stop);
        let seen = Arc::clone(&stop);
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                "c3",
                move || asked.store(true, Ordering::SeqCst),
                move |_| {
                    while !seen.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err("transcription cancelled".to_owned())
                },
            )
            .expect("started");
        jobs.shutdown();
        let job = jobs.get(&id).expect("job");
        assert_eq!(
            job.state,
            JobState::Failed("transcription cancelled".to_owned())
        );
    }

    #[test]
    fn export_events_drive_a_tracked_job_even_when_they_come_first() {
        let jobs = Jobs::default();
        jobs.record(&Event::ExportProgress {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            frame: 30,
            total: 120,
            stage: "video".to_owned(),
        });
        jobs.track("j1", JobKind::Export, "/out.mp4");
        let job = jobs.get("j1").expect("job");
        assert_eq!(job.subject, "/out.mp4");
        assert_eq!(
            job.state,
            JobState::Running {
                stage: "video".to_owned(),
                fraction: 0.25
            }
        );

        jobs.record(&Event::ExportDone {
            job: "j1".to_owned(),
            path: "/p".to_owned(),
            output: "/out.mp4".to_owned(),
            width: 1080,
            height: 1920,
        });
        let job = jobs.wait("j1", Duration::ZERO).expect("job");
        assert!(
            job.line().contains("wrote /out.mp4 (1080x1920)"),
            "{}",
            job.line()
        );
        assert!(
            jobs.cancel("j1").is_err(),
            "an ended job cannot be cancelled"
        );
    }

    #[test]
    fn wait_is_bounded() {
        assert_eq!(wait_from(None), Duration::from_secs_f64(WAIT_DEFAULT));
        assert_eq!(wait_from(Some(600.0)), Duration::from_secs_f64(WAIT_MAX));
        assert_eq!(wait_from(Some(-3.0)), Duration::ZERO);
        assert_eq!(wait_from(Some(f64::NAN)), Duration::ZERO);
    }
}
