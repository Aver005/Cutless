// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты пауз и чистки звука: анализ тишины, разметка, склейка,
//! подтверждение и звуковые пакеты.
//!
//! Каждый инструмент читает проект запросом `project.get` и правит его
//! командами через `edit.apply`; в `concat.json` напрямую никто не пишет.

use std::sync::Arc;

use concat_api::Request;
use concat_project::Command;
use concat_project::commands::pauses::PauseRange;
use concat_project::model::{Clip, ClipKind, Project};
use cutless_pauses::{FileRequest, PauseSettings, analyze_file};
use serde_json::Value;

use crate::{Registry, ToolContext, ToolError};

mod analyze;
mod clean;
mod marks;

/// Регистрирует инструменты группы.
pub fn register(registry: &mut Registry) {
    registry.register(Box::new(analyze::PausesAnalyze));
    registry.register(Box::new(analyze::PausesMark));
    registry.register(Box::new(marks::PausesList));
    registry.register(Box::new(marks::PausesRestore));
    registry.register(Box::new(marks::PausesConfirm));
    registry.register(Box::new(clean::AudioClean));
}

/// Результат правки: проект после неё и id, если команда что-то создала.
pub(crate) struct Applied {
    /// Активный таймлайн проекта после правки.
    pub(crate) project: Project,
    /// id, который команда намяла, если намяла.
    pub(crate) created_id: Option<String>,
}

/// Один клип, звук которого можно послушать: путь, окно и скорость.
pub(crate) struct Target {
    /// id клипа.
    pub(crate) clip_id: String,
    /// Путь к файлу клипа.
    pub(crate) path: String,
    /// Какой поток звука слушать.
    pub(crate) audio_stream: Option<usize>,
    /// Откуда в файле начинается окно клипа.
    pub(crate) source_start: f64,
    /// Длина окна в секундах источника.
    pub(crate) window: f64,
    /// Где клип стоит на таймлайне.
    pub(crate) start: f64,
    /// Скорость клипа: переводит секунды окна в секунды таймлайна.
    pub(crate) speed: f64,
}

/// Итог анализа одного клипа: что услышали и какие отрезки предложить.
pub(crate) struct ClipPauses {
    /// Клип.
    pub(crate) clip_id: String,
    /// Шумовой пол, дБ.
    pub(crate) noise_floor_db: f32,
    /// Уровень речи, дБ.
    pub(crate) speech_db: f32,
    /// Порог тишины, дБ.
    pub(crate) threshold_db: f32,
    /// Отрезки в секундах таймлайна.
    pub(crate) ranges: Vec<PauseRange>,
}

/// Настройки разметки из необязательных полей входа.
pub(crate) fn settings_from(
    threshold_db: Option<f32>,
    min_pause: Option<f64>,
    padding: Option<f64>,
) -> Result<PauseSettings, ToolError> {
    let defaults = PauseSettings::default();
    let min_pause = min_pause.unwrap_or(defaults.min_pause);
    let padding = padding.unwrap_or(defaults.padding);
    if threshold_db.is_some_and(|db| !db.is_finite()) {
        return Err(ToolError::BadInput(
            "threshold_db must be a finite number".to_owned(),
        ));
    }
    if !min_pause.is_finite() || min_pause < 0.0 {
        return Err(ToolError::BadInput(
            "min_pause must be zero or more seconds".to_owned(),
        ));
    }
    if !padding.is_finite() || padding < 0.0 {
        return Err(ToolError::BadInput(
            "padding must be zero or more seconds".to_owned(),
        ));
    }
    Ok(PauseSettings {
        threshold_db,
        min_pause,
        padding,
        window_ms: defaults.window_ms,
    })
}

/// Читает активный проект через `project.get`.
pub(crate) fn read_project(ctx: &mut ToolContext<'_>) -> Result<Project, ToolError> {
    let path = ctx.project()?;
    let value = ctx.call(Request::ProjectGet { path })?;
    Ok(project_of(&value)?.project)
}

/// Применяет команду через `edit.apply` и возвращает проект после неё.
pub(crate) fn apply_command(
    ctx: &mut ToolContext<'_>,
    command: Command,
) -> Result<Applied, ToolError> {
    let path = ctx.project()?;
    let value = ctx.call(Request::EditApply {
        path,
        command: Box::new(command),
    })?;
    project_of(&value)
}

/// Разбирает ответ редактора: проект и id, который команда намяла.
fn project_of(value: &Value) -> Result<Applied, ToolError> {
    let project = value
        .get("project")
        .cloned()
        .ok_or_else(|| ToolError::Failed("the editor returned no project".to_owned()))?;
    let project =
        serde_json::from_value(project).map_err(|error| ToolError::Failed(error.to_string()))?;
    let created_id = value
        .get("createdId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Applied {
        project,
        created_id,
    })
}

/// Фоновый трек: звук без `detached_from`. Его не режем и не чистим.
pub(crate) fn is_music(clip: &Clip) -> bool {
    clip.kind == ClipKind::Audio && clip.detached_from.is_none()
}

/// Есть ли у клипа звук, который человек слышит: видео со звуком и не
/// заглушённое, либо звуковой клип (в том числе отсоединённый).
pub(crate) fn is_sound_source(project: &Project, clip: &Clip) -> bool {
    match clip.kind {
        ClipKind::Audio => true,
        ClipKind::Video => {
            clip.muted != Some(true)
                && project
                    .media_by_id(&clip.media_id)
                    .is_some_and(|media| media.has_audio)
        }
        _ => false,
    }
}

/// Клипы, чей звук размечаем: названные, иначе все со звуком кроме музыки.
pub(crate) fn targets(
    project: &Project,
    clip_ids: Option<&[String]>,
) -> Result<Vec<Target>, ToolError> {
    let timeline = project.active();
    let chosen: Vec<&Clip> = match clip_ids {
        Some(ids) => {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                let clip = timeline.clip(id).ok_or_else(|| {
                    ToolError::Failed(format!("clip {id} is not on the active timeline"))
                })?;
                out.push(clip);
            }
            out
        }
        None => timeline
            .clips
            .iter()
            .map(Arc::as_ref)
            .filter(|clip| !is_music(clip) && is_sound_source(project, clip))
            .collect(),
    };
    chosen
        .into_iter()
        .map(|clip| target_of(project, clip))
        .collect()
}

/// Описание одного клипа для анализа или понятная причина отказа.
fn target_of(project: &Project, clip: &Clip) -> Result<Target, ToolError> {
    if is_music(clip) {
        return Err(ToolError::Failed(format!(
            "clip {} is background music: pauses are cut from speech, not music",
            clip.id
        )));
    }
    if !is_sound_source(project, clip) {
        return Err(ToolError::Failed(format!(
            "clip {} has no sound to work on",
            clip.id
        )));
    }
    if clip.speed_curve.is_some() {
        return Err(ToolError::Failed(format!(
            "clip {} has a speed curve: pauses cannot be analysed on it",
            clip.id
        )));
    }
    let speed = clip.speed;
    if !speed.is_finite() || speed <= 0.0 {
        return Err(ToolError::Failed(format!(
            "clip {} has an unusable speed",
            clip.id
        )));
    }
    let media = project.media_by_id(&clip.media_id).ok_or_else(|| {
        ToolError::Failed(format!(
            "clip {} points at media that is not in the bin",
            clip.id
        ))
    })?;
    Ok(Target {
        clip_id: clip.id.clone(),
        path: media.path.clone(),
        audio_stream: clip.audio_stream.map(|index| index as usize),
        source_start: clip.source_start,
        window: clip.duration * speed,
        start: clip.start,
        speed,
    })
}

/// Читает звук каждого клипа и переводит отрезки в секунды таймлайна.
pub(crate) fn analyse(
    targets: &[Target],
    settings: &PauseSettings,
) -> Result<Vec<ClipPauses>, ToolError> {
    let mut out = Vec::with_capacity(targets.len());
    for target in targets {
        let request = FileRequest {
            path: target.path.clone(),
            audio_stream: target.audio_stream,
            source_start: target.source_start,
            window: target.window,
            // Цепочка фильтров клипа живёт в `concat-export`, которого у этого
            // крейта нет; анализ идёт по чистому звуку файла.
            filters: Vec::new(),
        };
        let analysis = analyze_file(&request, settings)
            .map_err(|error| ToolError::Failed(format!("clip {}: {error}", target.clip_id)))?;
        let ranges = analysis
            .pauses
            .iter()
            .map(|range| to_timeline(target.start, target.speed, *range))
            .collect();
        out.push(ClipPauses {
            clip_id: target.clip_id.clone(),
            noise_floor_db: analysis.noise_floor_db,
            speech_db: analysis.speech_db,
            threshold_db: analysis.threshold_db,
            ranges,
        });
    }
    Ok(out)
}

/// Отрезок от начала окна клипа в секунды таймлайна.
fn to_timeline(start: f64, speed: f64, range: cutless_pauses::Range) -> PauseRange {
    PauseRange {
        start: start + range.start / speed,
        end: start + range.end / speed,
    }
}

/// Три знака после запятой — время во всех ответах в секундах.
pub(crate) fn fmt_seconds(value: f64) -> String {
    format!("{value:.3}")
}

/// id звукового пакета по имени пресета.
pub(crate) fn package_of(preset: &str) -> Result<&'static str, ToolError> {
    match preset {
        "phone_voice" => Ok("cutless.phone-voice"),
        "noise_gate" => Ok("cutless.noise-gate"),
        "band_cut" => Ok("cutless.band-cut"),
        "denoise" => Ok("concat.denoise"),
        "enhance_voice" => Ok("concat.enhance-voice"),
        other => Err(ToolError::BadInput(format!(
            "unknown preset {other}: use phone_voice, noise_gate, band_cut, denoise or enhance_voice"
        ))),
    }
}

/// Ответ с коротким текстом и структурными данными.
pub(crate) fn output(text: String, data: Value) -> crate::ToolOutput {
    crate::ToolOutput {
        text,
        data,
        images: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use concat_api::{ApiError, ErrorCode, Reply, Request, Response};
    use concat_host::{EditorView, SettingsView};
    use concat_project::Editor;
    use concat_project::commands::pauses::is_pause;
    use concat_project::commands::{ClipPatch, Command, NewMedia};
    use concat_project::model::{AppliedFilter, MediaKind};
    use serde_json::json;

    use crate::Editor as EditorTrait;
    use crate::Tool as _;

    use super::*;

    /// Путь проекта: инструментам он нужен только как имя открытого проекта.
    const PATH: &str = "/tmp/cutless-tests";

    /// Подставной редактор: отвечает проектом и применяет команды как движок.
    struct Fake {
        editor: Editor,
        calls: Vec<Request>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                editor: Editor::new(),
                calls: Vec::new(),
            }
        }

        fn view(&self, created_id: Option<String>) -> EditorView {
            let project = self.editor.project();
            let video = project.active().video;
            EditorView {
                project: project.clone(),
                can_undo: self.editor.can_undo(),
                can_redo: self.editor.can_redo(),
                settings: SettingsView {
                    name: "Test".to_owned(),
                    width: video.width,
                    height: video.height,
                    rate_num: video.rate_num,
                    rate_den: video.rate_den,
                },
                created_id,
            }
        }
    }

    impl EditorTrait for Fake {
        fn dispatch(&mut self, request: Request) -> Result<serde_json::Value, ApiError> {
            self.calls.push(request.clone());
            crate::answer(match request {
                Request::ProjectGet { .. } => {
                    Response::Result(Reply::View(Box::new(self.view(None))))
                }
                Request::EditApply { command, .. } => match self.editor.apply(*command) {
                    Ok(outcome) => {
                        Response::Result(Reply::View(Box::new(self.view(outcome.created_id))))
                    }
                    Err(error) => {
                        Response::Error(ApiError::new(ErrorCode::Refused, error.to_string()))
                    }
                },
                _ => Response::Error(ApiError::failed("unsupported request in tests")),
            })
        }
    }

    fn media(path: &str, duration: f64, kind: MediaKind, has_audio: bool) -> NewMedia {
        NewMedia {
            path: path.to_owned(),
            name: path.to_owned(),
            duration: Some(duration),
            kind,
            width: Some(64),
            height: Some(64),
            frame_rate: Some(30.0),
            frame_rate_fraction: Some("30/1".to_owned()),
            video_codec: None,
            audio_codec: has_audio.then(|| "aac".to_owned()),
            has_audio,
            audio_tracks: Vec::new(),
            origin: None,
            color_space: Default::default(),
        }
    }

    fn add_media(
        editor: &mut Editor,
        path: &str,
        duration: f64,
        kind: MediaKind,
        sound: bool,
    ) -> String {
        editor
            .apply(Command::AddMedia {
                item: media(path, duration, kind, sound),
            })
            .expect("media")
            .created_id
            .expect("id")
    }

    fn add_clip(editor: &mut Editor, media_id: &str, start: f64) -> String {
        let track = editor.project().active().tracks[0].id.clone();
        editor
            .apply(Command::AddClip {
                media_id: media_id.to_owned(),
                track_id: track,
                start,
                ripple: false,
            })
            .expect("clip")
            .created_id
            .expect("id")
    }

    fn with_ctx<T>(fake: &mut Fake, run: impl FnOnce(&mut ToolContext<'_>) -> T) -> T {
        let mut ctx = ToolContext {
            editor: fake,
            project: Some(PATH.to_owned()),
        };
        run(&mut ctx)
    }

    fn clip_count(editor: &Editor) -> usize {
        editor.project().active().clips.len()
    }

    fn filters_of(editor: &Editor, clip_id: &str) -> Vec<AppliedFilter> {
        editor
            .project()
            .active()
            .clip(clip_id)
            .expect("clip")
            .filters
            .clone()
    }

    fn pause_count(editor: &Editor) -> usize {
        editor
            .project()
            .active()
            .clips
            .iter()
            .map(Arc::as_ref)
            .filter(|clip| is_pause(clip))
            .count()
    }

    #[test]
    fn analyze_input_parses_and_rejects_a_bad_min_pause() {
        let mut fake = Fake::new();
        let error = with_ctx(&mut fake, |ctx| {
            analyze::PausesAnalyze
                .call(ctx, json!({ "min_pause": -1.0 }))
                .expect_err("negative")
        });
        assert!(matches!(error, ToolError::BadInput(_)), "{error}");
    }

    #[test]
    fn audio_clean_needs_a_known_preset() {
        let mut fake = Fake::new();
        let missing = with_ctx(&mut fake, |ctx| {
            clean::AudioClean
                .call(ctx, json!({}))
                .expect_err("no preset")
        });
        assert!(matches!(missing, ToolError::BadInput(_)), "{missing}");
        let unknown = with_ctx(&mut fake, |ctx| {
            clean::AudioClean
                .call(ctx, json!({ "preset": "reverb" }))
                .expect_err("unknown preset")
        });
        assert!(matches!(unknown, ToolError::BadInput(_)), "{unknown}");
    }

    #[test]
    fn mark_and_confirm_reject_malformed_input() {
        let mut fake = Fake::new();
        let mark = with_ctx(&mut fake, |ctx| {
            analyze::PausesMark
                .call(ctx, json!({ "ranges": "nope" }))
                .expect_err("ranges must be a list")
        });
        assert!(matches!(mark, ToolError::BadInput(_)), "{mark}");
        let confirm = with_ctx(&mut fake, |ctx| {
            marks::PausesConfirm
                .call(ctx, json!({ "clip_ids": 42 }))
                .expect_err("clip_ids must be a list")
        });
        assert!(matches!(confirm, ToolError::BadInput(_)), "{confirm}");
    }

    #[test]
    fn restore_needs_clip_ids() {
        let mut fake = Fake::new();
        let error = with_ctx(&mut fake, |ctx| {
            marks::PausesRestore
                .call(ctx, json!({}))
                .expect_err("no ids")
        });
        assert!(matches!(error, ToolError::BadInput(_)), "{error}");
    }

    #[test]
    fn mark_with_ranges_cuts_and_marks_in_one_undo_step() {
        let mut fake = Fake::new();
        let media = add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, true);
        let clip = add_clip(&mut fake.editor, &media, 0.0);

        let out = with_ctx(&mut fake, |ctx| {
            analyze::PausesMark
                .call(
                    ctx,
                    json!({ "ranges": [{ "clip_id": clip, "start": 2.0, "end": 3.0 }] }),
                )
                .expect("marks")
        });
        assert!(out.text.contains("marked"), "{}", out.text);
        assert_eq!(out.data["marked"], 1);
        assert_eq!(clip_count(&fake.editor), 3, "cut in two places");
        assert_eq!(pause_count(&fake.editor), 1);

        assert!(fake.editor.undo(), "one step goes back");
        assert_eq!(clip_count(&fake.editor), 1);
        assert_eq!(pause_count(&fake.editor), 0);
    }

    #[test]
    fn list_reports_the_marked_pieces_and_their_sum() {
        let mut fake = Fake::new();
        let media = add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, true);
        let clip = add_clip(&mut fake.editor, &media, 0.0);
        with_ctx(&mut fake, |ctx| {
            analyze::PausesMark
                .call(
                    ctx,
                    json!({ "ranges": [
                        { "clip_id": clip, "start": 2.0, "end": 3.0 },
                        { "clip_id": clip, "start": 6.0, "end": 6.5 }
                    ]}),
                )
                .expect("marks");
        });

        let out = with_ctx(&mut fake, |ctx| {
            marks::PausesList.call(ctx, json!({})).expect("lists")
        });
        assert_eq!(out.data["count"], 2);
        assert!((out.data["seconds"].as_f64().expect("seconds") - 1.5).abs() < 1e-9);
    }

    #[test]
    fn restore_merges_the_pieces_back() {
        let mut fake = Fake::new();
        let media = add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, true);
        let clip = add_clip(&mut fake.editor, &media, 0.0);
        with_ctx(&mut fake, |ctx| {
            analyze::PausesMark
                .call(
                    ctx,
                    json!({ "ranges": [{ "clip_id": clip, "start": 2.0, "end": 3.0 }] }),
                )
                .expect("marks");
        });
        let piece = {
            let timeline = fake.editor.project().active();
            timeline
                .clips
                .iter()
                .map(Arc::as_ref)
                .find(|clip| is_pause(clip))
                .expect("a piece")
                .id
                .clone()
        };
        with_ctx(&mut fake, |ctx| {
            marks::PausesRestore
                .call(ctx, json!({ "clip_ids": [piece] }))
                .expect("restores");
        });
        assert_eq!(clip_count(&fake.editor), 1);
        assert_eq!(pause_count(&fake.editor), 0);
    }

    #[test]
    fn confirm_removes_the_pauses_and_reports_the_shortening() {
        let mut fake = Fake::new();
        let media = add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, true);
        let clip = add_clip(&mut fake.editor, &media, 0.0);
        with_ctx(&mut fake, |ctx| {
            analyze::PausesMark
                .call(
                    ctx,
                    json!({ "ranges": [
                        { "clip_id": clip, "start": 2.0, "end": 3.0 },
                        { "clip_id": clip, "start": 6.0, "end": 7.0 }
                    ]}),
                )
                .expect("marks");
        });

        let out = with_ctx(&mut fake, |ctx| {
            marks::PausesConfirm.call(ctx, json!({})).expect("confirms")
        });
        assert_eq!(out.data["removed_pauses"], 2);
        assert!((out.data["shortened_seconds"].as_f64().expect("short") - 2.0).abs() < 1e-9);
        assert!((fake.editor.project().active().duration() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn audio_clean_appends_and_updates_without_erasing() {
        let mut fake = Fake::new();
        let media = add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, true);
        let clip = add_clip(&mut fake.editor, &media, 0.0);
        fake.editor
            .apply(Command::UpdateClip {
                clip_id: clip.clone(),
                patch: ClipPatch {
                    filters: Some(vec![AppliedFilter::new("echo")]),
                    ..ClipPatch::default()
                },
            })
            .expect("seeds a filter");

        let first = with_ctx(&mut fake, |ctx| {
            clean::AudioClean
                .call(ctx, json!({ "preset": "denoise" }))
                .expect("cleans")
        });
        assert_eq!(first.data["added"], 1);
        let chain = filters_of(&fake.editor, &clip);
        assert_eq!(chain.len(), 2, "echo stays, denoise lands");
        assert_eq!(chain[0].id, "echo");
        assert_eq!(chain[1].id, "concat.denoise");

        let second = with_ctx(&mut fake, |ctx| {
            clean::AudioClean
                .call(
                    ctx,
                    json!({ "preset": "denoise", "params": { "strength": 80.0 } }),
                )
                .expect("updates")
        });
        assert_eq!(second.data["updated"], 1);
        let chain = filters_of(&fake.editor, &clip);
        assert_eq!(chain.len(), 2, "no duplicate");
        assert_eq!(chain[1].params.get("strength"), Some(&80.0));
    }

    #[test]
    fn analyzing_a_project_without_sound_says_so() {
        let mut fake = Fake::new();
        add_media(&mut fake.editor, "/a.mp4", 10.0, MediaKind::Video, false);
        let media = add_media(&mut fake.editor, "/b.mp4", 10.0, MediaKind::Video, false);
        add_clip(&mut fake.editor, &media, 0.0);

        let out = with_ctx(&mut fake, |ctx| {
            analyze::PausesAnalyze
                .call(ctx, json!({}))
                .expect("answers")
        });
        assert!(out.text.contains("no clips"), "{}", out.text);
    }

    #[test]
    fn analyze_reads_a_file_and_reports_ranges_in_timeline_seconds() {
        let path = temp_wav_path();
        std::fs::write(&path, test_wav()).expect("writes wav");
        let mut fake = Fake::new();
        let media = add_media(
            &mut fake.editor,
            &path.to_string_lossy(),
            3.0,
            MediaKind::Video,
            true,
        );
        add_clip(&mut fake.editor, &media, 0.0);

        let out = with_ctx(&mut fake, |ctx| {
            analyze::PausesAnalyze
                .call(ctx, json!({}))
                .expect("analyses")
        });
        std::fs::remove_file(&path).ok();

        let pauses = out.data["clips"][0]["pauses"]
            .as_array()
            .expect("pauses")
            .clone();
        assert_eq!(pauses.len(), 1, "one silence in the middle: {}", out.text);
        let start = pauses[0]["start"].as_f64().expect("start");
        assert!((start - 1.333).abs() < 0.05, "start {start}");
    }

    /// Путь временного файла: один на процесс, тесты идут параллельно.
    fn temp_wav_path() -> PathBuf {
        std::env::temp_dir().join(format!("cutless-pauses-tool-{}.wav", std::process::id()))
    }

    /// WAV 16 кГц моно: секунда тона, секунда тишины, секунда тона.
    fn test_wav() -> Vec<u8> {
        const RATE: u32 = 16_000;
        let mut samples: Vec<i16> = Vec::new();
        for amplitude in [0.5_f32, 0.0, 0.5] {
            for index in 0..RATE {
                let t = index as f32 / RATE as f32;
                let value = amplitude * (2.0 * std::f32::consts::PI * 220.0 * t).sin();
                samples.push((value * 32_767.0) as i16);
            }
        }
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&RATE.to_le_bytes());
        bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
