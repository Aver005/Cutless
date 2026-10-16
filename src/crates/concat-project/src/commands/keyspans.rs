// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Ключи отрезком: генератор, ставящий ключ на каждый кадр, заменяет ключи на отрезке
//! готовым списком. Поштучный `SetClipKey` склеил бы соседние кадры длинного клипа.

use serde::{Deserialize, Serialize};

use super::*;
use crate::model::{ClipKey, ParamKey};

/// Чьи ключи меняются: свойство клипа или параметр эффекта в его цепочке.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum KeyTarget {
    /// Свойство клипа: масштаб, сдвиг, поворот, прозрачность, громкость.
    Property {
        /// Какое свойство.
        property: KeyProperty,
    },
    /// Параметр звена цепочки `video_effects`.
    Effect {
        /// Индекс звена; вне цепочки — команда ничего не делает.
        entry: usize,
        /// Ключ параметра в манифесте.
        key: String,
    },
}

/// Один ключ отрезка: доля клипа, значение, плавность.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanKey {
    /// Где в клипе, `0..=1`.
    pub at: f64,
    /// Значение в единицах свойства или параметра.
    pub value: f64,
    /// Как к ключу подходит кривая от предыдущего.
    #[serde(default)]
    pub ease: KeyEase,
}

/// Выполняет `SetKeySpan`.
pub(super) fn apply(project: &mut Project, command: Command) -> Result<Outcome, CommandError> {
    let Command::SetKeySpan {
        clip_id,
        target,
        from,
        to,
        keys,
    } = command
    else {
        return Ok(Outcome::default());
    };
    if !from.is_finite() || !to.is_finite() {
        return Ok(Outcome::default());
    }
    let (from, to) = (from.min(to).clamp(0.0, 1.0), from.max(to).clamp(0.0, 1.0));
    let keys: Vec<SpanKey> = keys
        .into_iter()
        .filter(|key| key.at.is_finite() && key.value.is_finite())
        .map(|key| SpanKey {
            at: key.at.clamp(from, to),
            ..key
        })
        .collect();
    let Some(clip) = project.active_mut().clip_mut(&clip_id) else {
        return Ok(Outcome::default());
    };
    let before = clip.clone();
    let inside = |at: f64| at >= from && at <= to;
    match &target {
        KeyTarget::Property { property } => {
            clip.keys
                .retain(|held| held.property != *property || !inside(held.at));
            clip.keys.extend(keys.iter().map(|key| ClipKey {
                property: *property,
                at: key.at,
                value: key.value,
                ease: key.ease,
            }));
        }
        KeyTarget::Effect { entry, key } => {
            let Some(link) = clip.video_effects.get_mut(*entry) else {
                return Ok(Outcome::default());
            };
            let run = link.keys.entry(key.clone()).or_default();
            run.retain(|held| !inside(held.at));
            run.extend(keys.iter().map(|span| ParamKey {
                at: span.at,
                value: span.value,
                ease: span.ease,
            }));
        }
    }
    // Тот же нормализатор, что у чтения документа: границы значений и порядок ключей.
    *clip = clip.clone().tidy();
    dedupe(clip, &target);
    Ok(Outcome {
        created_id: None,
        applied: *clip != before,
    })
}

/// Из ключей в одной точке оставляет последний: список отрезка главнее.
fn dedupe(clip: &mut Clip, target: &KeyTarget) {
    let same = |a: f64, b: f64| (a - b).abs() < 1e-9;
    match target {
        KeyTarget::Property { property } => {
            let mut kept: Vec<ClipKey> = Vec::with_capacity(clip.keys.len());
            for key in clip.keys.drain(..) {
                match kept.last_mut() {
                    Some(last)
                        if last.property == *property
                            && key.property == *property
                            && same(last.at, key.at) =>
                    {
                        *last = key;
                    }
                    _ => kept.push(key),
                }
            }
            clip.keys = kept;
        }
        KeyTarget::Effect { entry, key } => {
            if let Some(run) = clip
                .video_effects
                .get_mut(*entry)
                .and_then(|link| link.keys.get_mut(key))
            {
                let mut kept: Vec<ParamKey> = Vec::with_capacity(run.len());
                for held in run.drain(..) {
                    match kept.last_mut() {
                        Some(last) if same(last.at, held.at) => *last = held,
                        _ => kept.push(held),
                    }
                }
                *run = kept;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Editor;

    /// Проект с одним клипом длиной 60 с, id клипа.
    fn long_clip() -> (Editor, String) {
        let mut editor = Editor::new();
        let media = editor
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: "/m.mp4".to_owned(),
                    name: "m.mp4".to_owned(),
                    duration: Some(60.0),
                    kind: MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".to_owned()),
                    video_codec: Some("h264".to_owned()),
                    audio_codec: None,
                    has_audio: false,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("media")
            .created_id
            .expect("media id");
        let clip = editor
            .apply(Command::AddClipAtFirstFree {
                media_id: media,
                start: 0.0,
            })
            .expect("clip")
            .created_id
            .expect("clip id");
        (editor, clip)
    }

    fn clip<'a>(editor: &'a Editor, id: &str) -> &'a Clip {
        editor.project().active().clip(id).expect("the clip")
    }

    /// Ключи на каждом кадре 60-секундного клипа не склеиваются, а старые внутри отрезка уходят.
    #[test]
    fn a_frame_per_key_survives_on_a_long_clip() {
        let (mut editor, id) = long_clip();
        let frame = 1.0 / 30.0 / 60.0;
        editor
            .apply(Command::SetClipKey {
                clip_id: id.clone(),
                property: KeyProperty::Scale,
                at: 0.5 + 2.0 * frame,
                value: 3.0,
                ease: KeyEase::LINEAR,
            })
            .expect("an old key");
        let keys: Vec<SpanKey> = (0..10)
            .map(|k| SpanKey {
                at: 0.5 + f64::from(k) * frame,
                value: 1.0 + f64::from(k) * 0.01,
                ease: KeyEase::LINEAR,
            })
            .collect();
        editor
            .apply(Command::SetKeySpan {
                clip_id: id.clone(),
                target: KeyTarget::Property {
                    property: KeyProperty::Scale,
                },
                from: 0.5,
                to: 0.5 + 9.0 * frame,
                keys,
            })
            .expect("span");
        let held: Vec<&ClipKey> = clip(&editor, &id).keys_on(KeyProperty::Scale).collect();
        assert_eq!(held.len(), 10, "one key a frame, the old one replaced");
        assert!(held.iter().all(|key| key.value < 2.0));
    }

    /// Ключи параметра ложатся в своё звено; звено вне цепочки — ничего не меняется.
    #[test]
    fn an_effect_span_keys_its_link_only() {
        let (mut editor, id) = long_clip();
        editor
            .apply(Command::UpdateClip {
                clip_id: id.clone(),
                patch: ClipPatch {
                    video_effects: Some(vec![AppliedFilter::new("concat.glow")]),
                    ..ClipPatch::default()
                },
            })
            .expect("a chain");
        let span = |entry: usize| Command::SetKeySpan {
            clip_id: id.clone(),
            target: KeyTarget::Effect {
                entry,
                key: "amount".to_owned(),
            },
            from: 0.1,
            to: 0.2,
            keys: vec![
                SpanKey {
                    at: 0.1,
                    value: 0.0,
                    ease: KeyEase::LINEAR,
                },
                SpanKey {
                    at: 0.15,
                    value: 80.0,
                    ease: KeyEase::LINEAR,
                },
            ],
        };
        let missing = editor.apply(span(3)).expect("no-op");
        assert!(!missing.applied);
        editor.apply(span(0)).expect("span");
        let link = &clip(&editor, &id).video_effects[0];
        assert_eq!(link.keys_on("amount").len(), 2);
    }
}
