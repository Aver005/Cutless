// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! Speech transcription: audio in, timed caption segments out.
//!
//! transcribe.cpp runs in-process through `transcribe-cpp`, the way Kokoro
//! runs through sherpa-onnx: no binary to ship or find, and cancellation is a
//! token that another thread trips. It reads GGUF models (Whisper, Parakeet,
//! Nemotron, GigaAM) and the older whisper.cpp `.bin` files; on the Mac the
//! Metal build runs the encoder on the GPU.
//!
//! Two pieces:
//!
//! 1. **Models.** A catalogue of whisper.cpp, Parakeet and Nemotron models
//!    ([`crate::catalog`]), downloaded on demand into the app data folder and
//!    streamed with progress. Models already on disk - in the Hugging Face
//!    cache, or a file the user picked - are used where they lie. A loaded
//!    model is cached until another is asked for, because loading one takes
//!    seconds.
//! 2. **Transcription.** The engine's audio decoder hands over the clip's
//!    window as 16 kHz mono floats, and the segments come back relative to
//!    the window, each carrying the words the model timed inside it.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

use concat_host::{AppDirs, SingleFlight};
use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use serde::{Deserialize, Serialize};
use transcribe_cpp::{CancelToken, Model, RunOptions};

use crate::catalog;
use crate::{DownloadProgress, JobStage};

/// Where downloaded models live: `<app data>/whisper-models/<file>`.
fn models_dir(dirs: &AppDirs) -> Result<PathBuf, String> {
    let dir = dirs.data.join("whisper-models");
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    Ok(dir)
}

fn model_file(dirs: &AppDirs, id: &str) -> Result<PathBuf, String> {
    // Ids come from our own table; anything else is a bug, not input.
    let model = catalog::known(id).ok_or_else(|| format!("unknown model {id:?}"))?;
    Ok(models_dir(dirs)?.join(model.file))
}

/// The file a run reads: a model the catalogue placed, a file named outright
/// by `CONCAT_TRANSCRIBE_MODEL` - the hook the ignored `real_audio` test uses
/// to run a GGUF from anywhere on disk - or one the caller named by absolute
/// path: a model found in the Hugging Face cache, or one they picked.
///
/// A catalogue id is a bare name like "base.en"; a chosen file is an absolute
/// path, which is what tells the two apart.
fn run_model_file(dirs: &AppDirs, id: &str) -> Result<PathBuf, String> {
    match std::env::var_os("CONCAT_TRANSCRIBE_MODEL") {
        Some(path) => Ok(PathBuf::from(path)),
        None if Path::new(id).is_absolute() => Ok(PathBuf::from(id)),
        None => model_file(dirs, id),
    }
}

/// One model, as the settings panel shows it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    /// The model id, e.g. "base.en", or a file's path for a found or chosen
    /// model.
    pub id: String,
    /// Display name.
    pub label: String,
    /// One line on what this size trades away.
    pub blurb: String,
    /// Whether the model only understands English.
    pub english_only: bool,
    /// Approximate download size in bytes, for display.
    pub size_bytes: u64,
    /// Whether the file is on disk.
    pub downloaded: bool,
    /// Whether this is a model found on disk rather than one Concat offers.
    pub found: bool,
    /// The family the model belongs to, e.g. "whisper" or "parakeet".
    pub family: String,
}

/// What the settings panel shows.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TranscriberStatus {
    /// Where models are stored, so the settings panel can say so.
    pub models_dir: String,
    /// Every model the panel offers for download.
    pub models: Vec<ModelStatus>,
    /// Models already on disk in the Hugging Face cache.
    pub found: Vec<ModelStatus>,
}

/// One timed word inside a caption, relative to the window's start.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    /// Seconds from the window's start.
    pub start: f64,
    /// Seconds from the window's start.
    pub end: f64,
    /// The word as heard.
    pub text: String,
}

/// One caption, in seconds relative to the transcribed window's start.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Seconds from the window's start.
    pub start: f64,
    /// Seconds from the window's start.
    pub end: f64,
    /// What was said.
    pub text: String,
    /// The words the model timed inside the caption, when it timed any.
    #[serde(default)]
    pub words: Vec<Word>,
}

/// What to transcribe.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRequest {
    /// The media file to transcribe.
    pub path: String,
    /// Which of the file's audio streams, by index; `None` is the first -
    /// the same choice the clip's playback makes, so the words come from
    /// the track that is heard.
    pub audio_stream: Option<u32>,
    /// Seconds into the file where the clip's source window begins.
    pub source_start: f64,
    /// How much source the clip covers, in seconds (`duration * speed`).
    pub window: f64,
    /// Which model to use, e.g. "base.en".
    pub model_id: String,
    /// The language to hear, or `None` to let the model decide.
    #[serde(default)]
    pub language: Option<String>,
}

struct LoadedModel {
    id: String,
    model: Model,
}

/// The transcriber: the one transcription and the one model download that
/// can run at a time, and the model kept loaded between runs.
pub struct Transcriber {
    gate: Arc<SingleFlight>,
    downloads: Arc<SingleFlight>,
    /// Held for the whole transcription, which is what lets
    /// [`Transcriber::delete_model`] refuse to delete a model mid-use.
    model: Mutex<Option<LoadedModel>>,
}

impl Default for Transcriber {
    fn default() -> Self {
        Self::new()
    }
}

impl Transcriber {
    /// An idle transcriber, loading nothing until asked to transcribe.
    pub fn new() -> Self {
        Self {
            gate: Arc::new(SingleFlight::new()),
            downloads: Arc::new(SingleFlight::new()),
            model: Mutex::new(None),
        }
    }

    /// Per-model state, for the settings panel: what the catalogue offers,
    /// and what is already on disk in the Hugging Face cache.
    pub fn status(dirs: &AppDirs) -> Result<TranscriberStatus, String> {
        let dir = models_dir(dirs)?;
        let models = catalog::CATALOG
            .iter()
            .map(|model| ModelStatus {
                id: model.id.to_owned(),
                label: model.label.to_owned(),
                blurb: model.blurb.to_owned(),
                english_only: model.english_only,
                size_bytes: model.approx_bytes,
                downloaded: dir.join(model.file).is_file(),
                found: false,
                family: model.family.name().to_owned(),
            })
            .collect();
        let found = catalog::hf_cache_dir()
            .map(|root| catalog::scan_cache(&root))
            .unwrap_or_default()
            .into_iter()
            .map(|model| ModelStatus {
                id: model.path.to_string_lossy().into_owned(),
                label: catalog::display_name(&model.path),
                blurb: "Already on this computer (Hugging Face cache).".to_owned(),
                english_only: false,
                size_bytes: model.bytes,
                downloaded: true,
                found: true,
                family: model.family.name().to_owned(),
            })
            .collect();
        Ok(TranscriberStatus {
            models_dir: dir.to_string_lossy().into_owned(),
            models,
            found,
        })
    }

    /// Streams one catalogue model into the models folder - from Concat's
    /// mirror when it has one, straight from upstream when it does not.
    /// Blocks for the whole download: run it on its own thread.
    ///
    /// Written to `<file>.part` and renamed at the end: a download killed
    /// halfway must never be mistaken for a model, because the engine would
    /// fail on it with a message that blames the model rather than the
    /// download.
    pub fn download_model(
        &self,
        dirs: &AppDirs,
        id: &str,
        mut progress: impl FnMut(DownloadProgress),
    ) -> Result<(), String> {
        let model = catalog::known(id).ok_or_else(|| format!("unknown model {id:?}"))?;
        let job = self.downloads.begin("model download")?;
        let destination = model_file(dirs, id)?;
        if destination.is_file() {
            return Ok(());
        }
        let partial = destination.with_extension("bin.part");
        // A model whose mirror holds it tries the mirror first; one only its
        // upstream publishes asks upstream - our release would 404.
        let (received, total) = if model.mirrored {
            crate::fetch_model(
                model.file,
                model.upstream,
                &partial,
                id,
                model.approx_bytes,
                model.sha256,
                job.cancel_flag(),
                &mut progress,
            )?
        } else {
            crate::fetch_model_upstream(
                model.file,
                model.upstream,
                &partial,
                id,
                model.approx_bytes,
                model.sha256,
                job.cancel_flag(),
                &mut progress,
            )?
        };
        std::fs::rename(&partial, &destination)
            .map_err(|error| format!("could not finish {}: {error}", destination.display()))?;
        progress(DownloadProgress {
            id: id.to_owned(),
            received,
            total,
            unpacking: false,
            done: true,
        });
        Ok(())
    }

    /// Asks the running download to stop. Idle is a harmless no-op.
    pub fn cancel_download(&self) {
        self.downloads.cancel();
    }

    /// Removes a downloaded model file, unless a transcription is using it.
    /// Only a catalogue model Concat downloaded can be removed; a model found
    /// in the cache or picked by hand is not ours to delete.
    pub fn delete_model(&self, dirs: &AppDirs, id: &str) -> Result<(), String> {
        if catalog::known(id).is_none() {
            return Err(
                "this model is not one Concat downloaded, so it is not Concat's to remove"
                    .to_owned(),
            );
        }
        let file = model_file(dirs, id)?;
        let mut loaded = self.model.try_lock().map_err(|_| {
            "a transcription is running - wait for it or cancel it first".to_owned()
        })?;
        if loaded.as_ref().is_some_and(|model| model.id == id) {
            *loaded = None;
        }
        std::fs::remove_file(&file)
            .map_err(|error| format!("could not remove {}: {error}", file.display()))
    }

    /// The language codes a model knows, so the captions sheet can offer
    /// them. Loads the model if it is not already loaded; an empty list means
    /// the model lists no languages and lets the engine detect them.
    pub fn languages(&self, dirs: &AppDirs, id: &str) -> Result<Vec<String>, String> {
        let path = run_model_file(dirs, id)?;
        if !path.is_file() {
            return Err(format!(
                "model {id} is not downloaded - see Settings > Transcriber"
            ));
        }
        let mut loaded = self
            .model
            .lock()
            .map_err(|_| "transcriber state poisoned".to_owned())?;
        let model = ensure_loaded(&mut loaded, id, &path)?;
        let capabilities = model
            .capabilities()
            .map_err(|error| format!("could not read the model's capabilities: {error}"))?;
        Ok(capabilities.languages)
    }

    /// The model still loaded from the last run, which the next run on it
    /// does not load again. None while a run holds it.
    pub fn loaded_model(&self) -> Option<String> {
        let loaded = self.model.try_lock().ok()?;
        loaded.as_ref().map(|model| model.id.clone())
    }

    /// Transcribes one clip's audio window into caption segments. Blocks
    /// for the whole run: run it on its own thread. `progress` is told each
    /// stage as it begins, and in the work itself how many of the window's
    /// pieces are done - one piece, unless the model caps how much audio it
    /// takes at once, in which case the window is cut to fit.
    ///
    /// Timestamps come back relative to the window: 0 is `source_start`. The
    /// caller owns the clip, so the map onto the timeline (divide by speed,
    /// add the clip's start) happens there, next to the numbers it needs.
    pub fn transcribe(
        &self,
        dirs: &AppDirs,
        request: &TranscribeRequest,
        mut progress: impl FnMut(JobStage) + Send + 'static,
    ) -> Result<Vec<Segment>, String> {
        let job = self.gate.begin("transcription")?;
        let cancel = job.cancel_handle();

        let model_path = run_model_file(dirs, &request.model_id)?;
        if !model_path.is_file() {
            return Err(format!(
                "model {} is not downloaded - see Settings > Transcriber",
                request.model_id
            ));
        }
        // NaN is refused along with zero and negative windows.
        if request.window.is_nan() || request.window <= 0.0 {
            return Err("nothing to transcribe: the clip covers no time".to_owned());
        }

        // The engine's own chatter goes to the app's log, once.
        install_logging();

        // The window, as the engine wants it: 16 kHz mono floats.
        progress(JobStage::Reading);
        let mut decoder = AudioDecoder::open(
            &request.path,
            &AudioOptions {
                start: Some(request.source_start),
                duration: Some(request.window),
                filters: Vec::new(),
                rate: SAMPLE_RATE as u32,
                channels: 1,
                format: SampleFormat::F32,
                stream: request.audio_stream.map(|index| index as usize),
            },
        )
        .map_err(|error| error.to_string())?;
        let samples = decoder.collect_f32().map_err(|error| error.to_string())?;
        if samples.is_empty() {
            return Err("nothing to transcribe: the clip has no audio".to_owned());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("transcription cancelled".to_owned());
        }

        let mut loaded = self
            .model
            .lock()
            .map_err(|_| "transcriber state poisoned".to_owned())?;
        if loaded.as_ref().map(|model| model.id.as_str()) != Some(request.model_id.as_str()) {
            // The bar says a model is being read before the seconds-long wait.
            progress(JobStage::Loading);
        }
        // Clone the handle out instead of borrowing the guard, so there is no
        // `expect`; the guard itself stays held to keep deletes out.
        let model = ensure_loaded(&mut loaded, &request.model_id, &model_path)?;

        let capabilities = model
            .capabilities()
            .map_err(|error| format!("could not read the model's capabilities: {error}"))?;
        // An English-only model is always heard in English, whether the table
        // says so (the `.en` builds) or its own list does.
        let english_only = is_english_only(&request.model_id, &capabilities.languages);
        let language = select_language(
            request.language.as_deref(),
            english_only,
            &capabilities.languages,
        )?;
        let options = RunOptions {
            // Ask for the finest timing the model can give, so words come
            // back where it times them and segments always do.
            timestamps: capabilities.max_timestamp_kind,
            language,
            ..RunOptions::default()
        };

        let mut session = model
            .session()
            .map_err(|error| format!("could not start the engine: {error}"))?;
        let token = CancelToken::new();
        session.set_cancel_token(&token);
        // The engine polls this token between decode steps; the watcher turns
        // the job's plain flag into that token and stops with the run.
        let _watch = CancelWatch::spawn(Arc::clone(&cancel), token);

        let chunks = audio_chunks(samples.len(), capabilities.max_audio_ms);
        let total = chunks.len();
        let mut results: Vec<(f64, transcribe_cpp::Transcript)> = Vec::with_capacity(total);
        for (index, range) in chunks.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err("transcription cancelled".to_owned());
            }
            progress(JobStage::Working {
                done: index as f32 / total as f32,
                next: (index + 1) as f32 / total as f32,
            });
            let transcript = session
                .run(&samples[range.clone()], &options)
                .map_err(|error| {
                    if cancel.load(Ordering::Relaxed)
                        || matches!(error, transcribe_cpp::Error::Aborted { .. })
                    {
                        "transcription cancelled".to_owned()
                    } else {
                        format!("transcription failed: {error}")
                    }
                })?;
            results.push((range.start as f64 / SAMPLE_RATE as f64, transcript));
        }
        progress(JobStage::Working {
            done: 1.0,
            next: 1.0,
        });

        if cancel.load(Ordering::Relaxed) {
            return Err("transcription cancelled".to_owned());
        }

        Ok(collect_segments(&results))
    }

    /// Asks the running transcription to stop at its next check.
    pub fn cancel(&self) {
        self.gate.cancel();
    }

    /// Whether a transcription is running.
    pub fn is_busy(&self) -> bool {
        self.gate.is_busy()
    }
}

/// The engine's rate: 16 kHz mono, the one every model reads.
const SAMPLE_RATE: usize = 16_000;
/// Samples in a millisecond at [`SAMPLE_RATE`], for the chunk arithmetic.
const SAMPLES_PER_MS: usize = SAMPLE_RATE / 1000;
/// Slack under a model's audio cap for the join between pieces, in ms.
const JOINT_MS: usize = 500;

/// Sends the engine's native diagnostics into the app's `log` once.
fn install_logging() {
    static ONCE: Once = Once::new();
    ONCE.call_once(transcribe_cpp::init_logging);
}

/// The model `id`, loaded into the cache if it is not already there. A failed
/// load leaves the previous model in place, since the caller may still need it.
fn ensure_loaded(loaded: &mut Option<LoadedModel>, id: &str, path: &Path) -> Result<Model, String> {
    if loaded.as_ref().map(|model| model.id.as_str()) != Some(id) {
        install_logging();
        let model =
            Model::load(path).map_err(|error| format!("could not load the model: {error}"))?;
        *loaded = Some(LoadedModel {
            id: id.to_owned(),
            model,
        });
    }
    loaded
        .as_ref()
        .map(|loaded| loaded.model.clone())
        .ok_or_else(|| "the transcriber has no model".to_owned())
}

/// Whether every language the model offers is English. The `.en` whisper
/// builds say so in the table even when the model itself lists nothing.
fn is_english_only(id: &str, languages: &[String]) -> bool {
    catalog::known(id).is_some_and(|model| model.english_only)
        || (!languages.is_empty() && languages.iter().all(|code| code.starts_with("en")))
}

/// The code to hand the engine: the model's own spelling of the language it
/// was asked for, or `None` to let it listen and decide. A model that does
/// not know the language is a clear error, not a silent fallback.
fn select_language(
    requested: Option<&str>,
    english_only: bool,
    languages: &[String],
) -> Result<Option<String>, String> {
    // An English-only model always runs in English, whatever was asked for.
    let wanted = if english_only { Some("en") } else { requested };
    let Some(code) = wanted else {
        return Ok(None);
    };
    if languages.iter().any(|known| known == code) {
        return Ok(Some(code.to_owned()));
    }
    let prefix = format!("{code}-");
    if let Some(regional) = languages.iter().find(|known| known.starts_with(&prefix)) {
        return Ok(Some(regional.clone()));
    }
    // An English-only model that lists nothing still only hears English:
    // autodetection is the safe answer, not an error.
    if english_only && languages.is_empty() {
        return Ok(None);
    }
    Err(format!(
        "the model does not support the language {code:?} (it knows {})",
        if languages.is_empty() {
            "no languages".to_owned()
        } else {
            languages.join(", ")
        }
    ))
}

/// Sample ranges to transcribe: the whole buffer when the model takes it in
/// one go, or pieces a little under its cap when it does not - the slack
/// keeps a run from landing right on the limit.
fn audio_chunks(total: usize, max_audio_ms: i64) -> Vec<Range<usize>> {
    if max_audio_ms <= 0 || total == 0 {
        return std::iter::once(0..total).collect();
    }
    let limit = (max_audio_ms as u64).saturating_mul(SAMPLES_PER_MS as u64);
    let margin = (JOINT_MS * SAMPLES_PER_MS) as u64;
    let step = limit.saturating_sub(margin).max(1) as usize;
    (0..total)
        .step_by(step)
        .map(|start| start..(start + step).min(total))
        .collect()
}

/// Caption segments from every piece, timestamps moved onto the window, and
/// each piece's words placed in the segment their `seg_index` names.
fn collect_segments(chunks: &[(f64, transcribe_cpp::Transcript)]) -> Vec<Segment> {
    let mut captions = Vec::new();
    for (offset, transcript) in chunks {
        for (index, segment) in transcript.segments.iter().enumerate() {
            let start = segment.t0_ms as f64 / 1000.0 + offset;
            let end = segment.t1_ms as f64 / 1000.0 + offset;
            let text = segment.text.trim().to_owned();
            let words = transcript
                .words
                .iter()
                .filter(|word| word.seg_index == index as i32)
                .map(|word| Word {
                    start: word.t0_ms as f64 / 1000.0 + offset,
                    end: word.t1_ms as f64 / 1000.0 + offset,
                    text: word.text.trim().to_owned(),
                })
                .collect();
            if let Some(caption) = keep_segment(start, end, text, words) {
                captions.push(caption);
            }
        }
    }
    captions
}

/// Watches a run's cancel flag and trips the engine's token when it is set.
/// Dropping it stops the watcher, so it outlives exactly one run.
struct CancelWatch {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CancelWatch {
    fn spawn(cancel: Arc<AtomicBool>, token: CancelToken) -> CancelWatch {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if cancel.load(Ordering::Relaxed) {
                        token.cancel();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            })
        };
        CancelWatch {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for CancelWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The engine marks silence and music with bracketed stage directions
/// ("[BLANK_AUDIO]", "(upbeat music)"). Those are not captions, and neither
/// is an empty or degenerate span.
fn keep_segment(start: f64, end: f64, text: String, words: Vec<Word>) -> Option<Segment> {
    if text.is_empty() || (text.starts_with('[') && text.ends_with(']')) {
        return None;
    }
    (end > start).then_some(Segment {
        start,
        end,
        text,
        words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|code| (*code).to_owned()).collect()
    }

    #[test]
    fn stage_directions_and_degenerate_spans_are_not_captions() {
        let kept =
            keep_segment(0.0, 2.5, "Hello there.".to_owned(), Vec::new()).expect("a caption");
        assert_eq!(kept.text, "Hello there.");
        assert!(keep_segment(2.5, 2.5, "degenerate".to_owned(), Vec::new()).is_none());
        assert!(keep_segment(3.0, 4.0, "[BLANK_AUDIO]".to_owned(), Vec::new()).is_none());
        assert!(keep_segment(3.0, 4.0, String::new(), Vec::new()).is_none());
    }

    #[test]
    fn a_short_code_finds_the_models_regional_spelling() {
        // Exact wins, then the first regional variant.
        assert_eq!(
            select_language(Some("ru"), false, &words(&["ru", "en"])).unwrap(),
            Some("ru".to_owned())
        );
        assert_eq!(
            select_language(Some("ru"), false, &words(&["ru-RU", "en-US"])).unwrap(),
            Some("ru-RU".to_owned())
        );
        assert_eq!(
            select_language(Some("ru"), false, &words(&["en-US", "ru-RU"])).unwrap(),
            Some("ru-RU".to_owned())
        );
        // Auto-detection stays auto-detection.
        assert_eq!(
            select_language(None, false, &words(&["ru-RU"])).unwrap(),
            None
        );
    }

    #[test]
    fn an_english_only_model_always_hears_english() {
        assert_eq!(
            select_language(None, true, &words(&["en-US"])).unwrap(),
            Some("en-US".to_owned())
        );
        assert_eq!(
            select_language(Some("ru"), true, &words(&["en-US"])).unwrap(),
            Some("en-US".to_owned())
        );
        // A list it never filled still means English: no hint is the safe ask.
        assert_eq!(select_language(None, true, &[]).unwrap(), None);
    }

    #[test]
    fn a_language_the_model_does_not_know_is_an_error() {
        let error = select_language(Some("ru"), false, &words(&["en-US", "de-DE"])).unwrap_err();
        assert!(error.contains("ru"), "{error}");
        let empty = select_language(Some("ru"), false, &[]).unwrap_err();
        assert!(empty.contains("no languages"), "{empty}");
    }

    #[test]
    fn the_english_only_flag_reads_the_table_and_the_model() {
        assert!(is_english_only("tiny.en", &[]));
        assert!(is_english_only("", &words(&["en-US", "en-GB"])));
        assert!(!is_english_only("base", &words(&["en", "ru"])));
        assert!(!is_english_only("", &[]));
    }

    #[test]
    fn a_model_without_a_cap_takes_the_whole_buffer() {
        assert_eq!(audio_chunks(48_000, 0), vec![0..48_000]);
        assert_eq!(audio_chunks(48_000, -1), vec![0..48_000]);
    }

    #[test]
    fn a_capped_model_gets_pieces_under_the_cap() {
        // Ten seconds at 16 kHz, a four-second cap: 4 s - 0.5 s of slack per
        // piece, so three pieces of 3.5 s and a remainder.
        let chunks = audio_chunks(160_000, 4_000);
        assert_eq!(chunks, vec![0..56_000, 56_000..112_000, 112_000..160_000]);
        assert!(
            chunks
                .iter()
                .all(|range| range.len() <= 4_000 * SAMPLES_PER_MS)
        );
    }

    #[test]
    fn words_land_in_their_segment_with_shifted_times() {
        // One piece running 1.5 s into the window: a segment at 0.0-1.0 s of
        // the piece lands at 1.5-2.5 s of the window, its words carried along.
        let transcript = transcribe_cpp::Transcript {
            segments: vec![
                transcribe_cpp::Segment {
                    t0_ms: 0,
                    t1_ms: 1_000,
                    text: " Hello there.".to_owned(),
                    ..Default::default()
                },
                transcribe_cpp::Segment {
                    t0_ms: 1_000,
                    t1_ms: 2_000,
                    text: " Second line.".to_owned(),
                    ..Default::default()
                },
            ],
            words: vec![
                transcribe_cpp::Word {
                    t0_ms: 0,
                    t1_ms: 500,
                    seg_index: 0,
                    text: " Hello".to_owned(),
                    ..Default::default()
                },
                transcribe_cpp::Word {
                    t0_ms: 500,
                    t1_ms: 1_000,
                    seg_index: 0,
                    text: " there.".to_owned(),
                    ..Default::default()
                },
                transcribe_cpp::Word {
                    t0_ms: 1_000,
                    t1_ms: 1_500,
                    seg_index: 1,
                    text: " Second".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let captions = collect_segments(&[(1.5, transcript)]);
        assert_eq!(captions.len(), 2);
        assert_eq!(captions[0].text, "Hello there.");
        assert_eq!(captions[0].start, 1.5);
        assert_eq!(captions[0].words.len(), 2);
        assert_eq!(captions[0].words[0].text, "Hello");
        assert_eq!(captions[0].words[1].start, 2.0);
        assert_eq!(captions[1].words.len(), 1);
        assert_eq!(captions[1].words[0].text, "Second");
        assert_eq!(captions[1].start, 2.5);
    }

    #[test]
    fn status_lists_every_catalog_model_undownloaded_in_an_empty_data_dir() {
        let scratch =
            std::env::temp_dir().join(format!("concat-whisper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let status = Transcriber::status(&AppDirs::under(&scratch)).expect("status");
        assert_eq!(status.models.len(), catalog::CATALOG.len());
        assert!(status.models.iter().all(|model| !model.downloaded));
        assert!(status.models.iter().all(|model| !model.found));
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_named_file_is_used_where_it_lies() {
        // A catalogue id resolves under the models folder; an absolute path is
        // the file itself (a found or hand-picked model).
        let scratch =
            std::env::temp_dir().join(format!("concat-whisper-path-{}", std::process::id()));
        let dirs = AppDirs::under(&scratch);
        assert_eq!(
            run_model_file(&dirs, "base.en").expect("a path"),
            models_dir(&dirs).expect("a dir").join("ggml-base.en.bin")
        );
        let picked = scratch.join("my-model.gguf");
        assert_eq!(
            run_model_file(&dirs, &picked.to_string_lossy()).expect("a path"),
            picked
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }
}

#[cfg(test)]
mod real_audio {
    use super::*;

    /// The whole path through the engine on a real file with a real model -
    /// the thing the unit tests cannot cover. Ignored: it wants a file named
    /// in CONCAT_TRANSCRIBE_AUDIO, and optionally a model path in
    /// CONCAT_TRANSCRIBE_MODEL.
    ///
    ///   CONCAT_TRANSCRIBE_AUDIO=speech.wav CONCAT_TRANSCRIBE_MODEL=model.gguf \
    ///     cargo test -p concat-speech -- --ignored real_audio
    #[test]
    #[ignore = "needs CONCAT_TRANSCRIBE_AUDIO and a model"]
    fn transcribes_a_file_with_the_installed_model() {
        let Ok(audio) = std::env::var("CONCAT_TRANSCRIBE_AUDIO") else {
            eprintln!("CONCAT_TRANSCRIBE_AUDIO not set; nothing to transcribe");
            return;
        };
        let dirs = AppDirs::locate().expect("the app's directories");
        // A model named outright is not in the table, so it is left to
        // detect the language; otherwise it is the installed tiny.en.
        let model_id = if std::env::var_os("CONCAT_TRANSCRIBE_MODEL").is_some() {
            String::new()
        } else {
            "tiny.en".to_owned()
        };
        let transcriber = Transcriber::new();
        let request = TranscribeRequest {
            path: audio,
            audio_stream: None,
            source_start: 0.0,
            window: 30.0,
            model_id,
            language: None,
        };
        let segments = transcriber
            .transcribe(&dirs, &request, |stage| eprintln!("{stage:?}"))
            .expect("transcription");
        for segment in &segments {
            eprintln!("{:.2}-{:.2} {}", segment.start, segment.end, segment.text);
        }
        assert!(!segments.is_empty(), "no words heard");
    }
}
