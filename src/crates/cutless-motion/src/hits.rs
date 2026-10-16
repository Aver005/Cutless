// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Удары: наезд, тряска, вспышка и другие акценты в моменты музыки.
//! Ключ на каждый кадр затухания, близкие удары складываются в одну огибающую, всё — одним отрезком ключей.

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::commands::keyspans::{KeyTarget, SpanKey};
use concat_project::model::{AppliedFilter, Clip, ClipKind, KeyEase, KeyProperty};

/// Пакет виртуальной камеры: у слоя-обработки трансформ не работает, двигает он.
pub const CAMERA: &str = "cutless.camera";

/// Насколько наезд увеличивает картинку при силе 1.
const PUNCH: f64 = 0.08;
/// Размах тряски при силе 1, доля ширины кадра (около 22 px на 1080).
const SHAKE: f64 = 0.02;
/// Частоты тряски донора econcar, переведённые в радианы в секунду.
const SHAKE_X: f64 = 55.2;
const SHAKE_Y: f64 = 45.6;
/// Окно затухания в длинах `decay`: дальше остаётся меньше 0,1 % силы.
const WINDOW: f64 = 3.0;
/// Дольше затухать удару нельзя, секунды.
const MAX_DECAY: f64 = 2.0;

/// Вид удара.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// Наезд: картинка рывком увеличивается и возвращается.
    Punch,
    /// Затухающая тряска.
    Shake,
    /// Белая вспышка.
    Flash,
    /// Картинка на миг становится чёрно-белой.
    Bw,
    /// Цветовые каналы разъезжаются.
    Rgb,
    /// Вспышка свечения.
    Glow,
    /// Рывок зум-размытия.
    Blur,
}

impl HitKind {
    /// Все виды в порядке кнопок.
    pub const ALL: [HitKind; 7] = [
        HitKind::Punch,
        HitKind::Shake,
        HitKind::Flash,
        HitKind::Bw,
        HitKind::Rgb,
        HitKind::Glow,
        HitKind::Blur,
    ];

    /// Имя для агента и интерфейса.
    pub fn name(self) -> &'static str {
        match self {
            HitKind::Punch => "punch",
            HitKind::Shake => "shake",
            HitKind::Flash => "flash",
            HitKind::Bw => "bw",
            HitKind::Rgb => "rgb",
            HitKind::Glow => "glow",
            HitKind::Blur => "blur",
        }
    }

    /// Вид по имени.
    pub fn from_name(name: &str) -> Option<HitKind> {
        HitKind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Затухание по умолчанию, секунды: кадры донора при 24 fps.
    pub fn default_decay(self) -> f64 {
        let frames = match self {
            HitKind::Punch => 8.0,
            HitKind::Shake => 7.0,
            HitKind::Flash => 3.0,
            HitKind::Bw | HitKind::Rgb | HitKind::Glow | HitKind::Blur => 6.0,
        };
        frames / 24.0
    }

    /// Параметр эффекта, который удар качает; у наезда и тряски его нет.
    pub fn effect(self) -> Option<EffectHit> {
        let (package, param, neutral, peak, min, max) = match self {
            HitKind::Punch | HitKind::Shake => return None,
            HitKind::Flash => ("cutless.flash", "amount", 0.0, 100.0, 0.0, 100.0),
            HitKind::Bw => ("concat.saturation", "amount", 0.0, -100.0, -100.0, 100.0),
            HitKind::Rgb => ("cutless.rgb-split", "shift", 0.0, 0.8, 0.0, 3.0),
            HitKind::Glow => ("concat.glow", "amount", 0.0, 60.0, 0.0, 100.0),
            HitKind::Blur => ("concat.zoom-blur", "amount", 0.0, 35.0, 0.0, 100.0),
        };
        Some(EffectHit {
            package,
            param,
            neutral,
            peak,
            min,
            max,
        })
    }
}

/// Как удар живёт в цепочке эффектов: пакет, параметр, покой, пик при силе 1, границы.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectHit {
    /// Id пакета.
    pub package: &'static str,
    /// Ключ параметра.
    pub param: &'static str,
    /// Значение, при котором эффект ничего не делает.
    pub neutral: f64,
    /// Насколько удар силы 1 уводит параметр от покоя, в его единицах (со знаком).
    pub peak: f64,
    /// Нижняя граница параметра.
    pub min: f64,
    /// Верхняя граница параметра.
    pub max: f64,
}

/// Параметр камеры, который качают наезд и тряска на слое.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraParam {
    /// Ключ в манифесте `cutless.camera`.
    pub key: &'static str,
    /// Покой: камера ничего не меняет.
    pub default: f64,
    /// Нижняя граница.
    pub min: f64,
    /// Верхняя граница.
    pub max: f64,
}

/// Наезд камеры, проценты.
pub const CAMERA_ZOOM: CameraParam = CameraParam {
    key: "zoom",
    default: 100.0,
    min: 100.0,
    max: 400.0,
};
/// Сдвиг камеры по ширине, проценты кадра.
pub const CAMERA_X: CameraParam = CameraParam {
    key: "x",
    default: 0.0,
    min: -50.0,
    max: 50.0,
};
/// Сдвиг камеры по высоте, проценты кадра.
pub const CAMERA_Y: CameraParam = CameraParam {
    key: "y",
    default: 0.0,
    min: -50.0,
    max: 50.0,
};

/// Границы и умолчание параметра эффекта из манифеста: точнее таблицы видов.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamRange {
    /// Нижняя граница.
    pub min: f64,
    /// Верхняя граница.
    pub max: f64,
    /// Что пакет играет, пока параметр не задан.
    pub default: f64,
}

/// Что ударить и как.
#[derive(Clone, Debug)]
pub struct Hits<'a> {
    /// Клип в снимке до правки.
    pub clip: &'a Clip,
    /// Кадров в секунду проекта: ключи ложатся на сетку кадров.
    pub fps: f64,
    /// Ширина кадра к высоте: сдвиг по высоте считается в долях высоты.
    pub aspect: f64,
    /// Моменты ударов, секунды таймлайна, внутри клипа.
    pub times: Vec<f64>,
    /// Сила: 1 — обычный удар, 2 — дроп.
    pub strength: f64,
    /// Затухание в секундах; по умолчанию — своё у каждого вида.
    pub decay: Option<f64>,
    /// Границы и умолчание параметра эффекта из каталога; без них — из таблицы видов.
    pub param: Option<ParamRange>,
}

/// Почему удар не построить.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum HitError {
    /// Не дано ни одного момента.
    #[error("no hit times given")]
    NoTimes,
    /// Момент вне клипа.
    #[error("hit at {time:.3}s is outside clip {clip} ({start:.3}-{end:.3})")]
    Outside {
        /// Момент.
        time: f64,
        /// Клип.
        clip: String,
        /// Начало клипа.
        start: f64,
        /// Конец клипа.
        end: f64,
    },
    /// Частота кадров или пропорции кадра не годятся.
    #[error("the frame rate and the frame aspect must be positive")]
    BadFrame,
}

/// Команды удара: при нужде добавить эффект в цепочку, затем отрезки ключей. Один `Batch` у вызывающего.
pub fn commands(kind: HitKind, hits: &Hits<'_>) -> Result<Vec<Command>, HitError> {
    let plan = Plan::new(kind, hits)?;
    let clip = hits.clip;
    let mut chain = clip.video_effects.clone();
    let mut commands = Vec::new();
    let mut spans = Vec::new();

    match kind.effect() {
        Some(effect) => {
            let entry = ensure(
                &mut chain,
                effect.package,
                &[(effect.param, effect.neutral)],
            );
            let link = &chain[entry];
            // Пакет в цепочке без явного значения играет своё умолчание: от него и качаем.
            let range = hits.param.unwrap_or(ParamRange {
                min: effect.min,
                max: effect.max,
                default: effect.neutral,
            });
            for segment in &plan.segments {
                spans.push(plan.span(
                    segment,
                    KeyTarget::Effect {
                        entry,
                        key: effect.param.to_owned(),
                    },
                    |at, sample| {
                        let base = link.value_at(effect.param, at, range.default);
                        (base + effect.peak * plan.strength * sample.pulse)
                            .clamp(range.min, range.max)
                    },
                ));
            }
        }
        None if clip.kind == ClipKind::Layer => {
            let rest: Vec<(&str, f64)> = [CAMERA_ZOOM, CAMERA_X, CAMERA_Y]
                .iter()
                .map(|param| (param.key, param.default))
                .collect();
            let entry = ensure(&mut chain, CAMERA, &rest);
            let link = &chain[entry];
            let target = |param: &CameraParam| KeyTarget::Effect {
                entry,
                key: param.key.to_owned(),
            };
            for segment in &plan.segments {
                spans.push(plan.span(segment, target(&CAMERA_ZOOM), |at, sample| {
                    let base = link.value_at(CAMERA_ZOOM.key, at, CAMERA_ZOOM.default);
                    (base * plan.grow(kind, sample)).clamp(CAMERA_ZOOM.min, CAMERA_ZOOM.max)
                }));
                if kind == HitKind::Shake {
                    for (param, axis) in [(CAMERA_X, Axis::X), (CAMERA_Y, Axis::Y)] {
                        spans.push(plan.span(segment, target(&param), |at, sample| {
                            let base = link.value_at(param.key, at, param.default);
                            (base + 100.0 * sample.offset(axis)).clamp(param.min, param.max)
                        }));
                    }
                }
            }
        }
        None => {
            for segment in &plan.segments {
                spans.push(plan.span(
                    segment,
                    KeyTarget::Property {
                        property: KeyProperty::Scale,
                    },
                    |at, sample| clip.value_at(KeyProperty::Scale, at) * plan.grow(kind, sample),
                ));
                if kind == HitKind::Shake {
                    for (property, offset) in [
                        (KeyProperty::OffsetX, Axis::X),
                        (KeyProperty::OffsetY, Axis::Y),
                    ] {
                        spans.push(plan.span(
                            segment,
                            KeyTarget::Property { property },
                            |at, sample| clip.value_at(property, at) + sample.offset(offset),
                        ));
                    }
                }
            }
        }
    }

    if chain.len() != clip.video_effects.len() {
        commands.push(Command::UpdateClip {
            clip_id: clip.id.clone(),
            patch: ClipPatch {
                video_effects: Some(chain),
                ..ClipPatch::default()
            },
        });
    }
    commands.extend(spans);
    Ok(commands)
}

/// Индекс пакета в цепочке; нет — добавляет в конец в покое.
fn ensure(chain: &mut Vec<AppliedFilter>, package: &str, rest: &[(&str, f64)]) -> usize {
    if let Some(entry) = chain.iter().position(|link| link.id == package) {
        return entry;
    }
    let mut link = AppliedFilter::new(package);
    for (key, value) in rest {
        link.params.insert((*key).to_owned(), *value);
    }
    chain.push(link);
    chain.len() - 1
}

/// Ось сдвига тряски.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// Состояние удара в один кадр: огибающая и сдвиг тряски.
#[derive(Clone, Copy, Default)]
struct Sample {
    /// Сильнейший из импульсов, `0..=1`.
    pulse: f64,
    /// Сдвиг тряски по ширине, доля ширины кадра.
    x: f64,
    /// Сдвиг тряски по высоте, доля высоты кадра.
    y: f64,
}

impl Sample {
    fn offset(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
        }
    }
}

/// Отрезок кадров, где хотя бы один удар ещё звучит.
struct Segment {
    /// Секунды таймлайна, где стоят ключи.
    times: Vec<f64>,
}

/// Удар, разложенный по кадрам.
struct Plan<'a> {
    clip: &'a Clip,
    strength: f64,
    tau: f64,
    aspect: f64,
    hits: Vec<f64>,
    segments: Vec<Segment>,
}

impl<'a> Plan<'a> {
    fn new(kind: HitKind, hits: &Hits<'a>) -> Result<Plan<'a>, HitError> {
        let clip = hits.clip;
        if !(hits.fps.is_finite() && hits.fps > 0.0 && hits.aspect.is_finite() && hits.aspect > 0.0)
        {
            return Err(HitError::BadFrame);
        }
        if hits.times.is_empty() {
            return Err(HitError::NoTimes);
        }
        let end = clip.start + clip.duration;
        let mut times = hits.times.clone();
        for &time in &times {
            if !time.is_finite() || time < clip.start - 1e-6 || time > end + 1e-6 {
                return Err(HitError::Outside {
                    time,
                    clip: clip.id.clone(),
                    start: clip.start,
                    end,
                });
            }
        }
        // Удар — на кадре: иначе кадр удара получает ноль, а пик гаснет до следующего кадра.
        for time in &mut times {
            *time = ((*time * hits.fps).round() / hits.fps).clamp(clip.start, end);
        }
        times.sort_by(f64::total_cmp);
        let frame = 1.0 / hits.fps;
        let decay = hits
            .decay
            .filter(|decay| decay.is_finite())
            .unwrap_or_else(|| kind.default_decay())
            .max(frame)
            .min(MAX_DECAY);
        let strength = if hits.strength.is_finite() {
            hits.strength.clamp(0.1, 3.0)
        } else {
            1.0
        };

        // Окна ударов на сетке кадров: кадр до удара и затухание после; пересекающиеся сливаются.
        let tail = (WINDOW * decay * hits.fps).ceil() as i64;
        let mut windows: Vec<(i64, i64)> = Vec::new();
        for &time in &times {
            let hit = (time * hits.fps).round() as i64;
            let window = (hit - 1, hit + tail);
            match windows.last_mut() {
                Some(last) if window.0 <= last.1 + 1 => last.1 = last.1.max(window.1),
                _ => windows.push(window),
            }
        }
        let segments = windows
            .into_iter()
            .filter_map(|(first, last)| {
                let mut times: Vec<f64> = (first..=last)
                    .map(|k| k as f64 / hits.fps)
                    .filter(|time| *time >= clip.start && *time <= end)
                    .collect();
                // Затухание обрезано концом клипа: последний ключ — на самом конце.
                if (last as f64 / hits.fps) > end
                    && times.last().is_none_or(|time| end - time > 1e-9)
                {
                    times.push(end);
                }
                (times.len() >= 2).then_some(Segment { times })
            })
            .collect();
        Ok(Plan {
            clip,
            strength,
            tau: decay / 2.3,
            aspect: hits.aspect,
            hits: times,
            segments,
        })
    }

    /// Удар в момент `time`: сильнейший импульс и сумма тряски всех ударов.
    fn sample(&self, time: f64) -> Sample {
        let mut sample = Sample::default();
        for (index, hit) in self.hits.iter().enumerate() {
            let since = time - hit;
            if since < -1e-9 {
                continue;
            }
            let pulse = (-since.max(0.0) / self.tau).exp();
            sample.pulse = sample.pulse.max(pulse);
            // Своя фаза у каждого удара, чтобы соседние не повторяли друг друга.
            let phase = index as f64 * 2.399;
            let amp = SHAKE * self.strength * pulse;
            sample.x += amp * (SHAKE_X * since + phase).sin();
            sample.y += 0.7 * amp * (SHAKE_Y * since + phase).cos() * self.aspect;
        }
        sample
    }

    /// Во сколько раз увеличить картинку: наезд или запас, чтобы тряска не открыла край.
    fn grow(&self, kind: HitKind, sample: Sample) -> f64 {
        match kind {
            HitKind::Shake => 1.0 + 2.0 * sample.x.abs().max(sample.y.abs()),
            _ => 1.0 + PUNCH * self.strength * sample.pulse,
        }
    }

    /// Отрезок ключей одной цели; последний кадр окна — точно в покое, если удар уже отзвучал.
    fn span(
        &self,
        segment: &Segment,
        target: KeyTarget,
        value: impl Fn(f64, Sample) -> f64,
    ) -> Command {
        let at = |time: f64| ((time - self.clip.start) / self.clip.duration).clamp(0.0, 1.0);
        let last = segment.times.len() - 1;
        let keys: Vec<SpanKey> = segment
            .times
            .iter()
            .enumerate()
            .map(|(index, &time)| {
                let mut sample = self.sample(time);
                if index == last && sample.pulse < 0.01 {
                    sample = Sample::default();
                }
                SpanKey {
                    at: at(time),
                    value: value(at(time), sample),
                    ease: KeyEase::LINEAR,
                }
            })
            .collect();
        Command::SetKeySpan {
            clip_id: self.clip.id.clone(),
            target,
            from: at(segment.times[0]),
            to: at(segment.times[last]),
            keys,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::model::Clip;

    /// Клип 60 с с началом в 10 с.
    fn clip(kind: ClipKind) -> Clip {
        Clip::blank(
            "c1".to_owned(),
            "T1".to_owned(),
            kind,
            "clip".to_owned(),
            10.0,
            60.0,
        )
    }

    fn hits<'a>(clip: &'a Clip, times: &[f64]) -> Hits<'a> {
        Hits {
            clip,
            fps: 30.0,
            aspect: 9.0 / 16.0,
            times: times.to_vec(),
            strength: 1.0,
            decay: None,
            param: None,
        }
    }

    /// Ключи одного отрезка.
    fn span_keys(command: &Command) -> (&KeyTarget, &[SpanKey]) {
        match command {
            Command::SetKeySpan { target, keys, .. } => (target, keys),
            other => panic!("not a span: {other:?}"),
        }
    }

    #[test]
    fn a_punch_peaks_on_the_hit_and_settles_back() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Punch, &hits(&clip, &[20.0])).expect("punch");
        assert_eq!(commands.len(), 1);
        let (target, keys) = span_keys(&commands[0]);
        assert_eq!(
            target,
            &KeyTarget::Property {
                property: KeyProperty::Scale
            }
        );
        let peak = keys.iter().map(|key| key.value).fold(0.0, f64::max);
        assert!((peak - 1.08).abs() < 1e-9, "{peak}");
        assert_eq!(
            keys.first().map(|key| key.value),
            Some(1.0),
            "the frame before rests"
        );
        assert_eq!(
            keys.last().map(|key| key.value),
            Some(1.0),
            "the tail rests"
        );
        // Кадр на ключ: 1 до удара + удар + 3·(8/24)·30 = 30 после.
        assert_eq!(keys.len(), 32);
    }

    #[test]
    fn close_hits_share_one_span_and_never_stack() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Punch, &hits(&clip, &[20.0, 20.1])).expect("punch");
        assert_eq!(commands.len(), 1, "the windows overlap");
        let (_, keys) = span_keys(&commands[0]);
        let peak = keys.iter().map(|key| key.value).fold(0.0, f64::max);
        assert!(peak <= 1.08 + 1e-9, "max, not product: {peak}");
    }

    #[test]
    fn a_shake_moves_both_axes_and_keeps_the_edge_covered() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Shake, &hits(&clip, &[30.0])).expect("shake");
        assert_eq!(commands.len(), 3);
        let (_, scale) = span_keys(&commands[0]);
        let (_, x) = span_keys(&commands[1]);
        let (_, y) = span_keys(&commands[2]);
        for ((s, x), y) in scale.iter().zip(x).zip(y) {
            assert!(s.value + 1e-9 >= 1.0 + 2.0 * x.value.abs().max(y.value.abs()));
        }
        assert!(x.iter().any(|key| key.value.abs() > 0.005));
    }

    #[test]
    fn an_effect_hit_adds_its_package_at_rest_then_keys_it() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Flash, &hits(&clip, &[15.0])).expect("flash");
        let Command::UpdateClip { patch, .. } = &commands[0] else {
            panic!("the chain grows first");
        };
        let chain = patch.video_effects.as_ref().expect("a chain");
        assert_eq!(chain[0].id, "cutless.flash");
        assert_eq!(chain[0].params.get("amount"), Some(&0.0));
        let (target, keys) = span_keys(&commands[1]);
        assert_eq!(
            target,
            &KeyTarget::Effect {
                entry: 0,
                key: "amount".to_owned()
            }
        );
        assert!(keys.iter().any(|key| (key.value - 100.0).abs() < 1e-9));
    }

    #[test]
    fn a_layer_punches_through_its_camera() {
        let clip = clip(ClipKind::Layer);
        let commands = commands(HitKind::Punch, &hits(&clip, &[12.0])).expect("punch");
        let (target, keys) = span_keys(&commands[1]);
        assert_eq!(
            target,
            &KeyTarget::Effect {
                entry: 0,
                key: "zoom".to_owned()
            }
        );
        let peak = keys.iter().map(|key| key.value).fold(0.0, f64::max);
        assert!((peak - 108.0).abs() < 1e-6, "{peak}");
    }

    #[test]
    fn a_hit_by_the_end_is_cut_at_the_clip() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Punch, &hits(&clip, &[69.95])).expect("punch");
        let Command::SetKeySpan { to, keys, .. } = &commands[0] else {
            panic!("a span");
        };
        assert!((*to - 1.0).abs() < 1e-9);
        assert!(keys.iter().all(|key| (0.0..=1.0).contains(&key.at)));
    }

    #[test]
    fn hits_outside_the_clip_or_none_are_refused() {
        let clip = clip(ClipKind::Video);
        assert_eq!(
            commands(HitKind::Punch, &hits(&clip, &[])),
            Err(HitError::NoTimes)
        );
        assert!(matches!(
            commands(HitKind::Punch, &hits(&clip, &[5.0])),
            Err(HitError::Outside { .. })
        ));
    }

    #[test]
    fn an_off_grid_hit_still_peaks_in_full() {
        let clip = clip(ClipKind::Video);
        let commands = commands(HitKind::Punch, &hits(&clip, &[20.017])).expect("punch");
        let (_, keys) = span_keys(&commands[0]);
        let peak = keys.iter().map(|key| key.value).fold(0.0, f64::max);
        assert!((peak - 1.08).abs() < 1e-9, "{peak}");
    }

    #[test]
    fn an_effect_already_there_swings_from_its_package_default() {
        let mut clip = clip(ClipKind::Video);
        clip.video_effects = vec![AppliedFilter::new("concat.glow")];
        let mut plan = hits(&clip, &[30.0]);
        plan.param = Some(ParamRange {
            min: 0.0,
            max: 100.0,
            default: 45.0,
        });
        let commands = commands(HitKind::Glow, &plan).expect("glow");
        assert_eq!(commands.len(), 1, "the chain already has glow");
        let (_, keys) = span_keys(&commands[0]);
        assert_eq!(keys.first().map(|key| key.value), Some(45.0));
        assert_eq!(keys.last().map(|key| key.value), Some(45.0));
        let peak = keys.iter().map(|key| key.value).fold(0.0, f64::max);
        assert!(
            (peak - 100.0).abs() < 1e-9,
            "45 + 60, held to the range: {peak}"
        );
    }

    #[test]
    fn a_slow_frame_rate_does_not_panic() {
        let clip = clip(ClipKind::Video);
        let mut plan = hits(&clip, &[30.0]);
        plan.fps = 0.25;
        assert!(commands(HitKind::Punch, &plan).is_ok());
    }

    #[test]
    fn names_round_trip_and_effects_stay_in_range() {
        for kind in HitKind::ALL {
            assert_eq!(HitKind::from_name(kind.name()), Some(kind));
            if let Some(effect) = kind.effect() {
                assert!((effect.min..=effect.max).contains(&effect.neutral));
                assert!((effect.min..=effect.max).contains(&(effect.neutral + effect.peak)));
            }
        }
    }
}
