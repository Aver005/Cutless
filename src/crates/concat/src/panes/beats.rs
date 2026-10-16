// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! «Склянки»: сетка долей выделенного клипа на рабочем потоке — темп, такты, удары, дропы.
//! Результат ложится на медиа одной командой `SetMediaBeats`; риски таймлайн рисует сам.

use std::sync::atomic::AtomicBool;

use concat_project::Command;
use concat_project::beats::{self, BeatGrid};
use concat_project::model::ClipKind;

use crate::host::spawn_in_project;
use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;

/// Всё, что может случиться со склянками.
#[derive(Clone, Debug)]
pub enum BeatsMsg {
    /// «Отбить склянки» для выделенного клипа.
    Begin,
    /// Рабочий поток вернул сетку для медиа и потока или причину отказа.
    Finished(Result<Heard, String>),
}

/// Сетка, услышанная в одном потоке одного файла.
#[derive(Clone, Debug)]
pub struct Heard {
    /// Медиа, на которое ляжет сетка.
    pub media_id: String,
    /// Звуковой поток.
    pub stream: Option<u32>,
    /// Сама сетка.
    pub grid: BeatGrid,
}

/// Состояние склянок.
#[derive(Default)]
pub struct BeatsPane {
    /// Идёт разбор.
    pub running: bool,
}

/// Что разбирать: медиа, путь и поток единственного выделенного клипа со звуком.
fn target(studio: &Studio) -> Option<(String, String, Option<u32>)> {
    let id = studio.sole_selection()?;
    let clip = studio.clip(&id)?;
    if !matches!(clip.kind, ClipKind::Audio | ClipKind::Video) {
        return None;
    }
    let media = studio.project().media_by_id(&clip.media_id)?;
    if clip.kind == ClipKind::Video && !media.has_audio {
        return None;
    }
    Some((media.id.clone(), media.path.clone(), clip.audio_stream))
}

impl BeatsPane {
    /// Обрабатывает сообщение.
    pub fn update(&mut self, msg: BeatsMsg, studio: &mut Studio) {
        match msg {
            BeatsMsg::Begin => self.begin(studio),
            BeatsMsg::Finished(result) => self.finished(studio, result),
        }
    }

    fn begin(&mut self, studio: &mut Studio) {
        if self.running {
            return;
        }
        let Some((media_id, path, stream)) = target(studio) else {
            studio.notify(&t("beats.noTarget"), true);
            return;
        };
        self.running = true;
        spawn_in_project(
            move || {
                let stop = AtomicBool::new(false);
                cutless_beats::analyze_file(&path, stream.map(|index| index as usize), &stop)
                    .map(|grid| Heard {
                        media_id,
                        stream,
                        grid,
                    })
                    .map_err(|error| error.to_string())
            },
            |studio, _, _, result| studio.handle(Msg::Beats(BeatsMsg::Finished(result))),
        );
    }

    fn finished(&mut self, studio: &mut Studio, result: Result<Heard, String>) {
        self.running = false;
        let heard = match result {
            Ok(heard) => heard,
            Err(error) => {
                studio.notify(&tf("beats.failed", &[&error]), true);
                return;
            }
        };
        // Пока шёл разбор, медиа могли убрать из корзины.
        if studio.project().media_by_id(&heard.media_id).is_none() {
            return;
        }
        let bpm = format!("{:.1}", heard.grid.bpm);
        let line = match heard.grid.alt_bpm {
            Some(alt) => tf("beats.doneDoubt", &[&bpm, &format!("{alt:.1}")]),
            None => tf("beats.done", &[&bpm]),
        };
        studio.apply(Command::SetMediaBeats {
            media_id: heard.media_id,
            stream: heard.stream,
            grid: Some(heard.grid),
        });
        studio.notify(&line, false);
    }

    /// Можно ли разобрать выделенное и что о нём уже известно.
    pub fn state(&self, studio: &Studio) -> (bool, String) {
        let Some((media_id, _, stream)) = target(studio) else {
            return (false, String::new());
        };
        let summary = studio
            .project()
            .media_by_id(&media_id)
            .and_then(|media| beats::head(media, stream))
            .map(|head| tf("beats.summary", &[&format!("{:.1}", head.bpm)]))
            .unwrap_or_default();
        (true, summary)
    }
}
