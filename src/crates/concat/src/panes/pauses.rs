// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Панель пауз: окно настроек разметки и анализ звука на рабочем потоке.
//!
//! Анализ идёт по окну клипа, отрезки из [`cutless_pauses`] переводятся в
//! секунды таймлайна и размечаются одной командой [`Command::Batch`] — один
//! шаг undo. Помеченные куски таймлайн рисует сам, полоса над ним живёт на
//! глобальном [`crate::ui::PausesState`].

use concat_project::Command;
use concat_project::commands::pauses::PauseRange;
use concat_project::model::{Clip, ClipKind};
use cutless_pauses::{FileRequest, PauseSettings, analyze_file};

use crate::host::spawn_in_project;
use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::PausesSheetData;

/// Всё, что может случиться с окном пауз.
#[derive(Clone, Debug)]
pub enum PausesMsg {
    /// Кнопка на полке: окно с параметрами разметки.
    Open,
    Close,
    /// Порог считается из звука (`true`) или задан вручную.
    ThresholdAutoToggled(bool),
    /// Ручной порог тишины, дБ.
    ThresholdChanged(f64),
    /// Минимальная длина паузы, с.
    MinPauseChanged(f64),
    /// Отступ по краям паузы, с.
    PaddingChanged(f64),
    /// «Разметить»: анализ и разметка выделенного (или всего со звуком).
    Begin,
    /// Рабочий поток вернул разметку по клипам или причину отказа.
    Finished(Result<Vec<ClipPauses>, String>),
}

/// Готовые к разметке отрезки одного клипа, в секундах таймлайна.
#[derive(Clone, Debug)]
pub struct ClipPauses {
    /// Клип, который размечаем.
    pub clip_id: String,
    /// Отрезки пауз в секундах таймлайна.
    pub ranges: Vec<PauseRange>,
}

/// Состояние окна пауз.
pub struct PausesPane {
    pub open: bool,
    /// Порог вычисляется из звука.
    pub auto: bool,
    /// Ручной порог тишины, дБ.
    pub threshold_db: f32,
    /// Минимальная длина паузы, с.
    pub min_pause: f64,
    /// Отступ по краям паузы, с.
    pub padding: f64,
    pub running: bool,
    /// Почему последний запуск не удался.
    pub message: String,
    /// Режим плеера: помеченные отрезки перепрыгиваются.
    pub skip: bool,
    /// Сколько клипов с кривой скорости пропущено в последнем запуске.
    skipped: usize,
}

impl Default for PausesPane {
    fn default() -> Self {
        Self {
            open: false,
            auto: true,
            threshold_db: -40.0,
            min_pause: 0.3,
            padding: 1.0 / 3.0,
            running: false,
            message: String::new(),
            skip: false,
            skipped: 0,
        }
    }
}

impl PausesPane {
    /// Применяет одно сообщение. Студия — остальное окно.
    pub fn update(&mut self, msg: PausesMsg, studio: &mut Studio) {
        match msg {
            PausesMsg::Open => {
                let skip = self.skip;
                *self = PausesPane {
                    open: true,
                    skip,
                    ..PausesPane::default()
                };
            }
            PausesMsg::Close => {
                self.open = false;
                self.running = false;
            }
            PausesMsg::ThresholdAutoToggled(on) => {
                self.auto = on;
                self.message.clear();
            }
            PausesMsg::ThresholdChanged(db) => self.threshold_db = db as f32,
            PausesMsg::MinPauseChanged(seconds) => self.min_pause = seconds.max(0.0),
            PausesMsg::PaddingChanged(seconds) => self.padding = seconds.max(0.0),
            PausesMsg::Begin => self.begin(studio),
            PausesMsg::Finished(result) => self.finished(studio, result),
        }
    }

    /// Собирает настройки, находит цели и запускает анализ на потоке.
    fn begin(&mut self, studio: &mut Studio) {
        if self.running {
            return;
        }
        let targets = targets(studio);
        if targets.list.is_empty() {
            self.message = t("pauses.noTargets");
            return;
        }
        let settings = PauseSettings {
            threshold_db: (!self.auto).then_some(self.threshold_db),
            min_pause: self.min_pause,
            padding: self.padding,
            window_ms: 20,
        };
        self.skipped = targets.skipped;
        self.running = true;
        self.message.clear();
        spawn_in_project(
            move || analyse(&targets.list, &settings),
            |studio, _, _, result| studio.handle(Msg::Pauses(PausesMsg::Finished(result))),
        );
    }

    /// Разметка пришла: всё одним [`Command::Batch`] либо причина отказа.
    fn finished(&mut self, studio: &mut Studio, result: Result<Vec<ClipPauses>, String>) {
        self.running = false;
        let groups = match result {
            Ok(groups) => groups,
            Err(error) => {
                self.message = tf("pauses.analysisFailed", &[&error]);
                return;
            }
        };
        let marked: usize = groups.iter().map(|group| group.ranges.len()).sum();
        let commands: Vec<Command> = groups
            .into_iter()
            .map(|group| Command::MarkPauses {
                clip_id: group.clip_id,
                ranges: group.ranges,
            })
            .collect();
        self.open = false;
        if commands.is_empty() {
            let line = if self.skipped > 0 {
                tf("pauses.allSkipped", &[&self.skipped])
            } else {
                t("pauses.nothingFound")
            };
            studio.notify(&line, true);
        } else {
            studio.apply(Command::Batch { commands });
            let line = if self.skipped > 0 {
                tf("pauses.markedSomeSkipped", &[&marked, &self.skipped])
            } else {
                tf("pauses.marked", &[&marked])
            };
            studio.notify(&line, false);
        }
    }

    /// Окно так, как его показывает Slint.
    pub fn data(&self, studio: &Studio) -> PausesSheetData {
        let available = if self.open {
            targets(studio).list.len() as i32
        } else {
            0
        };
        PausesSheetData {
            open: self.open,
            threshold_auto: self.auto,
            threshold_db: self.threshold_db,
            min_pause: self.min_pause as f32,
            padding: self.padding as f32,
            running: self.running,
            message: self.message.as_str().into(),
            available,
        }
    }
}

/// Один клип, звук которого можно разметить.
#[derive(Clone, Debug)]
struct Target {
    clip_id: String,
    path: String,
    audio_stream: Option<usize>,
    source_start: f64,
    window: f64,
    filters: Vec<String>,
    start: f64,
    speed: f64,
}

/// Найденные цели и сколько клипов пропущено из-за кривой скорости.
struct Targets {
    list: Vec<Target>,
    skipped: usize,
}

/// Клипы, чей звук размечаем: выделенные, иначе все со звуком кроме музыки.
fn targets(studio: &Studio) -> Targets {
    let timeline = studio.timeline();
    let chosen: Vec<&Clip> = if studio.selection.is_empty() {
        timeline
            .clips
            .iter()
            .map(|clip| &**clip)
            .filter(|clip| include_all(studio, clip))
            .collect()
    } else {
        studio
            .selection
            .iter()
            .filter_map(|id| timeline.clip(id))
            .filter(|clip| !is_music(clip) && studio.clip_has_sound(clip))
            .collect()
    };
    let mut list = Vec::new();
    let mut skipped = 0;
    for clip in chosen {
        if clip.speed_curve.is_some() {
            skipped += 1;
            continue;
        }
        if let Some(target) = target_of(studio, clip) {
            list.push(target);
        }
    }
    Targets { list, skipped }
}

/// Клип в общем проходе: не музыка и не отсоединённый звук, со звуком.
fn include_all(studio: &Studio, clip: &Clip) -> bool {
    is_sound_candidate(clip) && studio.clip_has_sound(clip)
}

/// Не музыка и не отсоединённый звук.
fn is_sound_candidate(clip: &Clip) -> bool {
    clip.detached_from.is_none() && clip.kind != ClipKind::Audio
}

/// Фоновый трек: звук без `detached_from`.
fn is_music(clip: &Clip) -> bool {
    clip.kind == ClipKind::Audio && clip.detached_from.is_none()
}

/// Описание одного клипа для анализа; ничего, если клип не разметить.
fn target_of(studio: &Studio, clip: &Clip) -> Option<Target> {
    let speed = clip.speed;
    if !speed.is_finite() || speed <= 0.0 {
        return None;
    }
    let media = studio.project().media_by_id(&clip.media_id)?;
    let chain = concat_export::chains::audio_filter_chain(&clip.filters);
    let filters = if chain.is_empty() {
        Vec::new()
    } else {
        vec![chain]
    };
    Some(Target {
        clip_id: clip.id.clone(),
        path: media.path.clone(),
        audio_stream: clip.audio_stream.map(|index| index as usize),
        source_start: clip.source_start,
        window: clip.duration * speed,
        filters,
        start: clip.start,
        speed,
    })
}

/// Читает окно каждого клипа и переводит отрезки в секунды таймлайна.
fn analyse(targets: &[Target], settings: &PauseSettings) -> Result<Vec<ClipPauses>, String> {
    let mut out = Vec::new();
    for target in targets {
        let request = FileRequest {
            path: target.path.clone(),
            audio_stream: target.audio_stream,
            source_start: target.source_start,
            window: target.window,
            filters: target.filters.clone(),
        };
        let analysis = analyze_file(&request, settings).map_err(|error| error.to_string())?;
        let ranges: Vec<PauseRange> = analysis
            .pauses
            .iter()
            .map(|range| to_timeline(target.start, target.speed, *range))
            .collect();
        if !ranges.is_empty() {
            out.push(ClipPauses {
                clip_id: target.clip_id.clone(),
                ranges,
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_time_lands_on_the_timeline_by_speed() {
        let at = to_timeline(
            10.0,
            2.0,
            cutless_pauses::Range {
                start: 1.0,
                end: 3.0,
            },
        );
        assert!((at.start - 10.5).abs() < 1e-9);
        assert!((at.end - 11.5).abs() < 1e-9);
    }

    #[test]
    fn music_is_sound_without_a_source_and_a_detached_one_is_not() {
        let music = Clip::blank("c1", "t", ClipKind::Audio, "Music", 0.0, 5.0);
        assert!(is_music(&music));
        let mut detached = music.clone();
        detached.detached_from = Some("c9".to_owned());
        assert!(!is_music(&detached));
    }

    #[test]
    fn only_attached_pictures_are_sound_candidates() {
        let video = Clip::blank("c2", "t", ClipKind::Video, "Clip", 0.0, 5.0);
        assert!(is_sound_candidate(&video));
        let mut sound = video.clone();
        sound.kind = ClipKind::Audio;
        assert!(!is_sound_candidate(&sound));
    }
}
