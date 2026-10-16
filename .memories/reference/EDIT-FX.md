# EDIT-FX — эффекты для эдитов: что есть и быстрые победы
> Разведка под TikTok-монтаж, эдиты и игровые нарезки под музыку. Last updated: 2026-10-16 (разведка, кода нет).

**Коротко.** Движок умеет почти всё, на чём строятся эдиты: ключи с кривыми Безье, кривую скорости,
слой-обработку поверх дорожек, вырезание человека, пословную раскладку титров. Не хватает трёх вещей:
пакетов тряски, RGB-сдвига, вспышки и инверсии (апстрим удалил их 2026-09-26, исходники лежат в git);
инструментов агента для ключей, скорости, слоёв и переходов; сетки долей (Ф5.2). Первые победы — это
TOML/WGSL и мелкий Rust в `cutless-tools`, движок трогать не нужно.

Ниже `C:` = `src/crates/`. Факты по коду собраны разведкой, сборка и запуск не делались.

## 1. Что уже есть

| Что | Есть | Где | Агент может? |
|---|---|---|---|
| Пакеты | 127: 22 видеоэффекта, 57 лутов, 20 переходов, 28 звуковых. Встраиваются папкой (`build.rs`), свои грузятся `Catalogue::load_dir` | `C:concat-effects/packages/` | `effects_catalogue`, `effect_apply`, `effect_remove` |
| Видеоэффекты | glow, bloom-pulse (дышит по `time`), motion-blur (длина, угол), zoom-blur (сила, центр, ключи), gaussian-blur, chromatic-aberration, fisheye, pixelate, halftone, posterize, film-grain, vignette, exposure, contrast, saturation, curves, color-wheels, white-balance, adjust, chroma/luma-key, `cutless.video-denoise` | там же | Применить — да, ключи — нет |
| Переходы | whip-pan, zoom, zoom-blur, spin, swirl, glitch, glitch-warp, rgb-split-wipe, light-leak-flash, light-leak, film-burn, push, slide, cube, iris, wipes, dissolve, blur-dissolve. В модели только `id` и `duration` | `C:concat-project/src/model.rs` (`Transition`) | Нет |
| Звук | 25 пакетов апстрима + 3 наших (band-cut, noise-gate, phone-voice) | пакеты `kind = "audio"` | Только 5 пресетов `audio_clean` |
| Ключи клипа | `KeyProperty`: Scale, OffsetX, OffsetY, Rotation, Opacity, Volume; любая Безье (`KeyEase`); `at` — доля длины клипа | `model.rs`, `SetClipKey`, `ClearClipKey(s)` | Нет |
| Ключи параметров эффектов | `AppliedFilter.keys`, `SetEffectKey`. Рендер берёт ключи у **любого** параметра (`params_at`); флаг `animate` в TOML влияет только на ромбики в интерфейсе и стоит у 7 пакетов | `commands/mod.rs`, `catalogue.rs` | Нет |
| Скорость | Постоянная 0.0625…16 и **кривая** `SetClipSpeedCurve` (звук ступеньками по 0.1 с). Редактора кривой в интерфейсе нет | `model.rs`, `concat-core/src/retime.rs` | Нет |
| Freeze, реверс | `FreezeFrame`, реверс копией файла | `commands/mod.rs`, `concat-host/src/reverse.rs` | Нет, нет глагола API |
| Слой-обработка | `ClipKind::Layer`, `AddLayerClip`. У слоя работают **только ключи параметров эффектов**: трансформ не применяется, прозрачность = opacity × фейды, `clip_time` = 0 | `concat-render/src/plan.rs`, `concat-export/src/resolve.rs` | Нет |
| Бленды, кроп, флип | normal, multiply, screen, add, lighten, darken; crop, flip, stretch без ключей | `ClipPatch` | Только `clip_transform` без ключей |
| Вырезание фона | `Cutout` (человек, объект, ONNX), мазки | `concat-vision`, `SetClipCutout` | Нет |
| Текст | Шрифт, обводка, тень, плашка, трекинг; субтитры словами. **`reveal_order()` в контракте шейдера жив, но пакетов на нём нет** (`word-reveal` удалён) | `concat-effects/src/shader.rs`, `concat-host/src/titles.rs` | `text_*`, `captions_make` |
| Шаблоны со слотами | `template.save/instantiate` + `FillSlot` — аналог шаблонов CapCut | `concat-host/src/templates.rs` | Нет |
| «Мой корабль» | Цепочка `Vec<AppliedFilter>` с ключами на выделенные или все клипы | `concat/src/ships.rs` | Нет |

**Удалено апстримом, достаётся из git:**
- `3babe9f` (2026-09-26) — 56 пакетов формата 1: shake, camera-shake, rgb-split, rgb-glitch, strobe-flash,
  invert, word-reveal, impact-ring, hue-shift, duotone, lens-flare, light-leak, mirror, kaleidoscope, datamosh,
  маски. `git show 3babe9f^:src/crates/concat-effects/packages/concat.<id>/effect.wgsl`, шейдеры по 10–20 строк.
- `15711ef` (2026-09-24) — 50 пресетов анимации In/Out/Combo/Loop (Pop, Bounce, Elastic, Drop, Swing, Shake,
  Pulse, Heartbeat, Jelly). Таблица — `git show 15711ef^:src/crates/concat-project/src/animation.rs`.

## 2. Популярные приёмы и из чего они собраны

| Приём | Состав | К музыке |
|---|---|---|
| Velocity, speed ramp, slow-mo | Кривая скорости с easing (+ смаз) | Быстро между долями, медленно на ударе |
| Impact shake | Затухающая тряска на слое: 6→0 за ~20 кадров; в играх 2–3 кадра | Бочка, малый, выстрел |
| Camera shake | Шум позиции, слой увеличен | Весь дроп |
| Zoom punch-in | Scale 1→1.1→1 с overshoot + blur | Сильная доля |
| Zoom/whip/spin-переход | Ключи scale/pos/rot через стык + направленный blur | Стык на доле |
| White flash | Белый слой 1–2 кадра | **Только дроп** |
| Exposure/glow pulse, RGB split, glitch, ч/б или invert на ударе | Ключи параметров на 1–2 кадра | Удары, сбивки |
| Freeze frame | Стоп-кадр + обводка + текст | Пауза, дроп |
| Subject behind text | Дубль клипа с вырезанным человеком, текст между слоями | Дроп |
| Text pop, karaoke | Scale 0→110→100 по словам; подсветка активного слова | Слово на долю, по речи |
| Тишина перед дропом, tape stop | Мьют на долю; скорость и питч вниз | Перед дропом |

Самые популярные шаблоны CapCut — «slowmo», «velocity», «speedramp+shake», «shakes/flash/blur»:
по сути **кривая скорости + тряска + вспышка + blur**. Пайплайн CS2: клипы 3–8 с → маркеры долей, киллы на
доли → ramp перед киллом, punch на сильной доле, shake 2–3 кадра на выстреле, flash только на дропе → грейд, 9:16.

Формулы донора econcar (`src/lib/grid.ts`, `bits.tsx`, `fx.tsx`): `pulse = max(exp(-d/(decay/2.3)))`, decay 6;
`shakeAt`: `x += sin(t*2.3+h)*amp*e`, `y += cos(t*1.9+h)*amp*e*0.7`, amp 18–26 px, decay 7–8;
punch `scale = 1 + pulse(drop,8)*0.08 + pulse(strong,4)*0.012`; flash 1 → 0.45 за 2 кадра; дроп — минимум
вдвое сильнее остальных акцентов, пик в кадре удара.

Источники: [CapCut velocity](https://www.capcut.com/resource/how-to-do-velocity-on-capcut),
[CapCut beat detection](https://www.capcut.com/create/beat-detection-workflow-goal-compilation-edits),
[Boris FX shake](https://borisfx.com/blog/shake-effect-after-effects-tutorial/),
[School of Motion camera shake](https://schoolofmotion.com/blog/how-to-simulate-camera-shake-adobe-after-effects),
[PremiumBeat whip pan](https://www.premiumbeat.com/blog/create-seamless-transitons-whip-pan/),
[clip.dor.gg gaming montage](https://clip.dor.gg/en/blog/how-to-make-gaming-montage),
[pixflow CS2 frag movie](https://pixflow.net/blog/cs2-frag-movie-guide/),
[RSMB](https://revisionfx.com/products/rsmb/), [khs.ru, приёмы CapCut](https://khs.ru/blog/5ulzfpas91-capcut-top-priemov-montazha-na-telefone).

## 3. Быстрые победы (по выгоде)

| # | Что | Опора | Чем | Размер | Нужна сетка долей |
|---|---|---|---|---|---|
| 1 | `animate = true` у ударных параметров: glow, bloom-pulse, chromatic-aberration, motion-blur, gaussian-blur, vignette, fisheye, pixelate, film-grain | Рендер уже играет ключи любого параметра | TOML | S | Нет |
| 2 | Инструменты агента для готовых команд: `clip_keys`, `effect_keys`, `clip_speed` (+ кривая), `layer_add`, `transition_set`, `clip_look` (opacity, blend, flip, crop, фейды); min/max/default в `effects_catalogue` | `cutless-tools/src/edit/` | Rust в `cutless-tools` | S–M | Нет |
| 3 | Вернуть пакеты форматом 2 как `cutless.*`: shake, rgb-split, invert, flash, hue-shift, rgb-glitch, light-leak, lens-flare, mirror-tile. Линейный свет, `fixtures.toml` | `3babe9f^` | WGSL + TOML | S за пакет | Нет |
| 4 | Пресеты кривой скорости: Velocity, Montage, Hero, Bullet, Jump Cut, Flash In/Out, «Ramp в удар» | `SetClipSpeedCurve`, `retime.rs` | Таблица в Rust, чипсы в SpeedPanel | S агенту, M интерфейс | Позже |
| 5 | `cutless.camera` — виртуальная камера для слоя: zoom, x, y, rotation, tilt (2.5D), направленный blur, края зеркалом | Слой-обработка + п. 1 | WGSL + TOML | S–M | Нет |
| 6 | Генераторы «на удар»: punch, shake, pulse, flash, ч/б-, RGB-, glow-удар → ключи одним `Batch` | Формулы econcar, п. 1, 2, 5 | Чистые функции в `cutless-tools` | M | По ручным временам; с сеткой — сами |
| 7 | Пресеты появления и ухода (Pop, Bounce, Elastic, Drop, Swing; Loop Pulse, Heartbeat, Shake) как ключи клипа | `15711ef^:animation.rs` (значения относительные — умножать на scale/offset) | Rust | S–M | Нет |
| 8 | Пословное появление и караоке: вернуть `word-reveal` и вариант с `progress` под ключи; `captions_make {style:"karaoke"}` | `reveal_order()`, `WordRect` | WGSL + TOML + генератор ключей | S–M | Для Slam по долям |
| 9 | Звук на ключах громкости: мьют перед дропом, приглушение музыки под голос, «тише до удара»; tape stop | `KeyProperty::Volume`, econcar `mix.tsx` | Rust | S | Мьют перед дропом — да |
| 10 | Наборы эффектов в один клик («Абордаж» = shake + flash + blur + ramp), как «Мой корабль» | `ships.rs` | Rust + Slint | S–M | Нет |
| 11 | Сетка долей Ф5.2 `cutless-beats`: bpm ±1.5 шагом 0.02, фаза шагом 4 мс, бочка 25–150 Гц, малый 1.2–6 кГц, `peak_pick`; полосы бикводами без `rustfft` `[?]`; `extra["cutless.beats"]` | econcar `tools/audio.py`, чтение PCM как в `cutless-pauses` | Rust + маркеры (Slint) | M анализ, L маркеры | Это и есть основа |
| 12 | Subject behind text: дубль + `SetClipCutout(Person)` + титр между | `Cutout` | `Batch` + расчёт маски хостом `[?]` | M | Нет |
| 13 | Имитация motion blur: генератор знает свои ключи → ключи blur/angle для `cutless.camera` | п. 5, 6 | Rust | M | Нет |
| 14 | Freeze и реверс для агента: глаголы API | `FreezeFrame`, `reverse.rs` | Rust (`concat-api`) | M | Freeze на дроп — да |

**Порядок:** 1 → 2 → 3 → 5 → 4 → 6. После этого человек и агент делают shake, punch, flash, RGB, ramp и glow;
сетка долей (11) переводит всё это с «по курсору» на «по музыке».

## 4. Автоматизация

- Кнопка «На удар» по курсору или выделенным маркерам: приём и сила S/M/L/«Дроп» (×2), один `Batch` = один Ctrl+Z.
- «Применить к выделенным / ко всем» для наборов, пресетов скорости и анимаций.
- После Ф5.2: «каждая бочка → punch», «дроп → flash + shake ×2 + RGB», «резать по тактам», «ramp за долю до удара».
- Рецепт «Эдит CS2» (облегчённая Ф6): рефрейм 9:16 → грейд → анализ трека → хайлайты на доли → ramp перед
  киллом → punch на сильных долях → shake на выстрелах → одна вспышка на дропе → тишина перед дропом → субтитры.
- Агент на «сделай эдит под этот трек»: `beats_analyze` → `timeline_summary`/`frame_at` → `clips_split`/`clips_move`
  на тактах → `clip_speed` → `hit_fx` → `layer_add` + `cutless.camera` → звук → `captions_make` → проверка правилами
  promo-reel (не одна и та же вспышка на каждой склейке, дроп сильнее остальных, склейки ровно на сетке).
- В описание инструментов: «не больше одной вспышки на секцию, эффект не на каждую склейку».

## 5. Пробелы и риски

- **Нет настоящего motion blur** (временного смаза, оптического потока): шейдер видит только текущий кадр. Обход — п. 13
  и запись 60+ fps. Echo, trails, datamosh — туда же. Отложить (L, рендер-конвейер).
- **Нет 3D**, только 2D-трансформ; 2.5D-наклон — перспективой в `cutless.camera`.
- **У слоя-обработки нет трансформа и ключей прозрачности**, `clip_time` = 0 — отсюда отдельная камера-пакет.
- **Ключи хранятся долями длины клипа**: после обрезки удар уезжает. Решение — генерировать ключи заново
  («переложить на доли») или ставить удары на короткий отдельный слой.
- Нет ключей на `stretch_x/y` (squash & stretch) — новый `KeyProperty`, расхождение с апстримом. Отложить.
- Параметры перехода не хранятся (только `id`, `duration`); модель заморожена (`DOCUMENT_VERSION = 1`).
- `UpdateClip` заменяет цепочку эффектов целиком, `SetEffectKey` адресует эффект индексом — до Ф2.4 генераторы
  должны читать свежую ревизию и писать ключи одним `Batch`.
- `DECISIONS.md`: анимации текста — пакетами, не полями клипа. Пресеты п. 7 пишут обычные ключи (решение не
  нарушают), но для титров лучше пакеты п. 8. **Нужно подтверждение владельца**, что ключи-пресеты для видеоклипов допустимы.
- Восстановленные пакеты — под `cutless.*` с алиасами на старые `concat.*` `[?]`.
- Не проверено: звук кривой скорости ступеньками (tape stop может звучать грубо); маска cutout через API без окна.
- Отложить: оптический поток, 3D, VST, библиотеки SFX и оверлеев (лицензии ассетов), автопоиск киллов CS2 после сетки долей.
