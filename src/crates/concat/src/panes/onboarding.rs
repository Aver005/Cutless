// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Знакомство при первом запуске: три экрана о цели и снаряжении.
//!
//! Окно живёт поверх стартового экрана, пока в настройках `onboarding_done`
//! ложно. Первый экран показывает части окна, второй выбирает цель (она задаёт
//! формат новых проектов), третий предлагает модель распознавания. Кнопка в
//! настройках сбрасывает флаг и знакомство откроется заново со следующего
//! запуска.

use std::sync::Arc;

use concat_speech::transcribe::TranscriberStatus;

use crate::host::{on_ui, spawn};
use crate::i18n::{self, t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::OnboardingData;

/// Идентификатор цели «вертикальные ролики»: остальные цели — широкий кадр.
pub const GOAL_VERTICAL: &str = "vertical";

/// Последний экран знакомства: 0 приветствие, 1 цель, 2 снаряжение.
const LAST_STEP: i32 = 2;

/// Идентификатор рекомендованной модели: для русского интерфейса Parakeet v3,
/// иначе многоязычный Whisper base.
pub fn recommended_id(interface_language: &str) -> &'static str {
    if interface_language == "ru" {
        "parakeet-tdt-0.6b-v3"
    } else {
        "base"
    }
}

/// Индекс формата нового проекта по цели: вертикальные ролики — 9:16 (индекс 1
/// в `ASPECTS`), всё остальное — 16:9 (индекс 0).
pub fn aspect_for_goal(goal: Option<&str>) -> usize {
    if goal == Some(GOAL_VERTICAL) { 1 } else { 0 }
}

/// Имя подходящей модели, уже лежащей на диске: рекомендованная, иначе любая
/// скачанная из каталога, иначе найденная в кэше Hugging Face.
fn aboard_model(status: &TranscriberStatus, recommended: &str) -> Option<String> {
    if let Some(model) = status
        .models
        .iter()
        .find(|model| model.id == recommended && model.downloaded)
    {
        return Some(model.label.clone());
    }
    if let Some(model) = status.models.iter().find(|model| model.downloaded) {
        return Some(model.label.clone());
    }
    status.found.first().map(|model| model.label.clone())
}

/// Размер модели для строки: мегабайты без дробной части.
fn size_text(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}

/// Рекомендованная к скачиванию модель, как её показывает третий экран.
#[derive(Clone, Debug, PartialEq)]
struct Offer {
    id: String,
    name: String,
    size_bytes: u64,
}

/// Всё, что может случиться со знакомством.
#[derive(Clone, Debug)]
pub enum OnboardingMsg {
    /// Показать знакомство: первый экран и то, что уже есть на диске.
    Show,
    /// Экран дальше.
    Next,
    /// Экран назад.
    Back,
    /// Цель выбрана: сохраняем её и идём к снаряжению.
    GoalChosen(String),
    /// Скачать рекомендованную модель.
    Download,
    /// Отменить идущее скачивание.
    Cancel,
    /// Рабочий сообщает, сколько скачано.
    Progress {
        /// Мегабайты приняты.
        fetched: f32,
        /// Мегабайты всего, когда сервер сказал.
        total: Option<f32>,
        unpacking: bool,
    },
    /// Скачивание кончилось: модель на диске или почему нет.
    Finished {
        /// Счётчик запуска: результат устаревшего скачивания отбрасывается.
        generation: u64,
        id: String,
        result: Result<(), String>,
    },
    /// Закрыть знакомство и запомнить, что оно пройдено.
    Done,
}

/// Состояние окна знакомства.
#[derive(Default)]
pub struct OnboardingPane {
    pub open: bool,
    pub step: i32,
    goal: String,
    /// Модель, которую предлагаем скачать; `None`, когда ничего не нужно.
    offer: Option<Offer>,
    /// Имя уже имеющейся модели, если она нашлась; иначе пусто.
    aboard: String,
    downloading: bool,
    cancelled: bool,
    /// Номер запуска скачивания: старый результат по нему отбрасывается.
    generation: u64,
    progress: f32,
    transferred: String,
    error: String,
}

impl OnboardingPane {
    /// Применяет одно сообщение. Studio — остальное окно.
    pub fn update(&mut self, msg: OnboardingMsg, studio: &mut Studio) {
        match msg {
            OnboardingMsg::Show => self.show(studio),
            OnboardingMsg::Next => self.step = (self.step + 1).min(LAST_STEP),
            OnboardingMsg::Back => self.step = (self.step - 1).max(0),
            OnboardingMsg::GoalChosen(goal) => self.choose_goal(&goal, studio),
            OnboardingMsg::Download => self.download(studio),
            OnboardingMsg::Cancel => {
                if self.downloading {
                    self.cancelled = true;
                    studio.host.transcriber.cancel_download();
                }
            }
            OnboardingMsg::Progress {
                fetched,
                total,
                unpacking,
            } => {
                self.progress = total
                    .filter(|total| *total > 0.0)
                    .map_or(0.0, |total| (fetched / total).min(1.0));
                let total = total.unwrap_or(fetched);
                self.transferred = if unpacking {
                    t("settings.unpacking")
                } else {
                    tf(
                        "onboarding.mbOfMb",
                        &[&format!("{fetched:.0}"), &format!("{total:.0}")],
                    )
                };
            }
            OnboardingMsg::Finished {
                generation,
                id,
                result,
            } => self.finished(generation, id, result, studio),
            OnboardingMsg::Done => self.finish(studio),
        }
    }

    /// Открывает знакомство на первом экране и смотрит, что уже на диске.
    fn show(&mut self, studio: &Studio) {
        self.open = true;
        self.step = 0;
        self.goal = studio.prefs.goal.clone().unwrap_or_default();
        self.downloading = false;
        self.cancelled = false;
        self.progress = 0.0;
        self.transferred.clear();
        self.error.clear();
        self.refresh_offer(studio);
    }

    /// Решает, что предложить на третьем экране: уже имеющуюся модель или одну
    /// к скачиванию.
    fn refresh_offer(&mut self, studio: &Studio) {
        self.offer = None;
        self.aboard.clear();
        let id = recommended_id(&i18n::current());
        let Ok(status) = concat_speech::Transcriber::status(&studio.host.dirs) else {
            return;
        };
        if let Some(name) = aboard_model(&status, id) {
            self.aboard = name;
            return;
        }
        if let Some(model) = status.models.iter().find(|model| model.id == id) {
            self.offer = Some(Offer {
                id: model.id.clone(),
                name: model.label.clone(),
                size_bytes: model.size_bytes,
            });
        }
    }

    /// Запоминает выбранную цель, задаёт формат новых проектов и уходит к
    /// третьему экрану.
    fn choose_goal(&mut self, goal: &str, studio: &mut Studio) {
        self.goal = goal.to_owned();
        studio.prefs.goal = Some(self.goal.clone());
        studio.prefs.save(&studio.host.dirs);
        studio.start.apply_goal(Some(self.goal.as_str()));
        self.step = LAST_STEP;
    }

    /// Запускает скачивание рекомендованной модели на рабочем потоке.
    fn download(&mut self, studio: &Studio) {
        if self.downloading {
            return;
        }
        let Some(offer) = self.offer.clone() else {
            return;
        };
        self.downloading = true;
        self.cancelled = false;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.progress = 0.0;
        self.transferred = tf(
            "onboarding.mbOfMb",
            &[&"0".to_owned(), &size_text(offer.size_bytes)],
        );
        self.error.clear();
        let id = offer.id.clone();
        let dirs = studio.host.dirs.clone();
        let transcriber = Arc::clone(&studio.host.transcriber);
        spawn(
            move || {
                let report = |progress: concat_speech::DownloadProgress| {
                    let msg = OnboardingMsg::Progress {
                        fetched: progress.received as f32 / 1_000_000.0,
                        total: (progress.total > 0).then(|| progress.total as f32 / 1_000_000.0),
                        unpacking: progress.unpacking,
                    };
                    on_ui(move |studio, _, _| studio.handle(Msg::Onboarding(msg)));
                };
                let result = transcriber.download_model(&dirs, &id, report);
                (generation, id, result)
            },
            |studio, _, _, (generation, id, result)| {
                studio.handle(Msg::Onboarding(OnboardingMsg::Finished {
                    generation,
                    id,
                    result,
                }));
            },
        );
    }

    /// Скачивание кончилось: модель на диске или причина в окне. Результат
    /// устаревшего запуска — после отмены и нового «Скачать» — отбрасывается.
    fn finished(
        &mut self,
        generation: u64,
        id: String,
        result: Result<(), String>,
        studio: &mut Studio,
    ) {
        if generation != self.generation {
            return;
        }
        self.downloading = false;
        if self.cancelled {
            self.cancelled = false;
            self.progress = 0.0;
            self.transferred.clear();
            return;
        }
        match result {
            Ok(()) => {
                self.aboard = self
                    .offer
                    .as_ref()
                    .filter(|offer| offer.id == id)
                    .map(|offer| offer.name.clone())
                    .unwrap_or_default();
                self.offer = None;
                self.progress = 0.0;
                self.transferred.clear();
                // Скачанная модель становится рабочей, если ничего не выбрано.
                if studio.prefs.transcriber_model.is_none() {
                    studio.prefs.transcriber_model = Some(id);
                    studio.prefs.save(&studio.host.dirs);
                }
                studio.notify(&t("settings.modelReady"), false);
            }
            Err(error) => {
                self.error = error;
                self.progress = 0.0;
            }
        }
    }

    /// Закрывает знакомство и запоминает, что оно пройдено. Убегающее
    /// скачивание отменяется: его результат никого уже не ждёт.
    fn finish(&mut self, studio: &mut Studio) {
        self.open = false;
        if self.downloading {
            self.cancelled = true;
            studio.host.transcriber.cancel_download();
        }
        studio.prefs.onboarding_done = true;
        studio.prefs.save(&studio.host.dirs);
    }

    /// Окно знакомства, как его показывает Slint.
    pub fn data(&self) -> OnboardingData {
        OnboardingData {
            open: self.open,
            step: self.step,
            goal: self.goal.as_str().into(),
            aboard: self.aboard.as_str().into(),
            model_name: self
                .offer
                .as_ref()
                .map(|offer| offer.name.as_str())
                .unwrap_or_default()
                .into(),
            model_size: self
                .offer
                .as_ref()
                .map(|offer| size_text(offer.size_bytes))
                .unwrap_or_default()
                .into(),
            downloading: self.downloading,
            progress: self.progress,
            transferred: self.transferred.as_str().into(),
            error: self.error.as_str().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_speech::transcribe::ModelStatus;

    fn model(id: &str, label: &str, downloaded: bool) -> ModelStatus {
        ModelStatus {
            id: id.to_owned(),
            label: label.to_owned(),
            blurb: String::new(),
            english_only: false,
            size_bytes: 100_000_000,
            downloaded,
            found: false,
            family: "whisper".to_owned(),
        }
    }

    fn status(models: Vec<ModelStatus>, found: Vec<ModelStatus>) -> TranscriberStatus {
        TranscriberStatus {
            models_dir: String::new(),
            models,
            found,
        }
    }

    /// Русский интерфейс рекомендует Parakeet v3, любой другой — Whisper base.
    #[test]
    fn the_recommendation_follows_the_interface_language() {
        assert_eq!(recommended_id("ru"), "parakeet-tdt-0.6b-v3");
        assert_eq!(recommended_id("en"), "base");
        assert_eq!(recommended_id("de"), "base");
    }

    /// Вертикальная цель — формат 9:16, всё остальное — 16:9.
    #[test]
    fn the_goal_picks_the_project_shape() {
        assert_eq!(aspect_for_goal(Some(GOAL_VERTICAL)), 1);
        assert_eq!(aspect_for_goal(Some("youtube")), 0);
        assert_eq!(aspect_for_goal(Some("edit")), 0);
        assert_eq!(aspect_for_goal(None), 0);
    }

    /// Рекомендованная скачанная модель считается «на борту» первой, затем
    /// любая скачанная, затем найденная в кэше.
    #[test]
    fn an_installed_model_rides_along() {
        let recommended = status(
            vec![model("parakeet-tdt-0.6b-v3", "Parakeet", true)],
            Vec::new(),
        );
        assert_eq!(
            aboard_model(&recommended, "parakeet-tdt-0.6b-v3").as_deref(),
            Some("Parakeet")
        );

        let other = status(vec![model("small", "Small", true)], Vec::new());
        assert_eq!(aboard_model(&other, "base").as_deref(), Some("Small"));

        let found = status(
            Vec::new(),
            vec![model("/x/ggml-base.bin", "ggml-base", true)],
        );
        assert_eq!(aboard_model(&found, "base").as_deref(), Some("ggml-base"));

        let none = status(vec![model("base", "Base", false)], Vec::new());
        assert_eq!(aboard_model(&none, "base"), None);
    }

    /// Размер печатается в мегабайтах без дробной части.
    #[test]
    fn a_size_reads_in_megabytes() {
        assert_eq!(size_text(147_951_465), "148 MB");
        assert_eq!(size_text(0), "0 MB");
    }
}
