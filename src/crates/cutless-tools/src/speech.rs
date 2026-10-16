// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты группы «speech»: расшифровка речи клипа, чтение и поиск по
//! транскрипту, титры из слов. Распознаёт локальный [`Transcriber`], времена
//! в ответах — секунды таймлайна.

mod search;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use concat_host::AppDirs;
use concat_project::Command;
use concat_project::commands::links;
use concat_project::model::{ClipKind, Project, TextStyle};
use concat_speech::caption_lines::{LineRules, caption_lines};
use concat_speech::catalog;
use concat_speech::transcribe::{ModelStatus, Segment, TranscribeRequest, Word};
use concat_speech::{JobStage, Transcriber};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::jobs::{Job, JobKind, JobState, WAIT_MAX, wait_from};
use crate::{Registry, Tool, ToolClass, ToolContext, ToolError, ToolOutput, ToolSpec};

/// Куда по умолчанию садится титр по вертикали: нижняя треть.
const DEFAULT_OFFSET_Y: f64 = 0.35;
/// Минимальная длительность титра на таймлайне, чтобы он не был вспышкой.
const MIN_CAPTION_SECONDS: f64 = 0.2;
/// Сколько сегментов за раз отдаёт `transcript_get`, чтобы ответ не раздулся.
const MAX_SEGMENTS: usize = 200;

/// Один расшифрованный клип; времена — секунды таймлайна.
#[derive(Clone)]
struct CachedTranscript {
    clip_id: String,
    media: String,
    source_start: f64,
    window: f64,
    model: String,
    language: Option<String>,
    segments: Vec<Segment>,
}

/// Общее состояние группы: распознаватель и кэш расшифровок.
struct State {
    transcriber: Transcriber,
    cache: Mutex<Vec<CachedTranscript>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            transcriber: Transcriber::new(),
            cache: Mutex::new(Vec::new()),
        }
    }
}

impl State {
    /// Последняя расшифровка того же окна медиа, какая бы модель её ни дала.
    fn find(&self, media: &str, source_start: f64, window: f64) -> Option<CachedTranscript> {
        let cache = self.cache.lock().ok()?;
        cache
            .iter()
            .rev()
            .find(|entry| same_window(entry, media, source_start, window))
            .cloned()
    }

    /// Все расшифровки, для поиска без клипа.
    fn all(&self) -> Vec<CachedTranscript> {
        self.cache
            .lock()
            .map(|cache| cache.clone())
            .unwrap_or_default()
    }

    /// Кладёт расшифровку, заменяя прежнюю по той же модели и языку.
    fn put(&self, entry: CachedTranscript) -> Result<(), ToolError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| ToolError::Failed("the speech cache is poisoned".to_owned()))?;
        cache.retain(|kept| {
            !(same_window(kept, &entry.media, entry.source_start, entry.window)
                && kept.model == entry.model
                && kept.language == entry.language)
        });
        cache.push(entry);
        Ok(())
    }
}

/// Одна и та же ли это нарезка опорного времени.
fn same_window(entry: &CachedTranscript, media: &str, source_start: f64, window: f64) -> bool {
    entry.media == media
        && (entry.source_start - source_start).abs() < 1e-6
        && (entry.window - window).abs() < 1e-6
}

/// Клип, каким его видит расшифровка: медиа, окно и место на таймлайне.
#[derive(Clone)]
struct ClipRef {
    id: String,
    media_path: String,
    audio_stream: Option<u32>,
    start: f64,
    speed: f64,
    source_start: f64,
    window: f64,
}

/// Регистрирует инструменты группы.
pub fn register(registry: &mut Registry) {
    let state = Arc::new(State::default());
    registry.register(Box::new(SpeechModels {
        state: Arc::clone(&state),
    }));
    registry.register(Box::new(Transcribe {
        state: Arc::clone(&state),
    }));
    registry.register(Box::new(TranscriptGet {
        state: Arc::clone(&state),
    }));
    registry.register(Box::new(TranscriptSearch {
        state: Arc::clone(&state),
    }));
    registry.register(Box::new(CaptionsMake { state }));
}

/// `speech_models` — какие модели распознавания есть на машине.
struct SpeechModels {
    state: Arc<State>,
}

impl Tool for SpeechModels {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "speech_models",
            description: "List on-device speech recognition models: which are downloaded, which \
                          were found on disk, and which can be, with the model currently loaded.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, _input: Value) -> Result<ToolOutput, ToolError> {
        let dirs = dirs(ctx)?;
        let status = Transcriber::status(&dirs).map_err(ToolError::Failed)?;
        let loaded = self.state.transcriber.loaded_model();

        let mut text = format!("Speech models (folder {}).\n", status.models_dir);
        let downloaded: Vec<&ModelStatus> = status
            .models
            .iter()
            .filter(|model| model.downloaded)
            .collect();
        if downloaded.is_empty() {
            text.push_str("downloaded: none\n");
        } else {
            text.push_str("downloaded:\n");
            for model in downloaded {
                text.push_str(&format!(
                    "- {}  {}  {}\n",
                    model.id,
                    model.label,
                    megabytes(model.size_bytes)
                ));
            }
        }
        if !status.found.is_empty() {
            text.push_str("found on disk (model_id: its path or file name):\n");
            for model in &status.found {
                text.push_str(&format!("- {}  {}\n", model.id, model.label));
            }
        }
        let missing: Vec<&ModelStatus> = status
            .models
            .iter()
            .filter(|model| !model.downloaded)
            .collect();
        if !missing.is_empty() {
            text.push_str("can be downloaded (Settings > Transcriber):\n");
            for model in missing {
                text.push_str(&format!(
                    "- {}  {}  {}\n",
                    model.id,
                    model.label,
                    megabytes(model.size_bytes)
                ));
            }
        }
        if let Some(model) = &loaded {
            text.push_str(&format!("loaded now: {model}\n"));
        }

        let mut data =
            serde_json::to_value(&status).map_err(|error| ToolError::Failed(error.to_string()))?;
        data["loaded"] = json!(loaded);
        Ok(ToolOutput {
            text,
            data,
            ..ToolOutput::default()
        })
    }
}

/// Размер модели в мегабайтах, для строки списка.
fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes / (1024 * 1024))
}

/// `transcribe` — распознать речь клипа и запомнить результат.
struct Transcribe {
    state: Arc<State>,
}

/// Вход `transcribe`.
#[derive(Deserialize)]
struct TranscribeInput {
    clip_id: String,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    wait_seconds: Option<f64>,
}

impl Tool for Transcribe {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "transcribe",
            description: "Transcribe a clip's speech into timed segments on the timeline. Runs \
                          as a background job: a short clip answers at once, a long one answers \
                          with a job id to wait on with job_status. The result is cached for \
                          transcript_get, transcript_search and captions_make.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "The clip whose sound to transcribe." },
                    "model_id": { "type": "string", "description": "Model id from speech_models; the first downloaded model when absent." },
                    "language": { "type": "string", "description": "Language code like \"ru\"; auto-detect when absent." },
                    "wait_seconds": {
                        "type": "number",
                        "description": format!("How long to wait for the result before answering with the job id; 20 when absent, at most {WAIT_MAX}.")
                    }
                },
                "required": ["clip_id"],
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: TranscribeInput = crate::input(input)?;
        let ready = transcript(
            &self.state,
            ctx,
            &input.clip_id,
            input.model_id.as_deref(),
            input.language.as_deref(),
            wait_from(input.wait_seconds),
        )?;
        Ok(match ready {
            Ready::Done(entry) => transcribed(&entry),
            Ready::Running(job) => ToolOutput {
                text: format!("Transcription is still running: {}", job.hint()),
                data: job.data(),
                ..ToolOutput::default()
            },
        })
    }
}

/// Ответ о готовой расшифровке: сколько сегментов и слов, от и до.
fn transcribed(entry: &CachedTranscript) -> ToolOutput {
    let words: usize = entry
        .segments
        .iter()
        .map(|segment| segment.words.len())
        .sum();
    let first = entry.segments.first().map_or(0.0, |segment| segment.start);
    let last = entry.segments.last().map_or(0.0, |segment| segment.end);
    let text = format!(
        "Transcribed clip {}: {} segments, {} words, {:.3}s-{:.3}s (model {}).",
        entry.clip_id,
        entry.segments.len(),
        words,
        first,
        last,
        entry.model
    );
    ToolOutput {
        text,
        data: transcript_json(entry, &entry.segments, false),
        ..ToolOutput::default()
    }
}

/// `transcript_get` — сегменты расшифровки с таймкодами.
struct TranscriptGet {
    state: Arc<State>,
}

/// Вход `transcript_get`.
#[derive(Deserialize)]
struct GetInput {
    clip_id: String,
    #[serde(default)]
    from: Option<f64>,
    #[serde(default)]
    to: Option<f64>,
    #[serde(default)]
    words: bool,
}

impl Tool for TranscriptGet {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "transcript_get",
            description: "Read a clip's transcript as timed segments on the timeline, optionally \
                          a time range and the per-word timings. Transcribes the clip first when \
                          it was not transcribed yet.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "The clip whose transcript to read." },
                    "from": { "type": "number", "description": "Only segments ending after this timeline second." },
                    "to": { "type": "number", "description": "Only segments starting before this timeline second." },
                    "words": { "type": "boolean", "description": "Include per-word timings." }
                },
                "required": ["clip_id"],
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: GetInput = crate::input(input)?;
        let entry = ready_transcript(&self.state, ctx, &input.clip_id)?;
        let selected: Vec<Segment> = entry
            .segments
            .iter()
            .filter(|segment| {
                input.from.is_none_or(|from| segment.end > from)
                    && input.to.is_none_or(|to| segment.start < to)
            })
            .take(MAX_SEGMENTS)
            .cloned()
            .collect();

        let mut text = String::new();
        for segment in &selected {
            text.push_str(&format!(
                "{:.3}-{:.3}  {}\n",
                segment.start, segment.end, segment.text
            ));
            if input.words {
                for word in &segment.words {
                    text.push_str(&format!(
                        "    {:.3}-{:.3}  {}\n",
                        word.start, word.end, word.text
                    ));
                }
            }
        }
        if text.is_empty() {
            text.push_str("No transcript in that range.\n");
        }
        Ok(ToolOutput {
            text,
            data: transcript_json(&entry, &selected, input.words),
            ..ToolOutput::default()
        })
    }
}

/// `transcript_search` — найти фразу в расшифровке.
struct TranscriptSearch {
    state: Arc<State>,
}

/// Вход `transcript_search`.
#[derive(Deserialize)]
struct SearchInput {
    query: String,
    #[serde(default)]
    clip_id: Option<String>,
}

impl Tool for TranscriptSearch {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "transcript_search",
            description: "Find a spoken phrase in one clip's transcript, or in every transcript \
                          already made. Matches ignoring case, punctuation and ё/е, with a fuzzy \
                          match when most query words are heard.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The phrase to look for." },
                    "clip_id": { "type": "string", "description": "Restrict to one clip; every cached transcript when absent." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            class: ToolClass::Read,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: SearchInput = crate::input(input)?;
        if input.query.trim().is_empty() {
            return Err(ToolError::BadInput("query is empty".to_owned()));
        }
        let entries = match input.clip_id.as_deref() {
            Some(clip_id) => vec![ready_transcript(&self.state, ctx, clip_id)?],
            None => self.state.all(),
        };
        if entries.is_empty() {
            return Err(ToolError::Failed(
                "no transcript yet - call transcribe (or pass clipId) first".to_owned(),
            ));
        }

        let mut matches = Vec::new();
        for entry in &entries {
            for segment in &entry.segments {
                if let Some(kind) = search::matches(&segment.text, &input.query) {
                    matches.push(json!({
                        "clip_id": entry.clip_id,
                        "start": segment.start,
                        "end": segment.end,
                        "text": segment.text,
                        "kind": kind.as_str(),
                    }));
                }
            }
        }

        let mut text = if matches.is_empty() {
            format!("No match for {:?}.\n", input.query)
        } else {
            format!("{} match(es) for {:?}:\n", matches.len(), input.query)
        };
        for hit in &matches {
            text.push_str(&format!(
                "{}  {:.3}-{:.3}  {}\n",
                hit["clip_id"].as_str().unwrap_or("?"),
                hit["start"].as_f64().unwrap_or(0.0),
                hit["end"].as_f64().unwrap_or(0.0),
                hit["text"].as_str().unwrap_or("")
            ));
        }
        Ok(ToolOutput {
            text,
            data: json!({ "query": input.query, "matches": matches }),
            ..ToolOutput::default()
        })
    }
}

/// `captions_make` — титры из слов расшифровки одним шагом undo.
struct CaptionsMake {
    state: Arc<State>,
}

/// Вход `captions_make`.
#[derive(Deserialize)]
struct CaptionsInput {
    clip_id: String,
    #[serde(default)]
    style: Option<StyleInput>,
    #[serde(default)]
    rules: Option<RulesInput>,
}

/// Стиль титра; заданные поля заменяют умолчания.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StyleInput {
    font_family: Option<String>,
    font_size: Option<f64>,
    font_weight: Option<f64>,
    color: Option<String>,
    stroke_width: Option<f64>,
    stroke_color: Option<String>,
    background: Option<String>,
    shadow: Option<bool>,
    offset_y: Option<f64>,
}

/// Правила нарезки титров; заданные поля заменяют умолчания.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RulesInput {
    max_chars: Option<usize>,
    max_seconds: Option<f64>,
    pause_seconds: Option<f64>,
    min_seconds: Option<f64>,
    comma_after_chars: Option<usize>,
}

impl Tool for CaptionsMake {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "captions_make",
            description: "Make subtitles from a clip's transcribed words, as one undoable batch \
                          of title clips above the video. Defaults suit vertical video: large \
                          bold white text, dark outline, lower third.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "clip_id": { "type": "string", "description": "The clip whose words become captions." },
                    "style": {
                        "type": "object",
                        "description": "Title look; unset fields keep the default.",
                        "properties": {
                            "font_family": { "type": "string" },
                            "font_size": { "type": "number", "description": "Cap height as a fraction of frame height." },
                            "font_weight": { "type": "number" },
                            "color": { "type": "string", "description": "CSS hex fill colour." },
                            "stroke_width": { "type": "number" },
                            "stroke_color": { "type": "string" },
                            "background": { "type": "string", "description": "Solid plate colour, empty for none." },
                            "shadow": { "type": "boolean" },
                            "offset_y": { "type": "number", "description": "Frame-height fraction from centre, positive down." }
                        },
                        "additionalProperties": false
                    },
                    "rules": {
                        "type": "object",
                        "description": "How words are cut into lines.",
                        "properties": {
                            "max_chars": { "type": "integer" },
                            "max_seconds": { "type": "number" },
                            "pause_seconds": { "type": "number" },
                            "min_seconds": { "type": "number" },
                            "comma_after_chars": { "type": "integer" }
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["clip_id"],
                "additionalProperties": false
            }),
            class: ToolClass::Edit,
        }
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        let input: CaptionsInput = crate::input(input)?;
        let entry = ready_transcript(&self.state, ctx, &input.clip_id)?;
        if entry.segments.is_empty() {
            return Ok(ToolOutput::text(
                "The clip has no transcribed speech to caption.",
            ));
        }
        let style = CaptionStyle::from_input(input.style);
        let rules = rules_from(input.rules);
        let lines = caption_lines(&entry.segments, &rules);
        let offset_y = style.offset_y;
        let commands: Vec<Command> = lines
            .iter()
            .map(|line| Command::AddTextClip {
                track_id: None,
                above: true,
                start: line.start,
                style: Some(style.text_style(&line.text)),
                duration: Some((line.end - line.start).max(MIN_CAPTION_SECONDS)),
                offset_y: Some(offset_y),
            })
            .collect();
        let count = commands.len();

        let path = ctx.project()?;
        let before = load_project(ctx)?;
        let reply = ctx.call(concat_api::Request::EditApply {
            path,
            command: Box::new(Command::Batch { commands }),
        })?;
        let ids = new_clip_ids(&before, &reply)?;

        let text = format!(
            "Added {count} caption(s) over clip {} (ids: {}).",
            entry.clip_id,
            ids.join(", ")
        );
        Ok(ToolOutput {
            text,
            data: json!({ "count": count, "ids": ids }),
            ..ToolOutput::default()
        })
    }
}

/// Стиль титров: умолчания под вертикальное видео, поля входа их заменяют.
struct CaptionStyle {
    font_family: String,
    font_size: f64,
    font_weight: f64,
    color: String,
    stroke_width: f64,
    stroke_color: String,
    background: String,
    shadow: bool,
    offset_y: f64,
}

impl CaptionStyle {
    /// Стиль из входа или умолчание: крупный жирный белый с тёмной обводкой.
    fn from_input(input: Option<StyleInput>) -> CaptionStyle {
        let mut style = CaptionStyle {
            font_family: "Hanken Grotesk".to_owned(),
            font_size: 0.05,
            font_weight: 700.0,
            color: "#ffffff".to_owned(),
            stroke_width: 0.004,
            stroke_color: "#000000".to_owned(),
            background: String::new(),
            shadow: true,
            offset_y: DEFAULT_OFFSET_Y,
        };
        if let Some(input) = input {
            if let Some(value) = input.font_family {
                style.font_family = value;
            }
            if let Some(value) = input.font_size {
                style.font_size = value;
            }
            if let Some(value) = input.font_weight {
                style.font_weight = value;
            }
            if let Some(value) = input.color {
                style.color = value;
            }
            if let Some(value) = input.stroke_width {
                style.stroke_width = value;
            }
            if let Some(value) = input.stroke_color {
                style.stroke_color = value;
            }
            if let Some(value) = input.background {
                style.background = value;
            }
            if let Some(value) = input.shadow {
                style.shadow = value;
            }
            if let Some(value) = input.offset_y {
                style.offset_y = value;
            }
        }
        style
    }

    /// Один титр этого стиля с заданным текстом.
    fn text_style(&self, content: &str) -> TextStyle {
        TextStyle {
            content: content.to_owned(),
            font_family: self.font_family.clone(),
            font_size: self.font_size,
            font_weight: self.font_weight,
            color: self.color.clone(),
            stroke_width: self.stroke_width,
            stroke_color: self.stroke_color.clone(),
            background: self.background.clone(),
            shadow: self.shadow,
            ..TextStyle::default()
        }
    }
}

/// Правила нарезки из входа или умолчание.
fn rules_from(input: Option<RulesInput>) -> LineRules {
    let mut rules = LineRules::default();
    if let Some(input) = input {
        if let Some(value) = input.max_chars {
            rules.max_chars = value;
        }
        if let Some(value) = input.max_seconds {
            rules.max_seconds = value;
        }
        if let Some(value) = input.pause_seconds {
            rules.pause_seconds = value;
        }
        if let Some(value) = input.min_seconds {
            rules.min_seconds = value;
        }
        if let Some(value) = input.comma_after_chars {
            rules.comma_after_chars = value;
        }
    }
    rules
}

/// Расшифровка клипа: готова или ещё идёт фоновой задачей.
enum Ready {
    Done(CachedTranscript),
    Running(Job),
}

/// Готовая расшифровка для инструментов, которым она нужна целиком.
/// Если её нет, запускает распознавание и ждёт; не дождались — ошибка с id задачи.
fn ready_transcript(
    state: &Arc<State>,
    ctx: &mut ToolContext<'_>,
    clip_id: &str,
) -> Result<CachedTranscript, ToolError> {
    match transcript(state, ctx, clip_id, None, None, wait_from(None))? {
        Ready::Done(entry) => Ok(entry),
        Ready::Running(job) => Err(ToolError::Pending(format!(
            "The transcript of clip {clip_id} is not ready yet: {} Then call this tool again.",
            job.hint()
        ))),
    }
}

/// Расшифровка клипа: из кэша по модели и языку, из идущей задачи или новой задачей.
/// Ждёт её не дольше `wait`.
fn transcript(
    state: &Arc<State>,
    ctx: &mut ToolContext<'_>,
    clip_id: &str,
    model_id: Option<&str>,
    language: Option<&str>,
    wait: Duration,
) -> Result<Ready, ToolError> {
    let project = load_project(ctx)?;
    let clip = find_clip(&project, clip_id)?;
    if let Some(hint) = muted_with_sound(&project, clip_id) {
        return Err(ToolError::BadInput(hint));
    }

    if let Some(cached) = state.find(&clip.media_path, clip.source_start, clip.window) {
        let reuse = match model_id {
            None => true,
            Some(model) => cached.model == model && cached.language.as_deref() == language,
        };
        if reuse {
            return Ok(Ready::Done(cached));
        }
    }

    let id = match ctx.jobs.running_for(JobKind::Transcribe, &clip.id) {
        Some(job) => job.id,
        None => start_transcription(state, ctx, &clip, model_id, language)?,
    };
    let job = ctx
        .jobs
        .wait(&id, wait)
        .ok_or_else(|| ToolError::Failed(format!("job {id} is gone")))?;
    match job.state {
        JobState::Running { .. } => Ok(Ready::Running(job)),
        JobState::Failed(error) => Err(ToolError::Failed(error)),
        JobState::Done(_) => state
            .find(&clip.media_path, clip.source_start, clip.window)
            .map(Ready::Done)
            .ok_or_else(|| {
                ToolError::Failed(format!("the transcript of clip {} is missing", clip.id))
            }),
    }
}

/// Запускает распознавание клипа задачей и отдаёт её id. Распознаватель один:
/// пока идёт другая расшифровка, новая не начинается.
fn start_transcription(
    state: &Arc<State>,
    ctx: &mut ToolContext<'_>,
    clip: &ClipRef,
    model_id: Option<&str>,
    language: Option<&str>,
) -> Result<String, ToolError> {
    let other = ctx
        .jobs
        .list()
        .into_iter()
        .find(|job| job.kind == JobKind::Transcribe && job.is_running());
    if let Some(job) = other {
        return Err(ToolError::Pending(format!(
            "Another transcription is running, one at a time: {}",
            job.hint()
        )));
    }

    let dirs = dirs(ctx)?;
    let model = resolve_model(&dirs, model_id)?;
    let language = language.map(str::to_owned);
    let request = TranscribeRequest {
        path: clip.media_path.clone(),
        audio_stream: clip.audio_stream,
        source_start: clip.source_start,
        window: clip.window,
        model_id: model.clone(),
        language: language.clone(),
    };
    let clip = clip.clone();
    let worker = Arc::clone(state);
    let stopper = Arc::clone(state);
    ctx.jobs.spawn(
        JobKind::Transcribe,
        &clip.id.clone(),
        move || stopper.transcriber.cancel(),
        move |report| {
            let report = report.clone();
            let segments = worker
                .transcriber
                .transcribe(&dirs, &request, move |stage| {
                    let (name, fraction) = stage_of(stage);
                    report.stage(name, fraction);
                })?;
            let entry = CachedTranscript {
                clip_id: clip.id,
                media: clip.media_path,
                source_start: clip.source_start,
                window: clip.window,
                model,
                language,
                segments: onto_timeline(&segments, clip.start, clip.speed),
            };
            worker
                .put(entry.clone())
                .map_err(|error| error.to_string())?;
            Ok(transcribed(&entry))
        },
    )
}

/// Этап распознавания словами и доля сделанного.
fn stage_of(stage: JobStage) -> (&'static str, f32) {
    match stage {
        JobStage::Reading => ("reading audio", 0.0),
        JobStage::Loading => ("loading model", 0.0),
        JobStage::Working { done, .. } => ("transcribing", done),
    }
}

/// Смещает времена окна на таймлайн: `/ speed` и `+ start клипа`.
fn onto_timeline(segments: &[Segment], start: f64, speed: f64) -> Vec<Segment> {
    let speed = if speed > 0.0 { speed } else { 1.0 };
    let at = |seconds: f64| start + seconds / speed;
    segments
        .iter()
        .map(|segment| Segment {
            start: at(segment.start),
            end: at(segment.end),
            text: segment.text.clone(),
            words: segment
                .words
                .iter()
                .map(|word| Word {
                    start: at(word.start),
                    end: at(word.end),
                    text: word.text.clone(),
                })
                .collect(),
        })
        .collect()
}

/// Модель для запуска: спрошенная или первая скачанная.
fn resolve_model(dirs: &AppDirs, requested: Option<&str>) -> Result<String, ToolError> {
    if let Some(id) = requested {
        if catalog::known(id).is_some() || Path::new(id).is_absolute() {
            return Ok(id.to_owned());
        }
        let status = Transcriber::status(dirs).map_err(ToolError::Failed)?;
        if let Some(found) = status.found.iter().find(|model| names_model(model, id)) {
            return Ok(found.id.clone());
        }
        return Err(ToolError::BadInput(format!(
            "unknown model {id:?} - pass a catalogue id or a model found on disk (its path or \
             file name), see speech_models"
        )));
    }
    let status = Transcriber::status(dirs).map_err(ToolError::Failed)?;
    if let Some(model) = status.models.iter().find(|model| model.downloaded) {
        return Ok(model.id.clone());
    }
    if let Some(found) = status.found.first() {
        return Ok(found.id.clone());
    }
    Err(ToolError::Failed(
        "no transcription model is downloaded - call speech_models, then download one in \
         Settings > Transcriber"
            .to_owned(),
    ))
}

/// Так ли зовут модель с диска: подпись или имя файла без расширения, без учёта регистра.
fn names_model(model: &ModelStatus, name: &str) -> bool {
    let stem = Path::new(&model.id)
        .file_stem()
        .and_then(|stem| stem.to_str());
    model.label.eq_ignore_ascii_case(name)
        || stem.is_some_and(|stem| stem.eq_ignore_ascii_case(name))
}

/// Проект из ответа редактора.
fn load_project(ctx: &mut ToolContext<'_>) -> Result<Project, ToolError> {
    let path = ctx.project()?;
    let value = ctx.call(concat_api::Request::ProjectGet { path })?;
    project_from(&value)
}

/// Разбирает проект из JSON редактора.
fn project_from(value: &Value) -> Result<Project, ToolError> {
    let project = value
        .get("project")
        .cloned()
        .ok_or_else(|| ToolError::Failed("the editor reply has no project".to_owned()))?;
    serde_json::from_value(project).map_err(|error| ToolError::Failed(error.to_string()))
}

/// Клип по id, где-либо на таймлайнах проекта.
fn find_clip(project: &Project, clip_id: &str) -> Result<ClipRef, ToolError> {
    let clip = project
        .timelines
        .iter()
        .find_map(|timeline| {
            timeline
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| clip.as_ref())
        })
        .ok_or_else(|| ToolError::Failed(format!("clip {clip_id} is not on any timeline")))?;
    let media = project
        .media_by_id(&clip.media_id)
        .ok_or_else(|| ToolError::Failed(format!("clip {clip_id} has no media in the bin")))?;
    Ok(ClipRef {
        id: clip.id.clone(),
        media_path: media.path.clone(),
        audio_stream: clip.audio_stream,
        start: clip.start,
        speed: clip.speed,
        source_start: clip.source_start,
        window: clip.duration * clip.speed,
    })
}

/// Подсказка, если клип немой, а его звук разложен по связанным клипам: распознавать надо их.
fn muted_with_sound(project: &Project, clip_id: &str) -> Option<String> {
    let timeline = project
        .timelines
        .iter()
        .find(|timeline| timeline.clip(clip_id).is_some())?;
    let clip = timeline.clip(clip_id)?;
    if clip.muted != Some(true) {
        return None;
    }
    let link = links::link_of(clip)?;
    let sounds: Vec<String> = timeline
        .clips
        .iter()
        .filter(|other| {
            other.id != clip.id
                && other.kind == ClipKind::Audio
                && links::link_of(other) == Some(link)
        })
        .map(|other| match other.audio_stream {
            Some(stream) => format!("{} (stream {stream})", other.id),
            None => other.id.clone(),
        })
        .collect();
    if sounds.is_empty() {
        return None;
    }
    Some(format!(
        "clip {clip_id} is muted: its sound is in the linked clips {}. Transcribe the one that \
         carries the voice.",
        sounds.join(", ")
    ))
}

/// Компактный JSON расшифровки: клип, модель, язык и выбранные сегменты.
fn transcript_json(entry: &CachedTranscript, segments: &[Segment], words: bool) -> Value {
    let segments: Vec<Value> = segments
        .iter()
        .map(|segment| {
            let mut object = json!({
                "start": segment.start,
                "end": segment.end,
                "text": segment.text,
            });
            if words {
                object["words"] = Value::Array(
                    segment
                        .words
                        .iter()
                        .map(|word| {
                            json!({ "start": word.start, "end": word.end, "text": word.text })
                        })
                        .collect(),
                );
            }
            object
        })
        .collect();
    json!({
        "clip_id": entry.clip_id,
        "model": entry.model,
        "language": entry.language,
        "segments": segments,
    })
}

/// Папки приложения из ответа `version`.
fn dirs(ctx: &mut ToolContext<'_>) -> Result<AppDirs, ToolError> {
    let value = ctx.call(concat_api::Request::Version)?;
    let dirs = value
        .get("dirs")
        .ok_or_else(|| ToolError::Failed("the version reply has no dirs".to_owned()))?;
    let field = |name: &str| {
        dirs.get(name)
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| ToolError::Failed(format!("the version reply has no {name} dir")))
    };
    Ok(AppDirs {
        config: field("config")?,
        data: field("data")?,
    })
}

/// Ids клипов, появившихся после правки, по времени начала.
fn new_clip_ids(before: &Project, reply: &Value) -> Result<Vec<String>, ToolError> {
    let seen: std::collections::HashSet<&str> = before
        .timelines
        .iter()
        .flat_map(|timeline| timeline.clips.iter().map(|clip| clip.id.as_str()))
        .collect();
    let after = project_from(reply)?;
    let mut added: Vec<(f64, String)> = after
        .timelines
        .iter()
        .flat_map(|timeline| timeline.clips.iter())
        .filter(|clip| !seen.contains(clip.id.as_str()))
        .map(|clip| (clip.start, clip.id.clone()))
        .collect();
    added.sort_by(|left, right| left.0.total_cmp(&right.0));
    Ok(added.into_iter().map(|(_, id)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Editor;
    use concat_api::{Api, Request, Response};
    use concat_project::commands::NewMedia;
    use concat_project::model::{Clip, MediaKind};

    /// Редактор, к которому инструмент не должен обращаться.
    struct NullEditor;

    impl Editor for NullEditor {
        fn dispatch(
            &mut self,
            _request: Request,
        ) -> Result<serde_json::Value, concat_api::ApiError> {
            panic!("the editor should not be asked");
        }
    }

    /// Путь медиа в тестах; файла на диске нет, он и не нужен.
    const MEDIA: &str = "/nowhere/a.mp4";

    /// Временная папка с уникальным именем, убирается в конце теста.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("cutless-tools-speech-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        root
    }

    fn api(root: &Path) -> Api {
        Api::with_dirs(
            AppDirs {
                config: root.join("config"),
                data: root.join("data"),
            },
            Arc::new(|_| {}),
        )
    }

    fn dispatch(api: &mut Api, request: Request) -> Value {
        match api.dispatch(request) {
            Response::Result(reply) => serde_json::to_value(reply).expect("json"),
            Response::Error(error) => panic!("refused: {error}"),
        }
    }

    /// Создаёт проект с одним видео-клипом с sound и отдаёт путь и клип.
    fn project_with_clip(api: &mut Api, root: &Path, name: &str) -> (String, String, Clip) {
        let location = root.to_string_lossy().into_owned();
        dispatch(
            api,
            Request::ProjectCreate {
                location,
                name: name.to_owned(),
                video: None,
            },
        );
        let path = root.join(name).to_string_lossy().into_owned();
        let media = NewMedia {
            path: MEDIA.to_owned(),
            name: "a.mp4".to_owned(),
            duration: Some(8.0),
            kind: MediaKind::Video,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            frame_rate_fraction: Some("30/1".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: Some("aac".to_owned()),
            has_audio: true,
            audio_tracks: Vec::new(),
            origin: None,
            color_space: Default::default(),
        };
        let added = dispatch(
            api,
            Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddMedia { item: media }),
            },
        );
        let media_id = added["createdId"].as_str().expect("media id").to_owned();
        let placed = dispatch(
            api,
            Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddClipAtFirstFree {
                    media_id,
                    start: 2.0,
                }),
            },
        );
        let clip_id = placed["createdId"].as_str().expect("clip id").to_owned();
        let project: Project = serde_json::from_value(placed["project"].clone()).expect("project");
        let clip = project
            .timelines
            .iter()
            .flat_map(|timeline| timeline.clips.iter())
            .find(|clip| clip.id == clip_id)
            .expect("the clip")
            .as_ref()
            .clone();
        (path, clip_id, clip)
    }

    /// Расшифровка клипа в кэше с временами таймлайна.
    fn cached(clip: &Clip) -> CachedTranscript {
        CachedTranscript {
            clip_id: clip.id.clone(),
            media: MEDIA.to_owned(),
            source_start: clip.source_start,
            window: clip.duration * clip.speed,
            model: "base".to_owned(),
            language: None,
            segments: vec![Segment {
                start: clip.start,
                end: clip.start + 1.0,
                text: "Hello there.".to_owned(),
                words: vec![
                    Word {
                        start: clip.start,
                        end: clip.start + 0.5,
                        text: "Hello".to_owned(),
                    },
                    Word {
                        start: clip.start + 0.5,
                        end: clip.start + 1.0,
                        text: "there.".to_owned(),
                    },
                ],
            }],
        }
    }

    /// Ошибка разбора входа понятна: без clipId инструмент не зовёт редактор.
    #[test]
    fn transcribe_needs_a_clip_id() {
        let state = Arc::new(State::default());
        let tool = Transcribe { state };
        let mut editor = NullEditor;
        let mut ctx = ToolContext {
            editor: &mut editor,
            project: Some("/x".to_owned()),
            jobs: crate::Jobs::default(),
        };
        let error = tool.call(&mut ctx, json!({})).expect_err("bad input");
        assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
    }

    /// `transcript_get` и `captions_make` тоже требуют clipId.
    #[test]
    fn get_and_captions_need_a_clip_id() {
        let state = Arc::new(State::default());
        let get = TranscriptGet {
            state: Arc::clone(&state),
        };
        let captions = CaptionsMake { state };
        let mut editor = NullEditor;
        let mut ctx = ToolContext {
            editor: &mut editor,
            project: Some("/x".to_owned()),
            jobs: crate::Jobs::default(),
        };
        for error in [
            get.call(&mut ctx, json!({})).expect_err("bad input"),
            captions.call(&mut ctx, json!({})).expect_err("bad input"),
        ] {
            assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
        }
    }

    /// `speech_models` перечисляет каталог, когда ничего не скачано.
    #[test]
    fn speech_models_lists_the_catalogue() {
        let root = scratch("models");
        let mut api = api(&root);
        let tool = SpeechModels {
            state: Arc::new(State::default()),
        };
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
            jobs: crate::Jobs::default(),
        };
        let output = tool.call(&mut ctx, json!({})).expect("models");
        assert!(output.text.contains("tiny.en"), "{}", output.text);
        let models = output.data["models"].as_array().expect("models");
        assert!(!models.is_empty());
    }

    /// `transcript_get` читает слова из кэша, времена — секунды таймлайна.
    #[test]
    fn transcript_get_reads_words_from_the_cache() {
        let root = scratch("get");
        let mut api = api(&root);
        let (path, clip_id, clip) = project_with_clip(&mut api, &root, "Get");

        let state = Arc::new(State::default());
        state.cache.lock().expect("cache").push(cached(&clip));
        let tool = TranscriptGet {
            state: Arc::clone(&state),
        };
        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path),
            jobs: crate::Jobs::default(),
        };
        let output = tool
            .call(&mut ctx, json!({ "clip_id": clip_id, "words": true }))
            .expect("get");
        let segments = output.data["segments"].as_array().expect("segments");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0]["text"], "Hello there.");
        assert_eq!(segments[0]["start"].as_f64().expect("start"), clip.start);
        assert_eq!(segments[0]["words"].as_array().expect("words").len(), 2);
    }

    /// Титры из кэша становятся одним Batch над видео и отдают свои id.
    #[test]
    fn captions_make_batches_titles_over_the_clip() {
        let root = scratch("captions");
        let mut api = api(&root);
        let (path, clip_id, clip) = project_with_clip(&mut api, &root, "Captions");

        let state = Arc::new(State::default());
        state.cache.lock().expect("cache").push(cached(&clip));
        let tool = CaptionsMake {
            state: Arc::clone(&state),
        };
        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path.clone()),
            jobs: crate::Jobs::default(),
        };
        let output = tool
            .call(&mut ctx, json!({ "clip_id": clip_id }))
            .expect("captions");

        let ids = output.data["ids"].as_array().expect("ids");
        assert_eq!(ids.len(), 1);
        assert_eq!(output.data["count"], 1);

        let view_value = dispatch(&mut api, Request::ProjectGet { path });
        let view: Project = serde_json::from_value(view_value["project"].clone()).expect("project");
        let titles: Vec<&Clip> = view
            .timelines
            .iter()
            .flat_map(|timeline| timeline.clips.iter())
            .filter(|clip| clip.kind == concat_project::model::ClipKind::Text)
            .map(|clip| clip.as_ref())
            .collect();
        assert_eq!(titles.len(), 1);
        assert_eq!(
            titles[0].text.as_ref().expect("text").content,
            "Hello there."
        );
        assert!(titles[0].offset_y > 0.0, "lower third");
    }

    /// Поиск по уже сделанной расшифровке находит фразу и не транскрибирует.
    #[test]
    fn transcript_search_reads_the_cached_transcript() {
        let root = scratch("search");
        let mut api = api(&root);
        let (path, clip_id, clip) = project_with_clip(&mut api, &root, "Search");

        let state = Arc::new(State::default());
        state.cache.lock().expect("cache").push(cached(&clip));
        let tool = TranscriptSearch {
            state: Arc::clone(&state),
        };
        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path),
            jobs: crate::Jobs::default(),
        };
        let output = tool
            .call(&mut ctx, json!({ "query": "hello", "clip_id": clip_id }))
            .expect("search");
        let matches = output.data["matches"].as_array().expect("matches");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["kind"], "exact");
    }

    /// Пока расшифровка идёт, transcribe отвечает id задачи, а transcript_get
    /// дожидается её конца и читает результат.
    #[test]
    fn a_running_transcription_answers_with_its_job_then_its_result() {
        let root = scratch("job");
        let mut api = api(&root);
        let (path, clip_id, clip) = project_with_clip(&mut api, &root, "Job");

        let state = Arc::new(State::default());
        let jobs = crate::Jobs::default();
        let (go, gate) = std::sync::mpsc::channel::<()>();
        let worker = Arc::clone(&state);
        let entry = cached(&clip);
        let id = jobs
            .spawn(
                JobKind::Transcribe,
                &clip_id,
                || {},
                move |_| {
                    gate.recv().map_err(|error| error.to_string())?;
                    worker
                        .put(entry.clone())
                        .map_err(|error| error.to_string())?;
                    Ok(transcribed(&entry))
                },
            )
            .expect("started");

        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path),
            jobs: jobs.clone(),
        };
        let running = Transcribe {
            state: Arc::clone(&state),
        }
        .call(&mut ctx, json!({ "clip_id": clip_id, "wait_seconds": 0 }))
        .expect("running");
        assert!(running.text.contains("still running"), "{}", running.text);
        assert!(running.text.contains(&id), "{}", running.text);

        go.send(()).expect("go");
        let output = TranscriptGet {
            state: Arc::clone(&state),
        }
        .call(&mut ctx, json!({ "clip_id": clip_id }))
        .expect("waited for the job");
        assert_eq!(output.data["segments"][0]["text"], "Hello there.");
        jobs.shutdown();
    }

    /// Распознаватель один: вторую расшифровку не начинают, пока идёт первая.
    #[test]
    fn a_second_transcription_waits_for_the_first() {
        let root = scratch("second");
        let mut api = api(&root);
        let (path, clip_id, _clip) = project_with_clip(&mut api, &root, "Second");

        let jobs = crate::Jobs::default();
        let (go, gate) = std::sync::mpsc::channel::<()>();
        jobs.spawn(
            JobKind::Transcribe,
            "another-clip",
            || {},
            move |_| {
                gate.recv().map_err(|error| error.to_string())?;
                Ok(ToolOutput::default())
            },
        )
        .expect("started");

        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path),
            jobs: jobs.clone(),
        };
        let error = Transcribe {
            state: Arc::new(State::default()),
        }
        .call(&mut ctx, json!({ "clip_id": clip_id }))
        .expect_err("one at a time");
        assert!(matches!(error, ToolError::Pending(_)), "{error:?}");
        assert!(
            error.to_string().contains("Another transcription"),
            "{error}"
        );

        go.send(()).expect("go");
        jobs.shutdown();
    }

    /// Немое видео со звуком в связанных клипах не распознаётся: подсказка называет звуковые клипы.
    #[test]
    fn a_muted_video_points_at_its_linked_sound_clips() {
        use concat_project::model::{AudioTrack, MediaKind};

        let root = scratch("muted");
        let mut api = api(&root);
        let location = root.to_string_lossy().into_owned();
        dispatch(
            &mut api,
            Request::ProjectCreate {
                location,
                name: "Muted".to_owned(),
                video: None,
            },
        );
        let path = root.join("Muted").to_string_lossy().into_owned();
        let track = |index: u32| AudioTrack {
            index,
            codec: "aac".to_owned(),
            channels: 2,
            sample_rate: 48_000,
            title: String::new(),
            language: String::new(),
        };
        let media = NewMedia {
            path: MEDIA.to_owned(),
            name: "a.mp4".to_owned(),
            duration: Some(8.0),
            kind: MediaKind::Video,
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            frame_rate_fraction: Some("30/1".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: Some("aac".to_owned()),
            has_audio: true,
            audio_tracks: vec![track(1), track(2)],
            origin: None,
            color_space: Default::default(),
        };
        let added = dispatch(
            &mut api,
            Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddMedia { item: media }),
            },
        );
        let media_id = added["createdId"].as_str().expect("media id").to_owned();

        let jobs = crate::Jobs::default();
        let mut ctx = ToolContext {
            editor: &mut api,
            project: Some(path),
            jobs: jobs.clone(),
        };
        let registry = crate::default_registry();
        let placed = registry
            .call(
                &mut ctx,
                "clip_add",
                json!({ "media_id": media_id, "start": 0.0 }),
            )
            .expect("placed");
        let video = placed.data["createdId"].as_str().expect("id").to_owned();

        let error = registry
            .call(&mut ctx, "transcribe", json!({ "clip_id": video }))
            .expect_err("muted");
        assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
        let text = error.to_string();
        assert!(text.contains("is muted"), "{text}");
        assert!(text.contains("(stream 2)"), "{text}");
        assert!(jobs.list().is_empty(), "nothing was started");
    }

    /// Ответ без расшифровки подсказывает сначала позвать transcribe.
    #[test]
    fn transcript_search_without_a_transcript_says_so() {
        let state = Arc::new(State::default());
        let tool = TranscriptSearch {
            state: Arc::clone(&state),
        };
        let mut editor = NullEditor;
        let mut ctx = ToolContext {
            editor: &mut editor,
            project: Some("/x".to_owned()),
            jobs: crate::Jobs::default(),
        };
        let error = tool
            .call(&mut ctx, json!({ "query": "hello" }))
            .expect_err("no transcript");
        assert!(matches!(error, ToolError::Failed(_)), "{error:?}");
    }
}
