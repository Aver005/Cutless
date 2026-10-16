// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты проекта: открыть, создать, сохранить, сводка дорожки времени, деталь клипа, история.

use std::collections::BTreeMap;
use std::sync::Arc;

use concat_project::model::{Clip, ClipKind, Project, VideoSettings};
use serde::Deserialize;
use serde_json::{Value, json};

use super::support::{self, View};
use crate::{Registry, ToolClass, ToolContext, ToolError, ToolOutput};

/// Регистрирует инструменты проекта.
pub(crate) fn register(registry: &mut Registry) {
    support::add(
        registry,
        "project_open",
        "Open a project folder and remember it as the current project. Call before any edit.",
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Project folder path" } },
            "required": ["path"]
        }),
        ToolClass::Read,
        project_open,
    );
    support::add(
        registry,
        "project_create",
        "Create a new project folder under a location and open it. Width/height/fps default to 1920x1080 at 30.",
        json!({
            "type": "object",
            "properties": {
                "location": { "type": "string" },
                "name": { "type": "string" },
                "width": { "type": "integer" },
                "height": { "type": "integer" },
                "fps": { "type": "number" }
            },
            "required": ["location", "name"]
        }),
        ToolClass::Edit,
        project_create,
    );
    support::add(
        registry,
        "project_on_screen",
        "Make a project that is already open in the Cutless window the current one, so edits go to what the person sees. Call this first when the server is connected to a window.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Read,
        project_on_screen,
    );
    support::add(
        registry,
        "project_save",
        "Write the current project document to disk.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Edit,
        project_save,
    );
    support::add(
        registry,
        "project_settings",
        "Change the output frame size or rate of the active timeline, as an undoable edit.",
        json!({
            "type": "object",
            "properties": {
                "width": { "type": "integer" },
                "height": { "type": "integer" },
                "fps": { "type": "number" }
            }
        }),
        ToolClass::Edit,
        project_settings,
    );
    support::add(
        registry,
        "timeline_summary",
        "Compact text of the whole timeline: one line per clip with ids, times, sources and any differences from the defaults. The main way to see the project.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Read,
        timeline_summary,
    );
    support::add(
        registry,
        "clip_detail",
        "Every stored property of one clip, as JSON from the project document.",
        json!({
            "type": "object",
            "properties": { "clip_id": { "type": "string" } },
            "required": ["clip_id"]
        }),
        ToolClass::Read,
        clip_detail,
    );
    support::add(
        registry,
        "undo",
        "Step the edit history back one edit.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Edit,
        undo,
    );
    support::add(
        registry,
        "redo",
        "Step the edit history forward one edit.",
        json!({ "type": "object", "properties": {} }),
        ToolClass::Edit,
        redo,
    );
}

/// Вход `project_open`.
#[derive(Deserialize)]
struct OpenInput {
    path: String,
}

/// Вход `project_create`.
#[derive(Deserialize)]
struct CreateInput {
    location: String,
    name: String,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
}

/// Вход `project_settings`.
#[derive(Deserialize)]
struct SettingsInput {
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f64>,
}

/// Проект, открытый в окне, как его называет ответ `project.onScreen`.
#[derive(Deserialize)]
struct OnScreen {
    path: String,
    name: String,
}

/// Вход `clip_detail`.
#[derive(Deserialize)]
struct ClipInput {
    clip_id: String,
}

/// Открывает проект и запоминает папку в сессии.
fn project_open(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: OpenInput = crate::input(input)?;
    let view = support::parse_view(ctx.call(concat_api::Request::ProjectOpen {
        path: input.path.clone(),
    })?)?;
    ctx.project = Some(input.path.clone());
    Ok(ToolOutput {
        text: format!(
            "opened \"{}\" ({}x{} @{}) at {}",
            view.settings.name,
            view.settings.width,
            view.settings.height,
            support::fps(view.settings.rate_num, view.settings.rate_den),
            input.path
        ),
        data: json!({ "path": input.path, "name": view.settings.name }),
        ..ToolOutput::default()
    })
}

/// Создаёт проект и запоминает его папку в сессии.
fn project_create(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: CreateInput = crate::input(input)?;
    let (rate_num, rate_den) = input.fps.map(support::rate_of).unwrap_or((30, 1));
    let video = VideoSettings {
        width: input.width.unwrap_or(1920),
        height: input.height.unwrap_or(1080),
        rate_num,
        rate_den,
        color_space: Default::default(),
    };
    let view = support::parse_view(ctx.call(concat_api::Request::ProjectCreate {
        location: input.location.clone(),
        name: input.name.clone(),
        video: Some(video),
    })?)?;
    let path = std::path::Path::new(&input.location)
        .join(concat_host::projects::folder_name(&input.name))
        .to_string_lossy()
        .into_owned();
    ctx.project = Some(path.clone());
    Ok(ToolOutput {
        text: format!(
            "created \"{}\" at {} ({}x{} @{})",
            view.settings.name,
            path,
            view.settings.width,
            view.settings.height,
            support::fps(view.settings.rate_num, view.settings.rate_den)
        ),
        data: json!({ "path": path, "name": view.settings.name }),
        ..ToolOutput::default()
    })
}

/// Делает текущим проект, открытый в окне Cutless.
fn project_on_screen(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let value = ctx.call(concat_api::Request::ProjectOnScreen)?;
    let projects: Vec<OnScreen> =
        serde_json::from_value(value).map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(match projects.as_slice() {
        [] => ToolOutput::text(
            "no project is open in a Cutless window; use project_open or project_create",
        ),
        [only] => {
            ctx.project = Some(only.path.clone());
            ToolOutput {
                text: format!(
                    "current project: {} at {} (open in the Cutless window)",
                    only.name, only.path
                ),
                data: json!({ "path": only.path, "name": only.name }),
                ..ToolOutput::default()
            }
        }
        many => {
            let list = many
                .iter()
                .map(|project| project.path.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            ToolOutput {
                text: format!("projects open in the Cutless window:\n{list}"),
                data: json!({
                    "paths": many
                        .iter()
                        .map(|project| project.path.clone())
                        .collect::<Vec<_>>()
                }),
                ..ToolOutput::default()
            }
        }
    })
}

/// Сохраняет документ проекта.
fn project_save(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let path = ctx.project()?;
    ctx.call(concat_api::Request::ProjectSave {
        path: path.clone(),
        name: None,
    })?;
    Ok(ToolOutput::text(format!("saved {path}")))
}

/// Меняет кадр и частоту активной дорожки времени.
fn project_settings(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: SettingsInput = crate::input(input)?;
    let current = support::fetch(ctx)?;
    let mut video = current.project.active().video;
    if let Some(width) = input.width {
        video.width = width;
    }
    if let Some(height) = input.height {
        video.height = height;
    }
    if let Some(fps) = input.fps {
        let (rate_num, rate_den) = support::rate_of(fps);
        video.rate_num = rate_num;
        video.rate_den = rate_den;
    }
    let path = ctx.project()?;
    let view =
        support::parse_view(ctx.call(concat_api::Request::ProjectSetVideo { path, video })?)?;
    Ok(ToolOutput {
        text: format!(
            "settings: {}x{} @{}",
            view.settings.width,
            view.settings.height,
            support::fps(view.settings.rate_num, view.settings.rate_den)
        ),
        data: json!({
            "width": view.settings.width,
            "height": view.settings.height,
            "rateNum": view.settings.rate_num,
            "rateDen": view.settings.rate_den
        }),
        ..ToolOutput::default()
    })
}

/// Компактная сводка всей дорожки времени.
fn timeline_summary(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let view = support::fetch(ctx)?;
    let text = summary(&view);
    Ok(ToolOutput {
        text,
        data: json!({
            "name": view.settings.name,
            "revision": support::revision(&view.project),
            "clips": view.project.active().clips.len()
        }),
        ..ToolOutput::default()
    })
}

/// Все сохранённые свойства клипа, как JSON из документа.
fn clip_detail(ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
    let input: ClipInput = crate::input(input)?;
    let path = ctx.project()?;
    let document = ctx.call(concat_api::Request::ProjectDocument { path })?;
    let clip = document
        .get("clips")
        .and_then(Value::as_array)
        .and_then(|clips| {
            clips
                .iter()
                .find(|clip| clip.get("id").and_then(Value::as_str) == Some(input.clip_id.as_str()))
        })
        .ok_or_else(|| {
            ToolError::Failed(format!(
                "clip {} is not on the active timeline",
                input.clip_id
            ))
        })?;
    let text = serde_json::to_string_pretty(clip).unwrap_or_else(|_| clip.to_string());
    Ok(ToolOutput {
        text,
        data: clip.clone(),
        ..ToolOutput::default()
    })
}

/// Откатывает одну правку.
fn undo(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let path = ctx.project()?;
    let view = support::parse_view(ctx.call(concat_api::Request::EditUndo { path })?)?;
    Ok(history_output("undo", &view))
}

/// Повторяет одну правку.
fn redo(ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
    let path = ctx.project()?;
    let view = support::parse_view(ctx.call(concat_api::Request::EditRedo { path })?)?;
    Ok(history_output("redo", &view))
}

/// Короткий ответ на шаг истории.
fn history_output(what: &str, view: &View) -> ToolOutput {
    ToolOutput {
        text: format!(
            "{what}: {} clips on the timeline (undo {}, redo {})",
            view.project.active().clips.len(),
            view.can_undo,
            view.can_redo
        ),
        data: json!({ "canUndo": view.can_undo, "canRedo": view.can_redo }),
        ..ToolOutput::default()
    }
}

/// Текст сводки: шапка, затем дорожки и по строке на клип.
fn summary(view: &View) -> String {
    let active = view.project.active();
    let video = active.video;
    let mut out = format!(
        "project \"{}\" rev {} · {}x{} @{} · {}\n",
        view.settings.name,
        support::revision(&view.project),
        video.width,
        video.height,
        support::fps(video.rate_num, video.rate_den),
        support::time(active.duration())
    );
    let media = media_names(&view.project);
    for track in &active.tracks {
        out.push_str(&format!("track {}", track.id));
        if track.muted {
            out.push_str(" muted");
        }
        if !track.visible {
            out.push_str(" hidden");
        }
        out.push('\n');
        let mut clips: Vec<&Clip> = active
            .clips
            .iter()
            .filter(|clip| clip.track_id == track.id)
            .map(Arc::as_ref)
            .collect();
        clips.sort_by(|left, right| left.start.total_cmp(&right.start));
        for clip in clips {
            out.push_str(&clip_line(clip, &media));
            out.push_str(&beats_mark(&view.project, clip));
            out.push('\n');
        }
    }
    out.pop();
    out
}

/// Имена медиа по id, для подписи клипов.
fn media_names(project: &Project) -> BTreeMap<String, String> {
    project
        .media
        .iter()
        .map(|item| (item.id.clone(), item.name.clone()))
        .collect()
}

/// Одна строка сводки: id, интервал, вид, имя, исток и отличия от нормы.
fn clip_line(clip: &Clip, media: &BTreeMap<String, String>) -> String {
    let label = if clip.kind == ClipKind::Text {
        clip.text
            .as_ref()
            .map(|style| first_line(&style.content))
            .unwrap_or_else(|| clip.name.clone())
    } else {
        media
            .get(&clip.media_id)
            .cloned()
            .unwrap_or_else(|| clip.name.clone())
    };
    let mut line = format!(
        "  {}  {}-{}  {} \"{}\"",
        clip.id,
        support::time(clip.start),
        support::time(clip.start + clip.duration),
        kind_name(clip.kind),
        label
    );
    if !clip.media_id.is_empty() {
        line.push_str(&format!("  src {}", support::time(clip.source_start)));
    }
    if (clip.speed - 1.0).abs() > 1e-6 {
        line.push_str(&format!("  speed {}", support::num(clip.speed)));
    }
    if (clip.volume - 1.0).abs() > 1e-6 {
        line.push_str(&format!("  vol {}", support::num(clip.volume)));
    }
    if clip.fade_in > 0.0 {
        line.push_str(&format!("  fade-in {}", support::num(clip.fade_in)));
    }
    if clip.fade_out > 0.0 {
        line.push_str(&format!("  fade-out {}", support::num(clip.fade_out)));
    }
    if !clip.video_effects.is_empty() {
        let ids: Vec<String> = clip
            .video_effects
            .iter()
            .map(|effect| {
                if effect.keys.is_empty() {
                    effect.id.clone()
                } else {
                    let keyed: Vec<String> = effect
                        .keys
                        .iter()
                        .map(|(key, run)| {
                            format!(
                                "{key}@{}",
                                key_windows(clip, run.iter().map(|held| held.at))
                            )
                        })
                        .collect();
                    format!("{}(keys {})", effect.id, keyed.join(" "))
                }
            })
            .collect();
        line.push_str(&format!("  fx: {}", ids.join(" ")));
    }
    if !clip.keys.is_empty() {
        line.push_str(&format!("  keys {}", key_counts(clip)));
    }
    if clip.speed_curve.is_some() {
        line.push_str("  speed-curve");
    }
    if let Some(transition) = &clip.transition_in {
        line.push_str(&format!(
            "  transition-in {} {}",
            transition.id,
            support::num(transition.duration)
        ));
    }
    if let Some(stream) = clip.audio_stream {
        line.push_str(&format!("  stream {stream}"));
    }
    if clip.muted == Some(true) {
        line.push_str("  muted");
    }
    if let Some(group) = concat_project::commands::links::link_of(clip) {
        line.push_str(&format!("  group {group}"));
    }
    if concat_project::commands::pauses::is_pause(clip) {
        line.push_str("  cutless.pause");
    }
    line
}

/// Метка сетки долей у клипа, чей звук разобран: темп и первый такт под ним.
fn beats_mark(project: &Project, clip: &Clip) -> String {
    let Some(grid) = project
        .media_by_id(&clip.media_id)
        .and_then(|media| concat_project::beats::grid(media, clip.audio_stream))
    else {
        return String::new();
    };
    let bars =
        concat_project::beats::timeline_picks(&grid, clip, concat_project::beats::Pick::Bars);
    let first = bars
        .first()
        .map_or("-".to_owned(), |time| support::time(*time));
    format!("  beats {}bpm bar@{first}", support::num(grid.bpm))
}

/// Ключи свойств клипа: сколько и где, `scale×32@00:12.300-00:13.333`.
fn key_counts(clip: &Clip) -> String {
    let counts: Vec<String> = concat_project::model::KeyProperty::ALL
        .iter()
        .filter_map(|property| {
            let count = clip.keys_on(*property).count();
            (count > 0).then(|| {
                let windows = key_windows(clip, clip.keys_on(*property).map(|key| key.at));
                format!("{}×{count}@{windows}", support::property_name(*property))
            })
        })
        .collect();
    counts.join(" ")
}

/// Разрыв между ключами, после которого начинается новое окно, секунды.
const KEY_GAP: f64 = 0.25;
/// Сколько окон ключей показывать, остальные — числом.
const KEY_WINDOWS: usize = 3;

/// Где лежат ключи на таймлайне: окна `00:04.000-00:05.000,00:08.000-00:09.000`.
fn key_windows(clip: &Clip, ats: impl Iterator<Item = f64>) -> String {
    let mut times: Vec<f64> = ats.map(|at| clip.start + at * clip.duration).collect();
    times.sort_by(f64::total_cmp);
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for time in times {
        match windows.last_mut() {
            Some(last) if time - last.1 <= KEY_GAP => last.1 = time,
            _ => windows.push((time, time)),
        }
    }
    let mut shown: Vec<String> = windows
        .iter()
        .take(KEY_WINDOWS)
        .map(|(first, last)| format!("{}-{}", support::time(*first), support::time(*last)))
        .collect();
    if windows.len() > KEY_WINDOWS {
        shown.push(format!("+{} more", windows.len() - KEY_WINDOWS));
    }
    shown.join(",")
}

/// Имя вида клипа для сводки.
fn kind_name(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Video => "video",
        ClipKind::Audio => "audio",
        ClipKind::Image => "image",
        ClipKind::Text => "text",
        ClipKind::Layer => "layer",
        ClipKind::Shape => "shape",
    }
}

/// Первая непустая строка титра, обрезанная для сводки.
fn first_line(content: &str) -> String {
    let line = content
        .lines()
        .find(|candidate| !candidate.trim().is_empty())
        .unwrap_or("")
        .trim();
    line.chars().take(48).collect()
}
