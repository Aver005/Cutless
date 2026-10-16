// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Группы связанных клипов: видео и его дорожки звука двигаются, режутся и удаляются вместе.
//! Метка группы лежит в `clip.extra["cutless"]["link"]`; это id клипа, с которого группа началась.

use std::collections::HashMap;

use serde_json::Value;

use crate::cutless;

use super::*;

/// Ключ метки группы среди полей Cutless.
const LINK: &str = "link";

/// Метка группы клипа, если он в группе.
pub fn link_of(clip: &Clip) -> Option<&str> {
    cutless::get(clip, LINK).and_then(Value::as_str)
}

/// Ставит или снимает метку группы. Возвращает true, если что-то изменилось.
pub(super) fn set_link(clip: &mut Clip, link: Option<&str>) -> bool {
    match link {
        Some(link) => cutless::set(clip, LINK, Value::String(link.to_owned())),
        None => cutless::remove(clip, LINK),
    }
}

/// Клипы `ids` и все, кто с ними в одной группе; порядок — как в `ids`, затем остальные по таймлайну.
pub fn with_linked(timeline: &Timeline, ids: &[String]) -> Vec<String> {
    let links: Vec<&str> = ids
        .iter()
        .filter_map(|id| timeline.clip(id).and_then(link_of))
        .collect();
    let mut out: Vec<String> = ids.to_vec();
    for clip in &timeline.clips {
        if link_of(clip).is_some_and(|link| links.contains(&link)) && !out.contains(&clip.id) {
            out.push(clip.id.clone());
        }
    }
    out
}

/// Новые группы для хвостов, отрезанных в одной команде: куски одной группы,
/// отрезанные в один момент, остаются группой, отдельной от голов.
#[derive(Default)]
pub(super) struct Relinks(HashMap<(String, i64), String>);

impl Relinks {
    /// Перевешивает хвост `tail`, отрезанный в момент `at`, в группу своего разреза.
    pub(super) fn tail(&mut self, tail: &mut Clip, at: f64) {
        let Some(old) = link_of(tail).map(str::to_owned) else {
            return;
        };
        // Микросекунды: один разрез у разных клипов группы даёт один ключ.
        let key = (old, (at * 1_000_000.0).round() as i64);
        let link = self.0.entry(key).or_insert_with(|| tail.id.clone()).clone();
        set_link(tail, Some(&link));
    }
}

/// Применяет одну из команд этого модуля. Остальные сюда не маршрутизируются.
pub(super) fn apply(
    project: &mut Project,
    _mint: &mut IdMint,
    command: Command,
) -> Result<Outcome, CommandError> {
    match command {
        Command::Unlink { clip_ids } => {
            let timeline = project.active_mut();
            let members = with_linked(timeline, &clip_ids);
            let mut applied = false;
            for id in &members {
                if let Some(clip) = timeline.clip_mut(id) {
                    applied |= set_link(clip, None);
                }
            }
            Ok(Outcome {
                created_id: None,
                applied,
            })
        }
        _ => unreachable!("commands::apply routes only this module's commands here"),
    }
}
