// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! «Залп»: удар выбранного вида по клипу в момент курсора — кнопка к тому же, что `hit_fx` агента.
//! Удар строит `cutless-motion`; границы параметра эффекта берутся из встроенного каталога.

use concat_effects::Catalogue;
use concat_project::Command;
use concat_project::model::{Clip, ClipKind};
use cutless_motion::hits::{self, HitKind, Hits, ParamRange};

/// Во сколько раз удар «дроп» сильнее обычного.
const DROP: f64 = 2.0;

/// Кадр и его пропорции, в которых бьёт залп.
pub struct Frame {
    /// Кадров в секунду.
    pub fps: f64,
    /// Ширина к высоте.
    pub aspect: f64,
}

/// Команды удара по клипу в момент `time`; `None`, если клип не под курсором или это звук.
pub fn commands(
    clip: &Clip,
    kind: i32,
    strong: bool,
    time: f64,
    frame: &Frame,
) -> Option<Vec<Command>> {
    let kind = *HitKind::ALL.get(usize::try_from(kind).ok()?)?;
    // Полкадра допуска: курсор хранится во f32 и на краю клипа может чуть промахнуться.
    let slack = 0.5 / frame.fps.max(1.0);
    let end = clip.start + clip.duration;
    if clip.kind == ClipKind::Audio || time < clip.start - slack || time > end + slack {
        return None;
    }
    let time = time.clamp(clip.start, end);
    let param = kind
        .effect()
        .and_then(|effect| range(effect.package, effect.param));
    let hits = Hits {
        clip,
        fps: frame.fps,
        aspect: frame.aspect,
        times: vec![time],
        strength: if strong { DROP } else { 1.0 },
        decay: None,
        param,
    };
    hits::commands(kind, &hits).ok()
}

/// Границы и умолчание параметра пакета из встроенного каталога.
fn range(package: &str, param: &str) -> Option<ParamRange> {
    let package = Catalogue::builtin().get(package)?;
    let param = package
        .manifest
        .params
        .iter()
        .find(|known| known.key == param)?;
    Some(ParamRange {
        min: param.min,
        max: param.max,
        default: param.default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(kind: ClipKind) -> Clip {
        Clip::blank(
            "c1".to_owned(),
            "T1".to_owned(),
            kind,
            "clip".to_owned(),
            2.0,
            4.0,
        )
    }

    const FRAME: Frame = Frame {
        fps: 30.0,
        aspect: 9.0 / 16.0,
    };

    #[test]
    fn a_hit_under_the_playhead_keys_the_clip() {
        let commands = commands(&clip(ClipKind::Video), 0, false, 3.0, &FRAME).expect("a punch");
        assert!(matches!(commands[0], Command::SetKeySpan { .. }));
    }

    #[test]
    fn a_flash_reads_its_range_from_the_catalogue() {
        let commands = commands(&clip(ClipKind::Video), 2, true, 3.0, &FRAME).expect("a flash");
        assert!(
            matches!(commands[0], Command::UpdateClip { .. }),
            "flash joins the chain"
        );
    }

    #[test]
    fn a_playhead_a_hair_before_the_clip_still_hits() {
        let start = clip(ClipKind::Video).start;
        assert!(commands(&clip(ClipKind::Video), 0, false, start - 1e-5, &FRAME).is_some());
    }

    #[test]
    fn sound_clips_and_misses_are_left_alone() {
        assert!(commands(&clip(ClipKind::Audio), 0, false, 3.0, &FRAME).is_none());
        assert!(commands(&clip(ClipKind::Video), 0, false, 9.0, &FRAME).is_none());
        assert!(commands(&clip(ClipKind::Video), 99, false, 3.0, &FRAME).is_none());
    }
}
